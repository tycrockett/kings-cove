//! A computer player, for seats the server plays. In setup it takes the
//! corners whose numbers come up most. On its turns it rolls, moves a bandit
//! after a 7, sends its knights after items and bandits, builds what it can
//! (castle, town, outpost, then road), offering another player a 1:1 trade
//! or the bank 4:1 to get there, recruits and deploys knights, casts a spell
//! and ends the turn. Its [`Style`] decides whom it works against and which
//! trades it takes. It never reads what a hidden item is.

use serde::{Deserialize, Serialize};

use crate::{
    Action, Blockade, BuildingKind, EdgeId, Event, Game, ItemKind, Phase, PlayerId, Resource, Resources, Rng,
    RuleError, SetupPlacing, Spell, StructureKind, Target, TileId, Unit, UnitKind, VertexId, balance,
};

/// What a corner a town may still rise on is worth beyond its production.
const UPGRADE_BONUS: u32 = 3;
/// What each tile of a resource the bot has no building on yet adds.
const NEW_RESOURCE_BONUS: u32 = 2;
/// What a hidden item is worth walking to, in the units of [`Stakes::harm`].
const ITEM_BONUS: i64 = 12;
/// What each relic lying in the open is worth walking to.
const RELIC_BONUS: i64 = 30;
/// What a rival's knight felled by a ward is worth, per point of what the
/// bot weighs that rival's losses at.
const WARD_KNIGHT: i64 = 6;
/// What each tile of the way to a goal takes off its worth.
const STEP_COST: i64 = 3;
/// The least harm a knight blockades a tile for.
const BLOCKADE_MIN: i64 = 8;
/// The least a spell must be worth for the bot to pay its price.
const PAID_SPELL_MIN: i64 = 15;
/// What Windfall is worth per resource the bot's buildings touch.
const WINDFALL_PER_RESOURCE: i64 = 3;

/// How a bot treats the other players.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Style {
    /// Takes most trades it is offered, and works against no one until they
    /// are a point from winning.
    Friendly,
    /// Trades when it gains, and works against whoever leads it or nears the
    /// goal.
    #[default]
    Balanced,
    /// Trades only at a profit and only with players not ahead of it, and
    /// works against everyone, leaders most: bandits, frost, blockades and
    /// knights.
    Ruthless,
}

impl Style {
    pub const ALL: [Style; 3] = [Style::Friendly, Style::Balanced, Style::Ruthless];

    pub fn label(self) -> &'static str {
        match self {
            Style::Friendly => "Friendly",
            Style::Balanced => "Balanced",
            Style::Ruthless => "Ruthless",
        }
    }

    /// It works against `rival`.
    fn opposes(self, g: &Game, me: PlayerId, rival: PlayerId) -> bool {
        let theirs = g.points(rival);
        match self {
            Style::Friendly => theirs + 1 >= g.victory_points,
            Style::Balanced => theirs > g.points(me) || theirs + 3 >= g.victory_points,
            Style::Ruthless => true,
        }
    }

    /// What a resource `rival` loses is worth to it.
    fn rival_weight(self, g: &Game, me: PlayerId, rival: PlayerId) -> i64 {
        if !self.opposes(g, me, rival) {
            return 0;
        }
        let weight = 1 + i64::from(g.points(rival));
        if self == Style::Ruthless { 2 * weight } else { weight }
    }

    /// What a resource it loses itself costs it.
    fn own_weight(self) -> i64 {
        if self == Style::Ruthless { 2 } else { 3 }
    }

    /// It takes a trade bringing it `gain` for `loss`, as it values them.
    fn accepts(self, gain: u32, loss: u32) -> bool {
        match self {
            Style::Friendly => gain * 5 >= loss * 4,
            Style::Balanced => gain > loss,
            Style::Ruthless => gain * 2 >= loss * 3 && gain > loss,
        }
    }

    /// It trades with `other` at all.
    fn trades_with(self, g: &Game, me: PlayerId, other: PlayerId) -> bool {
        let theirs = g.points(other);
        let cutoff = match self {
            Style::Friendly => 1,
            Style::Balanced => 2,
            Style::Ruthless => 3,
        };
        theirs + cutoff < g.victory_points && (self != Style::Ruthless || theirs <= g.points(me))
    }

    /// Knights it keeps, on the board and in hand.
    fn knights(self) -> u32 {
        if self == Style::Ruthless { 2 } else { 1 }
    }
}

/// Where the bot left its turn.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Turn {
    /// The game moved on: a setup placement made, the turn passed or the
    /// game won.
    Over,
    /// It offered a trade and waits for the answer; [`play_turn`] goes on
    /// with the turn once the offer is answered or withdrawn.
    Waiting,
}

/// Plays `player`'s part while the game waits on them: in setup one
/// building and its road, in play a whole turn, unless it stops to wait on
/// a trade offer it made.
pub fn play_turn(g: &mut Game, player: PlayerId, style: Style, rng: &mut Rng) -> Result<Turn, RuleError> {
    if g.current != player {
        return Err(RuleError::NotYourTurn);
    }
    match g.phase {
        Phase::Setup { .. } => setup(g, player, rng),
        Phase::Playing => turn(g, player, style, rng),
        Phase::Finished { .. } => Err(RuleError::GameOver),
    }
}

/// Answers the open trade offer when it is `player`'s to answer: accepts
/// one its style takes, from someone it trades with, and declines the rest.
pub fn answer_trade(g: &mut Game, player: PlayerId, style: Style, rng: &mut Rng) {
    let Ok(offer) = g.trade_for(player) else {
        return;
    };
    let have = g.players[player.index()].resources;
    let accept = have.covers(&offer.want)
        && g.players[offer.from.index()].resources.covers(&offer.give)
        && style.trades_with(g, player, offer.from)
        && style.accepts(worth(&have, &offer.give), worth(&have, &offer.want));
    let action = if accept { Action::AcceptTrade } else { Action::DeclineTrade };
    let _ = g.apply(player, action, rng);
}

/// A bundle as the holder of `have` values it: what they hold little of
/// counts for more.
fn worth(have: &Resources, bundle: &Resources) -> u32 {
    Resource::ALL.iter().map(|&r| bundle.get(r) * 12 / (1 + have.get(r))).sum()
}

fn setup(g: &mut Game, player: PlayerId, rng: &mut Rng) -> Result<Turn, RuleError> {
    let touched = touched(g, player);
    if let Phase::Setup { placing: SetupPlacing::Building, .. } = g.phase {
        let spots = g.setup_vertices(player);
        let v = best(&spots, |&v| spot_value(g, &touched, v)).ok_or(RuleError::InvalidTarget)?;
        g.apply(player, Action::SetupBuilding(v), rng)?;
    }
    let edges = g.setup_edges(player);
    let e = best(&edges, |&e| road_value(g, player, &touched, e)).ok_or(RuleError::InvalidTarget)?;
    g.apply(player, Action::SetupRoad(e), rng)?;
    Ok(Turn::Over)
}

fn turn(g: &mut Game, player: PlayerId, style: Style, rng: &mut Rng) -> Result<Turn, RuleError> {
    if g.trade.as_ref().is_some_and(|t| t.from == player) {
        return Ok(Turn::Waiting);
    }
    if g.dice.is_none() {
        g.apply(player, Action::Roll, rng)?;
    }
    if g.robber_relocation {
        send_bandit(g, player, style, rng);
    }
    command_knights(g, player, style, rng);
    loop {
        if g.winner().is_some() {
            return Ok(Turn::Over);
        }
        match build_one(g, player, style, rng) {
            Step::Done => {}
            Step::Offered => return Ok(Turn::Waiting),
            Step::Stuck => break,
        }
    }
    recruit(g, player, style, rng);
    cast_spell(g, player, style, rng);
    if g.winner().is_none() {
        g.apply(player, Action::EndTurn, rng)?;
    }
    Ok(Turn::Over)
}

/// What each player's lost production is worth to the bot, its own
/// counting against it.
struct Stakes {
    me: PlayerId,
    weights: Vec<i64>,
}

impl Stakes {
    fn new(g: &Game, me: PlayerId, style: Style) -> Stakes {
        let weights = g
            .players
            .iter()
            .map(|p| if p.id == me { -style.own_weight() } else { style.rival_weight(g, me, p.id) })
            .collect();
        Stakes { me, weights }
    }

    /// Each building on `t` with what it collects there in 36 rolls.
    fn yields(g: &Game, t: TileId) -> impl Iterator<Item = (PlayerId, i64)> + '_ {
        let tile = g.board.tile(t);
        let rolls = i64::from(pips(tile.number));
        tile.vertices
            .iter()
            .filter_map(move |&v| g.building(v))
            .map(move |b| (b.owner, rolls * i64::from(b.kind.yield_per_tile())))
    }

    /// What `t` producing nothing is worth to the bot: its rivals' losses
    /// as it weighs them, less its own.
    fn harm(&self, g: &Game, t: TileId) -> i64 {
        Self::yields(g, t).map(|(owner, lost)| lost * self.weights[owner.index()]).sum()
    }

    /// What `t` producing nothing costs the others, as much as anyone.
    fn collateral(&self, g: &Game, t: TileId) -> i64 {
        Self::yields(g, t).filter(|&(owner, _)| owner != self.me).map(|(_, lost)| lost).sum()
    }

    /// What the bot collects on `t`.
    fn own(&self, g: &Game, t: TileId) -> i64 {
        Self::yields(g, t).filter(|&(owner, _)| owner == self.me).map(|(_, got)| got).sum()
    }

    /// Where a bandit is best for the bot: the most harm, then the least
    /// harm to bystanders.
    fn bandit_score(&self, g: &Game, t: TileId) -> (i64, i64) {
        (self.harm(g, t), -self.collateral(g, t))
    }
}

/// After a 7: moves the bandit (or places one) where it does the bot most
/// good, when that is better than where it stands.
fn send_bandit(g: &mut Game, player: PlayerId, style: Style, rng: &mut Rng) {
    let stakes = Stakes::new(g, player, style);
    let free: Vec<TileId> = g.board.tile_ids().filter(|&t| !g.occupied(t)).collect();
    let Some(to) = best(&free, |&t| stakes.bandit_score(g, t)) else {
        return;
    };
    let action = if g.robbers.is_empty() {
        Action::PlaceRobber { to }
    } else {
        let Some(robber) = g.robbers.iter().min_by_key(|r| stakes.bandit_score(g, r.tile)) else {
            return;
        };
        if stakes.bandit_score(g, to) <= stakes.bandit_score(g, robber.tile) {
            return;
        }
        Action::RelocateRobber { robber: robber.id, to }
    };
    let _ = g.apply(player, action, rng);
}

/// What became of one step of building.
enum Step {
    /// It built or traded with the bank.
    Done,
    /// It offered a player a trade.
    Offered,
    /// Nothing more to do.
    Stuck,
}

/// Builds the first thing it can of what it would like. Short of
/// everything, it offers a player a trade, once a turn, then trades with the
/// bank when that gets it there.
fn build_one(g: &mut Game, player: PlayerId, style: Style, rng: &mut Rng) -> Step {
    let plans = plans(g, player);
    let have = g.players[player.index()].resources;
    if let Some((action, _)) = plans.iter().find(|(_, cost)| have.covers(cost)) {
        return if g.apply(player, action.clone(), rng).is_ok() { Step::Done } else { Step::Stuck };
    }
    if !proposed_this_turn(g, player)
        && let Some(offer) = plans.iter().find_map(|(_, cost)| offer_for(g, player, style, cost))
        && g.apply(player, offer, rng).is_ok()
    {
        return Step::Offered;
    }
    match plans.iter().find_map(|(_, cost)| bank_trade(&have, cost)) {
        Some((give, get)) if g.apply(player, Action::TradeWithBank { give, get }, rng).is_ok() => Step::Done,
        _ => Step::Stuck,
    }
}

/// The player offered a trade since their turn started.
fn proposed_this_turn(g: &Game, player: PlayerId) -> bool {
    g.events
        .iter()
        .rev()
        .take_while(|e| !matches!(e.event, Event::TurnStarted { .. }))
        .any(|e| matches!(e.event, Event::TradeProposed { from, .. } if from == player))
}

/// One of what `player` holds most beyond `cost` for one of what it lacks
/// most, offered to whoever it trades with that holds most of it.
fn offer_for(g: &Game, player: PlayerId, style: Style, cost: &Resources) -> Option<Action> {
    let have = g.players[player.index()].resources;
    let get =
        Resource::ALL.into_iter().filter(|&r| cost.get(r) > have.get(r)).max_by_key(|&r| cost.get(r) - have.get(r))?;
    let give =
        Resource::ALL.into_iter().filter(|&r| have.get(r) > cost.get(r)).max_by_key(|&r| have.get(r) - cost.get(r))?;
    let to = g
        .players
        .iter()
        .filter(|p| p.id != player && p.resources.get(get) > 0 && style.trades_with(g, player, p.id))
        .max_by_key(|p| p.resources.get(get))?
        .id;
    Some(Action::ProposeTrade { to: Some(to), give: Resources::one(give), want: Resources::one(get) })
}

/// What the bot would build now if it could pay, most wanted first, with
/// what each costs: its best castle, town and outpost, and a road only when
/// there is nowhere left to put an outpost.
fn plans(g: &Game, player: PlayerId) -> Vec<(Action, Resources)> {
    let touched = touched(g, player);
    let open = |r: Result<(), RuleError>| matches!(r, Ok(()) | Err(RuleError::CannotAfford));
    let corners: Vec<VertexId> = g.board.vertices.iter().map(|v| v.id).collect();
    let upgrade = |to: BuildingKind| {
        let spots: Vec<VertexId> = corners.iter().copied().filter(|&v| open(g.can_upgrade(player, v, to))).collect();
        best(&spots, |&v| corner_value(g, &touched, v))
    };
    let mut plans = Vec::new();
    if let Some(v) = upgrade(BuildingKind::City) {
        plans.push((Action::UpgradeCity(v), balance::CITY_UPGRADE_COST));
    }
    if let Some(v) = upgrade(BuildingKind::Town) {
        plans.push((Action::UpgradeTown(v), balance::TOWN_UPGRADE_COST));
    }
    let outposts: Vec<VertexId> = corners.iter().copied().filter(|&v| open(g.can_build_outpost(player, v))).collect();
    match best(&outposts, |&v| spot_value(g, &touched, v)) {
        Some(v) => plans.push((Action::BuildOutpost(v), balance::OUTPOST_COST)),
        None => {
            let roads: Vec<EdgeId> = g
                .board
                .edges
                .iter()
                .map(|e| e.id)
                .filter(|&e| open(g.can_build_road(player, e)) && road_value(g, player, &touched, e) > 0)
                .collect();
            if let Some(e) = best(&roads, |&e| road_value(g, player, &touched, e)) {
                plans.push((Action::BuildRoad(e), balance::ROAD_COST));
            }
        }
    }
    plans
}

/// One bank trade toward `cost`, when the bank can make up all of it from
/// what `have` holds beyond it: the most plentiful resource for the one
/// most short.
fn bank_trade(have: &Resources, cost: &Resources) -> Option<(Resource, Resource)> {
    let short = |r: Resource| cost.get(r).saturating_sub(have.get(r));
    let spare = |r: Resource| have.get(r).saturating_sub(cost.get(r)) / balance::BANK_TRADE_RATE;
    let missing: u32 = Resource::ALL.iter().map(|&r| short(r)).sum();
    let tradable: u32 = Resource::ALL.iter().map(|&r| spare(r)).sum();
    if missing == 0 || tradable < missing {
        return None;
    }
    let give = Resource::ALL.into_iter().max_by_key(|&r| spare(r))?;
    let get = Resource::ALL.into_iter().max_by_key(|&r| short(r))?;
    Some((give, get))
}

/// Each ready knight attacks, blockades or walks toward its best goal.
fn command_knights(g: &mut Game, player: PlayerId, style: Style, rng: &mut Rng) {
    let ready: Vec<Unit> = g.units.iter().filter(|u| u.owner == player && u.ready).cloned().collect();
    for unit in ready {
        if g.winner().is_some() || g.unit(unit.id).is_none() {
            continue;
        }
        let stakes = Stakes::new(g, player, style);
        if let Some(order) = knight_order(g, &stakes, style, &unit) {
            let _ = g.apply(player, order, rng);
        }
    }
}

/// What an empty structure of `kind` is worth walking to, and a bandit
/// holding one worth bringing down.
fn structure_bonus(kind: Option<StructureKind>) -> i64 {
    match kind {
        Some(StructureKind::Shrine) => 40,
        Some(StructureKind::Mine) => 30,
        Some(StructureKind::LumberMill) => 25,
        None => 0,
    }
}

/// What a knight does now: lifts a blockade that no longer pays, holds a
/// structure it stands on, attacks a bandit on the bot's production or a
/// structure or a knight it works against, blockades where it stands, or
/// steps toward the best goal.
fn knight_order(g: &Game, stakes: &Stakes, style: Style, unit: &Unit) -> Option<Action> {
    match unit.blockade {
        Some(Blockade::Holding) => {
            return (stakes.harm(g, unit.tile) < BLOCKADE_MIN).then_some(Action::LiftBlockade { unit: unit.id });
        }
        Some(Blockade::Lifting { .. }) => return None,
        None => {}
    }
    if g.structure_at(unit.tile).is_some() {
        return (stakes.harm(g, unit.tile) >= BLOCKADE_MIN).then_some(Action::LayBlockade { unit: unit.id });
    }
    let attack_value = |target: Target| -> i64 {
        match target {
            Target::Robber(r) => {
                g.robber(r).map_or(0, |r| stakes.own(g, r.tile) + structure_bonus(g.structure_at(r.tile)))
            }
            Target::Unit(u) => {
                let Some(enemy) = g.unit(u) else { return 0 };
                let weight = stakes.weights[enemy.owner.index()];
                let blocks_me = if enemy.blockade.is_some() { stakes.own(g, enemy.tile) } else { 0 };
                let holds = i64::from(g.structure_at(enemy.tile).is_some());
                let hunted = weight > 0 && (enemy.relics > 0 || holds > 0 || style == Style::Ruthless);
                blocks_me + if hunted { weight * (1 + (i64::from(enemy.relics) + holds) * 5) } else { 0 }
            }
        }
    };
    let targets = g.attack_targets(stakes.me, unit.id);
    if let Some(target) = best(&targets, |&t| attack_value(t)).filter(|&t| attack_value(t) > 0) {
        return Some(Action::Attack { unit: unit.id, target });
    }
    let stay = stakes.harm(g, unit.tile);
    let routes = routes(g, unit.owner, unit.tile);
    let walk = goals(g, stakes)
        .into_iter()
        .filter_map(|(t, bonus)| routes[t.index()].map(|(dist, step)| (bonus - STEP_COST * i64::from(dist), step)))
        .max_by_key(|&(score, _)| score)
        .filter(|&(score, _)| score > 0);
    match walk {
        Some((score, _)) if stay >= BLOCKADE_MIN && stay >= score => Some(Action::LayBlockade { unit: unit.id }),
        None if stay >= BLOCKADE_MIN => Some(Action::LayBlockade { unit: unit.id }),
        Some((_, to)) => Some(Action::MoveUnit { unit: unit.id, to }),
        None => None,
    }
}

/// Tiles worth sending a knight to, with what reaching each is worth: items
/// (relics in the open most), empty structures, tiles to blockade, and tiles
/// beside a bandit on the bot's production or on a structure.
fn goals(g: &Game, stakes: &Stakes) -> Vec<(TileId, i64)> {
    let items = g.items.iter().map(|i| {
        let relic = i.revealed && i.kind == Some(ItemKind::Relic);
        (i.tile, if relic { RELIC_BONUS * i64::from(i.count) } else { ITEM_BONUS })
    });
    let structures =
        g.structures.iter().filter(|s| !g.occupied(s.tile)).map(|s| (s.tile, structure_bonus(Some(s.kind))));
    let blockades = g
        .board
        .tile_ids()
        .filter(|&t| !g.occupied(t) && g.item_at(t).is_none())
        .map(|t| (t, stakes.harm(g, t)))
        .filter(|&(_, harm)| harm >= BLOCKADE_MIN);
    let bandits = g.robbers.iter().flat_map(|r| {
        let worth = 2 * stakes.own(g, r.tile) + structure_bonus(g.structure_at(r.tile));
        g.board.tile(r.tile).neighbors.iter().filter(move |_| worth > 0).map(move |&t| (t, worth))
    });
    items.chain(structures).chain(blockades).chain(bandits).collect()
}

/// For every tile a knight of `owner`'s on `from` can walk to over free
/// tiles no rival's ward holds: how many steps away it is and the first
/// step there.
fn routes(g: &Game, owner: PlayerId, from: TileId) -> Vec<Option<(u32, TileId)>> {
    let open = |t: TileId| !g.occupied(t) && !g.warded_against(t, owner);
    let mut routes = vec![None; g.board.tiles.len()];
    let mut queue = std::collections::VecDeque::new();
    for &n in &g.board.tile(from).neighbors {
        if open(n) {
            routes[n.index()] = Some((1, n));
            queue.push_back(n);
        }
    }
    while let Some(t) = queue.pop_front() {
        let (dist, first) = routes[t.index()].expect("queued tiles are routed");
        for &n in &g.board.tile(t).neighbors {
            if n != from && routes[n.index()].is_none() && open(n) {
                routes[n.index()] = Some((dist + 1, first));
                queue.push_back(n);
            }
        }
    }
    routes
}

/// Tiles apart, as the crow flies.
fn hex_distance(g: &Game, a: TileId, b: TileId) -> i64 {
    let (a, b) = (g.board.tile(a), g.board.tile(b));
    let (dq, dr) = (i64::from(a.q - b.q), i64::from(a.r - b.r));
    (dq.abs() + dr.abs() + (dq + dr).abs()) / 2
}

/// Buys a knight token when the bot has fewer knights than its style keeps
/// and something for them to do, and deploys tokens nearest the goals.
fn recruit(g: &mut Game, player: PlayerId, style: Style, rng: &mut Rng) {
    let stakes = Stakes::new(g, player, style);
    let goals = goals(g, &stakes);
    let on_board = g.units.iter().filter(|u| u.owner == player).count() as u32;
    let wanted = style.knights();
    if !goals.is_empty() && on_board + g.players[player.index()].cards.knights < wanted {
        let _ = g.apply(player, Action::BuyCard(UnitKind::Knight), rng);
    }
    for _ in on_board..wanted {
        if g.players[player.index()].cards.knights == 0 {
            return;
        }
        let tiles: Vec<TileId> = g.deploy_tiles(player).into_iter().filter(|&t| !g.warded_against(t, player)).collect();
        let near = |t: TileId| goals.iter().map(|&(goal, bonus)| bonus - STEP_COST * hex_distance(g, t, goal)).max();
        let Some(tile) = best(&tiles, |&t| near(t)) else {
            return;
        };
        if g.apply(player, Action::Deploy { kind: UnitKind::Knight, tile }, rng).is_err() {
            return;
        }
    }
}

/// Casts the spell worth most to the bot, when it is free (a castle or a
/// scroll) and worth anything, or worth paying for.
fn cast_spell(g: &mut Game, player: PlayerId, style: Style, rng: &mut Rng) {
    let stakes = Stakes::new(g, player, style);
    let producing = |t: TileId| g.robber_at(t).is_none() && !g.blockaded(t) && g.enchantment_at(t).is_none();
    let mut options: Vec<(Spell, Vec<TileId>, i64)> = Vec::new();
    for t in g.board.tile_ids().filter(|&t| producing(t)) {
        let harm = stakes.harm(g, t);
        options.push((Spell::Frost, vec![t], harm));
        options.push((Spell::Bounty, vec![t], -harm));
        options.push((Spell::Luck, vec![t], -harm / 2));
    }
    let free: Vec<TileId> = g.board.tile_ids().filter(|&t| !g.occupied(t)).collect();
    if let Some(to) = best(&free, |&t| stakes.bandit_score(g, t)) {
        for r in &g.robbers {
            options.push((Spell::Lure, vec![r.tile, to], 2 * (stakes.harm(g, to) - stakes.harm(g, r.tile))));
        }
    }
    let mut resources = touched(g, player);
    resources.sort();
    resources.dedup();
    options.push((Spell::Windfall, Vec::new(), WINDFALL_PER_RESOURCE * resources.len() as i64));
    for t in g.board.tile_ids() {
        options.push((Spell::Ward, vec![t], ward_value(g, &stakes, t)));
    }

    let me = &g.players[player.index()];
    let castle = g.has_city(player);
    let worth_casting = |&(spell, ref tiles, value): &(Spell, Vec<TileId>, i64)| {
        let free = castle || me.scrolls.contains(&spell);
        value > 0 && (free || value >= PAID_SPELL_MIN) && g.can_cast(player, spell, tiles).is_ok()
    };
    let Some((spell, tiles, _)) = options.into_iter().filter(worth_casting).max_by_key(|o| o.2) else {
        return;
    };
    let _ = g.apply(player, Action::CastSpell { spell, tiles }, rng);
}

/// What a ward on `t` is worth to the bot: the bandits it fells (on the
/// bot's production or holding a structure) and the knights of rivals it
/// works against, more for those carrying relics or holding a structure.
fn ward_value(g: &Game, stakes: &Stakes, t: TileId) -> i64 {
    std::iter::once(t)
        .chain(g.board.tile(t).neighbors.iter().copied())
        .map(|z| {
            let bandit = g.robber_at(z).map_or(0, |_| stakes.own(g, z) + structure_bonus(g.structure_at(z)));
            let knight = g.unit_at(z).filter(|u| u.owner != stakes.me).map_or(0, |u| {
                let weight = stakes.weights[u.owner.index()];
                let holds = i64::from(g.structure_at(z).is_some());
                weight * WARD_KNIGHT * (1 + i64::from(u.relics) + holds)
            });
            bandit + knight
        })
        .sum()
}

/// How many of 36 rolls come up `number`.
fn pips(number: u8) -> u32 {
    6u32.saturating_sub(u32::from(number.abs_diff(7)))
}

/// The resources of the tiles `player`'s buildings touch.
fn touched(g: &Game, player: PlayerId) -> Vec<Resource> {
    g.board
        .vertices
        .iter()
        .filter(|v| g.building(v.id).is_some_and(|b| b.owner == player))
        .flat_map(|v| v.tiles.iter().map(|&t| g.board.tile(t).resource))
        .collect()
}

/// What a building on `v` collects in 36 rolls, more for resources not yet
/// `touched`.
fn corner_value(g: &Game, touched: &[Resource], v: VertexId) -> u32 {
    g.board
        .vertex(v)
        .tiles
        .iter()
        .map(|&t| {
            let tile = g.board.tile(t);
            pips(tile.number) + if touched.contains(&tile.resource) { 0 } else { NEW_RESOURCE_BONUS }
        })
        .sum()
}

/// An empty corner's worth for a new building, more where a town may later
/// rise; nothing when it is taken.
fn spot_value(g: &Game, touched: &[Resource], v: VertexId) -> u32 {
    if g.building(v).is_some() {
        return 0;
    }
    corner_value(g, touched, v) + if g.settlement_nearby(v) { 0 } else { UPGRADE_BONUS }
}

/// Where a road on `e` leads `player`: the best empty corner at its new end,
/// or (at half its worth) one side beyond. Nothing when both ends are
/// already reached.
fn road_value(g: &Game, player: PlayerId, touched: &[Resource], e: EdgeId) -> u32 {
    let reached = |v: VertexId| {
        g.building(v).is_some_and(|b| b.owner == player)
            || g.board.vertex(v).edges.iter().any(|&e| g.road(e) == Some(player))
    };
    g.board
        .edge(e)
        .vertices
        .iter()
        .copied()
        .filter(|&v| !reached(v) && g.building(v).is_none())
        .map(|v| {
            let beyond = g.board.vertex_neighbors(v).map(|w| spot_value(g, touched, w) / 2).max().unwrap_or(0);
            spot_value(g, touched, v).max(beyond)
        })
        .max()
        .unwrap_or(0)
}

/// The item scoring highest, the first of equals.
fn best<T: Copy, S: Ord>(items: &[T], score: impl Fn(&T) -> S) -> Option<T> {
    items.iter().copied().reduce(|a, b| if score(&b) > score(&a) { b } else { a })
}
