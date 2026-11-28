//! Drawings shared by the board, the HUD and the effects layer: resource
//! icons, pieces, dice and text, plus the easing curves the animations use.
//! Every function draws around a center in the painter's current units;
//! `line` is the stroke width in those units.

use std::f32::consts::{PI, TAU};

use kings_cove_engine::{BuildingKind, Resource, Spell};
use nsh_core::{
    canvas::{CanvasGradient, CanvasTextAlign, CanvasTextBaseline, LineCap},
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

/// Starts and ends gently, never fast: a camera gliding.
pub fn ease_in_out_sine(t: f32) -> f32 {
    0.5 - 0.5 * (std::f32::consts::PI * t).cos()
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

/// The corners of a pointy-top hexagon of corner radius `r` round `c`.
pub fn hex_points((cx, cy): (f32, f32), r: f32) -> Vec<(f32, f32)> {
    (0..6)
        .map(|i| {
            let a = -PI / 2.0 + i as f32 * PI / 3.0;
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
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

// ----- Backgrounds -----

/// A `w` × `h` sea of hexagonal water tiles, each a slightly different blue
/// with a pale rim and a wave or two, darker towards the edges: the home
/// screen's backdrop. It never moves, so it is drawn once per size.
pub fn water_tiles(p: Painter, w: f32, h: f32) -> Painter {
    const R: f32 = 34.0;
    let (dx, dy) = (R * 3f32.sqrt(), R * 1.5);
    let mut p = p.fill_color(hex(0x163845)).fill_rect(0.0, 0.0, w, h);
    for row in -1..(h / dy) as i32 + 2 {
        for col in -1..(w / dx) as i32 + 2 {
            let c = (col as f32 * dx + if row % 2 != 0 { dx / 2.0 } else { 0.0 }, row as f32 * dy);
            let seed = (row + 1) as u32 * 131 + (col + 1) as u32;
            let top = palette::lighten(0x2c6f82, 0.2 * hash01(seed));
            p = poly(p, &hex_points(c, R * 0.93))
                .fill_style(vgrad(c.1 - R, c.1 + R, top, palette::shade(top, 0.78)))
                .fill()
                .stroke_color(hexa(0x8fd3d0, 0.28))
                .line_width(1.5)
                .stroke();
            p = p.stroke_color(hexa(0xffffff, 0.16)).line_width(1.6);
            for i in 0..2 {
                let at = (c.0 + (hash01(seed * 7 + i) - 0.5) * R * 0.9, c.1 + (hash01(seed * 11 + i) - 0.5) * R * 0.8);
                let r = R * (0.14 + 0.08 * hash01(seed * 13 + i));
                p = p.begin_path().arc(at.0, at.1, r, PI * 1.15, PI * 1.85).stroke();
                p = p.begin_path().arc(at.0 + r * 1.3, at.1 + r * 0.1, r * 0.8, PI * 1.2, PI * 1.8).stroke();
            }
        }
    }
    let middle = point(px(w / 2.0), px(h * 0.35));
    let edges = CanvasGradient::radial(middle, px(0.0), middle, px(w.max(h) * 0.8))
        .add_color_stop(percentage(0.4), hexa(0x0b1f27, 0.0))
        .add_color_stop(percentage(1.0), hexa(0x0b1f27, 0.6));
    p.fill_style(edges).fill_rect(0.0, 0.0, w, h)
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

/// A knight: a shield bearing a sword.
pub fn knight(p: Painter, (x, y): (f32, f32), color: u32, r: f32, line: f32) -> Painter {
    let p = ground_shadow(p, (x, y + 1.05 * r), 0.95 * r);
    let shield = [
        (x - 0.85 * r, y - 0.9 * r),
        (x + 0.85 * r, y - 0.9 * r),
        (x + 0.85 * r, y + 0.05 * r),
        (x + 0.6 * r, y + 0.55 * r),
        (x, y + 1.0 * r),
        (x - 0.6 * r, y + 0.55 * r),
        (x - 0.85 * r, y + 0.05 * r),
    ];
    let p = poly(p, &shield)
        .fill_style(vgrad(y - r, y + r, palette::lighten(color, 0.25), palette::shade(color, 0.78)))
        .fill()
        .stroke_color(hex(palette::INK))
        .line_width(line)
        .stroke();
    p.stroke_color(hex(palette::ink_on(color)))
        .line_width(0.16 * r)
        .begin_path()
        .move_to(x, y - 0.6 * r)
        .line_to(x, y + 0.55 * r)
        .stroke()
        .begin_path()
        .move_to(x - 0.32 * r, y - 0.2 * r)
        .line_to(x + 0.32 * r, y - 0.2 * r)
        .stroke()
}

/// The colours of a spell: its fill, its deep tone and its glow.
pub fn spell_colors(spell: Spell) -> (u32, u32, u32) {
    match spell {
        Spell::Frost => (0xcfeeff, 0x2f7fb8, 0x9fdcff),
        Spell::Bounty => (0xffe58a, 0xb7791f, 0xffd447),
        Spell::Lure => (0xffd9d0, 0xa33a2a, 0xff8a6a),
        Spell::Swap => (0xf0dcff, 0x7a3fb8, 0xc99cff),
        Spell::Windfall => (0xfff0c8, 0xa86b00, 0xffc94a),
        Spell::Luck => (0xd2f5dc, 0x1f7a44, 0x6fe09a),
        Spell::Ward => (0xdfe6ff, 0x34479e, 0x8fa8ff),
    }
}

/// A six-armed snowflake of radius `r`.
pub fn snowflake(mut p: Painter, (x, y): (f32, f32), r: f32, color: u32, width: f32) -> Painter {
    p = p.stroke_color(hex(color)).line_width(width);
    for i in 0..6 {
        let a = i as f32 * PI / 3.0 - PI / 2.0;
        let (dx, dy) = (a.cos(), a.sin());
        p = p.begin_path().move_to(x, y).line_to(x + dx * r, y + dy * r).stroke();
        let (bx, by) = (x + dx * r * 0.55, y + dy * r * 0.55);
        for side in [-1.0, 1.0] {
            let b = a + side * 0.75;
            p = p.begin_path().move_to(bx, by).line_to(bx + b.cos() * r * 0.32, by + b.sin() * r * 0.32).stroke();
        }
    }
    p
}

/// A four-pointed sparkle of radius `r`.
pub fn sparkle(p: Painter, (x, y): (f32, f32), r: f32) -> Painter {
    let k = r * 0.22;
    poly(
        p,
        &[
            (x, y - r),
            (x + k, y - k),
            (x + r, y),
            (x + k, y + k),
            (x, y + r),
            (x - k, y + k),
            (x - r, y),
            (x - k, y - k),
        ],
    )
}

/// A spell's emblem: a round seal in the spell's colours bearing its sign.
pub fn spell_emblem(p: Painter, spell: Spell, (x, y): (f32, f32), r: f32, line: f32) -> Painter {
    let (fill, deep, _) = spell_colors(spell);
    let face = CanvasGradient::radial(point(px(x - 0.3 * r), px(y - 0.35 * r)), px(0.0), point(px(x), px(y)), px(r))
        .add_color_stop(percentage(0.0), hex(0xffffff))
        .add_color_stop(percentage(0.5), hex(fill))
        .add_color_stop(percentage(1.0), hex(palette::shade(fill, 0.8)));
    let p = circle(p, (x, y), r).fill_style(face).fill().stroke_color(hex(deep)).line_width(line).stroke();
    let at = |dx: f32, dy: f32| (x + dx * r, y + dy * r);
    let ink = hex(deep);
    let stroke = (r * 0.14).max(line);
    let segment =
        |p: Painter, a: (f32, f32), b: (f32, f32)| p.begin_path().move_to(a.0, a.1).line_to(b.0, b.1).stroke();
    match spell {
        Spell::Frost => snowflake(p, (x, y), r * 0.68, deep, (r * 0.13).max(line)),
        Spell::Bounty => {
            let mut p = p.fill_color(ink);
            for i in 0..8 {
                let a = i as f32 * PI / 4.0;
                let (r0, r1) = (0.36, if i % 2 == 0 { 0.78 } else { 0.6 });
                p = poly(
                    p,
                    &[
                        at((a - 0.18).cos() * r0, (a - 0.18).sin() * r0),
                        at(a.cos() * r1, a.sin() * r1),
                        at((a + 0.18).cos() * r0, (a + 0.18).sin() * r0),
                    ],
                )
                .fill();
            }
            circle(p, (x, y), r * 0.3).fill_color(hex(0xffffff)).fill().stroke_color(ink).line_width(line).stroke()
        }
        Spell::Lure => {
            let (a, b, c, d) = (at(0.15, 0.62), at(0.15, -0.18), at(0.15, -0.58), at(-0.18, -0.52));
            let (e, f) = (at(-0.46, -0.44), at(-0.4, -0.16));
            p.stroke_color(ink)
                .line_width(stroke * 1.2)
                .begin_path()
                .move_to(a.0, a.1)
                .line_to(b.0, b.1)
                .quadratic_curve_to(point(px(c.0), px(c.1)), point(px(d.0), px(d.1)))
                .quadratic_curve_to(point(px(e.0), px(e.1)), point(px(f.0), px(f.1)))
                .stroke()
        }
        Spell::Swap => {
            let mut p = p.stroke_color(ink).fill_color(ink).line_width(stroke);
            for start in [PI * 1.1, PI * 0.1] {
                let end = start + PI * 0.8;
                p = p.begin_path().arc(x, y, r * 0.5, start, end).stroke();
                let back = end - 0.5;
                let tip = at(end.cos() * 0.5, end.sin() * 0.5);
                let (t1, t2) = (at(back.cos() * 0.72, back.sin() * 0.72), at(back.cos() * 0.28, back.sin() * 0.28));
                p = poly(p, &[tip, t1, t2]).fill();
            }
            p
        }
        Spell::Luck => {
            let mut p = segment(p.stroke_color(ink).line_width(stroke), at(0.0, 0.05), at(0.22, 0.66));
            p = p.fill_color(ink);
            for (dx, dy) in [(-0.22, -0.22), (0.22, -0.22), (-0.22, 0.22), (0.22, 0.22)] {
                p = circle(p, at(dx, dy), r * 0.25).fill();
            }
            p
        }
        Spell::Windfall => {
            let mut p = p;
            for dy in [0.36, 0.14, -0.08] {
                p = ellipse(p, at(-0.05, dy), r * 0.42, r * 0.15, 0.0)
                    .fill_color(ink)
                    .fill()
                    .stroke_color(hex(0xffe9a0))
                    .line_width(line)
                    .stroke();
            }
            sparkle(p, at(0.42, -0.42), r * 0.26).fill_color(ink).fill()
        }
        Spell::Ward => {
            let shield = [at(-0.5, -0.55), at(0.5, -0.55), at(0.5, 0.0), at(0.0, 0.62), at(-0.5, 0.0)];
            let p = poly(p, &shield).fill_color(ink).fill();
            segment(p.stroke_color(hex(0xffffff)).line_width(stroke * 0.8), at(0.0, -0.4), at(0.0, 0.38))
        }
    }
}

/// A spellbook: a violet cover with a gold star and the pages' edge.
pub fn spellbook(p: Painter, w: f32, h: f32) -> Painter {
    let (x0, y0, bw, bh) = (w * 0.2, h * 0.08, w * 0.6, h * 0.84);
    let p = p.fill_color(hex(0xfffaf0)).fill_rect(x0 + bw - w * 0.05, y0 + h * 0.06, w * 0.07, bh - h * 0.12);
    let p = rrect(p, (x0 + bw / 2.0 - w * 0.02, y0 + bh / 2.0), bw, bh, w * 0.06)
        .fill_style(vgrad(y0, y0 + bh, 0x8a5cd0, 0x4b2a86))
        .fill()
        .stroke_color(hex(palette::INK))
        .line_width(1.2)
        .stroke();
    let p = p.fill_color(hex(0x3a1f6a)).fill_rect(x0 - w * 0.02, y0, w * 0.06, bh);
    star(p, (x0 + bw / 2.0, y0 + bh * 0.46), bh * 0.24)
        .fill_color(hex(palette::GOLD))
        .fill()
        .stroke_color(hex(0x8a5a10))
        .line_width(1.0)
        .stroke()
}

/// A bandit: a cloaked outlaw in a deep, peaked hood, the face lost in its
/// shadow but for two glinting eyes over a scarf, a dagger raised in one
/// hand and a purse of stolen coin at the belt.
pub fn bandit(p: Painter, (x, y): (f32, f32), u: f32, line: f32) -> Painter {
    let at = |dx: f32, dy: f32| (x + dx * u, y + dy * u);
    let pt = |dx: f32, dy: f32| point(px(x + dx * u), px(y + dy * u));
    let ink = hex(palette::INK);
    let cloak = vgrad(y - 1.15 * u, y + u, 0x6a5c50, 0x2a221c);
    let p = ground_shadow(p, at(0.0, 1.0), 0.9 * u);
    // The dagger, raised behind the shoulder, its guard above the fist.
    let p = poly(p, &[at(0.57, 0.0), at(0.67, 0.07), at(1.02, -0.62)])
        .fill_style(vgrad(y - 0.62 * u, y + 0.07 * u, 0xf4f6f8, 0x9aa3ad))
        .fill()
        .stroke_color(ink)
        .line_width(line * 0.8)
        .stroke();
    let (g0, g1) = (at(0.52, -0.1), at(0.78, 0.06));
    let p = p
        .stroke_color(hex(0xc9a227))
        .line_width(0.08 * u)
        .begin_path()
        .move_to(g0.0, g0.1)
        .line_to(g1.0, g1.1)
        .stroke();
    // The cloak, flaring to the ground.
    let p = p
        .begin_path()
        .move_to(x - 0.36 * u, y - 0.3 * u)
        .bezier_curve_to(pt(-0.5, 0.2), pt(-0.7, 0.6), pt(-0.78, 1.0))
        .line_to(x + 0.78 * u, y + u)
        .bezier_curve_to(pt(0.7, 0.6), pt(0.5, 0.2), pt(0.36, -0.3))
        .close_path()
        .fill_style(cloak.clone())
        .fill()
        .stroke_color(ink)
        .line_width(line)
        .stroke();
    // A belt, with a purse of coin hanging from it.
    let p = poly(p, &[at(-0.53, 0.3), at(0.53, 0.3), at(0.56, 0.42), at(-0.56, 0.42)]).fill_color(hex(0x4a3020)).fill();
    let p = rrect(p, at(0.0, 0.36), 0.14 * u, 0.13 * u, 0.02 * u).fill_color(hex(0xc9a227)).fill();
    let p = circle(p, at(-0.3, 0.6), 0.17 * u)
        .fill_style(vgrad(y + 0.43 * u, y + 0.77 * u, 0xb08a52, 0x6e5230))
        .fill()
        .stroke_color(ink)
        .line_width(line * 0.8)
        .stroke();
    let p = circle(p, at(-0.3, 0.45), 0.05 * u).fill_color(hex(0x4a3020)).fill();
    // The arm from the shoulder to the fist round the dagger's grip.
    let (s0, s1) = (at(0.28, -0.12), at(0.56, 0.12));
    let p = p
        .stroke_color(ink)
        .line_width(0.22 * u + 2.0 * line)
        .line_cap(LineCap::Round)
        .begin_path()
        .move_to(s0.0, s0.1)
        .line_to(s1.0, s1.1)
        .stroke()
        .stroke_color(hex(0x4e433a))
        .line_width(0.22 * u)
        .stroke()
        .line_cap(LineCap::Butt);
    let p = circle(p, at(0.62, 0.13), 0.11 * u)
        .fill_color(hex(0xd7a77a))
        .fill()
        .stroke_color(ink)
        .line_width(line)
        .stroke();
    // The hood: peaked, the peak falling back.
    let p = p
        .begin_path()
        .move_to(x - 0.5 * u, y - 0.2 * u)
        .bezier_curve_to(pt(-0.58, -0.78), pt(-0.22, -1.08), pt(0.18, -1.15))
        .bezier_curve_to(pt(0.3, -0.95), pt(0.58, -0.72), pt(0.5, -0.2))
        .quadratic_curve_to(pt(0.0, -0.02), pt(-0.5, -0.2))
        .close_path()
        .fill_style(cloak)
        .fill()
        .stroke_color(ink)
        .line_width(line)
        .stroke();
    // The face, lost in the hood's shadow inside its lit rim: glinting eyes
    // over a scarf.
    let p = ellipse(p, at(0.0, -0.5), 0.31 * u, 0.29 * u, 0.0).fill_color(hex(0x9a8a7a)).fill();
    let p = ellipse(p, at(0.0, -0.5), 0.27 * u, 0.25 * u, 0.0).fill_color(hex(0x120d0a)).fill();
    let p = p
        .begin_path()
        .move_to(x - 0.27 * u, y - 0.47 * u)
        .quadratic_curve_to(pt(0.0, -0.38), pt(0.27, -0.47))
        .quadratic_curve_to(pt(0.22, -0.28), pt(0.0, -0.25))
        .quadratic_curve_to(pt(-0.22, -0.28), pt(-0.27, -0.47))
        .close_path()
        .fill_color(hex(0x8f2a20))
        .fill();
    let p = ellipse(p, at(-0.1, -0.56), 0.065 * u, 0.035 * u, 0.15).fill_color(hex(0xffd34d)).fill();
    ellipse(p, at(0.1, -0.56), 0.065 * u, 0.035 * u, -0.15).fill_color(hex(0xffd34d)).fill()
}

/// A treasure chest: an item nobody has found yet.
pub fn chest(p: Painter, (x, y): (f32, f32), u: f32, line: f32) -> Painter {
    let ink = hex(palette::INK);
    let at = |dx: f32, dy: f32| point(px(x + dx * u), px(y + dy * u));
    let p = ground_shadow(p, (x, y + 0.78 * u), 1.15 * u);
    let p = rrect(p, (x, y + 0.25 * u), 2.1 * u, 0.95 * u, 0.1 * u)
        .fill_style(vgrad(y - 0.2 * u, y + 0.7 * u, 0xb7793f, 0x7a4a22))
        .fill()
        .stroke_color(ink)
        .line_width(line)
        .stroke();
    // A low domed lid, as wide as the chest.
    let p = p
        .begin_path()
        .move_to(x - 1.05 * u, y - 0.22 * u)
        .bezier_curve_to(at(-1.05, -1.0), at(1.05, -1.0), at(1.05, -0.22))
        .close_path()
        .fill_style(vgrad(y - 0.8 * u, y - 0.22 * u, 0xd08a48, 0x8f5a2b))
        .fill()
        .stroke();
    let p = p.fill_color(hex(palette::GOLD)).fill_rect(x - 0.12 * u, y - 0.8 * u, 0.24 * u, 1.52 * u);
    rrect(p, (x, y - 0.15 * u), 0.42 * u, 0.42 * u, 0.06 * u)
        .fill_color(hex(palette::GOLD))
        .fill()
        .stroke_color(ink)
        .line_width(line)
        .stroke()
}

/// A golden chalice set with a ruby: a relic.
pub fn relic(p: Painter, (x, y): (f32, f32), u: f32, line: f32) -> Painter {
    let gold = vgrad(y - u, y + u, 0xffe58a, 0xb7791f);
    let at = |dx: f32, dy: f32| (x + dx * u, y + dy * u);
    let cup = [at(-0.7, -0.9), at(0.7, -0.9), at(0.5, -0.25), at(0.15, 0.0), at(-0.15, 0.0), at(-0.5, -0.25)];
    let stem = [at(-0.12, 0.0), at(0.12, 0.0), at(0.12, 0.5), at(-0.12, 0.5)];
    let base = [at(-0.5, 0.8), at(0.5, 0.8), at(0.25, 0.5), at(-0.25, 0.5)];
    let mut p = ground_shadow(p, (x, y + 0.85 * u), 0.7 * u).stroke_color(hex(palette::INK)).line_width(line);
    for shape in [&cup[..], &stem[..], &base[..]] {
        p = poly(p, shape).fill_style(gold.clone()).fill().stroke();
    }
    circle(p, at(0.0, -0.55), 0.16 * u).fill_color(hex(0xd8342c)).fill()
}

/// A little stone temple, its flame `flame`: a shrine. `u` is about a
/// third of its height.
pub fn shrine(p: Painter, (x, y): (f32, f32), u: f32, flame: u32, line: f32) -> Painter {
    let at = |dx: f32, dy: f32| (x + dx * u, y + dy * u);
    let stone = vgrad(y - 1.1 * u, y + 0.9 * u, 0xfbf7ec, 0xc2b9a3);
    let ink = hex(0x4f4a3e);
    let mut p = ground_shadow(p, at(0.0, 0.95), 1.4 * u).stroke_color(ink).line_width(line);
    // Two steps, two columns, a lintel and a pediment.
    p = rrect(p, at(0.0, 0.78), 2.5 * u, 0.32 * u, 0.06 * u).fill_style(stone.clone()).fill().stroke();
    p = rrect(p, at(0.0, 0.5), 2.1 * u, 0.28 * u, 0.05 * u).fill_style(stone.clone()).fill().stroke();
    for dx in [-0.75, 0.75] {
        p = rrect(p, at(dx, -0.02), 0.3 * u, 0.82 * u, 0.04 * u).fill_style(stone.clone()).fill().stroke();
    }
    p = rrect(p, at(0.0, -0.5), 2.2 * u, 0.22 * u, 0.04 * u).fill_style(stone.clone()).fill().stroke();
    p = poly(p, &[at(-1.2, -0.62), at(1.2, -0.62), at(0.0, -1.15)]).fill_style(stone).fill().stroke();
    // The flame in its bowl, between the columns.
    p = p
        .begin_path()
        .move_to(x, y - 0.38 * u)
        .bezier_curve_to(
            point(px(x + 0.42 * u), px(y - 0.05 * u)),
            point(px(x + 0.3 * u), px(y + 0.2 * u)),
            point(px(x), px(y + 0.22 * u)),
        )
        .bezier_curve_to(
            point(px(x - 0.3 * u), px(y + 0.2 * u)),
            point(px(x - 0.42 * u), px(y - 0.05 * u)),
            point(px(x), px(y - 0.38 * u)),
        )
        .close_path()
        .fill_color(hex(flame))
        .fill()
        .stroke();
    rrect(p, at(0.0, 0.3), 0.7 * u, 0.16 * u, 0.05 * u).fill_color(hex(0x6b6658)).fill().stroke()
}

/// A timbered mine mouth in a heap of rock, its pennant `mark`: a mine. `u`
/// is about a third of its height.
pub fn mine(p: Painter, (x, y): (f32, f32), u: f32, mark: u32, line: f32) -> Painter {
    let at = |dx: f32, dy: f32| (x + dx * u, y + dy * u);
    let ink = hex(0x3d3a33);
    let mut p = ground_shadow(p, at(0.0, 0.95), 1.4 * u).stroke_color(ink).line_width(line);
    let heap = [at(-1.3, 0.85), at(-0.9, -0.2), at(-0.3, -0.75), at(0.4, -0.65), at(1.0, -0.1), at(1.3, 0.85)];
    p = poly(p, &heap).fill_style(vgrad(y - 0.8 * u, y + 0.9 * u, 0xb3ada0, 0x6f6a5e)).fill().stroke();
    // The dark mouth, framed in timber.
    p = rrect(p, at(0.0, 0.35), 0.9 * u, 1.0 * u, 0.12 * u).fill_color(hex(0x1f1d19)).fill();
    let timber = hex(0x8a5a2b);
    for dx in [-0.48, 0.48] {
        p = rrect(p, at(dx, 0.35), 0.18 * u, 1.05 * u, 0.03 * u).fill_color(timber).fill().stroke();
    }
    p = rrect(p, at(0.0, -0.18), 1.2 * u, 0.2 * u, 0.03 * u).fill_color(timber).fill().stroke();
    // A pennant on the heap.
    p = p.begin_path().move_to(x + 0.75 * u, y - 0.3 * u).line_to(x + 0.75 * u, y - 1.2 * u).stroke();
    poly(p, &[at(0.75, -1.2), at(1.35, -1.0), at(0.75, -0.8)]).fill_color(hex(mark)).fill().stroke()
}

/// A plank sawmill by a stack of logs, its pennant `mark`: a lumber mill.
/// `u` is about a third of its height.
pub fn lumber_mill(p: Painter, (x, y): (f32, f32), u: f32, mark: u32, line: f32) -> Painter {
    let at = |dx: f32, dy: f32| (x + dx * u, y + dy * u);
    let ink = hex(0x3d3a33);
    let mut p = ground_shadow(p, at(0.0, 0.95), 1.45 * u).stroke_color(ink).line_width(line);
    // The shed and its roof.
    p = rrect(p, at(-0.2, 0.3), 1.6 * u, 1.0 * u, 0.06 * u)
        .fill_style(vgrad(y - 0.2 * u, y + 0.8 * u, 0xc98f55, 0x8f5a2b))
        .fill()
        .stroke();
    p = poly(p, &[at(-1.15, -0.2), at(0.75, -0.2), at(-0.2, -1.0)]).fill_color(hex(0x6b4423)).fill().stroke();
    p = rrect(p, at(-0.2, 0.45), 0.45 * u, 0.7 * u, 0.04 * u).fill_color(hex(0x3b2614)).fill();
    // The saw blade at its side.
    p = circle(p, at(0.85, 0.35), 0.42 * u).fill_color(hex(0xd9dde2)).fill().stroke();
    p = circle(p, at(0.85, 0.35), 0.1 * u).fill_color(ink).fill();
    // Logs stacked in front.
    for (dx, dy) in [(-0.75, 0.8), (-0.25, 0.8), (-0.5, 0.55)] {
        p = circle(p, at(dx, dy), 0.2 * u).fill_color(hex(0xe2b47a)).fill().stroke();
    }
    // A pennant on the roof.
    p = p.begin_path().move_to(x - 0.2 * u, y - 1.0 * u).line_to(x - 0.2 * u, y - 1.5 * u).stroke();
    poly(p, &[at(-0.2, -1.5), at(0.4, -1.3), at(-0.2, -1.1)]).fill_color(hex(mark)).fill().stroke()
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
    Knight,
    Spell(Spell),
    Spellbook,
    Trade,
    Robber,
    Chest,
    Shrine,
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
        Glyph::Knight => knight(p, (c.0, h * 0.45), color, h * 0.36, 1.2),
        Glyph::Spell(spell) => spell_emblem(p, spell, c, h * 0.44, 1.2),
        Glyph::Spellbook => spellbook(p, w, h),
        Glyph::Robber => bandit(p, (c.0, h * 0.5), h * 0.36, 1.2),
        Glyph::Chest => chest(p, (c.0, h * 0.55), h * 0.4, 1.2),
        Glyph::Shrine => shrine(p, (c.0, h * 0.58), h * 0.27, palette::GOLD, 1.2),
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
