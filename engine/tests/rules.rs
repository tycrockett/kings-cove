use kings_cove_engine::{
    Action, Board, Building, BuildingKind, Event, Game, Phase, PlayerId, Resource, Resources, Rng, Robber, RobberId,
    RuleError, SetupPlacing, Target, TileId, Unit, UnitId, UnitKind, VertexId, balance,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn names(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("P{i}")).collect()
}

/// A game in play: setup skipped, player 0 to act, dice already rolled
/// (a 3), nobody holding anything.
fn playing(n: usize) -> Game {
    let mut g = Game::with_board(&names(n), Board::hexagon(balance::BOARD_RADIUS));
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
    g.units.push(Unit { id: UnitId(id), owner, kind, tile, ready: true });
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
    let a = Board::generate(&mut Rng::new(9));
    assert_eq!(a, Board::generate(&mut Rng::new(9)));
    assert!(a.numbers_balanced());
    for seed in 0..20 {
        let b = Board::generate(&mut Rng::new(seed));
        assert!(b.numbers_balanced());
        assert!(b.tiles.iter().all(|t| t.number != 7 && (2..=12).contains(&t.number)));
        for (resource, count) in balance::TILE_RESOURCES {
            assert_eq!(b.tiles.iter().filter(|t| t.resource == resource).count(), count);
        }
    }
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
            assert!(!g2.robber_relocation, "no robbers: a 7 has no further effect");
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
fn knights_kill_robbers_and_rams_and_trade_with_knights() {
    let mut g = playing(2);
    let mut r = rng();
    let t = centre(&g);
    let ns = g.board.tile(t).neighbors.clone();
    let k = unit(&mut g, 1, P0, UnitKind::Knight, t);
    let rob = robber(&mut g, 2, ns[0]);
    let ram = unit(&mut g, 3, P1, UnitKind::Ram, ns[1]);
    let enemy = unit(&mut g, 4, P1, UnitKind::Knight, ns[2]);
    let mine = unit(&mut g, 5, P0, UnitKind::Knight, ns[3]);
    let targets = g.attack_targets(P0, k);
    assert!(targets.contains(&Target::Robber(rob)));
    assert!(targets.contains(&Target::Unit(ram)));
    assert!(targets.contains(&Target::Unit(enemy)));
    assert!(!targets.contains(&Target::Unit(mine)), "no friendly fire");
    let v = g.board.tile(t).vertices[0];
    put(&mut g, v, P1, BuildingKind::Town);
    assert_eq!(
        g.apply(P0, Action::Attack { unit: k, target: Target::Building(v) }, &mut r),
        Err(RuleError::InvalidTarget),
        "knights cannot attack buildings"
    );

    g.apply(P0, Action::Attack { unit: k, target: Target::Robber(rob) }, &mut r).unwrap();
    assert!(g.robbers.is_empty());
    assert!(g.unit(k).is_some() && !g.unit(k).unwrap().ready);

    g.units.iter_mut().for_each(|u| u.ready = true);
    g.apply(P0, Action::Attack { unit: k, target: Target::Unit(ram) }, &mut r).unwrap();
    assert!(g.unit(ram).is_none() && g.unit(k).is_some());

    g.units.iter_mut().for_each(|u| u.ready = true);
    g.apply(P0, Action::Attack { unit: k, target: Target::Unit(enemy) }, &mut r).unwrap();
    assert!(g.unit(enemy).is_none() && g.unit(k).is_none(), "knights destroy each other");
}

#[test]
fn rams_downgrade_buildings_one_level_and_fight_nothing_else() {
    let mut g = playing(2);
    let mut r = rng();
    let t = centre(&g);
    let ns = g.board.tile(t).neighbors.clone();
    let ram = unit(&mut g, 1, P0, UnitKind::Ram, t);
    let v = g.board.tile(t).vertices[1];
    put(&mut g, v, P1, BuildingKind::City);
    let rob = robber(&mut g, 2, ns[0]);
    let knight = unit(&mut g, 3, P1, UnitKind::Knight, ns[1]);
    assert_eq!(
        g.apply(P0, Action::Attack { unit: ram, target: Target::Robber(rob) }, &mut r),
        Err(RuleError::InvalidTarget)
    );
    assert_eq!(
        g.apply(P0, Action::Attack { unit: ram, target: Target::Unit(knight) }, &mut r),
        Err(RuleError::InvalidTarget)
    );
    let far = g.board.vertices.iter().find(|x| !g.board.tile(t).vertices.contains(&x.id)).unwrap().id;
    put(&mut g, far, P1, BuildingKind::Town);
    assert_eq!(
        g.apply(P0, Action::Attack { unit: ram, target: Target::Building(far) }, &mut r),
        Err(RuleError::NotAdjacent)
    );
    let own = g.board.tile(t).vertices[3];
    put(&mut g, own, P0, BuildingKind::Town);
    assert_eq!(
        g.apply(P0, Action::Attack { unit: ram, target: Target::Building(own) }, &mut r),
        Err(RuleError::InvalidTarget)
    );

    for expected in [Some(BuildingKind::Town), Some(BuildingKind::Outpost), None] {
        g.apply(P0, Action::Attack { unit: ram, target: Target::Building(v) }, &mut r).unwrap();
        assert_eq!(g.building(v).map(|b| b.kind), expected);
        assert_eq!(
            g.apply(P0, Action::Attack { unit: ram, target: Target::Building(v) }, &mut r).unwrap_err(),
            RuleError::UnitExhausted
        );
        g.units.iter_mut().for_each(|u| u.ready = true);
    }
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
fn robbers_spawn_about_a_quarter_of_rounds_below_capacity_only() {
    let mut base = playing(2);
    for i in 0..4u16 {
        put(&mut base, VertexId(i * 5), P0, BuildingKind::Town);
    }
    assert_eq!(base.robber_capacity(), 1);
    let mut spawned = 0;
    for seed in 0..400 {
        let mut g = base.clone();
        g.end_round(&mut Rng::new(seed));
        assert!(g.robbers.len() <= 1);
        if let Some(rb) = g.robbers.first() {
            spawned += 1;
            assert!(g.unit_at(rb.tile).is_none());
            // At capacity: never a second one.
            let mut again = g.clone();
            for s in 0..20 {
                again.end_round(&mut Rng::new(s));
            }
            assert_eq!(again.robbers.len(), 1);
        }
    }
    assert!((70..=130).contains(&spawned), "25% of 400 rounds, got {spawned}");

    // No spawn at capacity 0; existing robbers stay when capacity drops.
    let mut g = playing(2);
    robber(&mut g, 1, TileId(0));
    robber(&mut g, 2, TileId(18));
    for s in 0..50 {
        g.end_round(&mut Rng::new(s));
    }
    assert_eq!(g.robbers.len(), 2);
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
        let mut g = Game::new(&names(2), &mut r);
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
