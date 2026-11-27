//! Practice matches, for trying the client alone during development: the
//! player against a rival the server plays (it only rolls and passes),
//! dealt into the middle of a game so every kind of piece is on the board
//! from the first turn.

use kings_cove_engine::{
    Action, Building, BuildingKind, Game, Phase, PlayerId, Rng, TileId, Unit, UnitId, UnitKind, VertexId, balance,
    balance::MapSize,
};

/// The rival's name.
pub const RIVAL: &str = "Rival";

/// What the player starts with: enough to build, upgrade and cast a while.
const PLAYER_STOCK: u32 = 12;
/// Knight tokens the player holds, besides the knights on the board.
const PLAYER_TOKENS: u32 = 2;
/// Knights each side has on the board.
const KNIGHTS_EACH: usize = 2;

/// A game of `name` against [`RIVAL`], setup done, on the player's first
/// turn, not yet rolled: the player's first town a castle, two knights a
/// side, one of the rival's next to one of the player's to attack, and a
/// full stock of resources.
pub fn game(name: &str, rng: &mut Rng) -> Game {
    let me = PlayerId(0);
    let names = [name.to_owned(), RIVAL.to_owned()];
    let mut g = Game::new(&names, balance::DEFAULT_VICTORY_POINTS, MapSize::default(), rng);
    // Setup as the players would: each building on a random inland corner,
    // with a road beside it.
    while matches!(g.phase, Phase::Setup { .. }) {
        let p = g.current;
        let corners = g.setup_vertices(p);
        let inland: Vec<VertexId> = corners.iter().copied().filter(|&v| g.board.vertex(v).tiles.len() == 3).collect();
        let v = rng.pick(if inland.is_empty() { &corners } else { &inland }).expect("a free corner");
        g.apply(p, Action::SetupBuilding(v), rng).expect("a legal corner");
        let e = rng.pick(&g.setup_edges(p)).expect("a free side");
        g.apply(p, Action::SetupRoad(e), rng).expect("a legal side");
    }

    let castle = g.buildings.iter().position(|b| b.is_some_and(|b| b.owner == me && b.kind == BuildingKind::Town));
    if let Some(v) = castle {
        g.buildings[v] = Some(Building { owner: me, kind: BuildingKind::City });
    }
    let player = &mut g.players[me.index()];
    player.resources = balance::res(PLAYER_STOCK, PLAYER_STOCK, PLAYER_STOCK, PLAYER_STOCK);
    player.cards.knights = PLAYER_TOKENS;

    let rival = PlayerId(1);
    for _ in 0..KNIGHTS_EACH {
        let tiles = home_tiles(&g, me);
        deploy(&mut g, me, &tiles, rng);
    }
    // The rival's first knight faces one of the player's, when there is room.
    let facing = g.units.first().map(|u| free_tiles(&g, &g.board.tile(u.tile).neighbors)).unwrap_or_default();
    for k in 0..KNIGHTS_EACH {
        let tiles = if k == 0 && !facing.is_empty() { facing.clone() } else { home_tiles(&g, rival) };
        deploy(&mut g, rival, &tiles, rng);
    }
    g
}

/// Of `tiles`, those a knight may stand on.
fn free_tiles(g: &Game, tiles: &[TileId]) -> Vec<TileId> {
    tiles.iter().copied().filter(|&t| !g.occupied(t) && g.item_at(t).is_none()).collect()
}

/// Where a knight of `owner`'s may start: where it could deploy, else on a
/// free tile by any of its buildings, else anywhere free.
fn home_tiles(g: &Game, owner: PlayerId) -> Vec<TileId> {
    let deploy = g.deploy_tiles(owner);
    if !deploy.is_empty() {
        return deploy;
    }
    let by_buildings: Vec<TileId> = g
        .board
        .vertices
        .iter()
        .filter(|v| g.building(v.id).is_some_and(|b| b.owner == owner))
        .flat_map(|v| v.tiles.iter().copied())
        .collect();
    let near = free_tiles(g, &by_buildings);
    if near.is_empty() { free_tiles(g, &g.board.tile_ids().collect::<Vec<_>>()) } else { near }
}

/// A knight of `owner`'s on a random one of `tiles`, ready on its owner's
/// turn.
fn deploy(g: &mut Game, owner: PlayerId, tiles: &[TileId], rng: &mut Rng) {
    let Some(tile) = rng.pick(tiles) else { return };
    let id = UnitId(g.next_piece_id);
    g.next_piece_id += 1;
    let ready = g.current == owner;
    g.units.push(Unit { id, owner, kind: UnitKind::Knight, tile, ready, relics: 0, blockade: None });
}

/// The rival's turns: it declines any open offer and, on its turn, rolls
/// and passes, until it is a person's turn again.
pub fn play_rivals(g: &mut Game, rivals: &[PlayerId], rng: &mut Rng) {
    for &p in rivals {
        if g.trade.as_ref().is_some_and(|t| t.from != p) {
            let _ = g.apply(p, Action::DeclineTrade, rng);
        }
    }
    while g.phase == Phase::Playing && rivals.contains(&g.current) {
        let p = g.current;
        if g.dice.is_none() {
            let _ = g.apply(p, Action::Roll, rng);
        }
        if g.apply(p, Action::EndTurn, rng).is_err() {
            break;
        }
    }
}
