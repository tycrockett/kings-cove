//! The board: where pieces sit on screen, what a tap there means, how it is
//! drawn, and the pinch, pan and tap gestures over it.

use std::f32::consts::{PI, TAU};

use kings_cove_engine::{
    Action, BuildingKind, EdgeId, Event, Game, Phase, PlayerId, Resource, RobberId, SetupPlacing, Target, TileId,
    UnitId, UnitKind, VertexId,
};
use nsh_core::{
    TouchPhase,
    canvas::{CanvasTextAlign, CanvasTextBaseline, LineCap, LineJoin},
    element::TouchEvent,
    prelude::*,
};

use crate::{
    palette::{self, hex, hexa},
    state::{AppState, Inspect, Mode, now_ms},
};

/// A hex's corner radius, in world units.
pub const S: f32 = 50.0;
const HALF_SQRT3: f32 = 0.866_025_4;
/// Space the HUD covers at the top and bottom, kept clear when fitting.
const FIT_TOP: f32 = 118.0;
const FIT_BOTTOM: f32 = 214.0;

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

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

fn lerp(a: (f32, f32), b: (f32, f32), t: f32) -> (f32, f32) {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
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

    /// Fits the board into a `w` × `h` viewport, clear of the HUD.
    pub fn fit(g: &Game, w: f32, h: f32) -> Camera {
        let pts: Vec<(f32, f32)> = g.board.vertices.iter().map(|v| lattice(v.x, v.y)).collect();
        let min_x = pts.iter().map(|p| p.0).fold(f32::MAX, f32::min) - S * 0.4;
        let max_x = pts.iter().map(|p| p.0).fold(f32::MIN, f32::max) + S * 0.4;
        let min_y = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min) - S * 0.4;
        let max_y = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max) + S * 0.4;
        let usable_h = (h - FIT_TOP - FIT_BOTTOM).max(h * 0.4);
        let scale = ((w - 16.0) / (max_x - min_x)).min(usable_h / (max_y - min_y)).max(0.05);
        let top = if h - FIT_TOP - FIT_BOTTOM >= h * 0.4 { FIT_TOP } else { (h - usable_h) / 2.0 };
        Camera {
            x: w / 2.0 - (min_x + max_x) / 2.0 * scale,
            y: top + usable_h / 2.0 - (min_y + max_y) / 2.0 * scale,
            scale,
            fit: scale,
        }
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

// ----- What a tap can do -----

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Spot {
    Vertex(VertexId),
    Edge(EdgeId),
    Tile(TileId),
}

/// A highlighted place and the action a tap on it sends.
#[derive(Clone, Debug, PartialEq)]
pub struct Opt {
    pub spot: Spot,
    pub action: Action,
    /// An attack.
    pub hostile: bool,
}

/// Legal targets for the seat in the current mode. These only guide the
/// player; the server validates every action again.
pub fn options(g: &Game, seat: PlayerId, mode: Mode) -> Vec<Opt> {
    let opt = |spot, action| Opt { spot, action, hostile: false };
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
        Mode::Deploy(kind) => g
            .deploy_tiles(seat)
            .into_iter()
            .filter(|&t| g.can_deploy(seat, kind, t).is_ok())
            .map(|t| opt(Spot::Tile(t), Action::Deploy { kind, tile: t }))
            .collect(),
        Mode::Unit(unit) => {
            let moves =
                g.move_targets(seat, unit).into_iter().map(|t| opt(Spot::Tile(t), Action::MoveUnit { unit, to: t }));
            let attacks = g.attack_targets(seat, unit).into_iter().filter_map(|target| {
                let spot = match target {
                    Target::Robber(r) => Spot::Tile(g.robber(r)?.tile),
                    Target::Unit(u) => Spot::Tile(g.unit(u)?.tile),
                    Target::Building(v) => Spot::Vertex(v),
                };
                Some(Opt { spot, action: Action::Attack { unit, target }, hostile: true })
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
        let next = if mode == Mode::Road { Mode::Road } else { Mode::Idle };
        return Tap::Act(o.action.clone(), next);
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

#[derive(Clone, Debug, PartialEq)]
pub enum AnimKind {
    /// A label rising and fading: collected resources.
    Float { at: (f32, f32), color: u32, label: String },
    /// A ring swelling out: something built or placed.
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
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// The animations an event starts. `before` is the state the client showed
/// (positions pieces slide from); `after` is the new one.
pub fn anims_for(before: Option<&Game>, after: &Game, event: &Event, start: f64) -> Vec<Anim> {
    let a = |dur, kind| Anim { start, dur, kind };
    let player_color = |p: PlayerId| palette::player(p);
    let g = after;
    match *event {
        Event::Produced { tile, resource, amount, player } => {
            let (x, y) = tile_pos(g, tile);
            let offset = (player.index() as f32 - 1.5) * 0.28 * S;
            vec![a(
                1100.0,
                AnimKind::Float {
                    at: (x + offset, y - 0.1 * S),
                    color: palette::resource(resource),
                    label: format!("+{amount}"),
                },
            )]
        }
        Event::ProductionBlocked { tile } => {
            vec![a(900.0, AnimKind::Float { at: tile_pos(g, tile), color: 0x444444, label: "blocked".to_owned() })]
        }
        Event::Built { player, vertex, .. } | Event::Upgraded { player, vertex, .. } => {
            vec![a(500.0, AnimKind::Pop { at: vertex_pos(g, vertex), color: player_color(player) })]
        }
        Event::BuiltRoad { player, edge } => {
            vec![a(450.0, AnimKind::Pop { at: edge_mid(g, edge), color: player_color(player) })]
        }
        Event::Deployed { player, tile, .. } => {
            vec![a(500.0, AnimKind::Pop { at: piece_pos(g, tile), color: player_color(player) })]
        }
        Event::RobberSpawned { tile, .. } => vec![a(700.0, AnimKind::Pop { at: piece_pos(g, tile), color: 0x222222 })],
        Event::UnitMoved { unit, from, .. } => {
            vec![a(380.0, AnimKind::Slide { piece: Piece::Unit(unit), from: piece_pos(g, from) })]
        }
        Event::RobberWandered { robber, from, .. } | Event::RobberRelocated { robber, from, .. } => {
            vec![a(520.0, AnimKind::Slide { piece: Piece::Robber(robber), from: piece_pos(g, from) })]
        }
        Event::KnightKilledRobber { tile, .. } | Event::RamDestroyed { tile, .. } => {
            vec![a(600.0, AnimKind::Burst { at: piece_pos(g, tile) })]
        }
        Event::KnightsClashed { tiles: (x, y), .. } => {
            let mid = lerp(piece_pos(g, x), piece_pos(g, y), 0.5);
            vec![
                a(600.0, AnimKind::Burst { at: mid }),
                a(600.0, AnimKind::Burst { at: piece_pos(g, x) }),
                a(600.0, AnimKind::Burst { at: piece_pos(g, y) }),
            ]
        }
        Event::BuildingRammed { vertex, .. } => vec![a(650.0, AnimKind::Burst { at: vertex_pos(g, vertex) })],
        _ => {
            let _ = before;
            Vec::new()
        }
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

fn poly(mut p: Painter, pts: &[(f32, f32)]) -> Painter {
    p = p.begin_path();
    for (i, &(x, y)) in pts.iter().enumerate() {
        p = if i == 0 { p.move_to(x, y) } else { p.line_to(x, y) };
    }
    p.close_path()
}

fn circle(p: Painter, (x, y): (f32, f32), r: f32) -> Painter {
    p.begin_path().arc(x, y, r, 0.0, TAU)
}

fn hex_points((cx, cy): (f32, f32), r: f32) -> Vec<(f32, f32)> {
    (0..6)
        .map(|i| {
            let a = -PI / 2.0 + i as f32 * PI / 3.0;
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

pub fn draw(s: &Scene) -> Painter {
    let g = s.game;
    let (w, h) = s.size;
    let px1 = 1.0 / s.cam.scale; // one screen point in world units
    let mut p = Painter::new().fill_color(hex(palette::TABLE)).fill_rect(0.0, 0.0, w, h);
    p = p.save().translate(s.cam.x, s.cam.y).scale(s.cam.scale, s.cam.scale);
    p = p.line_join(LineJoin::Round).line_cap(LineCap::Round);

    // The island's shore.
    for t in &g.board.tiles {
        p = poly(p, &hex_points(tile_pos(g, t.id), S * 1.22)).fill_color(hex(palette::SEA)).fill();
    }
    for t in &g.board.tiles {
        p = poly(p, &hex_points(tile_pos(g, t.id), S * 1.08)).fill_color(hex(0xcdbb8f)).fill();
    }

    // Tiles.
    for t in &g.board.tiles {
        let c = tile_pos(g, t.id);
        let base = palette::resource(t.resource);
        p = poly(p, &hex_points(c, S * 0.97)).fill_color(hex(base)).fill();
        p = poly(p, &hex_points(c, S * 0.97))
            .stroke_color(hex(palette::shade(base, 0.72)))
            .line_width(2.0 * px1)
            .stroke();
        p = glyph(p, t.resource, (c.0, c.1 - 0.56 * S), base);
        let robbed = g.robber_at(t.id).is_some();
        p = token(p, (c.0, c.1 - 0.08 * S), t.number, robbed, px1);
        if robbed {
            p = poly(p, &hex_points(c, S * 0.97)).fill_color(hexa(0x000000, 0.22)).fill();
        }
    }

    // Tile highlights.
    for o in s.opts {
        if let Spot::Tile(t) = o.spot {
            let color = if o.hostile { palette::DANGER } else { palette::GOLD };
            let c = tile_pos(g, t);
            p = poly(p, &hex_points(c, S * 0.9))
                .fill_color(hexa(color, 0.3))
                .fill()
                .stroke_color(hex(color))
                .line_width(4.0 * px1)
                .stroke();
        }
    }

    // Roads.
    for (i, owner) in g.roads.iter().enumerate() {
        if let Some(owner) = owner {
            let (a, b) = edge_ends(g, EdgeId(i as u16));
            let (a, b) = (lerp(a, b, 0.16), lerp(a, b, 0.84));
            p = p
                .begin_path()
                .move_to(a.0, a.1)
                .line_to(b.0, b.1)
                .stroke_color(hex(palette::INK))
                .line_width(0.2 * S)
                .stroke()
                .begin_path()
                .move_to(a.0, a.1)
                .line_to(b.0, b.1)
                .stroke_color(hex(palette::player(*owner)))
                .line_width(0.13 * S)
                .stroke();
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
                .stroke_color(hexa(0xffffff, 0.85))
                .line_width(0.12 * S)
                .stroke();
        }
    }

    // Buildings.
    for (i, b) in g.buildings.iter().enumerate() {
        if let Some(b) = b {
            p = building(p, vertex_pos(g, VertexId(i as u16)), b.kind, palette::player(b.owner), px1);
        }
    }
    for o in s.opts {
        if let Spot::Vertex(v) = o.spot {
            let color = if o.hostile { palette::DANGER } else { palette::GOLD };
            let r = if g.building(v).is_some() { 0.36 * S } else { 0.16 * S };
            p = circle(p, vertex_pos(g, v), r)
                .fill_color(hexa(0xffffff, if g.building(v).is_some() { 0.0 } else { 0.75 }))
                .fill()
                .stroke_color(hex(color))
                .line_width(3.0 * px1)
                .stroke();
        }
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
        p = robber(p, slide_pos(&Piece::Robber(r.id), piece_pos(g, r.tile)));
    }
    let my_turn = g.is_active(s.seat) && g.dice.is_some();
    for u in &g.units {
        let at = slide_pos(&Piece::Unit(u.id), piece_pos(g, u.tile));
        let selected = s.mode == Mode::Unit(u.id);
        p = unit(p, at, u.kind, palette::player(u.owner), px1);
        if my_turn && u.owner == s.seat && u.ready {
            p = circle(p, (at.0 + 0.2 * S, at.1 - 0.2 * S), 0.07 * S)
                .fill_color(hex(palette::GOLD))
                .fill()
                .stroke_color(hex(palette::INK))
                .line_width(1.5 * px1)
                .stroke();
        }
        if selected {
            p = circle(p, at, 0.34 * S).stroke_color(hex(0xffffff)).line_width(3.0 * px1).stroke();
        }
    }
    if let Mode::Robber(r) = s.mode
        && let Some(r) = g.robber(r)
    {
        p = circle(p, piece_pos(g, r.tile), 0.34 * S).stroke_color(hex(0xffffff)).line_width(3.0 * px1).stroke();
    }

    // What the player is looking at.
    if let Some(i) = s.inspect {
        p = match i {
            Inspect::Tile(t) => poly(p, &hex_points(tile_pos(g, t), S * 0.95)),
            Inspect::Vertex(v) => circle(p, vertex_pos(g, v), 0.34 * S),
            Inspect::Edge(e) => circle(p, edge_mid(g, e), 0.26 * S),
        }
        .stroke_color(hex(0xffffff))
        .line_width(3.0 * px1)
        .stroke();
    }

    // Effects.
    for a in s.anims.iter().filter(|a| a.running(s.now)) {
        let t = a.t(s.now);
        p = match &a.kind {
            AnimKind::Float { at, color, label } => {
                let y = at.1 - ease_out(t) * 0.7 * S;
                let alpha = if t < 0.7 { 1.0 } else { 1.0 - (t - 0.7) / 0.3 };
                let p = p.global_alpha(alpha);
                let p = p
                    .begin_path()
                    .round_rect(
                        bounds(point(px(at.0 - 0.3 * S), px(y - 0.17 * S)), size(px(0.6 * S), px(0.34 * S))),
                        px(0.17 * S),
                    )
                    .fill_color(hex(*color))
                    .fill()
                    .stroke_color(hex(palette::INK))
                    .line_width(1.5 * px1)
                    .stroke();
                text_at(p, label, (at.0, y), 0.22 * S, 0xffffff, 800, px1).global_alpha(1.0)
            }
            AnimKind::Pop { at, color } => circle(p, *at, (0.15 + 0.45 * ease_out(t)) * S)
                .stroke_color(hexa(*color, 1.0 - t))
                .line_width(4.0 * px1)
                .stroke(),
            AnimKind::Burst { at } => {
                let mut p = p.stroke_color(hexa(palette::DANGER, 1.0 - t)).line_width(3.0 * px1);
                for k in 0..8 {
                    let ang = k as f32 * TAU / 8.0;
                    let (r0, r1) = ((0.1 + 0.3 * t) * S, (0.25 + 0.45 * ease_out(t)) * S);
                    p = p
                        .begin_path()
                        .move_to(at.0 + r0 * ang.cos(), at.1 + r0 * ang.sin())
                        .line_to(at.0 + r1 * ang.cos(), at.1 + r1 * ang.sin())
                        .stroke();
                }
                circle(p, *at, 0.2 * S * (1.0 - t)).fill_color(hexa(palette::GOLD, 1.0 - t)).fill()
            }
            AnimKind::Slide { .. } => p,
        };
    }
    p.restore()
}

impl Anim {
    /// Started or about to: a slide holds its piece at the start until it runs.
    fn now_or_pending(&self, now: f64) -> bool {
        now < self.end()
    }
}

/// Text of `size` world units, laid out at the screen's own scale (`px1`
/// world units per point) so glyphs are hinted and spaced as on screen.
fn text_at(p: Painter, label: &str, (x, y): (f32, f32), size: f32, color: u32, weight: u32, px1: f32) -> Painter {
    p.save()
        .translate(x, y)
        .scale(px1, px1)
        .font(&format!("{weight} {:.1}px Helvetica, Arial, sans-serif", size / px1))
        .text_align(CanvasTextAlign::Center)
        .text_baseline(CanvasTextBaseline::Middle)
        .fill_color(hex(color))
        .fill_text(label, 0.0, 0.0)
        .restore()
}

/// The production number on its token, with pips for its odds.
fn token(mut p: Painter, (x, y): (f32, f32), number: u8, robbed: bool, px1: f32) -> Painter {
    let r = 0.3 * S;
    p = circle(p, (x, y + 0.02 * S), r)
        .fill_color(hex(palette::TOKEN))
        .fill()
        .stroke_color(hex(0x8a7a5a))
        .line_width(0.03 * S)
        .stroke();
    let hot = number == 6 || number == 8;
    let color = if hot { palette::HOT } else { palette::INK };
    p = text_at(p, &number.to_string(), (x, y - 0.03 * S), 0.3 * S, color, 800, px1);
    let pips = 6 - (7 - number as i32).abs();
    for i in 0..pips {
        let px = x + (i as f32 - (pips - 1) as f32 / 2.0) * 0.065 * S;
        p = circle(p, (px, y + 0.18 * S), 0.022 * S).fill_color(hex(color)).fill();
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

/// A small emblem of the tile's resource.
fn glyph(p: Painter, resource: Resource, (x, y): (f32, f32), base: u32) -> Painter {
    let ink = hex(palette::shade(base, 0.55));
    let u = 0.13 * S;
    match resource {
        Resource::Wood => {
            let mut p = p.fill_color(ink);
            for dx in [-1.1, 0.0, 1.1] {
                let cx = x + dx * u;
                let lift = if dx == 0.0 { 0.35 * u } else { 0.0 };
                p = poly(
                    p,
                    &[
                        (cx, y - 1.1 * u - lift),
                        (cx + 0.75 * u, y + 0.5 * u - lift),
                        (cx - 0.75 * u, y + 0.5 * u - lift),
                    ],
                )
                .fill()
                .fill_rect(cx - 0.12 * u, y + 0.5 * u - lift, 0.24 * u, 0.45 * u);
            }
            p
        }
        Resource::Stone => {
            let p = p.fill_color(ink);
            let p = circle(p, (x - 0.8 * u, y + 0.3 * u), 0.6 * u).fill();
            let p = circle(p, (x + 0.75 * u, y + 0.35 * u), 0.55 * u).fill();
            circle(p, (x, y - 0.35 * u), 0.7 * u).fill()
        }
        Resource::Food => {
            let mut p = p.stroke_color(ink).fill_color(ink).line_width(0.18 * u);
            for dx in [-0.8, 0.0, 0.8] {
                let cx = x + dx * u;
                p = p.begin_path().move_to(cx, y + 0.9 * u).line_to(cx, y - 0.5 * u).stroke();
                for k in 0..3 {
                    let ky = y - 0.9 * u + k as f32 * 0.42 * u;
                    p = p
                        .begin_path()
                        .ellipse(
                            point(px(cx), px(ky)),
                            size(px(0.2 * u), px(0.32 * u)),
                            radians(0.0),
                            radians(0.0),
                            radians(TAU),
                            false,
                        )
                        .fill();
                }
            }
            p
        }
        Resource::Iron => {
            let p = p.fill_color(ink);
            let p = poly(
                p,
                &[
                    (x - 1.2 * u, y + 0.7 * u),
                    (x + 1.2 * u, y + 0.7 * u),
                    (x + 0.8 * u, y - 0.1 * u),
                    (x - 0.8 * u, y - 0.1 * u),
                ],
            )
            .fill();
            poly(
                p,
                &[
                    (x - 0.7 * u, y - 0.25 * u),
                    (x + 0.7 * u, y - 0.25 * u),
                    (x + 0.4 * u, y - 0.9 * u),
                    (x - 0.4 * u, y - 0.9 * u),
                ],
            )
            .fill()
        }
    }
}

/// Outpost: a tent. Town: a house. City: a castle.
fn building(p: Painter, (x, y): (f32, f32), kind: BuildingKind, color: u32, px1: f32) -> Painter {
    let u = 0.11 * S;
    let shape: Vec<(f32, f32)> = match kind {
        BuildingKind::Outpost => vec![(x, y - 1.3 * u), (x + 1.25 * u, y + 0.9 * u), (x - 1.25 * u, y + 0.9 * u)],
        BuildingKind::Town => vec![
            (x - 1.2 * u, y + 1.1 * u),
            (x + 1.2 * u, y + 1.1 * u),
            (x + 1.2 * u, y - 0.3 * u),
            (x, y - 1.5 * u),
            (x - 1.2 * u, y - 0.3 * u),
        ],
        BuildingKind::City => [
            (-2.0, 1.3),
            (-2.0, -2.4),
            (-1.55, -2.4),
            (-1.55, -2.0),
            (-1.2, -2.0),
            (-1.2, -2.4),
            (-0.75, -2.4),
            (-0.75, -0.7),
            (0.75, -0.7),
            (0.75, -2.4),
            (1.2, -2.4),
            (1.2, -2.0),
            (1.55, -2.0),
            (1.55, -2.4),
            (2.0, -2.4),
            (2.0, 1.3),
        ]
        .iter()
        .map(|&(dx, dy)| (x + dx * u, y + dy * u))
        .collect(),
    };
    let mut p =
        poly(p, &shape).fill_color(hex(color)).fill().stroke_color(hex(palette::INK)).line_width(2.2 * px1).stroke();
    match kind {
        BuildingKind::Outpost => {
            p = p.begin_path().move_to(x, y - 1.3 * u).line_to(x, y + 0.9 * u).stroke();
        }
        BuildingKind::Town => {
            p = p.fill_color(hex(palette::INK)).fill_rect(x - 0.3 * u, y + 0.3 * u, 0.6 * u, 0.8 * u);
        }
        BuildingKind::City => {
            // The gate.
            p = p.fill_color(hex(palette::INK)).fill_rect(x - 0.4 * u, y + 0.3 * u, 0.8 * u, 1.0 * u);
        }
    }
    p
}

/// Knight: a shield bearing a sword. Ram: a wagon bearing a ram's log.
fn unit(p: Painter, at: (f32, f32), kind: UnitKind, color: u32, px1: f32) -> Painter {
    let r = 0.22 * S;
    let (x, y) = at;
    let mut p = match kind {
        UnitKind::Knight => poly(
            p,
            &[
                (x - 0.85 * r, y - 0.9 * r),
                (x + 0.85 * r, y - 0.9 * r),
                (x + 0.85 * r, y + 0.05 * r),
                (x + 0.6 * r, y + 0.55 * r),
                (x, y + 1.0 * r),
                (x - 0.6 * r, y + 0.55 * r),
                (x - 0.85 * r, y + 0.05 * r),
            ],
        ),
        UnitKind::Ram => p
            .begin_path()
            .round_rect(bounds(point(px(x - r), px(y - r * 0.85)), size(px(2.0 * r), px(1.7 * r))), px(0.25 * r)),
    }
    .fill_color(hex(color))
    .fill()
    .stroke_color(hex(palette::INK))
    .line_width(2.2 * px1)
    .stroke();
    let ink = if color == palette::PLAYERS[2] { palette::INK } else { 0xffffff };
    p = p.stroke_color(hex(ink)).fill_color(hex(ink)).line_width(0.16 * r);
    match kind {
        UnitKind::Knight => p
            .begin_path()
            .move_to(x, y - 0.6 * r)
            .line_to(x, y + 0.55 * r)
            .stroke()
            .begin_path()
            .move_to(x - 0.32 * r, y - 0.2 * r)
            .line_to(x + 0.32 * r, y - 0.2 * r)
            .stroke(),
        UnitKind::Ram => {
            // A log with a capped head on wheels.
            let p = p
                .begin_path()
                .move_to(at.0 - 0.65 * r, at.1 - 0.15 * r)
                .line_to(at.0 + 0.45 * r, at.1 - 0.15 * r)
                .stroke();
            let p = poly(
                p,
                &[
                    (at.0 + 0.45 * r, at.1 - 0.4 * r),
                    (at.0 + 0.75 * r, at.1 - 0.15 * r),
                    (at.0 + 0.45 * r, at.1 + 0.1 * r),
                ],
            )
            .fill();
            let p = circle(p, (at.0 - 0.35 * r, at.1 + 0.4 * r), 0.16 * r).fill();
            circle(p, (at.0 + 0.3 * r, at.1 + 0.4 * r), 0.16 * r).fill()
        }
    }
}

/// A hooded pawn.
fn robber(p: Painter, (x, y): (f32, f32)) -> Painter {
    let u = 0.2 * S;
    let p = poly(
        p,
        &[
            (x - 0.75 * u, y + 1.0 * u),
            (x + 0.75 * u, y + 1.0 * u),
            (x + 0.35 * u, y - 0.2 * u),
            (x - 0.35 * u, y - 0.2 * u),
        ],
    )
    .fill_color(hex(0x2b2b2e))
    .fill()
    .stroke_color(hex(0xd9d4c5))
    .line_width(0.08 * u)
    .stroke();
    circle(p, (x, y - 0.55 * u), 0.48 * u).fill_color(hex(0x2b2b2e)).fill().stroke_color(hex(0xd9d4c5)).stroke()
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

    // Fit the board when the viewport first knows its size or changes it.
    let refit = move || {
        let (w, h) = size.get_untracked();
        if w > 0.0
            && let Some(g) = app.game()
        {
            cam.set(Some(Camera::fit(&g, w, h)));
        }
    };
    // Fit once the first game state arrives, and whenever the player asks.
    effect(move || {
        let has_game = app.view.with(|v| v.as_ref().is_some_and(|v| v.game.is_some()));
        if has_game && cam.get_untracked().is_none() {
            untracked(refit);
        }
    });
    effect(move || {
        if app.fit_request.get() > 0 {
            untracked(refit);
        }
    });

    let tap = move |sx: f32, sy: f32| {
        let (Some(g), Some(seat), Some(c)) = (app.game(), app.seat(), cam.get_untracked()) else {
            return;
        };
        match resolve_tap(&g, seat, app.mode.get_untracked(), c.to_world(sx, sy)) {
            Tap::Act(action, next) => {
                app.inspect.set(None);
                app.mode.set(next);
                app.send(action);
            }
            Tap::Select(mode) => {
                app.inspect.set(None);
                app.mode.set(mode);
            }
            Tap::Inspect(i) => {
                if !matches!(
                    app.mode.get_untracked(),
                    Mode::Road | Mode::Outpost | Mode::UpgradeTown | Mode::UpgradeCity | Mode::Deploy(_)
                ) {
                    app.mode.set(Mode::Idle);
                }
                app.inspect.set(if app.inspect.get_untracked() == i { None } else { i });
            }
        }
    };

    let on_touch = move |e: &TouchEvent| {
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

    canvas(move || {
        let (w, h) = size.get();
        let anims = app.anims.get();
        let now = if anims.is_empty() { now_ms() } else { app.clock.get() };
        let Some(view) = app.view.get() else {
            return Painter::new();
        };
        let Some(g) = view.game.as_ref() else {
            return Painter::new();
        };
        let Some(c) = cam.get() else {
            return Painter::new().fill_color(hex(palette::TABLE)).fill_rect(0.0, 0.0, w, h);
        };
        let mode = app.mode.get();
        let opts = options(g, view.seat, mode);
        draw(&Scene {
            game: g,
            seat: view.seat,
            cam: c,
            size: (w, h),
            opts: &opts,
            mode,
            inspect: app.inspect.get(),
            anims: &anims,
            now,
        })
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
        let (sx, sy) = (e.position.x.0, e.position.y.0);
        cam.update(|c| {
            if let Some(c) = c {
                c.zoom(1.0 + e.delta.0, sx, sy);
            }
        });
    })
}
