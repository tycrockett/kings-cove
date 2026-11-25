use kings_cove_engine::{
    Action, Board, Building, BuildingKind, Event, Game, Item, ItemKind, Phase, PlayerId, Resource, Resources, Rng,
    Robber, RobberId, RuleError, SetupPlacing, Spell, Target, TileId, Unit, UnitId, UnitKind, VertexId, balance,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn names(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("P{i}")).collect()
}

/// A game in play: setup skipped, player 0 to act, dice already rolled
/// (a 3), nobody holding anything.
fn playing(n: usize) -> Game {
    let mut g = Game::with_board(&names(n), Board::hexagon(2));
    g.phase = Phase::Playing;
    g.round = 1;
    g.dice = Some((1, 2));
    for p in &mut g.players {
        p.resources = Resources::ZERO;
    }
    g
}

fn rich() -> Resources {
    Resources { wood: 50, stone: 50, food: 50, iron: 50 }
}

fn put(g: &mut Game, v: VertexId, owner: PlayerId, kind: BuildingKind) {
    g.buildings[v.index()] = Some(Building { owner, kind });
}

fn unit(g: &mut Game, id: u32, owner: PlayerId, kind: UnitKind, tile: TileId) -> UnitId {
    g.units.push(Unit { id: UnitId(id), owner, kind, tile, ready: true, relics: 0 });
    UnitId(id)
}

fn robber(g: &mut Game, id: u32, tile: TileId) -> RobberId {
    g.robbers.push(Robber { id: RobberId(id), tile, frozen_through_round: 0 });
    RobberId(id)
}

fn rng() -> Rng {
    Rng::new(42)
}

/// The centre tile, whose neighbours are all on the board.
fn centre(g: &Game) -> TileId {
    g.board.tiles.iter().find(|t| t.q == 0 && t.r == 0).unwrap().id
}

// ----- Board topology -----

#[test]
fn hexagon_shares_vertices_and_edges_between_tiles() {
    let b = Board::hexagon(2);
    assert_eq!((b.tiles.len(), b.vertices.len(), b.edges.len()), (19, 54, 72));
    for v in &b.vertices {
        assert!((1..=3).contains(&v.tiles.len()));
        assert!((2..=3).contains(&v.edges.len()));
    }
    for e in &b.edges {
        assert!((1..=2).contains(&e.tiles.len()));
    }
    // Neighbouring tiles share exactly two vertices and one edge.
    for t in &b.tiles {
        for &n in &t.neighbors {
            let other = b.tile(n);
            assert_eq!(t.vertices.iter().filter(|v| other.vertices.contains(v)).count(), 2);
            assert_eq!(t.edges.iter().filter(|e| other.edges.contains(e)).count(), 1);
        }
        for (i, e) in t.edges.iter().enumerate() {
            let mut ends = b.edge(*e).vertices;
            ends.sort();
            let mut expected = [t.vertices[i], t.vertices[(i + 1) % 6]];
            expected.sort();
            assert_eq!(ends, expected);
        }
    }
    assert_eq!(b.tile(centre_of(&b)).neighbors.len(), 6);
}

fn centre_of(b: &Board) -> TileId {
    b.tiles.iter().find(|t| t.q == 0 && t.r == 0).unwrap().id
}

#[test]
fn ids_are_stable_and_generation_is_seeded_and_balanced() {
    assert_eq!(Board::hexagon(2), Board::hexagon(2));
    let a = Board::generate(2, &mut Rng::new(9));
    assert_eq!(a, Board::generate(2, &mut Rng::new(9)));
    assert!(a.numbers_balanced());
    for seed in 0..20 {
        let b = Board::generate(2, &mut Rng::new(seed));
        assert!(b.numbers_balanced());
        assert!(b.tiles.iter().all(|t| t.number != 7 && (2..=12).contains(&t.number)));
        for (resource, count) in balance::TILE_RESOURCES {
            assert_eq!(b.tiles.iter().filter(|t| t.resource == resource).count(), count);
        }
    }
}

#[test]
fn each_map_size_plays_on_its_own_balanced_island() {
    for (size, tiles) in [(balance::MapSize::Small, 19), (balance::MapSize::Medium, 37), (balance::MapSize::Large, 61)] {
        for seed in 0..5 {
            let g = Game::new(&names(3), 10, size, &mut Rng::new(seed));
            assert_eq!(g.board.tiles.len(), tiles);
            assert!(g.board.numbers_balanced());
            assert!(g.board.tiles.iter().all(|t| t.number != 7 && (2..=12).contains(&t.number)));
            // Each resource keeps its share of the island.
            for (resource, per19) in balance::TILE_RESOURCES {
                let n = g.board.tiles.iter().filter(|t| t.resource == resource).count();
                assert!(n.abs_diff(per19 * tiles / 19) <= 2, "{resource:?}: {n} of {tiles}");
            }
        }
    }
    // The match ends at its own goal.
    let mut g = playing(2);
    g.victory_points = 20;
    let mut r = rng();
    g.players[0].resources = rich();
    for i in 0..9u16 {
        put(&mut g, VertexId(i * 6), P0, BuildingKind::City);
    }
    let v =
        g.board.vertices.iter().map(|v| v.id).find(|&v| g.building(v).is_none() && !g.settlement_nearby(v)).unwrap();
    put(&mut g, v, P0, BuildingKind::Town);
    assert_eq!(g.points(P0), 19);
    g.apply(P0, Action::UpgradeCity(v), &mut r).unwrap();
    assert_eq!(g.winner(), Some(P0));
}

#[test]
fn towns_and_cities_never_stand_side_by_side() {
    let mut g = playing(2);
    let mut r = rng();
    g.players[0].resources = rich();
    let v = VertexId(10);
    let next = g.board.vertex_neighbors(v).next().unwrap();
    put(&mut g, v, P1, BuildingKind::Town);
    put(&mut g, next, P0, BuildingKind::Outpost);
    assert_eq!(g.apply(P0, Action::UpgradeTown(next), &mut r), Err(RuleError::SettlementTooClose));
    // Outposts may sit next to anything; once the town is razed the way is clear.
    g.buildings[v.index()] = Some(Building { owner: P1, kind: BuildingKind::Outpost });
    g.apply(P0, Action::UpgradeTown(next), &mut r).unwrap();

    // Starting towns keep their distance too.
    let mut g = Game::with_board(&names(2), Board::hexagon(2));
    g.apply(P0, Action::SetupBuilding(v), &mut r).unwrap();
    let e = g.setup_edges(P0)[0];
    g.apply(P0, Action::SetupRoad(e), &mut r).unwrap();
    assert!(!g.setup_vertices(P1).contains(&next));
    assert_eq!(g.apply(P1, Action::SetupBuilding(next), &mut r), Err(RuleError::SettlementTooClose));
}

// ----- Setup -----

#[test]
fn setup_runs_in_snake_order_with_a_town_then_an_outpost_each() {
    let mut g = Game::with_board(&names(3), Board::hexagon(2));
    let mut r = rng();
    let order = g.setup_order();
    assert_eq!(order, [0, 1, 2, 2, 1, 0].map(PlayerId));
    let spots = [0u16, 10, 20, 30, 40, 50];
    for (step, &player) in order.iter().enumerate() {
        assert_eq!(g.current, player);
        let other = PlayerId((player.0 + 1) % 3);
        assert_eq!(g.apply(other, Action::SetupBuilding(VertexId(spots[step])), &mut r), Err(RuleError::NotYourTurn));
        let v = VertexId(spots[step]);
        g.apply(player, Action::SetupBuilding(v), &mut r).unwrap();
        let expected = if step < 3 { BuildingKind::Town } else { BuildingKind::Outpost };
        assert_eq!(g.building(v), Some(Building { owner: player, kind: expected }));
        // The road must touch the building just placed.
        let far = g.board.edges.iter().find(|e| !e.vertices.contains(&v) && g.road(e.id).is_none()).unwrap().id;
        assert_eq!(g.apply(player, Action::SetupRoad(far), &mut r), Err(RuleError::NotConnected));
        let near = g.setup_edges(player)[0];
        g.apply(player, Action::SetupRoad(near), &mut r).unwrap();
    }
    assert_eq!(g.phase, Phase::Playing);
    assert_eq!((g.current, g.round, g.dice), (P0, 1, None));
    for p in &g.players {
        assert_eq!(p.resources, balance::STARTING_RESOURCES);
        assert_eq!(g.points(p.id), balance::TOWN_POINTS);
    }
}

#[test]
fn setup_rejects_taken_vertices() {
    let mut g = Game::with_board(&names(2), Board::hexagon(2));
    let mut r = rng();
    g.apply(P0, Action::SetupBuilding(VertexId(5)), &mut r).unwrap();
    let e = g.setup_edges(P0)[0];
    g.apply(P0, Action::SetupRoad(e), &mut r).unwrap();
    assert_eq!(g.apply(P1, Action::SetupBuilding(VertexId(5)), &mut r), Err(RuleError::VertexTaken));
    assert!(matches!(g.phase, Phase::Setup { step: 1, placing: SetupPlacing::Building }));
}

// ----- Production -----

#[test]
fn every_building_level_produces_and_cities_double() {
    let mut g = playing(2);
    let t = centre(&g);
    g.board.tiles[t.index()].resource = Resource::Iron;
    g.board.tiles[t.index()].number = 9;
    for tile in &mut g.board.tiles {
        if tile.id != t {
            tile.number = 2;
        }
    }
    let vs = g.board.tile(t).vertices;
    put(&mut g, vs[0], P0, BuildingKind::Outpost);
    put(&mut g, vs[2], P0, BuildingKind::Town);
    put(&mut g, vs[4], P1, BuildingKind::City);
    g.produce(9);
    assert_eq!(g.players[0].resources.iron, 2, "outpost 1 + town 1");
    assert_eq!(g.players[1].resources.iron, 2, "city 2");
    g.produce(5);
    assert_eq!(g.players[0].resources.iron, 2, "non-matching number produces nothing");
}

#[test]
fn robber_blocks_its_tile_and_seven_produces_nothing() {
    let mut g = playing(2);
    let t = centre(&g);
    g.board.tiles[t.index()].number = 6;
    let v = g.board.tile(t).vertices[0];
    put(&mut g, v, P0, BuildingKind::Outpost);
    robber(&mut g, 1, t);
    g.produce(6);
    assert_eq!(g.players[0].resources.total(), 0);
    assert!(g.events.iter().any(|e| e.event == Event::ProductionBlocked { tile: t }));

    // A real roll: find a seed rolling 7 and check nothing was produced.
    for seed in 0..200 {
        let mut g2 = playing(2);
        g2.dice = None;
        for tile in &mut g2.board.tiles {
            tile.number = 8;
        }
        put(&mut g2, VertexId(0), P0, BuildingKind::City);
        let mut r = Rng::new(seed);
        g2.apply(P0, Action::Roll, &mut r).unwrap();
        let (a, b) = g2.dice.unwrap();
        if a + b == 7 {
            assert_eq!(g2.players[0].resources.total(), 0);
            assert!(g2.robber_relocation, "no robbers: the roller places one");
            return;
        }
    }
    panic!("no seed rolled a 7");
}

// ----- Construction -----

#[test]
fn new_buildings_are_outposts_on_the_owners_roads() {
    let mut g = playing(2);
    g.players[0].resources = rich();
    let e = g.board.edges[0].clone();
    let [a, b] = e.vertices;
    assert_eq!(g.can_build_outpost(P0, a), Err(RuleError::NotConnected));
    assert_eq!(g.can_build_road(P0, e.id), Err(RuleError::NotConnected));
    put(&mut g, a, P0, BuildingKind::Outpost);
    let mut r = rng();
    g.apply(P0, Action::BuildRoad(e.id), &mut r).unwrap();
    assert_eq!(g.apply(P0, Action::BuildRoad(e.id), &mut r), Err(RuleError::EdgeTaken));
    assert_eq!(g.can_build_outpost(P1, b), Err(RuleError::NotYourTurn));
    g.apply(P0, Action::BuildOutpost(b), &mut r).unwrap();
    assert_eq!(g.building(b), Some(Building { owner: P0, kind: BuildingKind::Outpost }));
    assert_eq!(g.apply(P0, Action::BuildOutpost(b), &mut r), Err(RuleError::VertexTaken));
    let spent = rich().total() - g.players[0].resources.total();
    assert_eq!(spent, balance::ROAD_COST.total() + balance::OUTPOST_COST.total());

    // Roads extend from existing roads too.
    let next = g.board.vertex(b).edges.iter().copied().find(|&x| x != e.id).unwrap();
    assert!(g.can_build_road(P0, next).is_ok());
}

#[test]
fn upgrades_go_outpost_town_city_in_order_and_cost_resources() {
    let mut g = playing(2);
    let v = VertexId(3);
    put(&mut g, v, P0, BuildingKind::Outpost);
    let mut r = rng();
    assert_eq!(g.apply(P0, Action::UpgradeTown(v), &mut r), Err(RuleError::CannotAfford));
    g.players[0].resources = rich();
    assert_eq!(g.apply(P0, Action::UpgradeCity(v), &mut r), Err(RuleError::NotUpgradable), "no skipping");
    g.apply(P0, Action::UpgradeTown(v), &mut r).unwrap();
    assert_eq!(g.building(v).unwrap().kind, BuildingKind::Town);
    assert_eq!(g.points(P0), balance::TOWN_POINTS);
    assert_eq!(g.apply(P0, Action::UpgradeTown(v), &mut r), Err(RuleError::NotUpgradable));
    g.apply(P0, Action::UpgradeCity(v), &mut r).unwrap();
    assert_eq!(g.building(v).unwrap().kind, BuildingKind::City);
    assert_eq!(g.points(P0), balance::CITY_POINTS);
    assert_eq!(g.apply(P0, Action::UpgradeCity(v), &mut r), Err(RuleError::NotUpgradable));
    let spent = rich().total() - g.players[0].resources.total();
    assert_eq!(spent, balance::TOWN_UPGRADE_COST.total() + balance::CITY_UPGRADE_COST.total());

    // Never someone else's.
    let w = VertexId(30);
    put(&mut g, w, P1, BuildingKind::Outpost);
    assert_eq!(g.apply(P0, Action::UpgradeTown(w), &mut r), Err(RuleError::NotUpgradable));
}

// ----- Turns -----

#[test]
fn only_the_active_player_acts_and_must_roll_first() {
    let mut g = playing(3);
    g.dice = None;
    let mut r = rng();
    assert_eq!(g.apply(P1, Action::Roll, &mut r), Err(RuleError::NotYourTurn));
    assert_eq!(g.apply(P0, Action::EndTurn, &mut r), Err(RuleError::MustRollFirst));
    assert_eq!(g.apply(P0, Action::BuyCard(UnitKind::Knight), &mut r), Err(RuleError::MustRollFirst));
    g.apply(P0, Action::Roll, &mut r).unwrap();
    assert_eq!(g.apply(P0, Action::Roll, &mut r), Err(RuleError::AlreadyRolled));
    g.apply(P0, Action::EndTurn, &mut r).unwrap();
    assert_eq!((g.current, g.dice, g.round), (P1, None, 1));
    for p in [1, 2] {
        g.apply(PlayerId(p), Action::Roll, &mut r).unwrap();
        g.apply(PlayerId(p), Action::EndTurn, &mut r).unwrap();
    }
    assert_eq!((g.current, g.round), (P0, 2), "a round is every player's turn");
    assert!(g.events.iter().any(|e| e.event == Event::RoundEnded { round: 1 }));
}

#[test]
fn trades_move_resources_between_players() {
    let mut g = playing(3);
    let mut r = rng();
    g.players[0].resources.wood = 2;
    g.players[1].resources.iron = 1;
    let give = Resources::one(Resource::Wood);
    let want = Resources::one(Resource::Iron);
    assert_eq!(g.apply(P1, Action::ProposeTrade { to: None, give, want }, &mut r), Err(RuleError::NotYourTurn));
    g.apply(P0, Action::ProposeTrade { to: None, give, want }, &mut r).unwrap();
    assert_eq!(g.apply(PlayerId(2), Action::AcceptTrade, &mut r), Err(RuleError::CannotAfford));
    assert_eq!(g.apply(P0, Action::AcceptTrade, &mut r), Err(RuleError::NotTradeRecipient));
    g.apply(P1, Action::AcceptTrade, &mut r).unwrap();
    assert_eq!((g.players[0].resources.wood, g.players[0].resources.iron), (1, 1));
    assert_eq!((g.players[1].resources.wood, g.players[1].resources.iron), (1, 0));
    assert!(g.trade.is_none());

    // Declined by everyone it was offered to: closed.
    g.apply(P0, Action::ProposeTrade { to: None, give, want }, &mut r).unwrap();
    g.apply(P1, Action::DeclineTrade, &mut r).unwrap();
    assert!(g.trade.is_some());
    g.apply(PlayerId(2), Action::DeclineTrade, &mut r).unwrap();
    assert!(g.trade.is_none());
}

#[test]
fn the_bank_trades_four_of_a_kind_for_one() {
    let mut g = playing(2);
    let mut r = rng();
    g.players[0].resources.wood = 5;
    let trade = |give, get| Action::TradeWithBank { give, get };
    assert_eq!(g.apply(P0, trade(Resource::Stone, Resource::Iron), &mut r), Err(RuleError::CannotAfford));
    assert_eq!(g.apply(P0, trade(Resource::Wood, Resource::Wood), &mut r), Err(RuleError::InvalidTrade));
    assert_eq!(g.apply(P1, trade(Resource::Wood, Resource::Iron), &mut r), Err(RuleError::NotYourTurn));
    g.apply(P0, trade(Resource::Wood, Resource::Iron), &mut r).unwrap();
    assert_eq!((g.players[0].resources.wood, g.players[0].resources.iron), (1, 1));
    assert_eq!(g.apply(P0, trade(Resource::Wood, Resource::Iron), &mut r), Err(RuleError::CannotAfford));
}

// ----- Units -----

#[test]
fn cards_deploy_only_next_to_own_towns_and_cities() {
    let mut g = playing(2);
    let mut r = rng();
    let t = centre(&g);
    let vs = g.board.tile(t).vertices;
    assert_eq!(g.apply(P0, Action::Deploy { kind: UnitKind::Knight, tile: t }, &mut r), Err(RuleError::NoCard));
    g.players[0].resources = balance::KNIGHT_CARD_COST;
    g.apply(P0, Action::BuyCard(UnitKind::Knight), &mut r).unwrap();
    assert_eq!(g.players[0].resources, Resources::ZERO);
    assert_eq!(g.players[0].cards.knights, 1);

    put(&mut g, vs[0], P0, BuildingKind::Outpost);
    assert_eq!(
        g.apply(P0, Action::Deploy { kind: UnitKind::Knight, tile: t }, &mut r),
        Err(RuleError::NotNextToYourTown),
        "outposts cannot deploy"
    );
    put(&mut g, vs[0], P0, BuildingKind::Town);
    robber(&mut g, 9, t);
    assert_eq!(g.apply(P0, Action::Deploy { kind: UnitKind::Knight, tile: t }, &mut r), Err(RuleError::TileOccupied));
    g.robbers.clear();
    g.apply(P0, Action::Deploy { kind: UnitKind::Knight, tile: t }, &mut r).unwrap();
    let knight = g.unit_at(t).unwrap().clone();
    assert!(!knight.ready, "new units wait for the owner's next turn");
    let to = g.board.tile(t).neighbors[0];
    assert_eq!(g.apply(P0, Action::MoveUnit { unit: knight.id, to }, &mut r), Err(RuleError::UnitExhausted));
    assert!(g.deploy_tiles(P0).iter().all(|&x| x != t));
    put(&mut g, vs[0], P0, BuildingKind::City);
    assert!(!g.deploy_tiles(P0).is_empty(), "cities deploy too");
}

#[test]
fn units_move_one_adjacent_unoccupied_tile_and_then_rest() {
    let mut g = playing(2);
    let mut r = rng();
    let t = centre(&g);
    let k = unit(&mut g, 1, P0, UnitKind::Knight, t);
    let ns = g.board.tile(t).neighbors.clone();
    let far = g.board.tile_ids().find(|&x| x != t && !ns.contains(&x)).unwrap();
    assert_eq!(g.apply(P0, Action::MoveUnit { unit: k, to: far }, &mut r), Err(RuleError::NotAdjacent));
    robber(&mut g, 5, ns[0]);
    assert_eq!(g.apply(P0, Action::MoveUnit { unit: k, to: ns[0] }, &mut r), Err(RuleError::TileOccupied));
    assert_eq!(g.move_targets(P0, k).len(), ns.len() - 1);
    assert_eq!(g.apply(P1, Action::MoveUnit { unit: k, to: ns[1] }, &mut r), Err(RuleError::NotYourTurn));
    g.apply(P0, Action::MoveUnit { unit: k, to: ns[1] }, &mut r).unwrap();
    assert_eq!(g.unit(k).unwrap().tile, ns[1]);
    // Moved: no attack, no second move this turn.
    assert_eq!(g.apply(P0, Action::MoveUnit { unit: k, to: t }, &mut r), Err(RuleError::UnitExhausted));
    assert_eq!(
        g.apply(P0, Action::Attack { unit: k, target: Target::Robber(RobberId(5)) }, &mut r),
        Err(RuleError::UnitExhausted)
    );
    // Refreshed at the owner's next turn.
    g.apply(P0, Action::EndTurn, &mut r).unwrap();
    g.apply(P1, Action::Roll, &mut r).unwrap();
    g.apply(P1, Action::EndTurn, &mut r).unwrap();
    assert!(g.unit(k).unwrap().ready);
}

// ----- Combat -----

#[test]
fn knights_fall_with_the_robber_or_knight_they_attack() {
    let mut g = playing(2);
    let mut r = rng();
    let t = centre(&g);
    let ns = g.board.tile(t).neighbors.clone();
    let k = unit(&mut g, 1, P0, UnitKind::Knight, t);
    let rob = robber(&mut g, 2, ns[0]);
    let enemy = unit(&mut g, 4, P1, UnitKind::Knight, ns[2]);
    let mine = unit(&mut g, 5, P0, UnitKind::Knight, ns[3]);
    let targets = g.attack_targets(P0, k);
    assert!(targets.contains(&Target::Robber(rob)));
    assert!(targets.contains(&Target::Unit(enemy)));
    assert!(!targets.contains(&Target::Unit(mine)), "no friendly fire");

    g.apply(P0, Action::Attack { unit: k, target: Target::Robber(rob) }, &mut r).unwrap();
    assert!(g.robbers.is_empty() && g.unit(k).is_none(), "the knight falls with the robber");

    // Another knight meets the enemy knight: both fall.
    let k2 = unit(&mut g, 6, P0, UnitKind::Knight, t);
    g.apply(P0, Action::Attack { unit: k2, target: Target::Unit(enemy) }, &mut r).unwrap();
    assert!(g.unit(enemy).is_none() && g.unit(k2).is_none(), "knights destroy each other");
}

// ----- Spells -----

/// A playing game where P0 owns a city, can pay for any spell, and the
/// centre tile is wood numbered 6 with a P1 town and a P0 outpost on it.
fn spellbook() -> (Game, TileId) {
    let mut g = playing(2);
    let t = centre(&g);
    g.board.tiles[t.index()].resource = Resource::Wood;
    g.board.tiles[t.index()].number = 6;
    for tile in &mut g.board.tiles {
        if tile.id != t && tile.number == 6 {
            tile.number = 5;
        }
    }
    let far = g.board.vertices.iter().find(|v| !g.board.tile(t).vertices.contains(&v.id)).unwrap().id;
    put(&mut g, far, P0, BuildingKind::City);
    let [a, b, ..] = g.board.tile(t).vertices[..] else { unreachable!() };
    put(&mut g, a, P1, BuildingKind::Town);
    put(&mut g, b, P0, BuildingKind::Outpost);
    g.players[0].resources = rich();
    (g, t)
}

fn cast(spell: Spell, tiles: &[TileId]) -> Action {
    Action::CastSpell { spell, tiles: tiles.to_vec() }
}

#[test]
fn spells_need_a_city_full_mana_fitting_targets_and_their_price() {
    let (mut g, t) = spellbook();
    let mut r = rng();
    assert_eq!(g.apply(P1, cast(Spell::Frost, &[t]), &mut r), Err(RuleError::NotYourTurn));
    // Frost and bounty reach any tile, whatever its resource.
    let stone = g.board.tiles.iter().find(|x| x.id != t).unwrap().id;
    g.board.tiles[stone.index()].resource = Resource::Stone;
    assert!(g.can_cast(P0, Spell::Frost, &[stone]).is_ok());
    assert_eq!(g.apply(P0, cast(Spell::Frost, &[]), &mut r), Err(RuleError::InvalidTarget));
    let before = g.players[0].resources;
    g.apply(P0, cast(Spell::Frost, &[t]), &mut r).unwrap();
    assert_eq!(before.total() - g.players[0].resources.total(), balance::FROST_COST.total());
    assert_eq!(g.apply(P0, cast(Spell::Bounty, &[stone]), &mut r), Err(RuleError::ManaEmpty), "one mana for all");
    // Mana is back after its recharge rounds; a new spell replaces the old one.
    g.round += balance::MANA_RECHARGE_ROUNDS;
    g.apply(P0, cast(Spell::Bounty, &[t]), &mut r).unwrap();
    assert_eq!(g.enchantments.len(), 1);
    assert_eq!(g.enchantment_at(t).unwrap().spell, Spell::Bounty);

    // Without a city, spells stay locked; without the price, unaffordable.
    let mut g = playing(2);
    g.players[0].resources = rich();
    assert_eq!(g.apply(P0, cast(Spell::Frost, &[centre(&g)]), &mut r), Err(RuleError::SpellsLocked));
    let (mut g, t) = spellbook();
    g.players[0].resources = Resources::ZERO;
    assert_eq!(g.apply(P0, cast(Spell::Bounty, &[t]), &mut r), Err(RuleError::CannotAfford));
}

#[test]
fn stopping_spells_stop_and_doubling_spells_double_the_next_two_rolls() {
    let (mut g, t) = spellbook();
    let mut r = rng();
    let wood = |g: &Game, p: usize| g.players[p].resources.wood;
    g.apply(P0, cast(Spell::Frost, &[t]), &mut r).unwrap();
    let (w0, w1) = (wood(&g, 0), wood(&g, 1));
    g.produce(5); // another number does not count
    g.produce(6);
    g.produce(6);
    assert_eq!((wood(&g, 0), wood(&g, 1)), (w0, w1), "two frozen rolls");
    assert!(g.enchantment_at(t).is_none(), "faded after two rolls");
    g.produce(6);
    assert_eq!((wood(&g, 0), wood(&g, 1)), (w0 + 1, w1 + 1), "thawed");

    g.round += balance::MANA_RECHARGE_ROUNDS;
    g.apply(P0, cast(Spell::Bounty, &[t]), &mut r).unwrap();
    let (w0, w1) = (wood(&g, 0), wood(&g, 1));
    g.produce(6);
    g.produce(6);
    assert_eq!((wood(&g, 0), wood(&g, 1)), (w0 + 4, w1 + 4), "double for everyone on the tile, twice");
    g.produce(6);
    assert_eq!((wood(&g, 0), wood(&g, 1)), (w0 + 5, w1 + 5));

    // The same on an iron tile.
    g.board.tiles[t.index()].resource = Resource::Iron;
    g.round += balance::MANA_RECHARGE_ROUNDS;
    g.apply(P0, cast(Spell::Frost, &[t]), &mut r).unwrap();
    let iron = g.players[1].resources.iron;
    g.produce(6);
    assert_eq!(g.players[1].resources.iron, iron);
    g.round += balance::MANA_RECHARGE_ROUNDS;
    g.apply(P0, cast(Spell::Bounty, &[t]), &mut r).unwrap();
    g.produce(6);
    assert_eq!(g.players[1].resources.iron, iron + 2);
}

#[test]
fn luck_makes_the_next_roll_its_tiles_number_half_the_time() {
    let (mut g, t) = spellbook();
    let mut r = rng();
    g.apply(P0, cast(Spell::Luck, &[t]), &mut r).unwrap();
    assert_eq!(g.luck, Some(t));
    g.dice = None;
    let trials = 2000;
    let hits = (0..trials)
        .filter(|&s| {
            let mut copy = g.clone();
            copy.apply(P0, Action::Roll, &mut Rng::new(s)).unwrap();
            assert_eq!(copy.luck, None, "spent by one roll");
            let (a, b) = copy.dice.unwrap();
            a + b == 6
        })
        .count();
    assert!((900..=1100).contains(&hits), "about half of {trials} rolls hit: {hits}");
}

#[test]
fn swap_trades_two_numbers_for_two_rolls_and_lure_moves_a_robber() {
    let (mut g, t) = spellbook();
    let mut r = rng();
    let other = g.board.tiles.iter().find(|x| x.number != 6).unwrap().id;
    let n = g.board.tile(other).number;
    assert_eq!(g.apply(P0, cast(Spell::Swap, &[t, t]), &mut r), Err(RuleError::SpellTarget));
    g.apply(P0, cast(Spell::Swap, &[t, other]), &mut r).unwrap();
    assert_eq!((g.board.tile(t).number, g.board.tile(other).number), (n, 6));
    let produced_on = |g: &Game, since: u64| {
        g.events.iter().any(|e| e.seq > since && matches!(e.event, Event::Produced { tile, .. } if tile == t))
    };
    let seq = g.event_seq;
    g.produce(6); // now the other tile's number: the wood tile stays idle
    assert!(!produced_on(&g, seq));
    let seq = g.event_seq;
    g.produce(n);
    assert!(produced_on(&g, seq), "the wood tile answers to its new number");
    assert_eq!((g.board.tile(t).number, g.board.tile(other).number), (6, n), "numbers back after two rolls");

    // Lure: any robber to any free tile, then it stays put like a 7's.
    g.round += balance::MANA_RECHARGE_ROUNDS;
    let rb = robber(&mut g, 91, TileId(0));
    g.apply(P0, cast(Spell::Lure, &[TileId(0), t]), &mut r).unwrap();
    assert_eq!(g.robber(rb).unwrap().tile, t);
    g.end_round(&mut Rng::new(3));
    assert_eq!(g.robber(rb).unwrap().tile, t);
}

#[test]
fn windfall_pays_on_the_next_roll() {
    let (mut g, _) = spellbook();
    let mut r = rng();
    g.apply(P0, cast(Spell::Windfall, &[]), &mut r).unwrap();
    assert!(g.players[0].windfall);
    // P0's buildings touch the wood centre and the tiles round the far city.
    let touched: std::collections::BTreeSet<Resource> = g
        .buildings
        .iter()
        .enumerate()
        .filter(|(_, b)| b.is_some_and(|b| b.owner == P0))
        .flat_map(|(v, _)| g.board.vertices[v].tiles.iter().map(|&x| g.board.tile(x).resource))
        .collect();
    let before = g.players[0].resources;
    g.dice = None;
    g.apply(P0, Action::Roll, &mut r).unwrap();
    assert!(!g.players[0].windfall, "paid once");
    let paid = g.events.iter().rev().find_map(|e| match &e.event {
        Event::WindfallPaid { gained, .. } => Some(*gained),
        _ => None,
    });
    let gained = paid.expect("paid on the roll");
    for res in Resource::ALL {
        assert_eq!(gained.get(res), u32::from(touched.contains(&res)));
        assert!(g.players[0].resources.get(res) >= before.get(res) + gained.get(res));
    }
}

// ----- Items -----

/// An unfound item of `kind` on `tile`, lasting through `last_round`.
fn item(g: &mut Game, tile: TileId, kind: ItemKind, last_round: u32) {
    g.items.push(Item { tile, kind: Some(kind), revealed: false, count: 1, last_round });
}

#[test]
fn items_appear_half_the_rounds_away_from_towns_and_vanish_after_three() {
    let mut g = playing(2);
    let town = g.board.tile(centre(&g)).vertices[0];
    put(&mut g, town, P0, BuildingKind::Town);
    let near: Vec<TileId> = g.board.tile_ids().filter(|&x| g.board.tile(x).vertices.contains(&town)).collect();
    let trials = 400;
    let mut appeared = 0;
    for s in 0..trials {
        let mut copy = g.clone();
        copy.end_round(&mut Rng::new(s));
        if let Some(it) = copy.items.first() {
            appeared += 1;
            assert!(!near.contains(&it.tile), "never on a tile touching a town");
            assert!(copy.robber_at(it.tile).is_none());
            assert_eq!(it.last_round, g.round + balance::ITEM_ROUNDS);
        }
    }
    assert!((150..=250).contains(&appeared), "about half the rounds: {appeared} of {trials}");

    // One left at the end of this round lasts the next three, guarded from
    // robbers by a knight standing on it.
    let far = g.board.tile_ids().find(|x| !near.contains(x)).unwrap();
    let last = g.round + balance::ITEM_ROUNDS;
    item(&mut g, far, ItemKind::IronOre, last);
    unit(&mut g, 1, P1, UnitKind::Knight, far);
    let present = |g: &Game| g.items.iter().any(|i| i.tile == far && i.last_round == last);
    for _ in 0..balance::ITEM_ROUNDS {
        g.end_round(&mut Rng::new(7));
        g.round += 1;
        assert!(present(&g), "still there in round {}", g.round);
    }
    g.end_round(&mut Rng::new(7));
    assert!(!present(&g), "gone after {} rounds", balance::ITEM_ROUNDS);
}

#[test]
fn knights_take_items_by_walking_onto_them_and_drop_relics_when_they_fall() {
    let mut g = playing(2);
    let mut r = rng();
    let t = centre(&g);
    let ns = g.board.tile(t).neighbors.clone();
    let k = unit(&mut g, 1, P0, UnitKind::Knight, t);
    item(&mut g, ns[0], ItemKind::IronOre, 9);
    g.apply(P0, Action::MoveUnit { unit: k, to: ns[0] }, &mut r).unwrap();
    assert_eq!(g.players[0].resources, balance::IRON_ORE_FIND);
    assert!(g.items.is_empty());

    item(&mut g, t, ItemKind::Relic, 9);
    g.units[0].ready = true;
    g.apply(P0, Action::MoveUnit { unit: k, to: t }, &mut r).unwrap();
    assert_eq!(g.unit(k).unwrap().relics, 1);
    assert_eq!(g.points(P0), balance::RELIC_POINTS);

    // A trap fells the knight; its relic drops where it fell, for all to see.
    item(&mut g, ns[1], ItemKind::Trap, 9);
    g.units[0].ready = true;
    g.apply(P0, Action::MoveUnit { unit: k, to: ns[1] }, &mut r).unwrap();
    assert!(g.unit(k).is_none());
    assert_eq!(g.points(P0), 0);
    let dropped = g.item_at(ns[1]).unwrap();
    assert_eq!((dropped.kind, dropped.revealed, dropped.count), (Some(ItemKind::Relic), true, 1));

    // Deploying onto it does not count: knights must walk there.
    let v = g.board.tile(ns[1]).vertices[0];
    put(&mut g, v, P0, BuildingKind::Town);
    g.players[0].cards.knights = 1;
    assert_eq!(g.can_deploy(P0, UnitKind::Knight, ns[1]), Err(RuleError::TileOccupied));
}

#[test]
fn a_scroll_casts_its_spell_free_without_a_city_or_mana() {
    let mut g = playing(2);
    let mut r = rng();
    let t = centre(&g);
    g.players[0].scrolls.push(Spell::Frost);
    g.players[0].mana_ready_round = g.round + 3;
    g.apply(P0, cast(Spell::Frost, &[t]), &mut r).unwrap();
    assert!(g.players[0].scrolls.is_empty());
    assert_eq!(g.players[0].mana_ready_round, g.round + 3, "the mana is untouched");
    assert_eq!(g.enchantment_at(t).unwrap().spell, Spell::Frost);
    assert_eq!(g.apply(P0, cast(Spell::Frost, &[t]), &mut r), Err(RuleError::SpellsLocked), "used up");
}

#[test]
fn robbers_take_items_they_land_on_and_views_hide_unfound_ones() {
    let mut g = playing(2);
    let mut r = rng();
    let t = centre(&g);
    let ns = g.board.tile(t).neighbors.clone();
    let rb = robber(&mut g, 1, ns[0]);
    item(&mut g, t, ItemKind::Hoard, 9);
    g.robber_relocation = true;
    g.apply(P0, Action::RelocateRobber { robber: rb, to: t }, &mut r).unwrap();
    assert!(g.items.is_empty());
    assert!(matches!(g.events.last().unwrap().event, Event::ItemStolen { tile } if tile == t));

    item(&mut g, ns[1], ItemKind::Trap, 9);
    g.items.push(Item { tile: ns[2], kind: Some(ItemKind::Relic), revealed: true, count: 1, last_round: 9 });
    g.hide_items();
    assert_eq!(g.item_at(ns[1]).unwrap().kind, None);
    assert_eq!(g.item_at(ns[2]).unwrap().kind, Some(ItemKind::Relic));
}

// ----- Robbers -----

#[test]
fn robber_capacity_counts_towns_and_cities_only() {
    let mut g = playing(2);
    for i in 0..8u16 {
        put(&mut g, VertexId(i * 3), P0, BuildingKind::Outpost);
    }
    assert_eq!(g.robber_capacity(), 0);
    put(&mut g, VertexId(0), P0, BuildingKind::City);
    put(&mut g, VertexId(3), P1, BuildingKind::Town);
    put(&mut g, VertexId(6), P0, BuildingKind::Town);
    assert_eq!(g.robber_capacity(), 0);
    put(&mut g, VertexId(9), P1, BuildingKind::City);
    assert_eq!((g.total_settlements(), g.robber_capacity()), (4, 1));
    for i in 4..8u16 {
        put(&mut g, VertexId(i * 3), P0, BuildingKind::Town);
    }
    assert_eq!(g.robber_capacity(), 2);
}

#[test]
fn extra_robbers_appear_about_a_quarter_of_rounds_below_capacity_only() {
    let mut base = playing(2);
    for i in 0..8u16 {
        put(&mut base, VertexId(i * 5), P0, BuildingKind::Town);
    }
    robber(&mut base, 100, TileId(0));
    assert_eq!(base.robber_capacity(), 2);
    let mut spawned = 0;
    for seed in 0..400 {
        let mut g = base.clone();
        g.end_round(&mut Rng::new(seed));
        assert!(g.robbers.len() <= 2);
        if g.robbers.len() == 2 {
            spawned += 1;
            assert!(g.robbers.iter().all(|rb| g.unit_at(rb.tile).is_none()));
            // At capacity: never a third one.
            let mut again = g.clone();
            for s in 0..20 {
                again.end_round(&mut Rng::new(s));
            }
            assert_eq!(again.robbers.len(), 2);
        }
    }
    assert!((70..=130).contains(&spawned), "25% of 400 rounds, got {spawned}");

    // Existing robbers stay when capacity drops below them.
    let mut g = playing(2);
    robber(&mut g, 1, TileId(0));
    robber(&mut g, 2, TileId(18));
    for s in 0..50 {
        g.end_round(&mut Rng::new(s));
    }
    assert_eq!(g.robbers.len(), 2);
}

#[test]
fn the_island_always_has_a_robber() {
    let g = Game::new(&names(2), 10, balance::MapSize::Small, &mut Rng::new(5));
    assert_eq!(g.robbers.len(), 1, "one stands from the start");

    // With none on the board and no towns at all, one appears at the round's end.
    let mut g = playing(2);
    assert_eq!(g.robber_capacity(), 0);
    g.end_round(&mut Rng::new(1));
    assert_eq!(g.robbers.len(), 1);
}

#[test]
fn a_slain_robber_returns_after_two_quiet_rounds() {
    let mut g = playing(2);
    let t = centre(&g);
    let k = unit(&mut g, 1, P0, UnitKind::Knight, t);
    let next = g.board.tile(t).neighbors[0];
    let rb = robber(&mut g, 2, next);
    g.apply(P0, Action::Attack { unit: k, target: Target::Robber(rb) }, &mut rng()).unwrap();
    assert!(g.robbers.is_empty());
    // The rest of round 1 and all of round 2 stay clear...
    g.end_round(&mut Rng::new(1));
    g.round += 1;
    g.end_round(&mut Rng::new(1));
    assert!(g.robbers.is_empty());
    // ...and one appears when round 3 ends.
    g.round += 1;
    g.end_round(&mut Rng::new(1));
    assert_eq!(g.robbers.len(), 1);
}

#[test]
fn robbers_wander_to_an_adjacent_free_tile_each_round() {
    let mut g = playing(2);
    let t = centre(&g);
    let rb = robber(&mut g, 1, t);
    let ns = g.board.tile(t).neighbors.clone();
    for (i, &n) in ns.iter().enumerate().skip(1) {
        unit(&mut g, 10 + i as u32, P1, UnitKind::Knight, n);
    }
    g.end_round(&mut rng());
    assert_eq!(g.robber(rb).unwrap().tile, ns[0], "the only free neighbour");
    // Fully surrounded: it stays.
    let mut g = playing(2);
    let rb = robber(&mut g, 1, t);
    for (i, &n) in ns.iter().enumerate() {
        unit(&mut g, 10 + i as u32, P1, UnitKind::Knight, n);
    }
    g.end_round(&mut rng());
    assert_eq!(g.robber(rb).unwrap().tile, t);
}

#[test]
fn rolling_seven_relocates_a_robber_which_then_rests_a_full_round() {
    let mut seed = 0;
    let (mut g, mut r) = loop {
        let mut g = playing(2);
        g.dice = None;
        robber(&mut g, 1, TileId(0));
        let mut probe = Rng::new(seed);
        g.apply(P0, Action::Roll, &mut probe).unwrap();
        if g.dice.map(|(a, b)| a + b) == Some(7) {
            break (g, probe);
        }
        seed += 1;
    };
    assert!(g.robber_relocation);
    let t = centre(&g);
    unit(&mut g, 7, P1, UnitKind::Knight, TileId(1));
    assert_eq!(
        g.apply(P0, Action::RelocateRobber { robber: RobberId(1), to: TileId(1) }, &mut r),
        Err(RuleError::TileOccupied)
    );
    assert_eq!(g.apply(P1, Action::RelocateRobber { robber: RobberId(1), to: t }, &mut r), Err(RuleError::NotYourTurn));
    g.apply(P0, Action::RelocateRobber { robber: RobberId(1), to: t }, &mut r).unwrap();
    assert_eq!(g.robber(RobberId(1)).unwrap().tile, t);
    assert_eq!(
        g.apply(P0, Action::RelocateRobber { robber: RobberId(1), to: TileId(2) }, &mut r),
        Err(RuleError::NoRobberRelocation),
        "one relocation per 7"
    );
    // Skips the move at the end of this round, stays all of the next one,
    // and wanders again when that complete round ends.
    g.end_round(&mut Rng::new(1));
    assert_eq!(g.robber(RobberId(1)).unwrap().tile, t);
    g.round += 1;
    g.end_round(&mut Rng::new(1));
    assert_ne!(g.robber(RobberId(1)).unwrap().tile, t, "resumes wandering");
}

#[test]
fn rolling_seven_with_no_robber_lets_the_roller_place_one() {
    let (mut g, mut r) = (0..)
        .find_map(|seed| {
            let mut g = playing(2);
            g.dice = None;
            let mut probe = Rng::new(seed);
            g.apply(P0, Action::Roll, &mut probe).unwrap();
            (g.dice.map(|(a, b)| a + b) == Some(7)).then_some((g, probe))
        })
        .unwrap();
    assert!(g.robbers.is_empty() && g.robber_relocation);
    let t = centre(&g);
    assert_eq!(g.apply(P1, Action::PlaceRobber { to: t }, &mut r), Err(RuleError::NotYourTurn));
    g.apply(P0, Action::PlaceRobber { to: t }, &mut r).unwrap();
    let placed = g.robber_at(t).expect("placed").id;
    assert_eq!(g.apply(P0, Action::PlaceRobber { to: TileId(2) }, &mut r), Err(RuleError::NoRobberRelocation));
    // Like a moved robber it skips this round's wander.
    g.end_round(&mut Rng::new(1));
    assert_eq!(g.robber(placed).unwrap().tile, t);
}

#[test]
fn a_robber_on_the_board_is_moved_not_placed() {
    let mut g = playing(2);
    robber(&mut g, 1, TileId(0));
    g.robber_relocation = true;
    assert_eq!(g.apply(P0, Action::PlaceRobber { to: centre(&g) }, &mut Rng::new(1)), Err(RuleError::RobberToMove));
}

// ----- Victory -----

#[test]
fn reaching_ten_points_wins_and_ends_the_game() {
    let mut g = playing(2);
    let mut r = rng();
    for i in 0..4u16 {
        put(&mut g, VertexId(i * 6), P0, BuildingKind::City);
    }
    put(&mut g, VertexId(30), P0, BuildingKind::Town);
    assert_eq!(g.points(P0), 9);
    g.players[0].resources = rich();
    g.apply(P0, Action::UpgradeCity(VertexId(30)), &mut r).unwrap();
    assert_eq!(g.winner(), Some(P0));
    assert!(g.events.iter().any(|e| e.event == Event::Won { player: P0, points: 10 }));
    assert_eq!(g.apply(P0, Action::EndTurn, &mut r), Err(RuleError::GameOver));
    assert_eq!(g.apply(P1, Action::Roll, &mut r), Err(RuleError::GameOver));
}

#[test]
fn a_seeded_game_replays_identically() {
    let play = |seed: u64| {
        let mut r = Rng::new(seed);
        let mut g = Game::new(&names(2), 10, balance::MapSize::Small, &mut r);
        let order = g.setup_order();
        for &p in &order {
            let v = g.setup_vertices(p).into_iter().find(|&v| g.board.vertex(v).tiles.len() == 3).unwrap();
            g.apply(p, Action::SetupBuilding(v), &mut r).unwrap();
            let e = g.setup_edges(p)[0];
            g.apply(p, Action::SetupRoad(e), &mut r).unwrap();
        }
        for _ in 0..20 {
            let p = g.current;
            g.apply(p, Action::Roll, &mut r).unwrap();
            g.apply(p, Action::EndTurn, &mut r).unwrap();
        }
        g
    };
    assert_eq!(play(5), play(5));
}
