//! The celebration layer over everything: the ribbon that announces a turn,
//! the dice tumbling in big before they fly to their corner, confetti. It
//! draws in screen points and never takes a touch.

use std::f32::consts::{PI, TAU};

use nsh_core::prelude::*;

use crate::{
    art::{self, ease_in, ease_in_out, ease_out, ease_out_back, ease_out_bounce, hash01, lerp, phase, text_at},
    palette::{self, hex, hexa},
    state::AppState,
};

#[derive(Clone, Debug, PartialEq)]
pub enum FxKind {
    /// A ribbon sweeping across the screen: whose turn it is.
    Banner { title: String, subtitle: String, color: u32, mine: bool },
    /// The dice tumbling in big, settling with their sum, then flying to
    /// their corner of the HUD.
    Dice { dice: (u8, u8), caption: String, note: Option<String> },
    /// Confetti bursting from the middle of the screen.
    Confetti,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fx {
    pub start: f64,
    pub dur: f64,
    pub kind: FxKind,
}

impl Fx {
    pub fn end(&self) -> f64 {
        self.start + self.dur
    }
}

pub const BANNER_MS: f64 = 1900.0;
/// A turn of your own is announced a little longer.
pub const MY_BANNER_MS: f64 = 2300.0;
pub const DICE_MS: f64 = 1900.0;
/// When the dice reach their corner, from the roll.
pub const DICE_LANDED_MS: f64 = 1850.0;
pub const CONFETTI_MS: f64 = 3200.0;

const TUMBLE_END: f32 = 850.0;
const FLY_START: f32 = 1400.0;
const FLY_END: f32 = 1850.0;
const BIG_DIE: f32 = 88.0;

/// The size of a die in the HUD's corner.
pub const SLOT_DIE: f32 = 30.0;
/// The width of the HUD's dice corner: two dice and the sum.
pub const SLOT_WIDTH: f32 = 92.0;

/// Centers of the two dice in the HUD's corner of a `w` wide screen whose
/// safe area starts `top` down.
fn dice_slot(w: f32, top: f32) -> [(f32, f32); 2] {
    let (left, y) = (w - 8.0 - SLOT_WIDTH, top + 8.0 + SLOT_DIE / 2.0);
    [(left + SLOT_DIE / 2.0, y), (left + SLOT_DIE * 1.5 + 4.0, y)]
}

pub fn draw(list: &[Fx], now: f64, size: (f32, f32), top: f32) -> Painter {
    let mut p = Painter::new();
    for fx in list.iter().filter(|f| now >= f.start && now < f.end()) {
        let ms = (now - fx.start) as f32;
        p = match &fx.kind {
            FxKind::Banner { title, subtitle, color, mine } => {
                banner(p, ms, fx.dur as f32, size, title, subtitle, *color, *mine)
            }
            FxKind::Dice { dice, caption, note } => dice_roll(p, ms, size, top, *dice, caption, note.as_deref()),
            FxKind::Confetti => confetti(p, ms, (size.0 / 2.0, size.1 * 0.4), 110, 7),
        };
    }
    p
}

#[allow(clippy::too_many_arguments)]
fn banner(
    mut p: Painter,
    ms: f32,
    dur: f32,
    (w, h): (f32, f32),
    title: &str,
    subtitle: &str,
    color: u32,
    mine: bool,
) -> Painter {
    let enter = ease_out_back(phase(ms, 0.0, 420.0));
    let leave = ease_in(phase(ms, dur - 380.0, dur));
    let shown = phase(ms, 0.0, 200.0) * (1.0 - phase(ms, dur - 320.0, dur));
    let x = (enter - 1.0) * w + leave * w;
    let cy = h * 0.42;
    let bh = if mine { 128.0 } else { 100.0 };
    p = p.fill_color(hexa(0x000000, 0.34 * shown)).fill_rect(0.0, 0.0, w, h);
    if mine {
        // Sun rays turning behind the ribbon.
        p = p.save().translate(w / 2.0, cy).rotate(ms / 1000.0 * 0.5).global_alpha(shown);
        let reach = w.max(h);
        for i in 0..16 {
            let a = i as f32 * TAU / 16.0;
            p = art::poly(
                p,
                &[
                    (0.0, 0.0),
                    (reach * (a - 0.08).cos(), reach * (a - 0.08).sin()),
                    (reach * (a + 0.08).cos(), reach * (a + 0.08).sin()),
                ],
            )
            .fill_color(hexa(palette::GOLD, 0.22))
            .fill();
        }
        p = p.restore().global_alpha(1.0);
    }
    p = p.save().translate(x, 0.0);
    let (y0, y1) = (cy - bh / 2.0, cy + bh / 2.0);
    p = art::shadow(p, 0.5, 20.0, 6.0);
    p = p.fill_style(art::vgrad(y0, y1, palette::lighten(color, 0.25), palette::shade(color, 0.75))).fill_rect(
        -40.0,
        y0,
        w + 80.0,
        bh,
    );
    p = art::no_shadow(p);
    let trim = if mine { hex(palette::GOLD) } else { hexa(0xffffff, 0.7) };
    let trim_h = if mine { 6.0 } else { 3.0 };
    p = p.fill_color(trim).fill_rect(-40.0, y0, w + 80.0, trim_h).fill_rect(-40.0, y1 - trim_h, w + 80.0, trim_h);
    let ink = palette::ink_on(color);
    let pop = ease_out_back(phase(ms, 120.0, 560.0)).max(0.01);
    p = p.save().translate(w / 2.0, cy).scale(pop, pop);
    if ink == 0xffffff {
        p = art::shadow(p, 0.45, 6.0, 2.0);
    }
    p = text_at(p, title, (0.0, -14.0), if mine { 46.0 } else { 36.0 }, ink, 900, 1.0);
    p = text_at(p, subtitle, (0.0, bh * 0.27), 16.0, ink, 700, 1.0);
    p = p.restore().restore();
    if mine { confetti(p, ms - 260.0, (w / 2.0, cy), 70, 3) } else { p }
}

fn dice_roll(
    mut p: Painter,
    ms: f32,
    (w, h): (f32, f32),
    top: f32,
    dice: (u8, u8),
    caption: &str,
    note: Option<&str>,
) -> Painter {
    let (cx, cy) = (w / 2.0, h * 0.42);
    let rest = [(cx - 52.0, cy), (cx + 52.0, cy)];
    let starts = [(-BIG_DIE, h * 0.8), (w + BIG_DIE, h * 0.72)];
    let slot = dice_slot(w, top);
    let tumble = phase(ms, 0.0, TUMBLE_END);
    let fly = ease_in_out(phase(ms, FLY_START, FLY_END));
    let shown = phase(ms, 0.0, 180.0) * (1.0 - phase(ms, FLY_START - 100.0, FLY_START + 250.0));
    p = p.fill_color(hexa(0x000000, 0.3 * shown)).fill_rect(0.0, 0.0, w, h);
    p = p.global_alpha(shown);
    p = art::shadow(p, 0.6, 6.0, 2.0);
    p = text_at(p, caption, (cx, cy - BIG_DIE * 0.95), 22.0, 0xffffff, 800, 1.0);
    p = art::no_shadow(p).global_alpha(1.0);
    for (i, value) in [dice.0, dice.1].into_iter().enumerate() {
        let dir = if i == 0 { 1.0 } else { -1.0 };
        let landed = (
            starts[i].0 + (rest[i].0 - starts[i].0) * ease_out(tumble),
            starts[i].1 + (rest[i].1 - starts[i].1) * ease_out_bounce(tumble),
        );
        let (x, y) = lerp(landed, slot[i], fly);
        let at = (x, y - (PI * fly).sin() * 70.0);
        let s = BIG_DIE + (SLOT_DIE - BIG_DIE) * fly;
        let rot = dir * ((1.0 - ease_out(tumble)) * TAU * 1.6 + (PI * fly).sin() * 0.8);
        let face = if tumble < 1.0 {
            let frame = (ease_out(tumble) * 14.0) as u32;
            1 + (hash01(frame * 2 + i as u32 + dice.0 as u32 * 31) * 5.99) as u8
        } else {
            value
        };
        p = art::shadow(p, 0.45, 12.0 * (1.0 - fly) + 2.0, 6.0 * (1.0 - fly) + 1.0);
        p = art::die(p, at, s, face, rot, 2.0 - fly);
        p = art::no_shadow(p);
    }
    let sum = dice.0 + dice.1;
    let pop = ease_out_back(phase(ms, TUMBLE_END - 30.0, TUMBLE_END + 260.0));
    let fade = 1.0 - phase(ms, FLY_START - 50.0, FLY_START + 200.0);
    if pop > 0.0 && fade > 0.0 {
        let at = (cx, cy + BIG_DIE * 0.98);
        let fill = if sum == 7 { palette::DANGER } else { palette::GOLD };
        p = p.global_alpha(fade).save().translate(at.0, at.1).scale(pop, pop);
        p = art::shadow(p, 0.5, 14.0, 4.0);
        p = art::circle(p, (0.0, 0.0), 38.0)
            .fill_style(art::vgrad(-38.0, 38.0, palette::lighten(fill, 0.3), palette::shade(fill, 0.85)))
            .fill();
        p = art::no_shadow(p);
        p = art::circle(p, (0.0, 0.0), 38.0).stroke_color(hex(0xffffff)).line_width(3.5).stroke();
        p = text_at(p, &sum.to_string(), (0.0, 1.0), 40.0, palette::ink_on(fill), 900, 1.0);
        p = p.restore();
        if let Some(note) = note {
            p = art::shadow(p, 0.6, 6.0, 2.0);
            p = text_at(p, note, (at.0, at.1 + 62.0), 20.0, 0xffffff, 800, 1.0);
            p = art::no_shadow(p);
        }
        p = p.global_alpha(1.0);
    }
    p
}

/// `count` paper pieces thrown up from `origin` and falling, `ms` in.
fn confetti(mut p: Painter, ms: f32, origin: (f32, f32), count: u32, seed: u32) -> Painter {
    if ms <= 0.0 {
        return p;
    }
    let colors = [palette::GOLD, 0xc8423a, 0x2f6db5, 0x8a4fb0, 0xffffff, 0x3f8f5a];
    let t = ms / 1000.0;
    let alpha = 1.0 - phase(ms, 1500.0, 2600.0);
    if alpha <= 0.0 {
        return p;
    }
    for i in 0..count {
        let k = seed * 1000 + i;
        let a = -PI / 2.0 + (hash01(k) - 0.5) * PI * 1.5;
        let speed = 380.0 + hash01(k + 300) * 700.0;
        // Air slows the throw; gravity takes over.
        let drag = (1.0 - (-2.2 * t).exp()) / 2.2;
        let x = origin.0 + a.cos() * speed * drag;
        let y = origin.1 + a.sin() * speed * drag + 520.0 * t * t;
        let spin = t * (5.0 + hash01(k + 600) * 9.0);
        let color = colors[(hash01(k + 900) * colors.len() as f32) as usize % colors.len()];
        let flutter = (spin * 1.7).cos().abs().max(0.15);
        p = p
            .save()
            .translate(x, y)
            .rotate(spin)
            .scale(1.0, flutter)
            .fill_color(hexa(color, alpha))
            .fill_rect(-5.0, -3.0, 10.0, 6.0)
            .restore();
    }
    p
}

/// The layer itself: redraws while the clock ticks for an effect.
pub fn fx_layer(app: AppState) -> impl IntoElement {
    let size = signal((0.0_f32, 0.0_f32));
    canvas(move || {
        let list = app.fx.get();
        if list.is_empty() {
            return Painter::new();
        }
        draw(&list, app.clock.get(), size.get(), safe_area().top.0)
    })
    .absolute()
    .inset_0()
    .pointer_events(PointerEvents::None)
    .on_layout(move |frame: &Bounds<Points>| size.set((frame.size.width.0, frame.size.height.0)))
}
