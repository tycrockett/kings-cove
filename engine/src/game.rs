//! The game as a deterministic state machine: [`Game::apply`] validates an
//! [`Action`] from a player and executes it, drawing any randomness from the
//! [`Rng`] its caller (the server) owns.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{
    balance,
    board::Board,
    rng::Rng,
    types::{EdgeId, PlayerId, Resource, Resources, RobberId, TileId, UnitId, VertexId},
};

/// One building, upgraded in place: Outpost → Town → City.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum BuildingKind {
    /// Every new building starts here: produces, worth nothing.
    Outpost,
    /// Produces, worth [`balance::TOWN_POINTS`], lets units deploy next to it.
    Town,
    /// Produces double, worth [`balance::CITY_POINTS`], lets units deploy next to it.
    City,
}

impl BuildingKind {
    pub fn points(self) -> u32 {
        match self {
            BuildingKind::Outpost => 0,
            BuildingKind::Town => balance::TOWN_POINTS,
            BuildingKind::City => balance::CITY_POINTS,
        }
    }

    /// Resources collected per matching adjacent tile.
    pub fn yield_per_tile(self) -> u32 {
        match self {
            BuildingKind::Outpost => balance::OUTPOST_YIELD,
            BuildingKind::Town => balance::TOWN_YIELD,
            BuildingKind::City => balance::CITY_YIELD,
        }
    }

    /// The level above, and what reaching it costs.
    pub fn upgrade(self) -> Option<(BuildingKind, Resources)> {
        match self {
            BuildingKind::Outpost => Some((BuildingKind::Town, balance::TOWN_UPGRADE_COST)),
            BuildingKind::Town => Some((BuildingKind::City, balance::CITY_UPGRADE_COST)),
            BuildingKind::City => None,
        }
    }

    /// Towns and cities: count toward robber capacity and deploy units.
    pub fn is_settlement(self) -> bool {
        self != BuildingKind::Outpost
    }

    pub fn name(self) -> &'static str {
        match self {
            BuildingKind::Outpost => "outpost",
            BuildingKind::Town => "town",
            BuildingKind::City => "city",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Building {
    pub owner: PlayerId,
    pub kind: BuildingKind,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UnitKind {
    /// Fights robbers and enemy knights.
    Knight,
}

impl UnitKind {
    pub const ALL: [UnitKind; 1] = [UnitKind::Knight];

    pub fn card_cost(self) -> Resources {
        match self {
            UnitKind::Knight => balance::KNIGHT_CARD_COST,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            UnitKind::Knight => "knight",
        }
    }
}

/// A spell a player with a city casts on their turn. All share the caster's
/// mana; each has its own price.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Spell {
    /// A wood or food tile produces nothing for its next rolls.
    Frost,
    /// A wood or food tile produces double for its next rolls.
    Bounty,
    /// A stone or iron tile produces nothing for its next rolls.
    Rockslide,
    /// A stone or iron tile produces double for its next rolls.
    Motherlode,
    /// No robber may enter or stay on a tile for some rounds.
    Ward,
    /// Moves a robber to any free tile.
    Lure,
    /// Two tiles trade numbers for their next rolls.
    Swap,
    /// The caster's knights are ready to act again this turn.
    Rally,
    /// The caster's next roll pays one of each resource their buildings touch.
    Windfall,
}

/// How long a spell on a tile lasts.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Lasting {
    /// Its charges are used one per roll of the tile's number.
    Rolls,
    /// Its charges are used one per round's end.
    Rounds,
}

impl Spell {
    pub const ALL: [Spell; 9] = [
        Spell::Frost,
        Spell::Bounty,
        Spell::Rockslide,
        Spell::Motherlode,
        Spell::Ward,
        Spell::Lure,
        Spell::Swap,
        Spell::Rally,
        Spell::Windfall,
    ];

    pub fn cost(self) -> Resources {
        match self {
            Spell::Frost => balance::FROST_COST,
            Spell::Bounty => balance::BOUNTY_COST,
            Spell::Rockslide => balance::ROCKSLIDE_COST,
            Spell::Motherlode => balance::MOTHERLODE_COST,
            Spell::Ward => balance::WARD_COST,
            Spell::Lure => balance::LURE_COST,
            Spell::Swap => balance::SWAP_COST,
            Spell::Rally => balance::RALLY_COST,
            Spell::Windfall => balance::WINDFALL_COST,
        }
    }

    /// How many tiles it is cast on: for Lure the robber's, then where it
    /// goes; for Swap the two that trade numbers.
    pub fn targets(self) -> usize {
        match self {
            Spell::Frost | Spell::Bounty | Spell::Rockslide | Spell::Motherlode | Spell::Ward => 1,
            Spell::Lure | Spell::Swap => 2,
            Spell::Rally | Spell::Windfall => 0,
        }
    }

    /// The tiles it may be cast on, by resource, for those that care.
    pub fn affects(self, resource: Resource) -> bool {
        match self {
            Spell::Frost | Spell::Bounty => matches!(resource, Resource::Wood | Resource::Food),
            Spell::Rockslide | Spell::Motherlode => matches!(resource, Resource::Stone | Resource::Iron),
            _ => true,
        }
    }

    /// Stops its tile producing.
    pub fn stops(self) -> bool {
        matches!(self, Spell::Frost | Spell::Rockslide)
    }

    /// Doubles its tile's production.
    pub fn doubles(self) -> bool {
        matches!(self, Spell::Bounty | Spell::Motherlode)
    }

    /// How it lingers on its tile, for those that do.
    pub fn lasting(self) -> Option<(Lasting, u32)> {
        match self {
            Spell::Frost | Spell::Bounty | Spell::Rockslide | Spell::Motherlode | Spell::Swap => {
                Some((Lasting::Rolls, balance::SPELL_CHARGES))
            }
            Spell::Ward => Some((Lasting::Rounds, balance::WARD_ROUNDS)),
            Spell::Lure | Spell::Rally | Spell::Windfall => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Spell::Frost => "frost",
            Spell::Bounty => "bounty",
            Spell::Rockslide => "rockslide",
            Spell::Motherlode => "motherlode",
            Spell::Ward => "ward",
            Spell::Lure => "lure",
            Spell::Swap => "swap",
            Spell::Rally => "rally",
            Spell::Windfall => "windfall",
        }
    }
}

/// A spell lingering on a tile (and, for Swap, its `partner`) for its
/// remaining `charges`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Enchantment {
    pub tile: TileId,
    pub spell: Spell,
    pub caster: PlayerId,
    pub charges: u32,
    #[serde(default)]
    pub partner: Option<TileId>,
}

impl Enchantment {
    pub fn touches(&self, tile: TileId) -> bool {
        self.tile == tile || self.partner == Some(tile)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unit {
    pub id: UnitId,
    pub owner: PlayerId,
    pub kind: UnitKind,
    pub tile: TileId,
    /// May move or attack this turn.
    pub ready: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Robber {
    pub id: RobberId,
    pub tile: TileId,
    /// Skips the end-of-round moves up to and including this round
    /// (set when a player relocates it on a 7).
    pub frozen_through_round: u32,
}

/// Unit cards in a player's hand.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cards {
    pub knights: u32,
}

impl Cards {
    pub fn get(&self, kind: UnitKind) -> u32 {
        match kind {
            UnitKind::Knight => self.knights,
        }
    }

    fn get_mut(&mut self, kind: UnitKind) -> &mut u32 {
        match kind {
            UnitKind::Knight => &mut self.knights,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    pub id: PlayerId,
    pub name: String,
    pub resources: Resources,
    pub cards: Cards,
    /// The round from which the player may cast a spell again.
    #[serde(default)]
    pub mana_ready_round: u32,
    /// Windfall pays out on the player's next roll.
    #[serde(default)]
    pub windfall: bool,
}

/// The active player's open offer: they give `give` for `want`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TradeOffer {
    pub from: PlayerId,
    /// One player, or anyone when `None`.
    pub to: Option<PlayerId>,
    pub give: Resources,
    pub want: Resources,
    pub declined: Vec<PlayerId>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetupPlacing {
    /// The player places their building (a town in the first pass, an
    /// outpost in the second).
    Building,
    /// The player places a road touching the building just placed.
    Road { from: VertexId },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    /// Snake-order initial placement: `step` indexes [`Game::setup_order`].
    Setup {
        step: usize,
        placing: SetupPlacing,
    },
    Playing,
    Finished {
        winner: PlayerId,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Target {
    Unit(UnitId),
    Robber(RobberId),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    SetupBuilding(VertexId),
    SetupRoad(EdgeId),
    Roll,
    BuildRoad(EdgeId),
    BuildOutpost(VertexId),
    /// Outpost → town.
    UpgradeTown(VertexId),
    /// Town → city.
    UpgradeCity(VertexId),
    BuyCard(UnitKind),
    Deploy {
        kind: UnitKind,
        tile: TileId,
    },
    MoveUnit {
        unit: UnitId,
        to: TileId,
    },
    Attack {
        unit: UnitId,
        target: Target,
    },
    RelocateRobber {
        robber: RobberId,
        to: TileId,
    },
    /// After a 7 with no robber on the board: put a new one on a free tile.
    PlaceRobber {
        to: TileId,
    },
    ProposeTrade {
        to: Option<PlayerId>,
        give: Resources,
        want: Resources,
    },
    CancelTrade,
    AcceptTrade,
    DeclineTrade,
    /// `balance::BANK_TRADE_RATE` of one resource for one of another, with
    /// no other player involved.
    TradeWithBank {
        give: Resource,
        get: Resource,
    },
    /// `tiles` as the spell's `targets()` asks.
    CastSpell {
        spell: Spell,
        tiles: Vec<TileId>,
    },
    EndTurn,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleError {
    GameOver,
    NotYourTurn,
    NotSetup,
    SetupInProgress,
    MustRollFirst,
    AlreadyRolled,
    CannotAfford,
    VertexTaken,
    EdgeTaken,
    TileOccupied,
    NotConnected,
    NotUpgradable,
    NoCard,
    NotNextToYourTown,
    NoSuchUnit,
    NotYourUnit,
    UnitExhausted,
    NotAdjacent,
    InvalidTarget,
    NoRobberRelocation,
    RobberToMove,
    NoSuchRobber,
    NoTrade,
    TradeAlreadyOpen,
    InvalidTrade,
    NotTradeRecipient,
    TraderCannotPay,
    NoSuchPlayer,
    SpellsLocked,
    ManaEmpty,
    SpellTarget,
    NoKnights,
    WindfallPending,
    Warded,
    SettlementTooClose,
}

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            RuleError::GameOver => "The game is over",
            RuleError::NotYourTurn => "It is not your turn",
            RuleError::NotSetup => "Initial placement is over",
            RuleError::SetupInProgress => "Finish initial placement first",
            RuleError::MustRollFirst => "Roll the dice first",
            RuleError::AlreadyRolled => "You already rolled this turn",
            RuleError::CannotAfford => "Not enough resources",
            RuleError::VertexTaken => "That spot already has a building",
            RuleError::EdgeTaken => "That edge already has a road",
            RuleError::TileOccupied => "That tile is occupied",
            RuleError::NotConnected => "Must connect to your road network",
            RuleError::NotUpgradable => "Only your own outposts and towns can be upgraded",
            RuleError::NoCard => "You have no such unit card",
            RuleError::NotNextToYourTown => "Units deploy next to your towns or cities",
            RuleError::NoSuchUnit => "No such unit",
            RuleError::NotYourUnit => "That unit is not yours",
            RuleError::UnitExhausted => "That unit cannot act until your next turn",
            RuleError::NotAdjacent => "Units only reach adjacent tiles",
            RuleError::InvalidTarget => "That unit cannot attack that",
            RuleError::NoRobberRelocation => "No robber to relocate",
            RuleError::RobberToMove => "Move a robber already on the board",
            RuleError::NoSuchRobber => "No such robber",
            RuleError::NoTrade => "No open trade",
            RuleError::TradeAlreadyOpen => "A trade is already open",
            RuleError::InvalidTrade => "A trade must give and ask for something",
            RuleError::NotTradeRecipient => "That trade is not offered to you",
            RuleError::TraderCannotPay => "The offering player no longer has those resources",
            RuleError::NoSuchPlayer => "No such player",
            RuleError::SpellsLocked => "Build a city to unlock spells",
            RuleError::ManaEmpty => "Your mana is still recharging",
            RuleError::SpellTarget => "That spell cannot be cast there",
            RuleError::NoKnights => "You have no knights on the board",
            RuleError::WindfallPending => "Windfall already waits for your next roll",
            RuleError::Warded => "A ward keeps robbers off that tile",
            RuleError::SettlementTooClose => "Towns and cities can't stand next to another town or city",
        })
    }
}

impl std::error::Error for RuleError {}

/// Something that happened: the history players read, and what clients animate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    TurnStarted {
        player: PlayerId,
        round: u32,
    },
    Rolled {
        player: PlayerId,
        dice: (u8, u8),
    },
    Produced {
        player: PlayerId,
        tile: TileId,
        resource: Resource,
        amount: u32,
    },
    ProductionBlocked {
        tile: TileId,
    },
    SpellCast {
        player: PlayerId,
        spell: Spell,
        tiles: Vec<TileId>,
    },
    /// A tile's number came up under frost or a rockslide: it produced nothing.
    ProductionStopped {
        tile: TileId,
        spell: Spell,
    },
    /// A lingering spell used its last roll or round.
    SpellFaded {
        tile: TileId,
        spell: Spell,
    },
    WindfallPaid {
        player: PlayerId,
        gained: Resources,
    },
    BuiltRoad {
        player: PlayerId,
        edge: EdgeId,
    },
    /// A new building (always an outpost, except the starting town).
    Built {
        player: PlayerId,
        vertex: VertexId,
        kind: BuildingKind,
    },
    Upgraded {
        player: PlayerId,
        vertex: VertexId,
        to: BuildingKind,
    },
    CardBought {
        player: PlayerId,
        kind: UnitKind,
    },
    Deployed {
        player: PlayerId,
        unit: UnitId,
        kind: UnitKind,
        tile: TileId,
    },
    UnitMoved {
        player: PlayerId,
        unit: UnitId,
        kind: UnitKind,
        from: TileId,
        to: TileId,
    },
    KnightKilledRobber {
        player: PlayerId,
        unit: UnitId,
        robber: RobberId,
        tile: TileId,
    },
    KnightsClashed {
        attacker: PlayerId,
        defender: PlayerId,
        tiles: (TileId, TileId),
    },
    RobberRelocated {
        player: PlayerId,
        robber: RobberId,
        from: TileId,
        to: TileId,
    },
    RobberWandered {
        robber: RobberId,
        from: TileId,
        to: TileId,
    },
    RobberSpawned {
        robber: RobberId,
        tile: TileId,
    },
    /// The roller of a 7 put a new robber on the board.
    RobberPlaced {
        player: PlayerId,
        robber: RobberId,
        tile: TileId,
    },
    TradeProposed {
        from: PlayerId,
        to: Option<PlayerId>,
        give: Resources,
        want: Resources,
    },
    TradeCancelled {
        from: PlayerId,
    },
    TradeDeclined {
        player: PlayerId,
    },
    Traded {
        from: PlayerId,
        to: PlayerId,
        give: Resources,
        want: Resources,
    },
    BankTraded {
        player: PlayerId,
        give: Resource,
        get: Resource,
    },
    RoundEnded {
        round: u32,
    },
    Won {
        player: PlayerId,
        points: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggedEvent {
    pub seq: u64,
    pub event: Event,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Game {
    pub board: Board,
    pub players: Vec<Player>,
    /// Indexed by vertex.
    pub buildings: Vec<Option<Building>>,
    /// Indexed by edge.
    pub roads: Vec<Option<PlayerId>>,
    pub units: Vec<Unit>,
    pub robbers: Vec<Robber>,
    pub phase: Phase,
    pub current: PlayerId,
    /// Rounds of play, from 1; 0 during setup.
    pub round: u32,
    /// This turn's roll, once rolled.
    pub dice: Option<(u8, u8)>,
    /// The active player rolled a 7 and may still relocate a robber.
    pub robber_relocation: bool,
    /// No robber appears at the end of a round before this one: a knight
    /// cleared one and the island gets a breather.
    #[serde(default)]
    pub robbers_quiet_until: u32,
    /// Spells on tiles, at most one per tile.
    #[serde(default)]
    pub enchantments: Vec<Enchantment>,
    /// Victory points that win this match.
    #[serde(default = "classic_goal")]
    pub victory_points: u32,
    pub trade: Option<TradeOffer>,
    pub next_piece_id: u32,
    /// The latest events, oldest first.
    pub events: Vec<LoggedEvent>,
    pub event_seq: u64,
}

impl Game {
    /// A new game for `names` played to `victory_points`, on a generated
    /// board sized for that goal.
    pub fn new(names: &[String], victory_points: u32, rng: &mut Rng) -> Game {
        let mut game = Game::with_board(names, Board::generate(balance::board_radius(victory_points), rng));
        game.victory_points = victory_points;
        // The island always has a robber, from the first turn on.
        let tiles: Vec<TileId> = game.board.tile_ids().collect();
        if let Some(tile) = rng.pick(&tiles) {
            let id = RobberId(game.next_id());
            game.robbers.push(Robber { id, tile, frozen_through_round: 0 });
        }
        game
    }

    pub fn with_board(names: &[String], board: Board) -> Game {
        assert!((balance::MIN_PLAYERS..=balance::MAX_PLAYERS).contains(&names.len()), "2–4 players");
        let players = names
            .iter()
            .enumerate()
            .map(|(i, name)| Player {
                id: PlayerId(i as u8),
                name: name.clone(),
                resources: balance::STARTING_RESOURCES,
                cards: Cards::default(),
                mana_ready_round: 0,
                windfall: false,
            })
            .collect();
        Game {
            buildings: vec![None; board.vertices.len()],
            roads: vec![None; board.edges.len()],
            board,
            players,
            units: Vec::new(),
            robbers: Vec::new(),
            phase: Phase::Setup { step: 0, placing: SetupPlacing::Building },
            current: PlayerId(0),
            round: 0,
            dice: None,
            robber_relocation: false,
            robbers_quiet_until: 0,
            enchantments: Vec::new(),
            victory_points: classic_goal(),
            trade: None,
            next_piece_id: 1,
            events: Vec::new(),
            event_seq: 0,
        }
    }

    // ----- Queries -----

    /// Turn order of initial placement: forward placing towns, then back
    /// placing outposts, so the last to place a town is first to place an outpost.
    pub fn setup_order(&self) -> Vec<PlayerId> {
        let n = self.players.len() as u8;
        (0..n).chain((0..n).rev()).map(PlayerId).collect()
    }

    /// What the player places at this setup step.
    pub fn setup_building_kind(&self, step: usize) -> BuildingKind {
        if step < self.players.len() { BuildingKind::Town } else { BuildingKind::Outpost }
    }

    pub fn player(&self, id: PlayerId) -> Option<&Player> {
        self.players.get(id.index())
    }

    pub fn building(&self, v: VertexId) -> Option<Building> {
        self.buildings.get(v.index()).copied().flatten()
    }

    pub fn road(&self, e: EdgeId) -> Option<PlayerId> {
        self.roads.get(e.index()).copied().flatten()
    }

    pub fn unit(&self, id: UnitId) -> Option<&Unit> {
        self.units.iter().find(|u| u.id == id)
    }

    pub fn unit_at(&self, tile: TileId) -> Option<&Unit> {
        self.units.iter().find(|u| u.tile == tile)
    }

    pub fn robber(&self, id: RobberId) -> Option<&Robber> {
        self.robbers.iter().find(|r| r.id == id)
    }

    pub fn robber_at(&self, tile: TileId) -> Option<&Robber> {
        self.robbers.iter().find(|r| r.tile == tile)
    }

    /// The spell on `tile`, including a swap with it as the partner.
    pub fn enchantment_at(&self, tile: TileId) -> Option<&Enchantment> {
        self.enchantments.iter().find(|e| e.touches(tile))
    }

    pub fn warded(&self, tile: TileId) -> bool {
        self.enchantments.iter().any(|e| e.spell == Spell::Ward && e.tile == tile)
    }

    /// A robber may move onto it: free and not warded.
    pub fn robber_may_enter(&self, tile: TileId) -> bool {
        tile.index() < self.board.tiles.len() && !self.occupied(tile) && !self.warded(tile)
    }

    /// Spells unlock with the player's first city.
    pub fn has_city(&self, player: PlayerId) -> bool {
        self.buildings.iter().flatten().any(|b| b.owner == player && b.kind == BuildingKind::City)
    }

    /// A unit or a robber stands on it.
    pub fn occupied(&self, tile: TileId) -> bool {
        self.unit_at(tile).is_some() || self.robber_at(tile).is_some()
    }

    pub fn points(&self, player: PlayerId) -> u32 {
        self.buildings.iter().flatten().filter(|b| b.owner == player).map(|b| b.kind.points()).sum()
    }

    /// Towns and cities of every player.
    pub fn total_settlements(&self) -> u32 {
        self.buildings.iter().flatten().filter(|b| b.kind.is_settlement()).count() as u32
    }

    pub fn robber_capacity(&self) -> u32 {
        self.total_settlements() / balance::SETTLEMENTS_PER_ROBBER
    }

    pub fn winner(&self) -> Option<PlayerId> {
        match self.phase {
            Phase::Finished { winner } => Some(winner),
            _ => None,
        }
    }

    /// The player whose input the game waits for (the active player).
    pub fn is_active(&self, player: PlayerId) -> bool {
        !matches!(self.phase, Phase::Finished { .. }) && self.current == player
    }

    fn touches_own_road(&self, player: PlayerId, v: VertexId) -> bool {
        self.board.vertex(v).edges.iter().any(|&e| self.road(e) == Some(player))
    }

    /// A road here would join the player's network.
    pub fn road_connects(&self, player: PlayerId, e: EdgeId) -> bool {
        self.board
            .edge(e)
            .vertices
            .iter()
            .any(|&v| self.building(v).is_some_and(|b| b.owner == player) || self.touches_own_road(player, v))
    }

    pub fn can_build_road(&self, player: PlayerId, e: EdgeId) -> Result<(), RuleError> {
        self.require_main_action(player)?;
        if e.index() >= self.roads.len() {
            return Err(RuleError::InvalidTarget);
        }
        if self.road(e).is_some() {
            return Err(RuleError::EdgeTaken);
        }
        if !self.road_connects(player, e) {
            return Err(RuleError::NotConnected);
        }
        self.require_funds(player, &balance::ROAD_COST)
    }

    /// A new building: always an outpost, on an empty vertex at the player's roads.
    pub fn can_build_outpost(&self, player: PlayerId, v: VertexId) -> Result<(), RuleError> {
        self.require_main_action(player)?;
        if v.index() >= self.buildings.len() {
            return Err(RuleError::InvalidTarget);
        }
        if self.building(v).is_some() {
            return Err(RuleError::VertexTaken);
        }
        if !self.touches_own_road(player, v) {
            return Err(RuleError::NotConnected);
        }
        self.require_funds(player, &balance::OUTPOST_COST)
    }

    /// Raising the player's building on `v` from the level below `to` to `to`.
    pub fn can_upgrade(&self, player: PlayerId, v: VertexId, to: BuildingKind) -> Result<(), RuleError> {
        self.require_main_action(player)?;
        let building = self.building(v).ok_or(RuleError::NotUpgradable)?;
        if building.owner != player {
            return Err(RuleError::NotUpgradable);
        }
        match building.kind.upgrade() {
            Some((next, cost)) if next == to => {
                if self.settlement_nearby(v) {
                    return Err(RuleError::SettlementTooClose);
                }
                self.require_funds(player, &cost)
            }
            _ => Err(RuleError::NotUpgradable),
        }
    }

    /// A town or city stands one side away from `v`: no town or city may
    /// stand on `v` then.
    pub fn settlement_nearby(&self, v: VertexId) -> bool {
        self.board.vertex_neighbors(v).any(|n| self.building(n).is_some_and(|b| b.kind.is_settlement()))
    }

    pub fn can_buy_card(&self, player: PlayerId, kind: UnitKind) -> Result<(), RuleError> {
        self.require_main_action(player)?;
        self.require_funds(player, &kind.card_cost())
    }

    /// Unoccupied tiles touching one of the player's towns or cities.
    pub fn deploy_tiles(&self, player: PlayerId) -> Vec<TileId> {
        let mut tiles: Vec<TileId> = self
            .board
            .vertices
            .iter()
            .filter(|v| self.building(v.id).is_some_and(|b| b.owner == player && b.kind.is_settlement()))
            .flat_map(|v| v.tiles.iter().copied())
            .filter(|&t| !self.occupied(t))
            .collect();
        tiles.sort();
        tiles.dedup();
        tiles
    }

    pub fn can_deploy(&self, player: PlayerId, kind: UnitKind, tile: TileId) -> Result<(), RuleError> {
        self.require_main_action(player)?;
        if self.players[player.index()].cards.get(kind) == 0 {
            return Err(RuleError::NoCard);
        }
        if tile.index() >= self.board.tiles.len() {
            return Err(RuleError::InvalidTarget);
        }
        if self.occupied(tile) {
            return Err(RuleError::TileOccupied);
        }
        if !self.deploy_tiles(player).contains(&tile) {
            return Err(RuleError::NotNextToYourTown);
        }
        Ok(())
    }

    fn ready_unit(&self, player: PlayerId, id: UnitId) -> Result<&Unit, RuleError> {
        self.require_main_action(player)?;
        let unit = self.unit(id).ok_or(RuleError::NoSuchUnit)?;
        if unit.owner != player {
            return Err(RuleError::NotYourUnit);
        }
        if !unit.ready {
            return Err(RuleError::UnitExhausted);
        }
        Ok(unit)
    }

    pub fn can_move(&self, player: PlayerId, id: UnitId, to: TileId) -> Result<(), RuleError> {
        let unit = self.ready_unit(player, id)?;
        if to.index() >= self.board.tiles.len() || !self.board.are_adjacent(unit.tile, to) {
            return Err(RuleError::NotAdjacent);
        }
        if self.occupied(to) {
            return Err(RuleError::TileOccupied);
        }
        Ok(())
    }

    /// Tiles the unit may move to now.
    pub fn move_targets(&self, player: PlayerId, id: UnitId) -> Vec<TileId> {
        let Some(unit) = self.unit(id) else {
            return Vec::new();
        };
        self.board.tile(unit.tile).neighbors.iter().copied().filter(|&t| self.can_move(player, id, t).is_ok()).collect()
    }

    pub fn can_attack(&self, player: PlayerId, id: UnitId, target: Target) -> Result<(), RuleError> {
        let unit = self.ready_unit(player, id)?;
        match target {
            Target::Robber(r) => {
                let robber = self.robber(r).ok_or(RuleError::NoSuchRobber)?;
                if !self.board.are_adjacent(unit.tile, robber.tile) {
                    return Err(RuleError::NotAdjacent);
                }
                Ok(())
            }
            Target::Unit(u) => {
                let other = self.unit(u).ok_or(RuleError::NoSuchUnit)?;
                if other.owner == player {
                    return Err(RuleError::InvalidTarget);
                }
                if !self.board.are_adjacent(unit.tile, other.tile) {
                    return Err(RuleError::NotAdjacent);
                }
                Ok(())
            }
        }
    }

    /// Everything the unit may attack now.
    pub fn attack_targets(&self, player: PlayerId, id: UnitId) -> Vec<Target> {
        let Some(unit) = self.unit(id) else {
            return Vec::new();
        };
        let near = &self.board.tile(unit.tile).neighbors;
        let candidates: Vec<Target> = self
            .robbers
            .iter()
            .filter(|r| near.contains(&r.tile))
            .map(|r| Target::Robber(r.id))
            .chain(self.units.iter().filter(|u| near.contains(&u.tile)).map(|u| Target::Unit(u.id)))
            .collect();
        candidates.into_iter().filter(|&t| self.can_attack(player, id, t).is_ok()).collect()
    }

    pub fn can_relocate_robber(&self, player: PlayerId, robber: RobberId, to: TileId) -> Result<(), RuleError> {
        self.require_main_action(player)?;
        if !self.robber_relocation {
            return Err(RuleError::NoRobberRelocation);
        }
        self.robber(robber).ok_or(RuleError::NoSuchRobber)?;
        if to.index() >= self.board.tiles.len() {
            return Err(RuleError::InvalidTarget);
        }
        if self.occupied(to) {
            return Err(RuleError::TileOccupied);
        }
        if self.warded(to) {
            return Err(RuleError::Warded);
        }
        Ok(())
    }

    /// After a 7 with no robber on the board, the roller places a new one.
    pub fn can_place_robber(&self, player: PlayerId, to: TileId) -> Result<(), RuleError> {
        self.require_main_action(player)?;
        if !self.robber_relocation {
            return Err(RuleError::NoRobberRelocation);
        }
        if !self.robbers.is_empty() {
            return Err(RuleError::RobberToMove);
        }
        if to.index() >= self.board.tiles.len() {
            return Err(RuleError::InvalidTarget);
        }
        if self.occupied(to) {
            return Err(RuleError::TileOccupied);
        }
        if self.warded(to) {
            return Err(RuleError::Warded);
        }
        Ok(())
    }

    /// Setup: where the player may place now.
    pub fn setup_vertices(&self, player: PlayerId) -> Vec<VertexId> {
        match self.phase {
            Phase::Setup { step, placing: SetupPlacing::Building } if self.current == player => {
                let town = self.setup_building_kind(step).is_settlement();
                self.board
                    .vertices
                    .iter()
                    .map(|v| v.id)
                    .filter(|&v| self.building(v).is_none() && !(town && self.settlement_nearby(v)))
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    pub fn setup_edges(&self, player: PlayerId) -> Vec<EdgeId> {
        match self.phase {
            Phase::Setup { placing: SetupPlacing::Road { from }, .. } if self.current == player => {
                self.board.vertex(from).edges.iter().copied().filter(|&e| self.road(e).is_none()).collect()
            }
            _ => Vec::new(),
        }
    }

    fn require_main_action(&self, player: PlayerId) -> Result<(), RuleError> {
        self.require_turn(player)?;
        if self.dice.is_none() {
            return Err(RuleError::MustRollFirst);
        }
        Ok(())
    }

    fn require_turn(&self, player: PlayerId) -> Result<(), RuleError> {
        match self.phase {
            Phase::Finished { .. } => Err(RuleError::GameOver),
            Phase::Setup { .. } => Err(RuleError::SetupInProgress),
            Phase::Playing if self.current != player => Err(RuleError::NotYourTurn),
            Phase::Playing => Ok(()),
        }
    }

    fn require_funds(&self, player: PlayerId, cost: &Resources) -> Result<(), RuleError> {
        if self.players[player.index()].resources.covers(cost) { Ok(()) } else { Err(RuleError::CannotAfford) }
    }

    /// Casting `spell` on `tiles` (as many as it targets): needs a city,
    /// full mana, the spell's price and fitting targets. A lingering spell
    /// replaces any already on its tiles.
    pub fn can_cast(&self, player: PlayerId, spell: Spell, tiles: &[TileId]) -> Result<(), RuleError> {
        self.require_main_action(player)?;
        if !self.has_city(player) {
            return Err(RuleError::SpellsLocked);
        }
        if self.round < self.players[player.index()].mana_ready_round {
            return Err(RuleError::ManaEmpty);
        }
        if tiles.len() != spell.targets() || tiles.iter().any(|t| t.index() >= self.board.tiles.len()) {
            return Err(RuleError::InvalidTarget);
        }
        if !tiles.iter().all(|&t| spell.affects(self.board.tile(t).resource)) {
            return Err(RuleError::SpellTarget);
        }
        match spell {
            Spell::Lure => {
                self.robber_at(tiles[0]).ok_or(RuleError::NoSuchRobber)?;
                if self.occupied(tiles[1]) {
                    return Err(RuleError::TileOccupied);
                }
                if self.warded(tiles[1]) {
                    return Err(RuleError::Warded);
                }
            }
            Spell::Swap => {
                if self.board.tile(tiles[0]).number == self.board.tile(tiles[1]).number {
                    return Err(RuleError::SpellTarget);
                }
            }
            Spell::Rally if !self.units.iter().any(|u| u.owner == player) => return Err(RuleError::NoKnights),
            Spell::Windfall if self.players[player.index()].windfall => return Err(RuleError::WindfallPending),
            _ => {}
        }
        self.require_funds(player, &spell.cost())
    }

    fn cast(&mut self, player: PlayerId, spell: Spell, tiles: &[TileId], rng: &mut Rng) -> Result<(), RuleError> {
        self.can_cast(player, spell, tiles)?;
        self.pay(player, &spell.cost());
        self.players[player.index()].mana_ready_round = self.round + balance::MANA_RECHARGE_ROUNDS;
        self.log(Event::SpellCast { player, spell, tiles: tiles.to_vec() });
        if let Some((_, charges)) = spell.lasting() {
            for &t in tiles {
                while let Some(i) = self.enchantments.iter().position(|e| e.touches(t)) {
                    self.lift(i);
                }
            }
            let partner = (spell == Spell::Swap).then(|| tiles[1]);
            if let Some(b) = partner {
                let a = tiles[0];
                let (na, nb) = (self.board.tile(a).number, self.board.tile(b).number);
                self.board.tiles[a.index()].number = nb;
                self.board.tiles[b.index()].number = na;
            }
            self.enchantments.push(Enchantment { tile: tiles[0], spell, caster: player, charges, partner });
        }
        match spell {
            Spell::Ward => {
                // A robber standing there is pushed out.
                let t = tiles[0];
                if let Some(i) = self.robbers.iter().position(|r| r.tile == t) {
                    let near: Vec<TileId> =
                        self.board.tile(t).neighbors.iter().copied().filter(|&n| self.robber_may_enter(n)).collect();
                    let anywhere: Vec<TileId> = self.board.tile_ids().filter(|&n| self.robber_may_enter(n)).collect();
                    if let Some(to) = rng.pick(&near).or_else(|| rng.pick(&anywhere)) {
                        self.robbers[i].tile = to;
                        let robber = self.robbers[i].id;
                        self.log(Event::RobberWandered { robber, from: t, to });
                    }
                }
            }
            Spell::Lure => {
                let (from, to) = (tiles[0], tiles[1]);
                let round = self.round;
                let r = self.robbers.iter_mut().find(|r| r.tile == from).expect("checked");
                r.tile = to;
                r.frozen_through_round = round;
                let robber = r.id;
                self.log(Event::RobberRelocated { player, robber, from, to });
            }
            Spell::Rally => {
                for u in self.units.iter_mut().filter(|u| u.owner == player) {
                    u.ready = true;
                }
            }
            Spell::Windfall => self.players[player.index()].windfall = true,
            _ => {}
        }
        Ok(())
    }

    /// Ends the spell at `index`: a swap gives the tiles their numbers back.
    fn lift(&mut self, index: usize) {
        let e = self.enchantments.remove(index);
        if let Some(b) = e.partner {
            let (na, nb) = (self.board.tile(e.tile).number, self.board.tile(b).number);
            self.board.tiles[e.tile.index()].number = nb;
            self.board.tiles[b.index()].number = na;
        }
        self.log(Event::SpellFaded { tile: e.tile, spell: e.spell });
    }

    /// Trading `balance::BANK_TRADE_RATE` of `give` for one `get` with the bank.
    pub fn can_trade_with_bank(&self, player: PlayerId, give: Resource, get: Resource) -> Result<(), RuleError> {
        self.require_main_action(player)?;
        if give == get {
            return Err(RuleError::InvalidTrade);
        }
        let mut cost = Resources::ZERO;
        *cost.get_mut(give) = balance::BANK_TRADE_RATE;
        self.require_funds(player, &cost)
    }

    // ----- Actions -----

    /// Validates `action` from `player` and executes it, or leaves the game
    /// untouched and says why not.
    pub fn apply(&mut self, player: PlayerId, action: Action, rng: &mut Rng) -> Result<(), RuleError> {
        if player.index() >= self.players.len() {
            return Err(RuleError::NoSuchPlayer);
        }
        match action {
            Action::SetupBuilding(v) => self.setup_building(player, v),
            Action::SetupRoad(e) => self.setup_road(player, e),
            Action::Roll => self.roll(player, rng),
            Action::BuildRoad(e) => {
                self.can_build_road(player, e)?;
                self.pay(player, &balance::ROAD_COST);
                self.roads[e.index()] = Some(player);
                self.log(Event::BuiltRoad { player, edge: e });
                Ok(())
            }
            Action::BuildOutpost(v) => self.build_outpost(player, v),
            Action::UpgradeTown(v) => self.upgrade(player, v, BuildingKind::Town),
            Action::UpgradeCity(v) => self.upgrade(player, v, BuildingKind::City),
            Action::CastSpell { spell, tiles } => self.cast(player, spell, &tiles, rng),
            Action::BuyCard(kind) => {
                self.can_buy_card(player, kind)?;
                self.pay(player, &kind.card_cost());
                *self.players[player.index()].cards.get_mut(kind) += 1;
                self.log(Event::CardBought { player, kind });
                Ok(())
            }
            Action::Deploy { kind, tile } => {
                self.can_deploy(player, kind, tile)?;
                *self.players[player.index()].cards.get_mut(kind) -= 1;
                let id = UnitId(self.next_id());
                self.units.push(Unit { id, owner: player, kind, tile, ready: false });
                self.log(Event::Deployed { player, unit: id, kind, tile });
                Ok(())
            }
            Action::MoveUnit { unit, to } => {
                self.can_move(player, unit, to)?;
                let u = self.units.iter_mut().find(|u| u.id == unit).expect("checked");
                let from = u.tile;
                u.tile = to;
                u.ready = false;
                let kind = u.kind;
                self.log(Event::UnitMoved { player, unit, kind, from, to });
                Ok(())
            }
            Action::Attack { unit, target } => self.attack(player, unit, target),
            Action::RelocateRobber { robber, to } => {
                self.can_relocate_robber(player, robber, to)?;
                let round = self.round;
                let r = self.robbers.iter_mut().find(|r| r.id == robber).expect("checked");
                let from = r.tile;
                r.tile = to;
                r.frozen_through_round = round;
                self.robber_relocation = false;
                self.log(Event::RobberRelocated { player, robber, from, to });
                Ok(())
            }
            Action::PlaceRobber { to } => {
                self.can_place_robber(player, to)?;
                let robber = RobberId(self.next_id());
                self.robbers.push(Robber { id: robber, tile: to, frozen_through_round: self.round });
                self.robber_relocation = false;
                self.log(Event::RobberPlaced { player, robber, tile: to });
                Ok(())
            }
            Action::ProposeTrade { to, give, want } => {
                self.require_main_action(player)?;
                if self.trade.is_some() {
                    return Err(RuleError::TradeAlreadyOpen);
                }
                if give.is_empty() || want.is_empty() || to == Some(player) {
                    return Err(RuleError::InvalidTrade);
                }
                if to.is_some_and(|p| p.index() >= self.players.len()) {
                    return Err(RuleError::NoSuchPlayer);
                }
                self.require_funds(player, &give)?;
                self.trade = Some(TradeOffer { from: player, to, give, want, declined: Vec::new() });
                self.log(Event::TradeProposed { from: player, to, give, want });
                Ok(())
            }
            Action::TradeWithBank { give, get } => {
                self.can_trade_with_bank(player, give, get)?;
                let r = &mut self.players[player.index()].resources;
                *r.get_mut(give) -= balance::BANK_TRADE_RATE;
                *r.get_mut(get) += 1;
                self.log(Event::BankTraded { player, give, get });
                Ok(())
            }
            Action::CancelTrade => {
                self.require_turn(player)?;
                if self.trade.take().is_none() {
                    return Err(RuleError::NoTrade);
                }
                self.log(Event::TradeCancelled { from: player });
                Ok(())
            }
            Action::AcceptTrade => {
                let offer = self.trade_for(player)?.clone();
                if !self.players[offer.from.index()].resources.covers(&offer.give) {
                    return Err(RuleError::TraderCannotPay);
                }
                self.require_funds(player, &offer.want)?;
                let (from, to) = (offer.from.index(), player.index());
                self.players[from].resources.remove(&offer.give);
                self.players[to].resources.add(&offer.give);
                self.players[to].resources.remove(&offer.want);
                self.players[from].resources.add(&offer.want);
                self.trade = None;
                self.log(Event::Traded { from: offer.from, to: player, give: offer.give, want: offer.want });
                Ok(())
            }
            Action::DeclineTrade => {
                self.trade_for(player)?;
                let others = self.players.len() - 1;
                let offer = self.trade.as_mut().expect("checked");
                offer.declined.push(player);
                if offer.to.is_some() || offer.declined.len() >= others {
                    self.trade = None;
                }
                self.log(Event::TradeDeclined { player });
                Ok(())
            }
            Action::EndTurn => {
                self.require_main_action(player)?;
                self.end_turn(rng);
                Ok(())
            }
        }
    }

    /// The open offer `player` may answer.
    fn trade_for(&self, player: PlayerId) -> Result<&TradeOffer, RuleError> {
        if matches!(self.phase, Phase::Finished { .. }) {
            return Err(RuleError::GameOver);
        }
        let offer = self.trade.as_ref().ok_or(RuleError::NoTrade)?;
        if offer.from == player || offer.to.is_some_and(|p| p != player) || offer.declined.contains(&player) {
            return Err(RuleError::NotTradeRecipient);
        }
        Ok(offer)
    }

    fn setup_building(&mut self, player: PlayerId, v: VertexId) -> Result<(), RuleError> {
        let Phase::Setup { step, placing } = self.phase else {
            return Err(RuleError::NotSetup);
        };
        if self.current != player {
            return Err(RuleError::NotYourTurn);
        }
        if placing != SetupPlacing::Building {
            return Err(RuleError::InvalidTarget);
        }
        if v.index() >= self.buildings.len() {
            return Err(RuleError::InvalidTarget);
        }
        if self.building(v).is_some() {
            return Err(RuleError::VertexTaken);
        }
        let kind = self.setup_building_kind(step);
        if kind.is_settlement() && self.settlement_nearby(v) {
            return Err(RuleError::SettlementTooClose);
        }
        self.buildings[v.index()] = Some(Building { owner: player, kind });
        self.phase = Phase::Setup { step, placing: SetupPlacing::Road { from: v } };
        self.log(Event::Built { player, vertex: v, kind });
        Ok(())
    }

    fn setup_road(&mut self, player: PlayerId, e: EdgeId) -> Result<(), RuleError> {
        let Phase::Setup { step, placing } = self.phase else {
            return Err(RuleError::NotSetup);
        };
        if self.current != player {
            return Err(RuleError::NotYourTurn);
        }
        let SetupPlacing::Road { from } = placing else {
            return Err(RuleError::InvalidTarget);
        };
        if e.index() >= self.roads.len() {
            return Err(RuleError::InvalidTarget);
        }
        if self.road(e).is_some() {
            return Err(RuleError::EdgeTaken);
        }
        if !self.board.edge(e).vertices.contains(&from) {
            return Err(RuleError::NotConnected);
        }
        self.roads[e.index()] = Some(player);
        self.log(Event::BuiltRoad { player, edge: e });
        let order = self.setup_order();
        let next = step + 1;
        if next < order.len() {
            self.phase = Phase::Setup { step: next, placing: SetupPlacing::Building };
            self.current = order[next];
        } else {
            self.phase = Phase::Playing;
            self.round = 1;
            self.begin_turn(PlayerId(0));
        }
        Ok(())
    }

    fn roll(&mut self, player: PlayerId, rng: &mut Rng) -> Result<(), RuleError> {
        self.require_turn(player)?;
        if self.dice.is_some() {
            return Err(RuleError::AlreadyRolled);
        }
        let dice = (rng.die(), rng.die());
        self.dice = Some(dice);
        self.log(Event::Rolled { player, dice });
        let total = dice.0 + dice.1;
        if total == balance::ROBBER_ROLL {
            // Move a robber, or place one when the board has none.
            self.robber_relocation = true;
            self.pay_windfall(player);
            return Ok(());
        }
        self.produce(total);
        self.pay_windfall(player);
        Ok(())
    }

    /// A pending windfall: one of each resource the player's buildings touch.
    fn pay_windfall(&mut self, player: PlayerId) {
        if !std::mem::take(&mut self.players[player.index()].windfall) {
            return;
        }
        let mut gained = Resources::ZERO;
        for (v, b) in self.buildings.iter().enumerate() {
            if b.is_some_and(|b| b.owner == player) {
                for &t in &self.board.vertices[v].tiles {
                    *gained.get_mut(self.board.tile(t).resource) = 1;
                }
            }
        }
        let r = &mut self.players[player.index()].resources;
        for res in Resource::ALL {
            *r.get_mut(res) += gained.get(res);
        }
        self.log(Event::WindfallPaid { player, gained });
    }

    /// Every building on a tile showing `number` collects its resource,
    /// unless a robber stands there. Frost and rockslide stop it; bounty and
    /// motherlode double it. Each roll of the number uses one of the
    /// tile's spell charges.
    pub fn produce(&mut self, number: u8) {
        let tiles: Vec<TileId> = self.board.tiles.iter().filter(|t| t.number == number).map(|t| t.id).collect();
        for tile in tiles {
            let spell = self.enchantment_at(tile).map(|e| e.spell);
            if self.robber_at(tile).is_some() {
                self.log(Event::ProductionBlocked { tile });
            } else if let Some(spell) = spell.filter(|s| s.stops()) {
                self.log(Event::ProductionStopped { tile, spell });
            } else {
                let factor = if spell.is_some_and(Spell::doubles) { 2 } else { 1 };
                let resource = self.board.tile(tile).resource;
                let mut gains = vec![0u32; self.players.len()];
                for &v in &self.board.tile(tile).vertices {
                    if let Some(b) = self.building(v) {
                        gains[b.owner.index()] += b.kind.yield_per_tile() * factor;
                    }
                }
                for (i, amount) in gains.into_iter().enumerate() {
                    if amount > 0 {
                        *self.players[i].resources.get_mut(resource) += amount;
                        self.log(Event::Produced { player: PlayerId(i as u8), tile, resource, amount });
                    }
                }
            }
            self.spend_charge(tile, Lasting::Rolls);
        }
    }

    /// A spell on `tile` lasting by `lasting` used a charge; on its last it
    /// fades.
    fn spend_charge(&mut self, tile: TileId, lasting: Lasting) {
        let found = self
            .enchantments
            .iter()
            .position(|e| e.touches(tile) && e.spell.lasting().is_some_and(|(l, _)| l == lasting));
        let Some(i) = found else { return };
        self.enchantments[i].charges -= 1;
        if self.enchantments[i].charges == 0 {
            self.lift(i);
        }
    }

    fn build_outpost(&mut self, player: PlayerId, v: VertexId) -> Result<(), RuleError> {
        self.can_build_outpost(player, v)?;
        self.pay(player, &balance::OUTPOST_COST);
        self.buildings[v.index()] = Some(Building { owner: player, kind: BuildingKind::Outpost });
        self.log(Event::Built { player, vertex: v, kind: BuildingKind::Outpost });
        Ok(())
    }

    fn upgrade(&mut self, player: PlayerId, v: VertexId, to: BuildingKind) -> Result<(), RuleError> {
        self.can_upgrade(player, v, to)?;
        let (_, cost) = self.building(v).and_then(|b| b.kind.upgrade()).expect("checked");
        self.pay(player, &cost);
        self.buildings[v.index()] = Some(Building { owner: player, kind: to });
        self.log(Event::Upgraded { player, vertex: v, to });
        self.check_victory(player);
        Ok(())
    }

    fn attack(&mut self, player: PlayerId, id: UnitId, target: Target) -> Result<(), RuleError> {
        self.can_attack(player, id, target)?;
        let attacker = self.unit(id).expect("checked").clone();
        match target {
            Target::Robber(r) => {
                // The knight falls with the robber it brings down.
                let tile = self.robber(r).expect("checked").tile;
                self.robbers.retain(|x| x.id != r);
                self.units.retain(|x| x.id != id);
                self.robbers_quiet_until = self.round + balance::ROBBER_RESPAWN_ROUNDS;
                self.log(Event::KnightKilledRobber { player, unit: id, robber: r, tile });
            }
            Target::Unit(u) => {
                let defender = self.unit(u).expect("checked").clone();
                self.units.retain(|x| x.id != u && x.id != id);
                self.log(Event::KnightsClashed {
                    attacker: player,
                    defender: defender.owner,
                    tiles: (attacker.tile, defender.tile),
                });
            }
        }
        Ok(())
    }

    fn end_turn(&mut self, rng: &mut Rng) {
        let next = (self.current.index() + 1) % self.players.len();
        if next == 0 {
            self.end_round(rng);
            self.round += 1;
        }
        self.begin_turn(PlayerId(next as u8));
    }

    fn begin_turn(&mut self, player: PlayerId) {
        self.current = player;
        self.dice = None;
        self.robber_relocation = false;
        self.trade = None;
        for unit in self.units.iter_mut().filter(|u| u.owner == player) {
            unit.ready = true;
        }
        self.log(Event::TurnStarted { player, round: self.round });
    }

    /// Robbers wander, then one may spawn if the board is below capacity.
    pub fn end_round(&mut self, rng: &mut Rng) {
        let round = self.round;
        for i in 0..self.robbers.len() {
            if self.robbers[i].frozen_through_round >= round {
                continue;
            }
            let from = self.robbers[i].tile;
            let free: Vec<TileId> =
                self.board.tile(from).neighbors.iter().copied().filter(|&t| self.robber_may_enter(t)).collect();
            if let Some(to) = rng.pick(&free) {
                self.robbers[i].tile = to;
                let robber = self.robbers[i].id;
                self.log(Event::RobberWandered { robber, from, to });
            }
        }
        // The island keeps its minimum of robbers, and may gain more while
        // below capacity; none appear during the breather after a kill.
        let count = self.robbers.len() as u32;
        let (num, den) = balance::ROBBER_SPAWN_CHANCE;
        let spawn = round >= self.robbers_quiet_until
            && (count < balance::MIN_ROBBERS || (count < self.robber_capacity() && rng.chance(num, den)));
        if spawn {
            let free: Vec<TileId> = self.board.tile_ids().filter(|&t| self.robber_may_enter(t)).collect();
            if let Some(tile) = rng.pick(&free) {
                let id = RobberId(self.next_id());
                self.robbers.push(Robber { id, tile, frozen_through_round: 0 });
                self.log(Event::RobberSpawned { robber: id, tile });
            }
        }
        // Wards last a number of rounds.
        let wards: Vec<TileId> = self.enchantments.iter().filter(|e| e.spell == Spell::Ward).map(|e| e.tile).collect();
        for t in wards {
            self.spend_charge(t, Lasting::Rounds);
        }
        self.log(Event::RoundEnded { round });
    }

    fn check_victory(&mut self, player: PlayerId) {
        let points = self.points(player);
        if points >= self.victory_points {
            self.phase = Phase::Finished { winner: player };
            self.trade = None;
            self.log(Event::Won { player, points });
        }
    }

    fn pay(&mut self, player: PlayerId, cost: &Resources) {
        self.players[player.index()].resources.remove(cost);
    }

    fn next_id(&mut self) -> u32 {
        let id = self.next_piece_id;
        self.next_piece_id += 1;
        id
    }

    fn log(&mut self, event: Event) {
        self.event_seq += 1;
        self.events.push(LoggedEvent { seq: self.event_seq, event });
        if self.events.len() > balance::EVENT_HISTORY {
            let excess = self.events.len() - balance::EVENT_HISTORY;
            self.events.drain(..excess);
        }
    }
}

/// The goal of games saved before goals could be chosen.
fn classic_goal() -> u32 {
    balance::VICTORY_POINT_GOALS[0]
}
