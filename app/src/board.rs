//! The board: where pieces sit on screen, what a tap there means, how it is
//! drawn, and the pinch, pan and tap gestures over it.

use std::f32::consts::{PI, TAU};

use kings_cove_engine::{
    Action, Blockade, BuildingKind, EdgeId, Enchantment, Event, Game, ItemKind, Phase, PlayerId, Resource, RobberId,
    SetupPlacing, Spell, StructureKind, Target, TileId, UnitId, VertexId, balance,
};
use nsh_core::{
    TouchPhase,
    canvas::{CanvasGradient, LineCap, LineJoin},
    element::TouchEvent,
    prelude::*,
};

use crate::{
    art::{self, circle, ease_in_out_sine, ease_out, ease_out_back, hash01, hex_points, lerp, phase, poly, text_at},
    palette::{self, hex, hexa},
    state::{AppState, Hud, Inspect, Mode, now_ms},
};

/// A hex's corner radius, in world units.
pub const S: f32 = 50.0;
const HALF_SQRT3: f32 = 0.866_025_4;
/// Space the HUD covers at the top and bottom, kept clear when fitting
/// before it is measured.
const FIT_TOP: f32 = 118.0;
const FIT_BOTTOM: f32 = 218.0;
/// Room kept between the board's fit and the measured HUD.
const FIT_MARGIN: f32 = 8.0;
/// How long the camera takes to fly to a tile or back to the whole island.
const FLY_MS: f64 = 300.0;

// ----- Geometry -----

fn lattice(x: i32, y: i32) -> (f32, f32) {
    (x as f32 * HALF_SQRT3 * S, y as f32 * 0.5 * S)
}

pub fn tile_pos(g: &Game, t: TileId) -> (f32, f32) {
    let (x, y) = g.board.tile_center(t);
    lattice(x, y)
}

pub fn vertex_pos(g: &Game, v: VertexId) -> (f32, f32) {
    let v = g.board.vertex(v);
    lattice(v.x, v.y)
}

pub fn edge_ends(g: &Game, e: EdgeId) -> ((f32, f32), (f32, f32)) {
    let [a, b] = g.board.edge(e).vertices;
    (vertex_pos(g, a), vertex_pos(g, b))
}

pub fn edge_mid(g: &Game, e: EdgeId) -> (f32, f32) {
    let (a, b) = edge_ends(g, e);
    ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)
}

/// Where a unit or robber stands on its tile: below the number token.
pub fn piece_pos(g: &Game, t: TileId) -> (f32, f32) {
    let (x, y) = tile_pos(g, t);
    (x, y + 0.44 * S)
}

/// Where a structure stands on its tile: in the middle, a little above the
/// spot a knight or bandit takes.
pub fn structure_pos(g: &Game, t: TileId) -> (f32, f32) {
    let (x, y) = tile_pos(g, t);
    (x, y - 0.12 * S)
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// Screen = world × scale + offset.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Camera {
    pub x: f32,
    pub y: f32,
    pub scale: f32,
    /// The scale that fits the board, for zoom limits.
    pub fit: f32,
}

impl Camera {
    pub fn to_world(self, sx: f32, sy: f32) -> (f32, f32) {
        ((sx - self.x) / self.scale, (sy - self.y) / self.scale)
    }

    /// Fits the board into a viewport `w` wide, in the band `(top, height)`
    /// the HUD leaves clear.
    pub fn fit(g: &Game, w: f32, (top, usable_h): (f32, f32)) -> Camera {
        let pts: Vec<(f32, f32)> = g.board.vertices.iter().map(|v| lattice(v.x, v.y)).collect();
        let min_x = pts.iter().map(|p| p.0).fold(f32::MAX, f32::min) - S * 0.4;
        let max_x = pts.iter().map(|p| p.0).fold(f32::MIN, f32::max) + S * 0.4;
        let min_y = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min) - S * 0.4;
        let max_y = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max) + S * 0.4;
        let scale = ((w - 16.0) / (max_x - min_x)).min(usable_h / (max_y - min_y)).max(0.05);
        Camera {
            x: w / 2.0 - (min_x + max_x) / 2.0 * scale,
            y: top + usable_h / 2.0 - (min_y + max_y) / 2.0 * scale,
            scale,
            fit: scale,
        }
    }

    /// Close on tile `t` in a viewport `w` wide, in the band `(top, height)`
    /// the HUD leaves clear: the tile and two rings of tiles around it fill
    /// the band, never smaller than the whole island.
    pub fn focus(g: &Game, t: TileId, w: f32, band: (f32, f32)) -> Camera {
        let fit = Camera::fit(g, w, band).fit;
        let (top, usable_h) = band;
        // Pointy-top hexes: five tiles across, and two rows of 1½ S plus a
        // corner above and below the middle.
        let (half_w, half_h) = (5.0 * HALF_SQRT3 * S + 0.2 * S, 4.2 * S);
        let scale = ((w - 16.0) / (2.0 * half_w)).min(usable_h / (2.0 * half_h)).clamp(fit, fit * 4.0);
        let (cx, cy) = tile_pos(g, t);
        Camera { x: w / 2.0 - cx * scale, y: top + usable_h / 2.0 - cy * scale, scale, fit }
    }

    /// The camera `k` (0..1) of the way to `to`, zooming about the one
    /// screen point both leave in place, so nothing swings sideways; with
    /// no change of scale, a straight pan. The scale changes evenly.
    fn toward(self, to: Camera, k: f32) -> Camera {
        let r = to.scale / self.scale;
        let f = r.powf(k);
        let scale = self.scale * f;
        if (r - 1.0).abs() < 1e-3 {
            let (x, y) = lerp((self.x, self.y), (to.x, to.y), k);
            return Camera { x, y, scale, fit: to.fit };
        }
        let still = ((to.x - r * self.x) / (1.0 - r), (to.y - r * self.y) / (1.0 - r));
        Camera { x: still.0 - (still.0 - self.x) * f, y: still.1 - (still.1 - self.y) * f, scale, fit: to.fit }
    }

    /// Zooms by `factor` keeping the screen point `(sx, sy)` still.
    pub fn zoom(&mut self, factor: f32, sx: f32, sy: f32) {
        let next = (self.scale * factor).clamp(self.fit * 0.6, self.fit * 4.0);
        let f = next / self.scale;
        self.x = sx - (sx - self.x) * f;
        self.y = sy - (sy - self.y) * f;
        self.scale = next;
    }
}

/// The band of an `h`-tall viewport the HUD leaves clear, a little apart
/// from it: its top and height. Before the HUD is measured, a guess.
fn clear_band(h: f32, hud: Hud) -> (f32, f32) {
    let (above, below) =
        if hud.measured() { (hud.top + FIT_MARGIN, hud.strip + hud.dock + FIT_MARGIN) } else { (FIT_TOP, FIT_BOTTOM) };
    let usable_h = (h - above - below).max(h * 0.4);
    let top = if h - above - below >= h * 0.4 { above } else { (h - usable_h) / 2.0 };
    (top, usable_h)
}

/// The tile under the world point `p`, if it is on the island.
fn tile_under(g: &Game, p: (f32, f32)) -> Option<TileId> {
    g.board
        .tile_ids()
        .map(|t| (dist(tile_pos(g, t), p), t))
        .filter(|(d, _)| *d <= S)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, t)| t)
}

// ----- What a tap can do -----

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Spot {
    Vertex(VertexId),
    Edge(EdgeId),
    Tile(TileId),
}

/// What tapping a highlighted place does.
#[derive(Clone, Debug, PartialEq)]
pub enum Then {
    /// Sends the action to the server.
    Send(Action),
    /// Picks this place and goes on choosing (a two-tile spell's first tile).
    Pick(Mode),
}

/// A highlighted place and what a tap on it does.
#[derive(Clone, Debug, PartialEq)]
pub struct Opt {
    pub spot: Spot,
    pub then: Then,
    /// An attack.
    pub hostile: bool,
}

/// Legal targets for the seat in the current mode. These only guide the
/// player; the server validates every action again.
pub fn options(g: &Game, seat: PlayerId, mode: Mode) -> Vec<Opt> {
    let opt = |spot, action| Opt { spot, then: Then::Send(action), hostile: false };
    if let Phase::Setup { placing, .. } = g.phase {
        return match placing {
            SetupPlacing::Building => {
                g.setup_vertices(seat).into_iter().map(|v| opt(Spot::Vertex(v), Action::SetupBuilding(v))).collect()
            }
            SetupPlacing::Road { .. } => {
                g.setup_edges(seat).into_iter().map(|e| opt(Spot::Edge(e), Action::SetupRoad(e))).collect()
            }
        };
    }
    if !g.is_active(seat) || g.dice.is_none() {
        return Vec::new();
    }
    let vertices = || g.board.vertices.iter().map(|v| v.id);
    match mode {
        // A 7 with no robber on the island: the roller places a new one.
        Mode::Idle if g.robber_relocation && g.robbers.is_empty() => g
            .board
            .tile_ids()
            .filter(|&t| g.can_place_robber(seat, t).is_ok())
            .map(|t| opt(Spot::Tile(t), Action::PlaceRobber { to: t }))
            .collect(),
        Mode::Idle => Vec::new(),
        Mode::Road => g
            .board
            .edges
            .iter()
            .filter(|e| g.can_build_road(seat, e.id).is_ok())
            .map(|e| opt(Spot::Edge(e.id), Action::BuildRoad(e.id)))
            .collect(),
        Mode::Outpost => vertices()
            .filter(|&v| g.can_build_outpost(seat, v).is_ok())
            .map(|v| opt(Spot::Vertex(v), Action::BuildOutpost(v)))
            .collect(),
        Mode::UpgradeTown => vertices()
            .filter(|&v| g.can_upgrade(seat, v, BuildingKind::Town).is_ok())
            .map(|v| opt(Spot::Vertex(v), Action::UpgradeTown(v)))
            .collect(),
        Mode::UpgradeCity => vertices()
            .filter(|&v| g.can_upgrade(seat, v, BuildingKind::City).is_ok())
            .map(|v| opt(Spot::Vertex(v), Action::UpgradeCity(v)))
            .collect(),
        // A rival's ward would fell the knight: asked first, in red.
        Mode::Deploy(kind) => g
            .deploy_tiles(seat)
            .into_iter()
            .filter(|&t| g.can_deploy(seat, kind, t).is_ok())
            .map(|t| Opt {
                spot: Spot::Tile(t),
                then: Then::Send(Action::Deploy { kind, tile: t }),
                hostile: g.warded_against(t, seat),
            })
            .collect(),
        Mode::Cast(spell, first) => {
            let tiles = || g.board.tile_ids();
            let cast = |picked: Vec<TileId>| Action::CastSpell { spell, tiles: picked };
            match (spell.targets(), first) {
                (1, _) => tiles()
                    .filter(|&t| g.can_cast(seat, spell, &[t]).is_ok())
                    .map(|t| opt(Spot::Tile(t), cast(vec![t])))
                    .collect(),
                (2, None) => tiles()
                    .filter(|&a| tiles().any(|b| g.can_cast(seat, spell, &[a, b]).is_ok()))
                    .map(|a| Opt { spot: Spot::Tile(a), then: Then::Pick(Mode::Cast(spell, Some(a))), hostile: false })
                    .collect(),
                (2, Some(a)) => tiles()
                    .filter(|&b| g.can_cast(seat, spell, &[a, b]).is_ok())
                    .map(|b| opt(Spot::Tile(b), cast(vec![a, b])))
                    .collect(),
                _ => Vec::new(),
            }
        }
        Mode::Unit(unit) => {
            let moves = g.move_targets(seat, unit).into_iter().map(|t| Opt {
                spot: Spot::Tile(t),
                then: Then::Send(Action::MoveUnit { unit, to: t }),
                hostile: g.warded_against(t, seat),
            });
            let attacks = g.attack_targets(seat, unit).into_iter().filter_map(|target| {
                let spot = match target {
                    Target::Robber(r) => Spot::Tile(g.robber(r)?.tile),
                    Target::Unit(u) => Spot::Tile(g.unit(u)?.tile),
                };
                Some(Opt { spot, then: Then::Send(Action::Attack { unit, target }), hostile: true })
            });
            moves.chain(attacks).collect()
        }
        Mode::Robber(robber) => g
            .board
            .tile_ids()
            .filter(|&t| g.can_relocate_robber(seat, robber, t).is_ok())
            .map(|t| opt(Spot::Tile(t), Action::RelocateRobber { robber, to: t }))
            .collect(),
    }
}

pub enum Tap {
    Act(Action, Mode),
    Select(Mode),
    /// An attack: show its outcome and let the player confirm it.
    Confirm(Action),
    Inspect(Option<Inspect>),
}

fn spot_distance(g: &Game, spot: Spot, p: (f32, f32)) -> f32 {
    // Distance as a fraction of each kind's reach, so the nearest wins fairly.
    match spot {
        Spot::Vertex(v) => dist(vertex_pos(g, v), p) / (0.42 * S),
        Spot::Edge(e) => dist(edge_mid(g, e), p) / (0.4 * S),
        Spot::Tile(t) => dist(tile_pos(g, t), p).min(dist(piece_pos(g, t), p)) / (0.86 * S),
    }
}

/// What a tap at world point `p` means.
pub fn resolve_tap(g: &Game, seat: PlayerId, mode: Mode, p: (f32, f32)) -> Tap {
    let opts = options(g, seat, mode);
    let best = opts
        .iter()
        .map(|o| (spot_distance(g, o.spot, p), o))
        .filter(|(d, _)| *d <= 1.0)
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, o)) = best {
        return match &o.then {
            Then::Pick(next) => Tap::Select(*next),
            Then::Send(action) if o.hostile => Tap::Confirm(action.clone()),
            Then::Send(action) => Tap::Act(action.clone(), if mode == Mode::Road { Mode::Road } else { Mode::Idle }),
        };
    }
    let my_move = g.is_active(seat) && g.dice.is_some();
    let tile = g.board.tile_ids().find(|&t| dist(tile_pos(g, t), p) <= 0.86 * S);
    if let Some(t) = tile {
        let near_piece = dist(piece_pos(g, t), p) <= 0.4 * S;
        if my_move && near_piece {
            if let Some(u) = g.unit_at(t).filter(|u| u.owner == seat && u.ready) {
                return Tap::Select(if mode == Mode::Unit(u.id) { Mode::Idle } else { Mode::Unit(u.id) });
            }
            if let Some(r) = g.robber_at(t).filter(|_| g.robber_relocation) {
                return Tap::Select(Mode::Robber(r.id));
            }
        }
    }
    let vertex = g
        .board
        .vertices
        .iter()
        .map(|v| v.id)
        .find(|&v| g.building(v).is_some() && dist(vertex_pos(g, v), p) <= 0.3 * S);
    if let Some(v) = vertex {
        return Tap::Inspect(Some(Inspect::Vertex(v)));
    }
    let edge = g.board.edges.iter().map(|e| e.id).find(|&e| g.road(e).is_some() && dist(edge_mid(g, e), p) <= 0.22 * S);
    if let Some(e) = edge {
        return Tap::Inspect(Some(Inspect::Edge(e)));
    }
    Tap::Inspect(tile.map(Inspect::Tile))
}

// ----- Animations -----

#[derive(Clone, Debug, PartialEq)]
pub enum Piece {
    Unit(UnitId),
    Robber(RobberId),
}

/// How a tile whose number came up takes it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Mood {
    /// Produces: swells and wobbles.
    Normal,
    /// A robber stands on it: shakes no, in red.
    Blocked,
    /// A spell stops it: shivers in the spell's colour.
    Stopped(Spell),
    /// A spell doubles it: swells big in the spell's colour.
    Doubled(Spell),
}

#[derive(Clone, Debug, PartialEq)]
pub enum AnimKind {
    /// A tile whose number came up, wobbling as its mood says.
    Jelly { tile: TileId, mood: Mood },
    /// A spell landing on a tile.
    Cast { tile: TileId, spell: Spell },
    /// Collected resources: a badge with the amount rising from the tile,
    /// `slot` of `of` badges side by side.
    Gain { tile: TileId, resource: Resource, amount: u32, color: u32, mine: bool, slot: u32, of: u32 },
    /// Victory points gained, rising from a building.
    Points { at: (f32, f32), amount: u32 },
    /// A label rising and fading.
    Float { at: (f32, f32), color: u32, label: String },
    /// A ring swelling out with sparks: something built or placed.
    Pop { at: (f32, f32), color: u32 },
    /// A burst: combat.
    Burst { at: (f32, f32) },
    /// A piece sliding to where it now stands.
    Slide { piece: Piece, from: (f32, f32) },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Anim {
    pub start: f64,
    pub dur: f64,
    pub kind: AnimKind,
}

impl Anim {
    pub fn end(&self) -> f64 {
        self.start + self.dur
    }

    /// Progress 0..=1 (0 before it starts).
    fn t(&self, now: f64) -> f32 {
        ((now - self.start) / self.dur).clamp(0.0, 1.0) as f32
    }

    fn running(&self, now: f64) -> bool {
        now >= self.start && now < self.end()
    }

    /// Started or about to: a slide holds its piece at the start until it runs.
    fn now_or_pending(&self, now: f64) -> bool {
        now < self.end()
    }
}

pub const JELLY_MS: f64 = 1000.0;
pub const GAIN_MS: f64 = 1900.0;

/// The animations an event other than a roll's starts at `start`.
pub fn anims_for(g: &Game, event: &Event, start: f64) -> Vec<Anim> {
    let a = |dur, kind| Anim { start, dur, kind };
    let color = palette::player;
    let points = |vertex: VertexId, amount: u32| {
        let (x, y) = vertex_pos(g, vertex);
        a(1500.0, AnimKind::Points { at: (x, y - 0.3 * S), amount })
    };
    match *event {
        Event::Built { player, vertex, kind } => {
            let mut v = vec![a(700.0, AnimKind::Pop { at: vertex_pos(g, vertex), color: color(player) })];
            if kind.points() > 0 {
                v.push(points(vertex, kind.points()));
            }
            v
        }
        Event::Upgraded { player, vertex, .. } => {
            vec![a(700.0, AnimKind::Pop { at: vertex_pos(g, vertex), color: color(player) }), points(vertex, 1)]
        }
        Event::BuiltRoad { player, edge } => {
            vec![a(550.0, AnimKind::Pop { at: edge_mid(g, edge), color: color(player) })]
        }
        Event::Deployed { player, tile, .. } => {
            vec![a(650.0, AnimKind::Pop { at: piece_pos(g, tile), color: color(player) })]
        }
        Event::RobberSpawned { tile, .. } | Event::RobberPlaced { tile, .. } => vec![
            a(800.0, AnimKind::Pop { at: piece_pos(g, tile), color: 0x222222 }),
            a(1300.0, AnimKind::Float { at: piece_pos(g, tile), color: 0x2b2b2e, label: "A bandit!".to_owned() }),
        ],
        Event::UnitMoved { unit, from, .. } => {
            vec![a(380.0, AnimKind::Slide { piece: Piece::Unit(unit), from: piece_pos(g, from) })]
        }
        Event::RobberWandered { robber, from, .. } | Event::RobberRelocated { robber, from, .. } => {
            vec![a(520.0, AnimKind::Slide { piece: Piece::Robber(robber), from: piece_pos(g, from) })]
        }
        Event::KnightKilledRobber { tile, .. } => {
            vec![a(650.0, AnimKind::Burst { at: piece_pos(g, tile) })]
        }
        Event::KnightsClashed { tiles: (x, y), .. } => {
            let mid = lerp(piece_pos(g, x), piece_pos(g, y), 0.5);
            vec![
                a(650.0, AnimKind::Burst { at: mid }),
                a(650.0, AnimKind::Burst { at: piece_pos(g, x) }),
                a(650.0, AnimKind::Burst { at: piece_pos(g, y) }),
            ]
        }
        Event::SpellCast { spell, ref tiles, .. } => {
            let (_, deep, _) = art::spell_colors(spell);
            let label = format!("{}!", crate::rules::spell_title(spell));
            // Lure lands where the robber goes.
            let struck: Vec<TileId> = match spell {
                Spell::Lure => tiles.get(1).copied().into_iter().collect(),
                _ => tiles.clone(),
            };
            let at = struck.first().map_or((0.0, 0.0), |&t| tile_pos(g, t));
            let mut v: Vec<Anim> = struck.into_iter().map(|tile| a(1200.0, AnimKind::Cast { tile, spell })).collect();
            v.push(Anim { start: start + 350.0, dur: 1400.0, kind: AnimKind::Float { at, color: deep, label } });
            v
        }
        Event::ItemAppeared { tile } => {
            let (at, label) = (piece_pos(g, tile), "Something appeared!".to_owned());
            vec![
                a(800.0, AnimKind::Pop { at, color: palette::GOLD }),
                a(1300.0, AnimKind::Float { at, color: palette::GOLD, label }),
            ]
        }
        Event::ItemFound { player, tile, kind, count, gained, scroll } => {
            let at = piece_pos(g, tile);
            let label = crate::rules::item_found(kind, count, &gained, scroll);
            let mut v = vec![
                a(700.0, AnimKind::Pop { at, color: color(player) }),
                a(1700.0, AnimKind::Float { at, color: color(player), label }),
            ];
            if kind == ItemKind::Trap {
                v.push(a(650.0, AnimKind::Burst { at }));
            }
            v
        }
        Event::ItemVanished { tile } | Event::ItemStolen { tile } | Event::RelicDropped { tile, .. } => {
            let label = match *event {
                Event::ItemVanished { .. } => "Vanished",
                Event::ItemStolen { .. } => "Stolen by a bandit!",
                _ => "Relic dropped",
            };
            vec![a(1300.0, AnimKind::Float { at: piece_pos(g, tile), color: 0x2b2b2e, label: label.to_owned() })]
        }
        Event::BlockadeLaid { player, tile, .. }
        | Event::BlockadeLifting { player, tile, .. }
        | Event::BlockadeEnded { player, tile, .. } => {
            let at = piece_pos(g, tile);
            let label = match *event {
                Event::BlockadeLaid { .. } => "Blockade!",
                Event::BlockadeLifting { .. } => "Packing up",
                _ => "Blockade lifted",
            };
            vec![
                a(700.0, AnimKind::Pop { at, color: color(player) }),
                a(1300.0, AnimKind::Float { at, color: color(player), label: label.to_owned() }),
            ]
        }
        Event::StructureTaken { player, tile, kind, .. } => {
            let at = structure_pos(g, tile);
            let mut v = vec![a(700.0, AnimKind::Pop { at, color: color(player) })];
            let later = |dur, kind| Anim { start: start + 200.0, dur, kind };
            v.push(match kind {
                StructureKind::Shrine => later(1500.0, AnimKind::Points { at, amount: balance::SHRINE_POINTS }),
                _ => {
                    let label = format!("{} taken!", crate::rules::structure_title(kind));
                    later(1300.0, AnimKind::Float { at, color: color(player), label })
                }
            });
            v
        }
        Event::StructureSeized { tile, kind, .. } => {
            let label = format!("{} seized!", crate::rules::structure_title(kind));
            vec![a(1300.0, AnimKind::Float { at: structure_pos(g, tile), color: 0x2b2b2e, label })]
        }
        Event::WardStruck { caster, tile, .. } => {
            let at = piece_pos(g, tile);
            let (_, deep, _) = art::spell_colors(Spell::Ward);
            vec![
                a(650.0, AnimKind::Burst { at }),
                a(1300.0, AnimKind::Float { at, color: deep, label: "Warded!".to_owned() }),
                a(700.0, AnimKind::Pop { at, color: color(caster) }),
            ]
        }
        _ => Vec::new(),
    }
}

// ----- Drawing -----

pub struct Scene<'a> {
    pub game: &'a Game,
    pub seat: PlayerId,
    pub cam: Camera,
    pub size: (f32, f32),
    pub opts: &'a [Opt],
    pub mode: Mode,
    pub inspect: Option<Inspect>,
    pub anims: &'a [Anim],
    pub now: f64,
}

impl<'a> Scene<'a> {
    /// The board alone: nothing highlighted, chosen or animating.
    fn bare(game: &'a Game, seat: PlayerId, cam: Camera, size: (f32, f32)) -> Scene<'a> {
        Scene { game, seat, cam, size, opts: &[], mode: Mode::Idle, inspect: None, anims: &[], now: 0.0 }
    }
}

const SAND: u32 = 0xdcc893;

/// Tiles smaller on screen than this radius (points) draw simpler scenery.
const DETAIL_RADIUS: f32 = 30.0;

/// Whether the world point `at` is within `margin` world units of the screen.
fn on_screen(s: &Scene, at: (f32, f32), margin: f32) -> bool {
    let (w, h) = s.size;
    let (x, y) = (s.cam.x + at.0 * s.cam.scale, s.cam.y + at.1 * s.cam.scale);
    let m = margin * s.cam.scale;
    x > -m && x < w + m && y > -m && y < h + m
}

/// `p` moved into world space.
fn world(s: &Scene, p: Painter) -> Painter {
    p.save()
        .translate(s.cam.x, s.cam.y)
        .scale(s.cam.scale, s.cam.scale)
        .line_join(LineJoin::Round)
        .line_cap(LineCap::Round)
}

/// The tiles some animation wobbles, now or soon: the spells layer draws
/// them, the ground leaves them out.
pub fn jelly_tiles(anims: &[Anim]) -> Vec<TileId> {
    let mut tiles: Vec<TileId> = anims
        .iter()
        .filter_map(|a| match a.kind {
            AnimKind::Jelly { tile, .. } => Some(tile),
            _ => None,
        })
        .collect();
    tiles.sort_unstable_by_key(|t| t.0);
    tiles.dedup();
    tiles
}

/// The bottom layer, which changes only with the game and the camera: the
/// sea, the beach and every tile but those in `skip`. Off-screen tiles are
/// left out, and small ones get simpler scenery.
pub fn draw_ground(s: &Scene, skip: &[TileId]) -> Painter {
    let g = s.game;
    let (w, h) = s.size;
    let px1 = 1.0 / s.cam.scale; // one screen point in world units

    // The sea, brightest around the island (the world origin is its center).
    let center = point(px(s.cam.x), px(s.cam.y));
    let sea = CanvasGradient::radial(center, px(0.0), center, px(w.max(h) * 0.9))
        .add_color_stop(percentage(0.0), hex(0x3c8597))
        .add_color_stop(percentage(0.5), hex(0x2a6475))
        .add_color_stop(percentage(1.0), hex(0x163845));
    let mut p = world(s, Painter::new().fill_style(sea).fill_rect(0.0, 0.0, w, h));
    let rings = g.board.tiles.iter().map(|t| t.q.abs().max(t.r.abs()).max((t.q + t.r).abs())).max().unwrap_or(2);
    p = waves(p, px1, rings as f32);

    // Shallows and the beach.
    let visible: Vec<TileId> = g.board.tile_ids().filter(|&t| on_screen(s, tile_pos(g, t), 1.4 * S)).collect();
    for &t in &visible {
        p = poly(p, &hex_points(tile_pos(g, t), S * 1.34)).fill_color(hexa(0x8fd3d0, 0.22)).fill();
    }
    for &t in &visible {
        p = poly(p, &hex_points(tile_pos(g, t), S * 1.14)).fill_color(hex(SAND)).fill();
    }
    for &t in &visible {
        p = poly(p, &hex_points(tile_pos(g, t), S * 1.04)).fill_color(hex(0xc9b47e)).fill();
    }
    let detail = S * s.cam.scale >= DETAIL_RADIUS;
    for &t in visible.iter().filter(|t| !skip.contains(t)) {
        p = tile_art(p, g, t, px1, detail);
    }
    p.restore()
}

/// The middle layer, the only one that moves on its own: tiles whose number
/// came up wobble, and spells shimmer on their tiles. It redraws every tick
/// of the clock, so it holds only what moves; the spells' seals, which do
/// not, are on the top layer. Tiles off screen are left out.
pub fn draw_spells(s: &Scene) -> Painter {
    let g = s.game;
    let px1 = 1.0 / s.cam.scale;
    let mut p = world(s, Painter::new());
    let seen = |t: TileId| on_screen(s, tile_pos(g, t), S);
    for t in jelly_tiles(s.anims).into_iter().filter(|&t| seen(t)) {
        let running = s.anims.iter().filter(|a| a.running(s.now)).find_map(|a| match a.kind {
            AnimKind::Jelly { tile, mood } if tile == t => Some((a.t(s.now), mood)),
            _ => None,
        });
        p = match running {
            Some((k, mood)) => wobbling_tile(p, g, t, k, mood, px1),
            None => tile_art(p, g, t, px1, true),
        };
    }
    for (e, t) in spelled(g).filter(|&(_, t)| seen(t)) {
        p = enchanted_tile(p, g, e, t, s.now, px1);
    }
    if let Some(t) = g.luck.filter(|&t| seen(t)) {
        p = lucky_tile(p, g, t, s.now, px1);
    }
    p.restore()
}

/// Every tile under a spell, with its spell: both tiles of a swap.
fn spelled(g: &Game) -> impl Iterator<Item = (&Enchantment, TileId)> {
    g.enchantments.iter().flat_map(|e| std::iter::once(e.tile).chain(e.partner).map(move |t| (e, t)))
}

/// The top layer: highlights, roads, buildings, pieces and the effects of
/// events. It redraws with the player's choices, and every frame only while
/// something animates.
pub fn draw_pieces(s: &Scene) -> Painter {
    let g = s.game;
    let px1 = 1.0 / s.cam.scale;
    let mut p = world(s, Painter::new());

    // Each spell's seal, counting the rolls it has left.
    for (e, t) in spelled(g) {
        p = spell_seal(p, g, e, t, px1);
    }

    // Tile highlights.
    for o in s.opts {
        if let Spot::Tile(t) = o.spot {
            let color = if o.hostile { palette::DANGER } else { palette::GOLD };
            let c = tile_pos(g, t);
            p = poly(p, &hex_points(c, S * 0.88))
                .fill_color(hexa(color, 0.28))
                .fill()
                .stroke_color(hex(color))
                .line_width(4.0 * px1)
                .stroke();
        }
    }

    // Tile outlines: the tile looked at, and a two-tile spell's first pick.
    // Under the roads and buildings, and set out past the roads on the
    // tile's sides so they stay in sight.
    let outline = |t: TileId| hex_points(tile_pos(g, t), S * 1.14);
    if let Mode::Cast(spell, Some(t)) = s.mode {
        let (_, _, glow) = art::spell_colors(spell);
        p = poly(p, &outline(t)).stroke_color(hex(glow)).line_width(5.0 * px1).stroke();
        p = poly(p, &outline(t)).stroke_color(hex(0xffffff)).line_width(2.0 * px1).stroke();
    }
    if let Some(Inspect::Tile(t)) = s.inspect {
        p = poly(p, &outline(t)).stroke_color(hex(0xffffff)).line_width(3.0 * px1).stroke();
    }

    // Each ward's reach: its tile and those around, ringed in its colour.
    for e in g.enchantments.iter().filter(|e| e.spell == Spell::Ward) {
        let (_, deep, glow) = art::spell_colors(Spell::Ward);
        for t in std::iter::once(e.tile).chain(g.board.tile(e.tile).neighbors.iter().copied()) {
            let ring = hex_points(tile_pos(g, t), S * 0.93);
            p = poly(p, &ring).fill_color(hexa(glow, 0.12)).fill();
            p = poly(p, &ring).stroke_color(hexa(deep, 0.75)).line_width(2.5 * px1).stroke();
        }
    }

    // Structures: a held one rings its tile in the holder's colour and
    // flies it; a bandit's flies dark. Never smaller on screen than a badge.
    let u = (0.24 * S).max(7.0 * px1);
    for s in &g.structures {
        let holder = g.structure_holder(s.tile);
        if let Some(owner) = holder {
            p = poly(p, &hex_points(tile_pos(g, s.tile), S * 0.9))
                .stroke_color(hex(palette::player(owner)))
                .line_width(4.0 * px1)
                .stroke();
        }
        let mark = match holder {
            Some(owner) => palette::player(owner),
            None if g.robber_at(s.tile).is_some() => 0x2b2b2e,
            None => palette::GOLD,
        };
        let at = structure_pos(g, s.tile);
        p = match s.kind {
            StructureKind::Shrine => art::shrine(p, at, u, mark, px1),
            StructureKind::Mine => art::mine(p, at, u, mark, px1),
            StructureKind::LumberMill => art::lumber_mill(p, at, u, mark, px1),
        };
    }

    // Roads.
    for (i, owner) in g.roads.iter().enumerate() {
        if let Some(owner) = owner {
            let (a, b) = edge_ends(g, EdgeId(i as u16));
            p = art::road(p, lerp(a, b, 0.17), lerp(a, b, 0.83), palette::player(*owner), 0.2 * S);
        }
    }
    for o in s.opts {
        if let Spot::Edge(e) = o.spot {
            let (a, b) = edge_ends(g, e);
            let (a, b) = (lerp(a, b, 0.2), lerp(a, b, 0.8));
            p = p
                .begin_path()
                .move_to(a.0, a.1)
                .line_to(b.0, b.1)
                .stroke_color(hexa(palette::GOLD, 0.55))
                .line_width(0.2 * S)
                .stroke()
                .begin_path()
                .move_to(a.0, a.1)
                .line_to(b.0, b.1)
                .stroke_color(hexa(0xffffff, 0.95))
                .line_width(0.09 * S)
                .stroke();
        }
    }

    // Buildings.
    for (i, b) in g.buildings.iter().enumerate() {
        if let Some(b) = b {
            let at = vertex_pos(g, VertexId(i as u16));
            p = art::building(p, at, b.kind, palette::player(b.owner), 0.155 * S, 2.0 * px1);
        }
    }
    for o in s.opts {
        if let Spot::Vertex(v) = o.spot {
            let color = if o.hostile { palette::DANGER } else { palette::GOLD };
            let built = g.building(v).is_some();
            let r = if built { 0.44 * S } else { 0.17 * S };
            p = circle(p, vertex_pos(g, v), r)
                .fill_color(hexa(0xffffff, if built { 0.0 } else { 0.85 }))
                .fill()
                .stroke_color(hex(color))
                .line_width(3.5 * px1)
                .stroke();
        }
    }

    // Items waiting for a knight: a chest, or a relic everyone saw drop,
    // with the rounds it has left.
    for item in &g.items {
        let at = piece_pos(g, item.tile);
        // As big as a knight, shrinking with the board when zoomed out to a
        // few points; its badge, off its upper right, stays readable.
        let u = (0.25 * S).max(5.0 * px1);
        p = match item.kind {
            Some(ItemKind::Relic) => art::relic(p, at, 0.88 * u, 1.6 * px1),
            _ => art::chest(p, at, u, 1.6 * px1),
        };
        let left = (item.last_round + 1).saturating_sub(g.round);
        p = count_badge(p, (at.0 + 1.05 * u + 4.0 * px1, at.1 - 0.55 * u - 3.0 * px1), left, palette::INK, px1);
    }

    // Units and robbers (sliding when they just moved).
    let slide_pos = |piece: &Piece, to: (f32, f32)| {
        s.anims
            .iter()
            .filter(|a| a.now_or_pending(s.now))
            .find_map(|a| match &a.kind {
                AnimKind::Slide { piece: q, from } if q == piece => Some(lerp(*from, to, ease_out(a.t(s.now)))),
                _ => None,
            })
            .unwrap_or(to)
    };
    for r in &g.robbers {
        p = art::bandit(p, slide_pos(&Piece::Robber(r.id), piece_pos(g, r.tile)), 0.22 * S, 1.6 * px1);
    }
    let my_turn = g.is_active(s.seat) && g.dice.is_some();
    for u in &g.units {
        let at = slide_pos(&Piece::Unit(u.id), piece_pos(g, u.tile));
        if let Some(b) = u.blockade {
            p = palisade(p, g, u.tile, u.owner, b, px1);
        }
        p = art::knight(p, at, palette::player(u.owner), 0.25 * S, 2.0 * px1);
        if u.relics > 0 {
            p = art::relic(p, (at.0 - 0.26 * S, at.1 + 0.12 * S), 0.11 * S, px1);
        }
        if my_turn && u.owner == s.seat && u.ready {
            p = circle(p, (at.0 + 0.24 * S, at.1 - 0.24 * S), 0.08 * S)
                .fill_color(hex(palette::GOLD))
                .fill()
                .stroke_color(hex(palette::INK))
                .line_width(1.5 * px1)
                .stroke();
        }
        if s.mode == Mode::Unit(u.id) {
            p = circle(p, at, 0.38 * S).stroke_color(hex(0xffffff)).line_width(3.0 * px1).stroke();
        }
    }
    if let Mode::Robber(r) = s.mode
        && let Some(r) = g.robber(r)
    {
        p = circle(p, piece_pos(g, r.tile), 0.38 * S).stroke_color(hex(0xffffff)).line_width(3.0 * px1).stroke();
    }
    // A robber waiting to be placed: a faint one on every tile it may go.
    for o in s.opts {
        if let (Spot::Tile(t), Then::Send(Action::PlaceRobber { .. })) = (o.spot, &o.then) {
            p = art::bandit(p.global_alpha(0.45), piece_pos(g, t), 0.22 * S, 1.6 * px1).global_alpha(1.0);
        }
    }

    // The building or road looked at.
    let ring = match s.inspect {
        Some(Inspect::Vertex(v)) => Some((vertex_pos(g, v), 0.4 * S)),
        Some(Inspect::Edge(e)) => Some((edge_mid(g, e), 0.26 * S)),
        Some(Inspect::Tile(_)) | None => None,
    };
    if let Some((at, r)) = ring {
        p = circle(p, at, r).stroke_color(hex(0xffffff)).line_width(3.0 * px1).stroke();
    }

    // Effects, at screen size whatever the zoom.
    for a in s.anims.iter().filter(|a| a.running(s.now)) {
        let t = a.t(s.now);
        p = match &a.kind {
            AnimKind::Gain { tile, resource, amount, color, mine, slot, of } => {
                let c = tile_pos(g, *tile);
                let k = if *mine { 1.15 } else { 0.85 };
                let x = c.0 + (*slot as f32 - (*of as f32 - 1.0) / 2.0) * 96.0 * k * px1;
                let y = c.1 - 0.1 * S - ease_out(t) * 70.0 * px1;
                let pop = ease_out_back(phase(t, 0.0, 0.16)).max(0.05);
                let p = p.global_alpha(1.0 - phase(t, 0.75, 1.0));
                gain_badge(p, (x, y), *resource, *amount, *color, *mine, px1 * k * pop, px1).global_alpha(1.0)
            }
            AnimKind::Points { at, amount } => {
                let y = at.1 - ease_out(t) * 56.0 * px1;
                let pop = ease_out_back(phase(t, 0.0, 0.2)).max(0.05);
                let p = p.global_alpha(1.0 - phase(t, 0.75, 1.0));
                points_badge(p, (at.0, y), *amount, px1 * pop, px1).global_alpha(1.0)
            }
            AnimKind::Float { at, color, label } => {
                let y = at.1 - ease_out(t) * 44.0 * px1;
                let p = p.global_alpha(1.0 - phase(t, 0.7, 1.0));
                let w = (label.chars().count() as f32 * 9.0 + 24.0) * px1;
                let p = art::rrect(p, (at.0, y), w, 28.0 * px1, 14.0 * px1).fill_color(hex(*color)).fill();
                text_at(p, label, (at.0, y), 15.0 * px1, palette::ink_on(*color), 800, px1).global_alpha(1.0)
            }
            AnimKind::Pop { at, color } => {
                let e = ease_out(t);
                let mut p = circle(p, *at, (0.15 + 0.65 * e) * S)
                    .stroke_color(hexa(*color, 1.0 - t))
                    .line_width(5.0 * px1 * (1.0 - t) + px1)
                    .stroke();
                for k in 0..10 {
                    let ang = k as f32 * TAU / 10.0 + 0.3;
                    let r = (0.25 + 0.7 * e) * S;
                    let spark = (at.0 + r * ang.cos(), at.1 + r * ang.sin());
                    p = art::star(p, spark, (1.0 - t) * 6.0 * px1)
                        .fill_color(hexa(if k % 2 == 0 { palette::GOLD } else { 0xffffff }, 1.0 - t))
                        .fill();
                }
                p
            }
            AnimKind::Burst { at } => {
                let mut p = p.stroke_color(hexa(palette::DANGER, 1.0 - t)).line_width(4.0 * px1);
                for k in 0..10 {
                    let ang = k as f32 * TAU / 10.0;
                    let (r0, r1) = ((0.1 + 0.35 * t) * S, (0.3 + 0.55 * ease_out(t)) * S);
                    p = p
                        .begin_path()
                        .move_to(at.0 + r0 * ang.cos(), at.1 + r0 * ang.sin())
                        .line_to(at.0 + r1 * ang.cos(), at.1 + r1 * ang.sin())
                        .stroke();
                }
                circle(p, *at, 0.28 * S * (1.0 - t)).fill_color(hexa(palette::GOLD, 1.0 - t)).fill()
            }
            AnimKind::Cast { tile, spell } => {
                let c = tile_pos(g, *tile);
                let e = ease_out(t);
                let (_, _, glow) = art::spell_colors(*spell);
                let mut p = poly(p, &hex_points(c, S * 0.96)).fill_color(hexa(0xffffff, 0.6 * (1.0 - t))).fill();
                p = poly(p, &hex_points(c, S * (0.9 + 0.9 * e)))
                    .stroke_color(hexa(glow, 1.0 - t))
                    .line_width(6.0 * px1 * (1.0 - t) + px1)
                    .stroke();
                for k in 0..12 {
                    let ang = k as f32 * TAU / 12.0 + 0.2;
                    let r = (0.3 + 1.1 * e) * S;
                    let at = (c.0 + r * ang.cos(), c.1 + r * ang.sin());
                    p = match spell {
                        Spell::Frost => art::snowflake(p, at, 0.13 * S * (1.0 - t) + px1, 0xffffff, 1.6 * px1),
                        _ => art::sparkle(p, at, 0.14 * S * (1.0 - t) + px1)
                            .fill_color(hexa(if k % 2 == 0 { glow } else { 0xffffff }, 1.0 - t * 0.6))
                            .fill(),
                    };
                }
                // The spell's seal pops in at the centre, then fades into the tile.
                let pop = ease_out_back(phase(t, 0.0, 0.35)).max(0.01);
                let p = p.global_alpha(1.0 - phase(t, 0.7, 1.0));
                art::spell_emblem(p, *spell, c, 0.42 * S * pop, 2.0 * px1).global_alpha(1.0)
            }
            AnimKind::Jelly { .. } | AnimKind::Slide { .. } => p,
        };
    }
    p.restore()
}

/// Wave marks in a band of sea around an island of `rings` rings.
fn waves(mut p: Painter, px1: f32, rings: f32) -> Painter {
    p = p.stroke_color(hexa(0xffffff, 0.13)).line_width(2.0 * px1);
    // Each ring widens the island by about √3 tile sizes.
    let shore = 5.3 + (rings - 2.0) * 1.73;
    for i in 0..(44.0 * (rings + 3.0) / 5.0) as u32 {
        let ang = hash01(i) * TAU;
        let d = (shore + hash01(i + 100) * 4.0) * S;
        let (x, y) = (d * ang.cos() * 1.1, d * ang.sin());
        let r = (0.16 + hash01(i + 200) * 0.12) * S;
        p = p.begin_path().arc(x, y, r, PI * 1.15, PI * 1.85).stroke();
        p = p.begin_path().arc(x + r * 1.3, y + r * 0.1, r * 0.8, PI * 1.2, PI * 1.8).stroke();
    }
    p
}

/// Places on a tile for scenery, clear of the token and the piece spot.
const SPOTS: [(f32, f32); 8] =
    [(-0.5, -0.42), (0.5, -0.42), (-0.2, -0.62), (0.22, -0.6), (-0.64, 0.1), (0.64, 0.1), (-0.44, 0.5), (0.44, 0.5)];

/// The ground of a structure's tile: a bare clearing of trodden earth.
const CLEARING: u32 = 0xc9bc92;

/// One tile: terrain, its scenery (simpler without `detail`) and its number
/// token; a structure's tile is a bare clearing, with neither.
fn tile_art(mut p: Painter, g: &Game, t: TileId, px1: f32, detail: bool) -> Painter {
    let tile = g.board.tile(t);
    let c = tile_pos(g, t);
    let barren = g.structure_at(t).is_some();
    let base = if barren { CLEARING } else { palette::resource(tile.resource) };
    let outline = hex_points(c, S * 0.96);
    p = poly(p, &outline)
        .fill_style(art::vgrad(c.1 - S, c.1 + S, palette::lighten(base, 0.2), palette::shade(base, 0.8)))
        .fill();
    if barren {
        p = art::ellipse(p, (c.0, c.1 + 0.1 * S), 0.7 * S, 0.5 * S, 0.0).fill_color(hexa(0xffffff, 0.16)).fill();
    } else {
        p = poly(p.save(), &outline).clip();
        p = scenery(p, tile.resource, c, t.0 as u32, base, px1, detail).restore();
    }
    p = poly(p, &hex_points(c, S * 0.9)).stroke_color(hexa(0xffffff, 0.2)).line_width(2.0 * px1).stroke();
    p = poly(p, &outline).stroke_color(hex(palette::shade(base, 0.6))).line_width(1.5 * px1).stroke();
    let robbed = g.robber_at(t).is_some();
    if !barren {
        p = token(p, (c.0, c.1 - 0.08 * S), tile.number, robbed, px1);
    }
    if robbed {
        p = poly(p, &outline).fill_color(hexa(0x000000, 0.25)).fill();
    }
    p
}

/// A tile whose number came up: it swells, wobbles and glows; blocked by a
/// robber it shakes in red; under a spell it shivers or swells in the
/// spell's colour. `k` runs 0..1 through it.
fn wobbling_tile(p: Painter, g: &Game, t: TileId, k: f32, mood: Mood, px1: f32) -> Painter {
    let c = tile_pos(g, t);
    let fade = 1.0 - k;
    let (dx, sx, sy) = match mood {
        Mood::Blocked => ((k * TAU * 4.0).sin() * fade * 0.1 * S, 1.0, 1.0),
        Mood::Stopped(_) => ((k * TAU * 9.0).sin() * fade * 0.05 * S, 1.0, 1.0),
        Mood::Normal | Mood::Doubled(_) => {
            let size = if mood == Mood::Normal { 0.14 } else { 0.24 };
            let swell = size * (PI * phase(k, 0.0, 0.3)).sin();
            let wobble = 0.13 * fade * fade * (k * TAU * 3.0).sin();
            (0.0, 1.0 + swell + wobble, 1.0 + swell - wobble)
        }
    };
    let (tint, glow) = match mood {
        Mood::Normal => (0xffffff, palette::GOLD),
        Mood::Blocked => (palette::DANGER, palette::DANGER),
        Mood::Stopped(s) | Mood::Doubled(s) => (art::spell_colors(s).2, art::spell_colors(s).2),
    };
    let mut p = p.save().translate(c.0 + dx, c.1).scale(sx, sy).translate(-c.0, -c.1);
    p = art::shadow(p, 0.45 * fade, 14.0, 4.0);
    p = poly(p, &hex_points(c, S * 0.96)).fill_color(hex(SAND)).fill();
    p = art::no_shadow(p);
    p = tile_art(p, g, t, px1, true);
    p = poly(p, &hex_points(c, S * 0.96))
        .fill_color(hexa(tint, 0.35 * fade))
        .fill()
        .stroke_color(hexa(glow, fade))
        .line_width(6.0 * px1)
        .stroke();
    p = p.restore();
    // A shockwave rolling off the tile.
    let wave = phase(k, 0.0, 0.6);
    if wave < 1.0 {
        p = poly(p, &hex_points(c, S * (1.0 + 0.7 * ease_out(wave))))
            .stroke_color(hexa(glow, 0.8 * (1.0 - wave)))
            .line_width(5.0 * px1)
            .stroke();
    }
    p
}

/// A tile under a spell, `tile` being the spell's own or (for a swap) its
/// partner. Each spell has a look of its own that moves with `now` (ms).
fn enchanted_tile(mut p: Painter, g: &Game, e: &Enchantment, tile: TileId, now: f64, px1: f32) -> Painter {
    let c = tile_pos(g, tile);
    let t = ((now / 1000.0) % 10_000.0) as f32;
    let seed = tile.0 as u32 * 37;
    let outline = hex_points(c, S * 0.96);
    let (fill, deep, glow) = art::spell_colors(e.spell);
    let pulse = 0.5 + 0.5 * (t * 2.4).sin();
    p = poly(p.save(), &outline).clip();
    match e.spell {
        Spell::Frost => {
            p = poly(p, &outline).fill_color(hexa(fill, 0.5)).fill();
            p = p.stroke_color(hexa(0xffffff, 0.8)).line_width(1.6 * px1);
            for i in 0..14 {
                let a = i as f32 * TAU / 14.0 + hash01(seed + i) * 0.3;
                let (r0, r1) = (0.98 * S, (0.6 + 0.18 * hash01(seed + i + 20)) * S);
                let (from, to) = ((c.0 + a.cos() * r0, c.1 + a.sin() * r0), (c.0 + a.cos() * r1, c.1 + a.sin() * r1));
                p = p.begin_path().move_to(from.0, from.1).line_to(to.0, to.1).stroke();
                let mid = lerp(from, to, 0.5);
                for side in [-0.6, 0.6] {
                    let b = a + PI + side;
                    let len = 0.1 * S;
                    p = p
                        .begin_path()
                        .move_to(mid.0, mid.1)
                        .line_to(mid.0 + b.cos() * len, mid.1 + b.sin() * len)
                        .stroke();
                }
            }
            for i in 0..9 {
                let x = c.0 + (hash01(seed + i + 40) - 0.5) * 1.6 * S + (t * 1.3 + i as f32).sin() * 0.06 * S;
                let fall = (t * 0.22 + hash01(seed + i + 60)) % 1.0;
                let y = c.1 - S + fall * 2.0 * S;
                let r = (0.05 + 0.03 * hash01(seed + i + 80)) * S;
                p = art::snowflake(p, (x, y), r, 0xffffff, 1.3 * px1);
            }
        }
        Spell::Bounty => {
            p = poly(p, &outline).fill_color(hexa(0xfff0a0, 0.28 + 0.2 * pulse)).fill();
            p = light_rays(p, c, t, 0xffffff, 0.22 + 0.12 * pulse);
            p = rising_motes(p, c, t, seed, 0xffe066);
            p = twinkles(p, c, t, seed, 0xffffff);
        }
        Spell::Swap => {
            p = poly(p, &outline).fill_color(hexa(glow, 0.2 + 0.1 * pulse)).fill();
            // Two arrows chasing each other round the number.
            p = p.stroke_color(hex(deep)).fill_color(hex(deep)).line_width(4.0 * px1);
            for k in 0..2 {
                let start = t * 1.2 + k as f32 * PI;
                let end = start + PI * 0.7;
                let r = 0.6 * S;
                p = p.begin_path().arc(c.0, c.1, r, start, end).stroke();
                let tip = (c.0 + end.cos() * r, c.1 + end.sin() * r);
                let back = end - 0.3;
                let (o, i) = (r + 0.12 * S, r - 0.12 * S);
                p = poly(
                    p,
                    &[tip, (c.0 + back.cos() * o, c.1 + back.sin() * o), (c.0 + back.cos() * i, c.1 + back.sin() * i)],
                )
                .fill();
            }
        }
        Spell::Ward => {
            p = poly(p, &outline).fill_color(hexa(glow, 0.22 + 0.14 * pulse)).fill();
            p = twinkles(p, c, t, seed, 0xffffff);
        }
        Spell::Lure | Spell::Windfall | Spell::Luck => {}
    }
    p = p.restore();
    // A bright rim in the spell's glow, lined in white.
    if e.spell != Spell::Frost {
        p = poly(p, &outline).stroke_color(hex(glow)).line_width(5.0 * px1).stroke();
    }
    p = poly(p, &hex_points(c, S * 0.92)).stroke_color(hexa(0xffffff, 0.9)).line_width(2.5 * px1).stroke();
    // The number stays readable above the spell; a structure's tile has none.
    if g.structure_at(tile).is_some() {
        return p;
    }
    let number = g.board.tile(tile).number;
    token(p, (c.0, c.1 - 0.08 * S), number, g.robber_at(tile).is_some(), px1)
}

/// The tile a cast Luck favours until the next roll: a green shimmer and a
/// clover seal on its upper left.
fn lucky_tile(mut p: Painter, g: &Game, tile: TileId, now: f64, px1: f32) -> Painter {
    let c = tile_pos(g, tile);
    let t = ((now / 1000.0) % 10_000.0) as f32;
    let outline = hex_points(c, S * 0.96);
    let (_, _, glow) = art::spell_colors(Spell::Luck);
    let pulse = 0.5 + 0.5 * (t * 2.4).sin();
    p = poly(p.save(), &outline).clip();
    p = poly(p, &outline).fill_color(hexa(glow, 0.2 + 0.15 * pulse)).fill();
    p = twinkles(p, c, t, tile.0 as u32 * 37, 0xffffff);
    p = p.restore();
    p = poly(p, &outline).stroke_color(hex(glow)).line_width(5.0 * px1).stroke();
    p = poly(p, &hex_points(c, S * 0.92)).stroke_color(hexa(0xffffff, 0.9)).line_width(2.5 * px1).stroke();
    p = token(p, (c.0, c.1 - 0.08 * S), g.board.tile(tile).number, g.robber_at(tile).is_some(), px1);
    p = p.save().translate(c.0 - 0.5 * S, c.1 - 0.5 * S).scale(px1, px1);
    p = art::shadow(p, 0.45, 6.0, 2.0);
    p = art::spell_emblem(p, Spell::Luck, (0.0, 0.0), 11.0, 1.5);
    art::no_shadow(p).restore()
}

/// Slowly turning rays of light from a tile's centre.
fn light_rays(mut p: Painter, c: (f32, f32), t: f32, color: u32, alpha: f32) -> Painter {
    p = p.save().translate(c.0, c.1).rotate(t * 0.35);
    for i in 0..8 {
        let a = i as f32 * TAU / 8.0;
        let r = 1.3 * S;
        p = poly(
            p,
            &[(0.0, 0.0), (r * (a - 0.13).cos(), r * (a - 0.13).sin()), (r * (a + 0.13).cos(), r * (a + 0.13).sin())],
        )
        .fill_color(hexa(color, alpha))
        .fill();
    }
    p.restore()
}

/// Motes of light drifting up a tile.
fn rising_motes(mut p: Painter, c: (f32, f32), t: f32, seed: u32, color: u32) -> Painter {
    for i in 0..8 {
        let x = c.0 + (hash01(seed + i + 40) - 0.5) * 1.5 * S;
        let rise = (t * 0.2 + hash01(seed + i + 60)) % 1.0;
        let y = c.1 + S - rise * 2.0 * S;
        p = circle(p, (x, y), 0.04 * S).fill_color(hexa(color, 1.0 - rise)).fill();
    }
    p
}

/// Sparkles winking on and off across a tile.
fn twinkles(mut p: Painter, c: (f32, f32), t: f32, seed: u32, color: u32) -> Painter {
    for i in 0..5 {
        let a = hash01(seed + i + 100) * TAU;
        let d = (0.25 + 0.45 * hash01(seed + i + 120)) * S;
        let twinkle = (t * 2.5 + hash01(seed + i + 140) * 6.0).sin().max(0.0);
        p = art::sparkle(p, (c.0 + a.cos() * d, c.1 + a.sin() * d), 0.11 * S * twinkle)
            .fill_color(hexa(color, twinkle))
            .fill();
    }
    p
}

/// A knight's blockade: a ring of stakes round its tile in the owner's
/// colour, faded while it packs up, with the rounds it still holds.
fn palisade(mut p: Painter, g: &Game, tile: TileId, owner: PlayerId, b: Blockade, px1: f32) -> Painter {
    let c = tile_pos(g, tile);
    let color = palette::player(owner);
    let alpha = if b == Blockade::Holding { 0.95 } else { 0.5 };
    p = poly(p, &hex_points(c, S * 0.8)).stroke_color(hexa(color, alpha)).line_width(4.0 * px1).stroke();
    for i in 0..18 {
        let a = i as f32 * TAU / 18.0;
        let (x, y) = (c.0 + a.cos() * 0.7 * S, c.1 + a.sin() * 0.7 * S);
        p = poly(p, &[(x - 0.035 * S, y + 0.05 * S), (x + 0.035 * S, y + 0.05 * S), (x, y - 0.08 * S)])
            .fill_color(hexa(palette::shade(color, 0.7), alpha))
            .fill()
            .stroke_color(hexa(palette::INK, alpha))
            .line_width(px1)
            .stroke();
    }
    if let Blockade::Lifting { free_after } = b {
        let left = (free_after + 1).saturating_sub(g.round);
        let badge = (c.0 - 0.5 * S, c.1 + 0.45 * S);
        p = circle(p, badge, 0.1 * S).fill_color(hex(color)).fill();
        p = text_at(p, &left.to_string(), badge, 0.13 * S, palette::ink_on(color), 900, px1);
    }
    p
}

/// The spell's seal at the top of its tile, with the rolls it has left,
/// legible at any zoom. It stays clear of the number token below it
/// (raised to its spell's place, its top 0.38 S above the centre): zoomed
/// out, where the tile is too small for both, it rises past the tile's top.
fn spell_seal(p: Painter, g: &Game, e: &Enchantment, tile: TileId, px1: f32) -> Painter {
    let c = tile_pos(g, tile);
    let (_, deep, _) = art::spell_colors(e.spell);
    // The seal reaches 11 points below its centre; 2 more keep it apart.
    // A structure on the tile pushes it aside.
    let lift = (0.62 * S).max(0.38 * S + 13.0 * px1);
    let aside = if g.structure_at(tile).is_some() { 0.45 * S } else { 0.0 };
    let mut p = p.save().translate(c.0 + aside, c.1 - lift).scale(px1, px1);
    p = art::shadow(p, 0.45, 6.0, 2.0);
    p = art::spell_emblem(p, e.spell, (0.0, 0.0), 11.0, 1.5);
    p = art::no_shadow(p);
    // Rolls left, or a ward's rounds.
    let left = e.last_round.map_or(e.charges, |last| (last + 1).saturating_sub(g.round));
    p = count_badge(p, (10.0, 4.0), left, deep, 1.0);
    p.restore()
}

/// A count on a dot of `fill`, ringed in white, the same size on screen at
/// any zoom (`pt` world units to a point): rolls or rounds left.
fn count_badge(p: Painter, at: (f32, f32), n: u32, fill: u32, pt: f32) -> Painter {
    let p =
        circle(p, at, 7.0 * pt).fill_color(hex(fill)).fill().stroke_color(hex(0xffffff)).line_width(1.5 * pt).stroke();
    text_at(p, &n.to_string(), (at.0, at.1 + 0.5 * pt), 10.0 * pt, 0xffffff, 900, pt)
}

/// Trees, peaks, fields or ore hills, placed the same way every frame;
/// without `detail` (a tile small on screen) only a few.
fn scenery(mut p: Painter, r: Resource, c: (f32, f32), seed: u32, base: u32, px1: f32, detail: bool) -> Painter {
    let spots = SPOTS
        .iter()
        .enumerate()
        .filter(|&(i, _)| hash01(seed * 8 + i as u32) > 0.18)
        .take(if detail { SPOTS.len() } else { 3 })
        .map(|(i, &(x, y))| ((c.0 + x * S, c.1 + y * S), 0.85 + hash01(seed * 8 + i as u32 + 50) * 0.35));
    match r {
        Resource::Wood => {
            for ((x, y), k) in spots {
                let u = 0.15 * S * k;
                p = p.fill_color(hex(0x6b4a2f)).fill_rect(x - 0.12 * u, y + 0.55 * u, 0.24 * u, 0.4 * u);
                p = poly(p, &[(x, y - 1.1 * u), (x + 0.75 * u, y + 0.6 * u), (x - 0.75 * u, y + 0.6 * u)])
                    .fill_color(hex(palette::shade(base, 0.62)))
                    .fill();
                p = poly(p, &[(x, y - 1.1 * u), (x + 0.75 * u, y + 0.6 * u), (x, y + 0.6 * u)])
                    .fill_color(hex(palette::shade(base, 0.48)))
                    .fill();
                p = poly(p, &[(x, y - 1.1 * u), (x - 0.3 * u, y - 0.2 * u), (x, y - 0.35 * u)])
                    .fill_color(hexa(0xffffff, 0.18))
                    .fill();
            }
            p
        }
        Resource::Stone => {
            for ((x, y), k) in spots {
                let u = 0.17 * S * k;
                let peak = [(x - 1.0 * u, y + 0.6 * u), (x, y - 0.9 * u), (x + 1.0 * u, y + 0.6 * u)];
                p = poly(p, &peak).fill_color(hex(palette::shade(base, 0.82))).fill();
                p = poly(p, &[(x, y - 0.9 * u), (x + 1.0 * u, y + 0.6 * u), (x + 0.15 * u, y + 0.6 * u)])
                    .fill_color(hex(palette::shade(base, 0.62)))
                    .fill();
                p = poly(
                    p,
                    &[(x, y - 0.9 * u), (x - 0.38 * u, y - 0.33 * u), (x, y - 0.45 * u), (x + 0.38 * u, y - 0.33 * u)],
                )
                .fill_color(hex(0xf4f2ec))
                .fill();
            }
            p
        }
        Resource::Food => {
            p = p.line_width(0.075 * S);
            // Furrows; small tiles keep only the light ones.
            for i in (-9..=9).filter(|i| detail || i % 2 == 0) {
                let x = c.0 + i as f32 * 0.15 * S;
                let tone = if i % 2 == 0 { palette::lighten(base, 0.16) } else { palette::shade(base, 0.9) };
                p = p
                    .stroke_color(hex(tone))
                    .begin_path()
                    .move_to(x - 0.6 * S, c.1 + 1.0 * S)
                    .line_to(x + 0.6 * S, c.1 - 1.0 * S)
                    .stroke();
            }
            p = p.stroke_color(hex(palette::shade(base, 0.6))).line_width(1.6 * px1);
            for ((x, y), k) in spots.take(5) {
                let u = 0.1 * S * k;
                for dx in [-0.5, 0.0, 0.5] {
                    p = p.begin_path().move_to(x + dx * 0.4 * u, y + u).line_to(x + dx * u, y - u).stroke();
                    p = art::ellipse(p, (x + dx * u, y - u), 0.16 * u, 0.32 * u, dx * 0.8)
                        .fill_color(hex(palette::shade(base, 0.7)))
                        .fill();
                }
            }
            p
        }
        Resource::Iron => {
            for ((x, y), k) in spots {
                let u = 0.16 * S * k;
                p = p
                    .begin_path()
                    .move_to(x - 1.0 * u, y + 0.5 * u)
                    .quadratic_curve_to(point(px(x), px(y - 1.2 * u)), point(px(x + 1.0 * u), px(y + 0.5 * u)))
                    .close_path()
                    .fill_color(hex(palette::shade(base, 0.78)))
                    .fill();
                p = circle(p, (x - 0.25 * u, y + 0.05 * u), 0.2 * u).fill_color(hex(0x3a2f2b)).fill();
                p = circle(p, (x + 0.3 * u, y + 0.2 * u), 0.15 * u).fill_color(hex(0x3a2f2b)).fill();
                p = circle(p, (x - 0.3 * u, y - 0.02 * u), 0.06 * u).fill_color(hex(0xd8dde2)).fill();
            }
            p
        }
    }
}

/// The production number on its token, with pips for its odds.
fn token(mut p: Painter, (x, y): (f32, f32), number: u8, robbed: bool, px1: f32) -> Painter {
    let r = 0.3 * S;
    let face = CanvasGradient::radial(point(px(x - 0.3 * r), px(y - 0.4 * r)), px(0.0), point(px(x), px(y)), px(r))
        .add_color_stop(percentage(0.0), hex(0xfffcf2))
        .add_color_stop(percentage(1.0), hex(0xe8dbb8));
    p = art::shadow(p, 0.35, 5.0, 2.0);
    p = circle(p, (x, y + 0.02 * S), r).fill_style(face).fill();
    p = art::no_shadow(p);
    p = circle(p, (x, y + 0.02 * S), r).stroke_color(hex(0x8a7a5a)).line_width(1.5 * px1).stroke();
    let hot = number == 6 || number == 8;
    let color = if hot { palette::HOT } else { palette::INK };
    p = text_at(p, &number.to_string(), (x, y - 0.03 * S), if hot { 0.34 * S } else { 0.3 * S }, color, 800, px1);
    let pips = 6 - (7 - number as i32).abs();
    for i in 0..pips {
        let px_ = x + (i as f32 - (pips - 1) as f32 / 2.0) * 0.065 * S;
        p = circle(p, (px_, y + 0.18 * S), 0.022 * S).fill_color(hex(color)).fill();
    }
    if robbed {
        p = p
            .begin_path()
            .move_to(x - r * 0.7, y - r * 0.7)
            .line_to(x + r * 0.7, y + r * 0.7)
            .stroke_color(hexa(palette::DANGER, 0.9))
            .line_width(0.05 * S)
            .stroke();
    }
    p
}

/// A collected-resources badge in the collector's color: the resource's
/// icon, ringed so it stands out on any color, and `+amount`, drawn in
/// badge points of `unit` world units each.
#[allow(clippy::too_many_arguments)]
fn gain_badge(
    p: Painter,
    (x, y): (f32, f32),
    r: Resource,
    amount: u32,
    color: u32,
    mine: bool,
    unit: f32,
    px1: f32,
) -> Painter {
    let mut p = p.save().translate(x, y).scale(unit, unit);
    if mine {
        p = art::rrect(p, (0.0, 0.0), 104.0, 56.0, 28.0).fill_color(hexa(palette::GOLD, 0.55)).fill();
    }
    p = art::shadow(p, 0.4, 10.0, 4.0);
    p = art::rrect(p, (0.0, 0.0), 94.0, 46.0, 23.0).fill_color(hex(color)).fill();
    p = art::no_shadow(p);
    p = art::rrect(p, (-23.0, 0.0), 38.0, 38.0, 11.0).fill_color(hex(0xfffaf0)).fill();
    p = art::resource_icon(p, r, (-23.0, 0.0), 32.0, 1.2);
    p = text_at(p, &format!("+{amount}"), (19.0, 1.0), 28.0, palette::ink_on(color), 900, px1 / unit);
    p.restore()
}

/// `+amount ★` in gold: victory points.
fn points_badge(p: Painter, (x, y): (f32, f32), amount: u32, unit: f32, px1: f32) -> Painter {
    let mut p = p.save().translate(x, y).scale(unit, unit);
    p = art::shadow(p, 0.4, 10.0, 4.0);
    p = art::rrect(p, (0.0, 0.0), 84.0, 40.0, 20.0).fill_style(art::vgrad(-20.0, 20.0, 0xffe08a, 0xe0a72e)).fill();
    p = art::no_shadow(p);
    p = art::rrect(p, (0.0, 0.0), 84.0, 40.0, 20.0).stroke_color(hex(0x8a5a10)).line_width(2.0).stroke();
    p = art::star(p, (-20.0, 0.0), 13.0).fill_color(hex(0xffffff)).fill().stroke_color(hex(0x8a5a10)).stroke();
    p = text_at(p, &format!("+{amount}"), (14.0, 1.0), 22.0, 0x5a3a08, 900, px1 / unit);
    p.restore()
}

// ----- The view -----

#[derive(Default)]
struct Gesture {
    /// Fingers down: id and position in the canvas.
    fingers: Vec<(u32, (f32, f32))>,
    /// How far the current touch sequence moved, and whether it had two
    /// fingers: either makes it a pan or pinch rather than a tap.
    travelled: f32,
    multi: bool,
}

fn centroid(f: &[(u32, (f32, f32))]) -> (f32, f32) {
    let n = f.len().max(1) as f32;
    (f.iter().map(|p| p.1.0).sum::<f32>() / n, f.iter().map(|p| p.1.1).sum::<f32>() / n)
}

fn spread(f: &[(u32, (f32, f32))]) -> f32 {
    let c = centroid(f);
    f.iter().map(|p| dist(p.1, c)).sum::<f32>() / f.len().max(1) as f32
}

pub fn board_view(app: AppState) -> impl IntoElement {
    let size = signal((0.0_f32, 0.0_f32));
    let cam = signal(None::<Camera>);
    let gesture = stored(Gesture::default());
    let owner = Owner::current();
    // The camera's flight to where it belongs, while it lasts.
    let flight = stored(None::<Spawned<()>>);
    let land = move || {
        if let Some(f) = flight.replace(None) {
            f.cancel();
        }
    };
    // The band the HUD leaves clear, as laid out now.
    let band = move || clear_band(size.get_untracked().1, app.hud.get_untracked());
    // Where the camera belongs: close on the focused tile, else the whole
    // island, centred in the band.
    let goal = move || {
        let w = size.get_untracked().0;
        let g = app.game().filter(|_| w > 0.0)?;
        Some(match app.focus.get_untracked() {
            Some(t) => Camera::focus(&g, t, w, band()),
            None => Camera::fit(&g, w, band()),
        })
    };

    // Put the camera where it belongs when the viewport first knows its
    // size or changes it.
    let refit = move || {
        land();
        if let Some(c) = goal() {
            cam.set(Some(c));
        }
    };
    // Fit once the first game state arrives.
    effect(move || {
        let has_game = app.view.with(|v| v.as_ref().is_some_and(|v| v.game.is_some()));
        if has_game && cam.get_untracked().is_none() {
            untracked(refit);
        }
    });
    // Fit again once the HUD is first measured: the first fit guessed it.
    let measured = stored(false);
    effect(move || {
        if app.hud.with(Hud::measured) && !measured.replace(true) && cam.get_untracked().is_some() {
            untracked(refit);
        }
    });
    // Glide there whenever the focus is written, even unchanged: the player
    // may have panned away from it.
    let fly = move || {
        land();
        let (Some(from), Some(to)) = (cam.get_untracked(), goal()) else { return };
        let start = now_ms();
        let task = owner.run(|| {
            spawn(async move {
                loop {
                    next_frame().await;
                    let k = ((now_ms() - start) / FLY_MS).min(1.0) as f32;
                    cam.set(Some(if k < 1.0 { from.toward(to, ease_in_out_sine(k)) } else { to }));
                    if k >= 1.0 {
                        break;
                    }
                }
            })
        });
        flight.replace(Some(task));
    };
    let first = stored(true);
    effect(move || {
        app.focus.with(|_| ());
        if !first.replace(false) {
            untracked(fly);
        }
    });

    let tap = move |sx: f32, sy: f32| {
        let (Some(g), Some(seat), Some(c)) = (app.game(), app.seat(), cam.get_untracked()) else {
            return;
        };
        let p = c.to_world(sx, sy);
        match resolve_tap(&g, seat, app.mode.get_untracked(), p) {
            Tap::Act(action, next) => {
                app.inspect.set(None);
                app.mode.set(next);
                app.send(action);
            }
            Tap::Select(mode) => {
                app.inspect.set(None);
                app.mode.set(mode);
                // Tapping the selected knight again lets it go: back out.
                app.look_at(if mode == Mode::Idle { None } else { tile_under(&g, p) });
            }
            Tap::Confirm(action) => {
                app.inspect.set(None);
                app.confirm_attack.set(Some(action));
            }
            Tap::Inspect(i) => {
                if !matches!(
                    app.mode.get_untracked(),
                    Mode::Road
                        | Mode::Outpost
                        | Mode::UpgradeTown
                        | Mode::UpgradeCity
                        | Mode::Deploy(_)
                        | Mode::Cast(..)
                ) {
                    app.mode.set(Mode::Idle);
                }
                // Tapping the tile looked at or zoomed in on again lets it
                // go and shows the whole island.
                let again = match i {
                    Some(Inspect::Tile(t)) => app.inspect.get_untracked() == i || app.focus.get_untracked() == Some(t),
                    _ => false,
                };
                if again {
                    app.inspect.set(None);
                    app.look_at(None);
                } else {
                    app.inspect.set(if app.inspect.get_untracked() == i { None } else { i });
                    app.look_at(tile_under(&g, p));
                }
            }
        }
    };

    let on_touch = move |e: &TouchEvent| {
        if e.phase == TouchPhase::Start {
            land();
        }
        let ended = matches!(e.phase, TouchPhase::End | TouchPhase::Cancel);
        let now: Vec<(u32, (f32, f32))> = e
            .points
            .iter()
            .filter(|p| !(ended && p.changed))
            .map(|p| (p.id, (p.location.x.0, p.location.y.0)))
            .collect();
        gesture.with(|gs| {
            let same = gs.fingers.len() == now.len() && gs.fingers.iter().zip(&now).all(|(a, b)| a.0 == b.0);
            if e.phase == TouchPhase::Start && gs.fingers.is_empty() {
                gs.travelled = 0.0;
                gs.multi = false;
            }
            gs.multi |= now.len() > 1;
            if same && !now.is_empty() {
                let (c0, c1) = (centroid(&gs.fingers), centroid(&now));
                let (dx, dy) = (c1.0 - c0.0, c1.1 - c0.1);
                gs.travelled += dx.abs() + dy.abs();
                if gs.travelled > 8.0 || gs.multi {
                    cam.update(|c| {
                        if let Some(c) = c {
                            c.x += dx;
                            c.y += dy;
                            if now.len() > 1 {
                                let (s0, s1) = (spread(&gs.fingers), spread(&now));
                                if s0 > 1.0 {
                                    c.zoom(s1 / s0, c1.0, c1.1);
                                }
                            }
                        }
                    });
                }
            }
            gs.fingers = now;
        });
    };

    // Three layers, each redrawn only for what it shows. The ground (most of
    // the drawing) follows the game and the camera; the spells layer the
    // shimmer of spells and wobbling tiles; the pieces the player's choices,
    // and every frame only while an animation runs.
    let ground = canvas(move || {
        let (w, h) = size.get();
        let skip = app.anims.with(|a| jelly_tiles(a));
        app.view
            .with(|v| {
                let v = v.as_ref()?;
                Some(draw_ground(&Scene::bare(v.game.as_ref()?, v.seat, cam.get()?, (w, h)), &skip))
            })
            .unwrap_or_else(|| Painter::new().fill_color(hex(palette::TABLE)).fill_rect(0.0, 0.0, w, h))
    })
    .absolute()
    .inset_0()
    .pointer_events(PointerEvents::None);
    let spells = canvas(move || {
        let size = size.get();
        let anims = app.anims.get();
        let wobbling = anims.iter().any(|a| matches!(a.kind, AnimKind::Jelly { .. }));
        app.view
            .with(|v| {
                let v = v.as_ref()?;
                let g = v.game.as_ref()?;
                if !wobbling && !g.spells_on_board() {
                    return None;
                }
                let now = app.clock.get();
                Some(draw_spells(&Scene { anims: &anims, now, ..Scene::bare(g, v.seat, cam.get()?, size) }))
            })
            .unwrap_or_default()
    })
    .absolute()
    .inset_0()
    .pointer_events(PointerEvents::None);
    // The highlighted places, worked out again only when the game or the
    // player's mode changes.
    let opts = computed(move || {
        let mode = app.mode.get();
        app.view.with(|v| v.as_ref().and_then(|v| Some(options(v.game.as_ref()?, v.seat, mode)))).unwrap_or_default()
    });
    let pieces = canvas(move || {
        let size = size.get();
        let anims = app.anims.get();
        let now = if anims.is_empty() { now_ms() } else { app.clock.get() };
        let mode = app.mode.get();
        let inspect = app.inspect.get();
        opts.with(|opts| {
            app.view.with(|v| {
                let v = v.as_ref()?;
                let base = Scene::bare(v.game.as_ref()?, v.seat, cam.get()?, size);
                Some(draw_pieces(&Scene { opts, mode, inspect, anims: &anims, now, ..base }))
            })
        })
        .unwrap_or_default()
    })
    .absolute()
    .inset_0()
    .test_id("board")
    .on_layout(move |frame: &Bounds<Points>| {
        size.set((frame.size.width.0, frame.size.height.0));
        refit();
    })
    .on_touch(on_touch)
    .on_press(move |e: &PressEvent| {
        let was_gesture = gesture.with(|gs| gs.travelled > 8.0 || gs.multi);
        if !was_gesture {
            tap(e.position.x.0, e.position.y.0);
        }
    })
    .on_wheel(move |e: &WheelEvent| {
        land();
        let (sx, sy) = (e.position.x.0, e.position.y.0);
        cam.update(|c| {
            let Some(c) = c else { return };
            match e.delta {
                WheelDelta::Points(d) if !e.modifiers.ctrl => {
                    c.x -= d.x.0;
                    c.y -= d.y.0;
                }
                WheelDelta::Points(d) => c.zoom((1.0 - d.y.0 * 0.01).clamp(0.8, 1.25), sx, sy),
                WheelDelta::Lines(d) => c.zoom(if d.y > 0.0 { 1.0 / 1.1 } else { 1.1 }, sx, sy),
            }
        });
    })
    .on_pinch(move |e: &PinchEvent| {
        land();
        let (sx, sy) = (e.position.x.0, e.position.y.0);
        cam.update(|c| {
            if let Some(c) = c {
                c.zoom(1.0 + e.delta.0, sx, sy);
            }
        });
    });
    view().absolute().inset_0().child(ground).child(spells).child(pieces)
}
