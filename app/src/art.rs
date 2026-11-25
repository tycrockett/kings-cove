//! Drawings shared by the board, the HUD and the effects layer: resource
//! icons, pieces, dice and text, plus the easing curves the animations use.
//! Every function draws around a center in the painter's current units;
//! `line` is the stroke width in those units.

use std::f32::consts::{PI, TAU};

use kings_cove_engine::{BuildingKind, Resource, UnitKind};
use nsh_core::{
    canvas::{CanvasGradient, CanvasTextAlign, CanvasTextBaseline},
    prelude::*,
};

use crate::palette::{self, hex, hexa};

// ----- Easing -----

pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

pub fn ease_in(t: f32) -> f32 {
    t * t * t
}

pub fn ease_in_out(t: f32) -> f32 {
    if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 }
}

/// Overshoots and settles back: things popping into place.
pub fn ease_out_back(t: f32) -> f32 {
    let (c1, c3) = (1.9, 2.9);
    1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
}

/// Falls and bounces to rest.
pub fn ease_out_bounce(t: f32) -> f32 {
    let (n, d) = (7.5625, 2.75);
    if t < 1.0 / d {
        n * t * t
    } else if t < 2.0 / d {
        let t = t - 1.5 / d;
        n * t * t + 0.75
    } else if t < 2.5 / d {
        let t = t - 2.25 / d;
        n * t * t + 0.9375
    } else {
        let t = t - 2.625 / d;
        n * t * t + 0.984375
    }
}

/// `t` through the window `from..to`, clamped to 0..=1.
pub fn phase(t: f32, from: f32, to: f32) -> f32 {
    ((t - from) / (to - from)).clamp(0.0, 1.0)
}

/// A stable pseudo-random 0..1 for `n`: scenery and particles that must not
/// flicker between frames.
pub fn hash01(n: u32) -> f32 {
    let mut x = n.wrapping_mul(0x9e37_79b9) ^ 0x85eb_ca6b;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297a_2d39);
    x ^= x >> 15;
    (x & 0xffff) as f32 / 65535.0
}

// ----- Primitives -----

pub fn lerp(a: (f32, f32), b: (f32, f32), t: f32) -> (f32, f32) {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

pub fn poly(mut p: Painter, pts: &[(f32, f32)]) -> Painter {
    p = p.begin_path();
    for (i, &(x, y)) in pts.iter().enumerate() {
        p = if i == 0 { p.move_to(x, y) } else { p.line_to(x, y) };
    }
    p.close_path()
}

pub fn circle(p: Painter, (x, y): (f32, f32), r: f32) -> Painter {
    p.begin_path().arc(x, y, r, 0.0, TAU)
}

pub fn ellipse(p: Painter, (x, y): (f32, f32), rx: f32, ry: f32, rotation: f32) -> Painter {
    p.begin_path().ellipse(
        point(px(x), px(y)),
        size(px(rx), px(ry)),
        radians(rotation),
        radians(0.0),
        radians(TAU),
        false,
    )
}

pub fn rrect(p: Painter, (x, y): (f32, f32), w: f32, h: f32, r: f32) -> Painter {
    p.begin_path().round_rect(bounds(point(px(x - w / 2.0), px(y - h / 2.0)), size(px(w), px(h))), px(r))
}

/// Top-to-bottom gradient between `y0` and `y1`.
pub fn vgrad(y0: f32, y1: f32, top: u32, bottom: u32) -> CanvasGradient {
    CanvasGradient::linear(point(px(0.0), px(y0)), point(px(0.0), px(y1)))
        .add_color_stop(percentage(0.0), hex(top))
        .add_color_stop(percentage(1.0), hex(bottom))
}

/// A soft drop shadow for what is drawn next (screen points; transforms do
/// not scale it).
pub fn shadow(p: Painter, alpha: f32, blur: f32, dy: f32) -> Painter {
    p.shadow(hexa(0x000000, alpha), px(blur), point(px(0.0), px(dy)))
}

pub fn no_shadow(p: Painter) -> Painter {
    p.shadow(hexa(0x000000, 0.0), px(0.0), point(px(0.0), px(0.0)))
}

/// Text of `size` units centered on a point, laid out at the screen's own
/// scale (`px1` units per screen point) so glyphs are hinted as on screen.
pub fn text_at(p: Painter, label: &str, (x, y): (f32, f32), size: f32, color: u32, weight: u32, px1: f32) -> Painter {
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

/// A five-pointed star.
pub fn star(p: Painter, (x, y): (f32, f32), r: f32) -> Painter {
    let pts: Vec<(f32, f32)> = (0..10)
        .map(|i| {
            let a = -PI / 2.0 + i as f32 * PI / 5.0;
            let rr = if i % 2 == 0 { r } else { r * 0.45 };
            (x + rr * a.cos(), y + rr * a.sin())
        })
        .collect();
    poly(p, &pts)
}

// ----- Resource icons -----

/// A resource's tile-shaped badge with its emblem, `s` across.
pub fn resource_icon(p: Painter, r: Resource, (x, y): (f32, f32), s: f32, line: f32) -> Painter {
    let base = palette::resource(r);
    let h = s / 2.0;
    let mut p = rrect(p, (x, y), s, s, s * 0.26)
        .fill_style(vgrad(y - h, y + h, palette::lighten(base, 0.28), palette::shade(base, 0.8)))
        .fill()
        .stroke_color(hex(palette::shade(base, 0.5)))
        .line_width(line)
        .stroke();
    let u = h * 0.74;
    let at = |dx: f32, dy: f32| (x + dx * u, y + dy * u);
    let ink = hexa(0xfffbf0, 0.97);
    let dark = hexa(palette::shade(base, 0.45), 0.45);
    match r {
        Resource::Wood => {
            p = p.fill_color(ink).fill_rect(x - 0.13 * u, y + 0.5 * u, 0.26 * u, 0.36 * u);
            for (top, half, bottom) in [(-0.9, 0.42, -0.28), (-0.55, 0.58, 0.12), (-0.18, 0.76, 0.56)] {
                p = poly(p, &[at(0.0, top), at(half, bottom), at(-half, bottom)]).fill();
            }
            poly(p, &[at(0.0, -0.9), at(0.76, 0.56), at(0.0, 0.56)]).fill_color(dark).fill()
        }
        Resource::Stone => {
            p = poly(p, &[at(-0.05, 0.75), at(0.45, -0.1), at(0.92, 0.75)]).fill_color(hexa(0xfffbf0, 0.75)).fill();
            p = poly(p, &[at(-0.9, 0.75), at(-0.12, -0.78), at(0.62, 0.75)]).fill_color(ink).fill();
            p = poly(p, &[at(-0.12, -0.78), at(0.62, 0.75), at(0.12, 0.75)]).fill_color(dark).fill();
            poly(p, &[at(-0.12, -0.78), at(-0.36, -0.3), at(-0.12, -0.38), at(0.12, -0.3)])
                .fill_color(hex(0xffffff))
                .fill()
        }
        Resource::Food => {
            p = p
                .stroke_color(ink)
                .line_width(0.12 * u)
                .begin_path()
                .move_to(x, y + 0.9 * u)
                .line_to(x, y - 0.55 * u)
                .stroke()
                .fill_color(ink);
            for k in 0..4 {
                let gy = y + (-0.5 + k as f32 * 0.32) * u;
                p = ellipse(p, (x - 0.2 * u, gy), 0.14 * u, 0.27 * u, -0.55).fill();
                p = ellipse(p, (x + 0.2 * u, gy), 0.14 * u, 0.27 * u, 0.55).fill();
            }
            ellipse(p, (x, y - 0.8 * u), 0.13 * u, 0.25 * u, 0.0).fill()
        }
        Resource::Iron => {
            let bar = |p: Painter, cx: f32, cy: f32| {
                let p = poly(
                    p,
                    &[
                        at(cx - 0.62, cy + 0.3),
                        at(cx + 0.62, cy + 0.3),
                        at(cx + 0.42, cy - 0.2),
                        at(cx - 0.42, cy - 0.2),
                    ],
                )
                .fill_color(ink)
                .fill();
                poly(
                    p,
                    &[
                        at(cx + 0.62, cy + 0.3),
                        at(cx + 0.42, cy - 0.2),
                        at(cx + 0.3, cy - 0.2),
                        at(cx + 0.45, cy + 0.3),
                    ],
                )
                .fill_color(dark)
                .fill()
            };
            p = bar(p, -0.36, 0.42);
            p = bar(p, 0.36, 0.42);
            bar(p, 0.0, -0.16)
        }
    }
}

// ----- Pieces -----

fn ground_shadow(p: Painter, (x, y): (f32, f32), rx: f32) -> Painter {
    ellipse(p, (x, y), rx, rx * 0.32, 0.0).fill_color(hexa(0x000000, 0.28)).fill()
}

/// Outpost: a tent with a pennant. Town: a house. City: a castle. `u` is
/// the piece's unit (about a ninth of its height).
pub fn building(p: Painter, (x, y): (f32, f32), kind: BuildingKind, color: u32, u: f32, line: f32) -> Painter {
    let at = |dx: f32, dy: f32| (x + dx * u, y + dy * u);
    let dark = palette::shade(color, 0.72);
    let ink = hex(palette::INK);
    let window = hex(0xffe39a);
    let mut p = ground_shadow(p, at(0.0, 1.15), 1.9 * u);
    p = p.stroke_color(ink).line_width(line);
    match kind {
        BuildingKind::Outpost => {
            p = p.begin_path().move_to(x, y - 1.3 * u).line_to(x, y - 2.2 * u).stroke();
            p = poly(p, &[at(0.0, -2.2), at(0.95, -1.9), at(0.0, -1.6)]).fill_color(hex(color)).fill().stroke();
            let tent = [at(0.0, -1.35), at(1.35, 1.05), at(-1.35, 1.05)];
            p = poly(p, &tent).fill_color(hex(color)).fill();
            p = poly(p, &[at(0.0, -1.35), at(1.35, 1.05), at(0.0, 1.05)]).fill_color(hex(dark)).fill();
            p = poly(p, &tent).stroke();
            poly(p, &[at(0.0, 0.0), at(0.5, 1.05), at(-0.5, 1.05)]).fill_color(ink).fill()
        }
        BuildingKind::Town => {
            let walls = [at(-1.15, -0.15), at(1.15, -0.15), at(1.15, 1.1), at(-1.15, 1.1)];
            p = poly(p, &walls).fill_color(hex(color)).fill().stroke();
            p = poly(p, &[at(-1.45, -0.05), at(0.0, -1.55), at(1.45, -0.05)]).fill_color(hex(dark)).fill().stroke();
            p = p.fill_color(ink).fill_rect(x - 0.3 * u, y + 0.35 * u, 0.6 * u, 0.75 * u);
            p.fill_color(window).fill_rect(x + 0.48 * u, y + 0.2 * u, 0.42 * u, 0.4 * u)
        }
        BuildingKind::City => {
            let shape: Vec<(f32, f32)> = [
                (-2.0, 1.25),
                (-2.0, -2.2),
                (-1.6, -2.2),
                (-1.6, -1.85),
                (-1.2, -1.85),
                (-1.2, -2.2),
                (-0.8, -2.2),
                (-0.8, -0.6),
                (0.8, -0.6),
                (0.8, -2.2),
                (1.2, -2.2),
                (1.2, -1.85),
                (1.6, -1.85),
                (1.6, -2.2),
                (2.0, -2.2),
                (2.0, 1.25),
            ]
            .iter()
            .map(|&(dx, dy)| at(dx, dy))
            .collect();
            p = poly(p, &shape).fill_color(hex(color)).fill();
            p = poly(p, &[at(0.8, -0.6), at(0.8, 1.25), at(2.0, 1.25), at(2.0, -2.2), at(0.8, -2.2)])
                .fill_color(hex(dark))
                .fill();
            p = poly(p, &shape).stroke();
            p = p.begin_path().move_to(x - 1.4 * u, y - 2.2 * u).line_to(x - 1.4 * u, y - 3.1 * u).stroke();
            p = poly(p, &[at(-1.4, -3.1), at(-0.5, -2.85), at(-1.4, -2.6)])
                .fill_color(hex(palette::GOLD))
                .fill()
                .stroke();
            p = p
                .begin_path()
                .move_to(x - 0.45 * u, y + 1.25 * u)
                .line_to(x - 0.45 * u, y + 0.35 * u)
                .arc(x, y + 0.35 * u, 0.45 * u, PI, 0.0)
                .line_to(x + 0.45 * u, y + 1.25 * u)
                .close_path()
                .fill_color(ink)
                .fill();
            p.fill_color(window).fill_rect(x - 1.6 * u, y - 1.3 * u, 0.4 * u, 0.55 * u).fill_rect(
                x + 1.2 * u,
                y - 1.3 * u,
                0.4 * u,
                0.55 * u,
            )
        }
    }
}

/// Knight: a shield bearing a sword. Ram: a wagon bearing a ram's log.
pub fn unit(p: Painter, (x, y): (f32, f32), kind: UnitKind, color: u32, r: f32, line: f32) -> Painter {
    let mut p = ground_shadow(p, (x, y + 1.05 * r), 0.95 * r);
    let body = match kind {
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
        UnitKind::Ram => rrect(p, (x, y), 2.0 * r, 1.7 * r, 0.25 * r),
    };
    p = body
        .fill_style(vgrad(y - r, y + r, palette::lighten(color, 0.25), palette::shade(color, 0.78)))
        .fill()
        .stroke_color(hex(palette::INK))
        .line_width(line)
        .stroke();
    let ink = palette::ink_on(color);
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
            let p = p.begin_path().move_to(x - 0.65 * r, y - 0.15 * r).line_to(x + 0.45 * r, y - 0.15 * r).stroke();
            let p = poly(p, &[(x + 0.45 * r, y - 0.4 * r), (x + 0.75 * r, y - 0.15 * r), (x + 0.45 * r, y + 0.1 * r)])
                .fill();
            let p = circle(p, (x - 0.35 * r, y + 0.4 * r), 0.16 * r).fill();
            circle(p, (x + 0.3 * r, y + 0.4 * r), 0.16 * r).fill()
        }
    }
}

/// A hooded pawn.
pub fn robber(p: Painter, (x, y): (f32, f32), u: f32, line: f32) -> Painter {
    let p = ground_shadow(p, (x, y + 1.0 * u), 0.95 * u);
    let cloak = vgrad(y - u, y + u, 0x4a4a52, 0x17171a);
    let p = poly(
        p,
        &[
            (x - 0.8 * u, y + 1.0 * u),
            (x + 0.8 * u, y + 1.0 * u),
            (x + 0.35 * u, y - 0.2 * u),
            (x - 0.35 * u, y - 0.2 * u),
        ],
    )
    .fill_style(cloak.clone())
    .fill()
    .stroke_color(hex(0xd9d4c5))
    .line_width(line)
    .stroke();
    let p = circle(p, (x, y - 0.55 * u), 0.5 * u).fill_style(cloak).fill().stroke();
    p.fill_color(hex(0xffd35a)).fill_rect(x - 0.24 * u, y - 0.6 * u, 0.16 * u, 0.1 * u).fill_rect(
        x + 0.08 * u,
        y - 0.6 * u,
        0.16 * u,
        0.1 * u,
    )
}

/// A road from `a` to `b`, `w` wide.
pub fn road(p: Painter, a: (f32, f32), b: (f32, f32), color: u32, w: f32) -> Painter {
    let seg = |p: Painter| p.begin_path().move_to(a.0, a.1).line_to(b.0, b.1);
    let p = seg(p).stroke_color(hex(palette::INK)).line_width(w).stroke();
    let p = seg(p).stroke_color(hex(color)).line_width(w * 0.66).stroke();
    seg(p).stroke_color(hexa(0xffffff, 0.28)).line_width(w * 0.18).stroke()
}

/// One die, `s` across, turned by `rot` radians.
pub fn die(p: Painter, (x, y): (f32, f32), s: f32, value: u8, rot: f32, line: f32) -> Painter {
    let pips: &[(f32, f32)] = match value {
        1 => &[(0.0, 0.0)],
        2 => &[(-0.22, -0.22), (0.22, 0.22)],
        3 => &[(-0.25, -0.25), (0.0, 0.0), (0.25, 0.25)],
        4 => &[(-0.22, -0.22), (0.22, -0.22), (-0.22, 0.22), (0.22, 0.22)],
        5 => &[(-0.25, -0.25), (0.25, -0.25), (0.0, 0.0), (-0.25, 0.25), (0.25, 0.25)],
        _ => &[(-0.22, -0.28), (0.22, -0.28), (-0.22, 0.0), (0.22, 0.0), (-0.22, 0.28), (0.22, 0.28)],
    };
    let mut p = p.save().translate(x, y).rotate(rot);
    p = rrect(p, (0.0, 0.0), s, s, s * 0.2)
        .fill_style(vgrad(-s / 2.0, s / 2.0, 0xffffff, 0xe6dccb))
        .fill()
        .stroke_color(hex(0x6e6553))
        .line_width(line)
        .stroke();
    let pip = if value == 1 { palette::HOT } else { palette::INK };
    for &(px_, py_) in pips {
        p = circle(p, (px_ * s, py_ * s), s * if value == 1 { 0.13 } else { 0.085 }).fill_color(hex(pip)).fill();
    }
    p.restore()
}

/// What a tray button shows.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Glyph {
    Road,
    Building(BuildingKind),
    Unit(UnitKind),
    Trade,
    Robber,
}

/// `glyph` drawn to fit a `w` × `h` box at the origin, in `color`.
pub fn glyph(p: Painter, glyph: Glyph, color: u32, w: f32, h: f32) -> Painter {
    let c = (w / 2.0, h / 2.0);
    match glyph {
        Glyph::Road => road(p, (w * 0.18, h * 0.75), (w * 0.82, h * 0.25), color, h * 0.26),
        Glyph::Building(kind) => {
            let u = h / if kind == BuildingKind::City { 6.2 } else { 4.6 };
            let y = if kind == BuildingKind::City { h * 0.6 } else { h * 0.52 };
            building(p, (c.0, y), kind, color, u, 1.2)
        }
        Glyph::Unit(kind) => unit(p, (c.0, h * 0.45), kind, color, h * 0.36, 1.2),
        Glyph::Robber => robber(p, (c.0, h * 0.5), h * 0.36, 1.2),
        Glyph::Trade => {
            let arrow = |p: Painter, y: f32, dir: f32, color: u32| {
                let (x0, x1) = if dir > 0.0 { (w * 0.2, w * 0.8) } else { (w * 0.8, w * 0.2) };
                let p = p
                    .stroke_color(hex(color))
                    .line_width(h * 0.13)
                    .begin_path()
                    .move_to(x0, y)
                    .line_to(x1 - dir * h * 0.12, y)
                    .stroke();
                poly(p, &[(x1, y), (x1 - dir * h * 0.25, y - h * 0.17), (x1 - dir * h * 0.25, y + h * 0.17)])
                    .fill_color(hex(color))
                    .fill()
            };
            let p = arrow(p, h * 0.33, 1.0, 0x2f6db5);
            arrow(p, h * 0.68, -1.0, palette::GO)
        }
    }
}
