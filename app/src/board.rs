//! The board: where pieces sit on screen, what a tap there means, how it is
//! drawn, and the pinch, pan and tap gestures over it.

use std::f32::consts::{PI, TAU};

use kings_cove_engine::{
    Action, BuildingKind, EdgeId, Event, Game, Phase, PlayerId, Resource, RobberId, SetupPlacing, Target, TileId,
    UnitId, VertexId,
};
use nsh_core::{
    TouchPhase,
    canvas::{CanvasGradient, LineCap, LineJoin},
    element::TouchEvent,
    prelude::*,
};

use crate::{
    art::{self, circle, ease_out, ease_out_back, hash01, lerp, phase, poly, text_at},
    palette::{self, hex, hexa},
    state::{AppState, Inspect, Mode, now_ms},
};

/// A hex's corner radius, in world units.
pub const S: f32 = 50.0;
const HALF_SQRT3: f32 = 0.866_025_4;
/// Space the HUD covers at the top and bottom, kept clear when fitting.
const FIT_TOP: f32 = 118.0;
const FIT_BOTTOM: f32 = 218.0;

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
    /// A tile whose number came up wobbling like jelly; a `blocked` one
    /// (a robber stands on it) shakes no instead.
    Jelly { tile: TileId, blocked: bool },
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
        Event::RobberSpawned { tile, .. } => vec![
            a(800.0, AnimKind::Pop { at: piece_pos(g, tile), color: 0x222222 }),
            a(1300.0, AnimKind::Float { at: piece_pos(g, tile), color: 0x2b2b2e, label: "A robber!".to_owned() }),
        ],
        Event::UnitMoved { unit, from, .. } => {
            vec![a(380.0, AnimKind::Slide { piece: Piece::Unit(unit), from: piece_pos(g, from) })]
        }
        Event::RobberWandered { robber, from, .. } | Event::RobberRelocated { robber, from, .. } => {
            vec![a(520.0, AnimKind::Slide { piece: Piece::Robber(robber), from: piece_pos(g, from) })]
        }
        Event::KnightKilledRobber { tile, .. } | Event::RamDestroyed { tile, .. } => {
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
        Event::BuildingRammed { vertex, .. } => vec![a(700.0, AnimKind::Burst { at: vertex_pos(g, vertex) })],
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

fn hex_points((cx, cy): (f32, f32), r: f32) -> Vec<(f32, f32)> {
    (0..6)
        .map(|i| {
            let a = -PI / 2.0 + i as f32 * PI / 3.0;
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

const SAND: u32 = 0xdcc893;

pub fn draw(s: &Scene) -> Painter {
    let g = s.game;
    let (w, h) = s.size;
    let px1 = 1.0 / s.cam.scale; // one screen point in world units

    // The sea, brightest around the island (the world origin is its center).
    let center = point(px(s.cam.x), px(s.cam.y));
    let sea = CanvasGradient::radial(center, px(0.0), center, px(w.max(h) * 0.9))
        .add_color_stop(percentage(0.0), hex(0x3c8597))
        .add_color_stop(percentage(0.5), hex(0x2a6475))
        .add_color_stop(percentage(1.0), hex(0x163845));
    let mut p = Painter::new().fill_style(sea).fill_rect(0.0, 0.0, w, h);
    p = p.save().translate(s.cam.x, s.cam.y).scale(s.cam.scale, s.cam.scale);
    p = p.line_join(LineJoin::Round).line_cap(LineCap::Round);
    p = waves(p, px1);

    // Shallows and the beach.
    for t in &g.board.tiles {
        p = poly(p, &hex_points(tile_pos(g, t.id), S * 1.34)).fill_color(hexa(0x8fd3d0, 0.22)).fill();
    }
    for t in &g.board.tiles {
        p = poly(p, &hex_points(tile_pos(g, t.id), S * 1.14)).fill_color(hex(SAND)).fill();
    }
    for t in &g.board.tiles {
        p = poly(p, &hex_points(tile_pos(g, t.id), S * 1.04)).fill_color(hex(0xc9b47e)).fill();
    }

    // Tiles; those whose number just came up wobble on top of the rest.
    let jelly = |t: TileId| {
        s.anims.iter().filter(|a| a.running(s.now)).find_map(|a| match a.kind {
            AnimKind::Jelly { tile, blocked } if tile == t => Some((a.t(s.now), blocked)),
            _ => None,
        })
    };
    for t in g.board.tile_ids().filter(|&t| jelly(t).is_none()) {
        p = tile_art(p, g, t, px1);
    }
    for t in g.board.tile_ids() {
        if let Some((k, blocked)) = jelly(t) {
            p = wobbling_tile(p, g, t, k, blocked, px1);
        }
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
        p = art::robber(p, slide_pos(&Piece::Robber(r.id), piece_pos(g, r.tile)), 0.22 * S, 1.6 * px1);
    }
    let my_turn = g.is_active(s.seat) && g.dice.is_some();
    for u in &g.units {
        let at = slide_pos(&Piece::Unit(u.id), piece_pos(g, u.tile));
        p = art::unit(p, at, u.kind, palette::player(u.owner), 0.25 * S, 2.0 * px1);
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

    // What the player is looking at.
    if let Some(i) = s.inspect {
        p = match i {
            Inspect::Tile(t) => poly(p, &hex_points(tile_pos(g, t), S * 0.95)),
            Inspect::Vertex(v) => circle(p, vertex_pos(g, v), 0.4 * S),
            Inspect::Edge(e) => circle(p, edge_mid(g, e), 0.26 * S),
        }
        .stroke_color(hex(0xffffff))
        .line_width(3.0 * px1)
        .stroke();
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
            AnimKind::Jelly { .. } | AnimKind::Slide { .. } => p,
        };
    }
    p.restore()
}

/// Foam marks on the open sea around the island.
fn waves(mut p: Painter, px1: f32) -> Painter {
    p = p.stroke_color(hexa(0xffffff, 0.13)).line_width(2.0 * px1);
    for i in 0..44 {
        let ang = hash01(i) * TAU;
        let d = (5.3 + hash01(i + 100) * 4.0) * S;
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

/// One tile: terrain, its scenery and its number token.
fn tile_art(mut p: Painter, g: &Game, t: TileId, px1: f32) -> Painter {
    let tile = g.board.tile(t);
    let c = tile_pos(g, t);
    let base = palette::resource(tile.resource);
    let outline = hex_points(c, S * 0.96);
    p = poly(p, &outline)
        .fill_style(art::vgrad(c.1 - S, c.1 + S, palette::lighten(base, 0.2), palette::shade(base, 0.8)))
        .fill();
    p = poly(p.save(), &outline).clip();
    p = scenery(p, tile.resource, c, t.0 as u32, base, px1).restore();
    p = poly(p, &hex_points(c, S * 0.9)).stroke_color(hexa(0xffffff, 0.2)).line_width(2.0 * px1).stroke();
    p = poly(p, &outline).stroke_color(hex(palette::shade(base, 0.6))).line_width(1.5 * px1).stroke();
    let robbed = g.robber_at(t).is_some();
    p = token(p, (c.0, c.1 - 0.08 * S), tile.number, robbed, px1);
    if robbed {
        p = poly(p, &outline).fill_color(hexa(0x000000, 0.25)).fill();
    }
    p
}

/// A tile whose number came up: it swells, wobbles and glows (or, blocked
/// by a robber, shakes and flushes red), `k` 0..1 through it.
fn wobbling_tile(p: Painter, g: &Game, t: TileId, k: f32, blocked: bool, px1: f32) -> Painter {
    let c = tile_pos(g, t);
    let fade = 1.0 - k;
    let (dx, sx, sy) = if blocked {
        ((k * TAU * 4.0).sin() * fade * 0.1 * S, 1.0, 1.0)
    } else {
        let swell = 0.14 * (PI * phase(k, 0.0, 0.3)).sin();
        let wobble = 0.13 * fade * fade * (k * TAU * 3.0).sin();
        (0.0, 1.0 + swell + wobble, 1.0 + swell - wobble)
    };
    let mut p = p.save().translate(c.0 + dx, c.1).scale(sx, sy).translate(-c.0, -c.1);
    p = art::shadow(p, 0.45 * fade, 14.0, 4.0);
    p = poly(p, &hex_points(c, S * 0.96)).fill_color(hex(SAND)).fill();
    p = art::no_shadow(p);
    p = tile_art(p, g, t, px1);
    let glow = if blocked { palette::DANGER } else { palette::GOLD };
    p = poly(p, &hex_points(c, S * 0.96))
        .fill_color(hexa(if blocked { palette::DANGER } else { 0xffffff }, 0.35 * fade))
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

/// Trees, peaks, fields or ore hills, placed the same way every frame.
fn scenery(mut p: Painter, r: Resource, c: (f32, f32), seed: u32, base: u32, px1: f32) -> Painter {
    let spots = SPOTS
        .iter()
        .enumerate()
        .filter(|&(i, _)| hash01(seed * 8 + i as u32) > 0.18)
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
            for i in -9..=9 {
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

/// A collected-resources badge: the resource's icon and `+amount`, drawn in
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
    p = art::rrect(p, (0.0, 0.0), 94.0, 46.0, 23.0).fill_color(hex(0xfffaf0)).fill();
    p = art::no_shadow(p);
    p = art::rrect(p, (0.0, 0.0), 94.0, 46.0, 23.0).stroke_color(hex(palette::INK)).line_width(1.5).stroke();
    p = art::rrect(p, (0.0, 0.0), 89.0, 41.0, 20.5).stroke_color(hex(color)).line_width(3.5).stroke();
    p = art::resource_icon(p, r, (-23.0, 0.0), 32.0, 1.2);
    p = text_at(p, &format!("+{amount}"), (19.0, 1.0), 28.0, palette::INK, 900, px1 / unit);
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
