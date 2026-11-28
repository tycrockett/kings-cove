//! The game's sounds, synthesized when the app starts, so there are no sound
//! files to ship or license. Most are struck things: a blow (a knuckle, a
//! hammer, a soft mallet) sets the modes of wood, skin or metal ringing in a
//! bank of resonators, each drifting a little from strike to strike as real
//! ones do. Beside them: plucked lyre strings (Karplus–Strong), a war horn, a
//! bowed drone, and a hall to put them in. Each comes out as mono 16-bit WAV
//! at [`RATE`], kept low and warm.
//!
//! The music is cut into bars, each a sound of its own (the engine plays
//! sounds of at most 5 s): a bar rings on over the next one, and the drone is
//! tuned to whole cycles of a bar, so one bar's drone fades into the next
//! one's without a seam.

use std::{
    collections::HashMap,
    f32::consts::{PI, TAU},
};

use crate::sound::Cue;

/// Samples a second.
pub const RATE: u32 = 22_050;
const SR: f32 = RATE as f32;

/// One bar of music: four beats at 90 a minute, in samples.
pub const BAR: usize = 58_800;
/// How long a bar rings on over the next one, in samples (1.6 s).
const TAIL: usize = 35_280;
const EIGHTH: usize = BAR / 8;
/// How long one bar's drone fades into the next one's, in samples.
const SEAM: usize = 1_764;

/// D2, a hair sharp, so that it and its fifth fit a bar in whole cycles
/// (196 and 294).
const D2: f32 = 73.5;
const D4: f32 = D2 * 4.0;
/// The steps of the Dorian mode, in semitones above its tonic.
const DORIAN: [i32; 7] = [0, 2, 3, 5, 7, 9, 10];

/// The tune, bar by bar: the eighth each note falls on and its degree
/// ([`note`], played an octave down). The first bar is the drums and the
/// drone alone.
const TUNE: [&[(usize, i32)]; 9] = [
    &[],
    &[(0, 0), (2, 0), (3, 1), (4, 2), (6, 4)],
    &[(0, 3), (2, 2), (3, 1), (4, 0), (6, -1)],
    &[(0, 0), (2, 2), (3, 4), (4, 5), (6, 4)],
    &[(0, 3), (1, 2), (2, 1), (4, 0)],
    &[(0, 4), (2, 5), (3, 6), (4, 7), (6, 6)],
    &[(0, 5), (2, 4), (4, 2), (6, 3)],
    &[(0, 4), (2, 7), (3, 6), (4, 5), (6, 4)],
    &[(0, 2), (2, 1), (3, -1), (4, 0)],
];

/// How many different bars the music has ([`Band::bar`]).
pub const BARS: usize = TUNE.len();

/// The bars in the order they play; after the last, it starts over.
pub const SCORE: [usize; 18] = [0, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 6, 7, 8, 1, 2, 3, 4];

/// The pitch `degree` steps of D Dorian above D4 (below, when negative).
fn note(degree: i32) -> f32 {
    let semis = 12 * degree.div_euclid(7) + DORIAN[degree.rem_euclid(7) as usize];
    D4 * 2f32.powf(semis as f32 / 12.0)
}

fn secs(s: f32) -> usize {
    (s * SR) as usize
}

/// White noise (xorshift), seeded, so every build sounds the same.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

/// Adds `src` into `dst` from sample `at`, times `gain`, lengthening `dst`
/// as needed.
fn add(dst: &mut Vec<f32>, src: &[f32], at: usize, gain: f32) {
    if dst.len() < at + src.len() {
        dst.resize(at + src.len(), 0.0);
    }
    for (d, s) in dst[at..].iter_mut().zip(src) {
        *d += s * gain;
    }
}

fn lowpass(buf: &mut [f32], cutoff: f32) {
    let a = 1.0 - (-TAU * cutoff / SR).exp();
    let mut y = 0.0;
    for s in buf {
        y += a * (*s - y);
        *s = y;
    }
}

fn highpass(buf: &mut [f32], cutoff: f32) {
    let a = 1.0 - (-TAU * cutoff / SR).exp();
    let mut y = 0.0;
    for s in buf {
        y += a * (*s - y);
        *s -= y;
    }
}

/// Scales `buf` so its loudest sample is `peak`.
fn normalize(buf: &mut [f32], peak: f32) {
    let max = buf.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
    if max > 0.0 {
        let k = peak / max;
        buf.iter_mut().for_each(|s| *s *= k);
    }
}

/// Fades out the last `n` samples of `buf`.
fn fade_out(buf: &mut [f32], n: usize) {
    let len = buf.len();
    let n = n.min(len);
    for (i, s) in buf[len - n..].iter_mut().enumerate() {
        *s *= 1.0 - i as f32 / n as f32;
    }
}

/// A wooden hall (Freeverb's tunings at half rate: eight damped combs, four
/// allpasses) mixed into `buf` at `wet`. What rings past the end of `buf`
/// is cut off.
fn reverb(buf: &mut [f32], wet: f32) {
    const COMBS: [usize; 8] = [558, 594, 638, 678, 711, 745, 778, 808];
    const ALLPASSES: [usize; 4] = [278, 220, 170, 112];
    let mut hall = vec![0.0; buf.len()];
    for delay in COMBS {
        let (mut line, mut at, mut damped) = (vec![0.0_f32; delay], 0, 0.0);
        for (h, x) in hall.iter_mut().zip(buf.iter()) {
            let y = line[at];
            damped = y * 0.45 + damped * 0.55;
            line[at] = x * 0.03 + damped * 0.78;
            at = (at + 1) % delay;
            *h += y;
        }
    }
    for delay in ALLPASSES {
        let (mut line, mut at) = (vec![0.0_f32; delay], 0);
        for h in hall.iter_mut() {
            let b = line[at];
            line[at] = *h + b * 0.5;
            at = (at + 1) % delay;
            *h = b - *h;
        }
    }
    for (s, h) in buf.iter_mut().zip(hall) {
        *s += h * wet;
    }
}

/// What `excite` sets ringing in `modes` (frequency in Hz, gain, decay a
/// second), `len` samples of it: a bank of two-pole resonators, the way a
/// plank, a skin or a blade rings when struck.
fn ring(excite: &[f32], modes: &[(f32, f32, f32)], len: usize) -> Vec<f32> {
    let mut out = vec![0.0; len];
    for &(freq, gain, decay) in modes.iter().filter(|m| m.0 < SR * 0.45) {
        let (r, w) = ((-decay / SR).exp(), TAU * freq / SR);
        let (a1, a2, k) = (2.0 * r * w.cos(), -r * r, gain * w.sin());
        let (mut y1, mut y2) = (0.0_f32, 0.0_f32);
        for (i, o) in out.iter_mut().enumerate() {
            let y = excite.get(i).copied().unwrap_or(0.0) + a1 * y1 + a2 * y2;
            (y2, y1) = (y1, y);
            *o += y * k;
        }
    }
    out
}

/// The blow that sets something ringing: a pulse `ms` long (a soft
/// mallet's long, a hammer's short) with `grit` of the contact's roughness.
fn strike(ms: f32, grit: f32, noise: &mut Noise) -> Vec<f32> {
    let n = secs(ms / 1000.0).max(2);
    (0..n).map(|i| (PI * i as f32 / n as f32).sin() * (1.0 + grit * noise.next())).collect()
}

/// The crack of contact: a few ms of noise, gone at once, the lows and the
/// hiss taken out. Gives a knock its edge without making it ring.
fn crack(ms: f32, noise: &mut Noise) -> Vec<f32> {
    let n = secs(ms / 1000.0).max(2);
    let mut out: Vec<f32> = (0..n).map(|i| noise.next() * (1.0 - i as f32 / n as f32).powi(2)).collect();
    highpass(&mut out, 700.0);
    lowpass(&mut out, 3500.0);
    normalize(&mut out, 1.0);
    out
}

/// `modes` (ratio to `pitch`, gain, decay a second) at `pitch`, each off by
/// up to `drift` of it, and dying `damp` times as fast.
fn tune(modes: &[(f32, f32, f32)], pitch: f32, drift: f32, damp: f32, noise: &mut Noise) -> Vec<(f32, f32, f32)> {
    modes.iter().map(|&(r, g, d)| (pitch * r * (1.0 + drift * noise.next()), g, d * damp)).collect()
}

/// A knock on wood at `pitch` (its lowest mode): `hard` (0 to 1) from a
/// knuckle to a hammer, `damp` how soon it dies (1 a tabletop, more a
/// solid block). Its body rings low and briefly; the contact's crack gives
/// it its edge.
fn wood(pitch: f32, hard: f32, damp: f32, noise: &mut Noise) -> Vec<f32> {
    const MODES: [(f32, f32, f32); 4] = [(1.0, 1.0, 30.0), (1.62, 0.55, 42.0), (2.31, 0.3, 60.0), (3.94, 0.1, 90.0)];
    let modes = tune(&MODES, pitch, 0.04, damp, noise);
    let excite = strike(3.0 - 2.2 * hard, 0.2 + 0.4 * hard, noise);
    let mut out = ring(&excite, &modes, secs((5.0 / (30.0 * damp)).min(1.2)));
    normalize(&mut out, 1.0);
    add(&mut out, &crack(3.0, noise), 0, 0.1 + 0.2 * hard);
    normalize(&mut out, 1.0);
    out
}

/// A skin drum at `pitch`, struck by a soft mallet `soft` ms long, dying at
/// `decay` a second: a round membrane's modes, the higher ones sooner, and
/// the rasp of the skin as the mallet lands.
fn skin(pitch: f32, decay: f32, soft: f32, noise: &mut Noise) -> Vec<f32> {
    const MODES: [(f32, f32, f32); 8] = [
        (1.0, 1.0, 1.0),
        (1.59, 0.7, 1.3),
        (2.14, 0.5, 1.6),
        (2.30, 0.4, 1.8),
        (2.65, 0.3, 2.2),
        (2.92, 0.25, 2.5),
        (3.16, 0.2, 2.8),
        (3.50, 0.15, 3.2),
    ];
    let modes = tune(&MODES, pitch, 0.01, decay, noise);
    let mut out = ring(&strike(soft, 0.3, noise), &modes, secs((5.0 / decay).min(2.5)));
    normalize(&mut out, 1.0);
    let mut rasp: Vec<f32> = (0..secs(0.05)).map(|i| noise.next() * (-(i as f32 / SR) * 60.0).exp()).collect();
    highpass(&mut rasp, 200.0);
    lowpass(&mut rasp, 1800.0);
    normalize(&mut rasp, 1.0);
    add(&mut out, &rasp, 0, 0.1);
    lowpass(&mut out, 2500.0);
    normalize(&mut out, 1.0);
    out
}

/// Struck metal at `freq`: its `partials` (ratio to `freq`, gain, decay a
/// second), rung by a short, hard blow.
fn metal(freq: f32, partials: &[(f32, f32, f32)], dur: f32, noise: &mut Noise) -> Vec<f32> {
    let modes = tune(partials, freq, 0.005, 1.0, noise);
    let mut out = ring(&strike(0.4, 0.5, noise), &modes, secs(dur));
    normalize(&mut out, 1.0);
    out
}

/// A footstep on packed earth: a dull thud, and grit crunching under it.
fn step(noise: &mut Noise) -> Vec<f32> {
    let modes = tune(&[(1.0, 1.0, 40.0), (1.8, 0.5, 60.0), (3.1, 0.25, 80.0)], 70.0, 0.05, 1.0, noise);
    let mut out = ring(&strike(9.0, 0.3, noise), &modes, secs(0.18));
    normalize(&mut out, 1.0);
    let mut grit: Vec<f32> = (0..secs(0.12))
        .map(|i| {
            let x = noise.next();
            x * x * x * (-(i as f32 / SR) * 30.0).exp()
        })
        .collect();
    highpass(&mut grit, 500.0);
    lowpass(&mut grit, 2500.0);
    normalize(&mut grit, 1.0);
    add(&mut out, &grit, secs(0.005), 0.3);
    out
}

/// A hammer driving a nail into solid timber: a deep, short thud with the
/// crack of the blow on it.
fn hammer(pitch: f32, noise: &mut Noise) -> Vec<f32> {
    wood(pitch, 0.85, 1.6, noise)
}

/// A die landing on a wooden table, `hard` (0 to 1) as it lands: the die's
/// click (a small cube's modes, dead at once, a different face or edge each
/// time, and the crack of the hit) over a short knock of the tabletop that
/// only the harder landings wake.
fn die(hard: f32, noise: &mut Noise) -> Vec<f32> {
    const CUBE: [(f32, f32, f32); 3] = [(1.0, 1.0, 320.0), (1.47, 0.6, 380.0), (2.09, 0.3, 440.0)];
    let modes = tune(&CUBE, 460.0, 0.15, 1.0, noise);
    let mut out = ring(&strike(0.5, 0.6, noise), &modes, secs(0.04));
    normalize(&mut out, 1.0);
    add(&mut out, &crack(2.0, noise), 0, 0.25);
    add(&mut out, &wood(150.0, 0.6, 3.0, noise), 0, 0.2 + 0.5 * hard);
    out
}

/// A plucked string at `freq` (Karplus–Strong), dying away over `dur`
/// seconds; `bright` (0 to 1) is how hard it is plucked.
fn pluck(freq: f32, dur: f32, bright: f32, noise: &mut Noise) -> Vec<f32> {
    // The loop's two-sample average delays half a sample more.
    let period = SR / freq - 0.5;
    let mut excite: Vec<f32> = (0..period.ceil() as usize + 1).map(|_| noise.next()).collect();
    lowpass(&mut excite, 400.0 + 3500.0 * bright);
    // What the loop keeps each time round: the fundamental falls 60 dB in `dur`.
    let keep = 0.001_f32.powf(1.0 / (freq * dur));
    let mut out = vec![0.0; secs(dur)];
    for i in 0..out.len() {
        let pos = i as f32 - period;
        let back = if pos >= 1.0 {
            let (j, f) = (pos as usize, pos.fract());
            let now = out[j] * (1.0 - f) + out[j + 1] * f;
            let before = out[j - 1] * (1.0 - f) + out[j] * f;
            keep * 0.5 * (now + before)
        } else {
            0.0
        };
        out[i] = excite.get(i).copied().unwrap_or(0.0) + back;
    }
    fade_out(&mut out, secs(0.02));
    normalize(&mut out, 1.0);
    out
}

/// A two-pole resonance with unity gain at its peak: a horn's body.
struct Resonance {
    a1: f32,
    a2: f32,
    k: f32,
    y1: f32,
    y2: f32,
}

impl Resonance {
    fn new(freq: f32, decay: f32) -> Self {
        let (r, w) = ((-decay / SR).exp(), TAU * freq / SR);
        Resonance { a1: 2.0 * r * w.cos(), a2: -r * r, k: 2.0 * (1.0 - r) * w.sin(), y1: 0.0, y2: 0.0 }
    }

    fn tick(&mut self, x: f32) -> f32 {
        let y = x * self.k + self.a1 * self.y1 + self.a2 * self.y2;
        (self.y2, self.y1) = (self.y1, y);
        y
    }
}

/// A war horn sounding `notes` (start and length in seconds, pitch): a
/// breathy buzz through a horn's body, brightening as it swells, each note
/// scooping up into its pitch and wavering a little as a player's would.
fn horn(notes: &[(f32, f32, f32)], noise: &mut Noise) -> Vec<f32> {
    let end = notes.iter().map(|n| n.0 + n.1).fold(0.0, f32::max) + 0.25;
    let glide = 1.0 - (-1.0 / (0.04 * SR)).exp();
    let waver = 1.0 - (-TAU * 3.0 / SR).exp();
    let (mut freq, mut phase, mut wobble, mut low, mut lower) = (notes[0].2, 0.0_f32, 0.0_f32, 0.0, 0.0);
    let mut body = [Resonance::new(420.0, 400.0), Resonance::new(950.0, 700.0)];
    let mut out: Vec<f32> = (0..secs(end))
        .map(|i| {
            let t = i as f32 / SR;
            let (mut swell, mut target) = (0.0_f32, freq);
            for &(start, len, f) in notes {
                if t >= start {
                    target = f * (1.0 - 0.05 * (-(t - start) / 0.06).exp());
                }
                let rise = ((t - start) / 0.09).clamp(0.0, 1.0);
                let fall = ((start + len - t) / 0.18).clamp(0.0, 1.0);
                swell = swell.max(rise.min(fall));
            }
            freq += (target - freq) * glide;
            wobble += waver * (noise.next() - wobble);
            phase = (phase + freq * (1.0 + 0.6 * wobble) / SR).fract();
            let harmonics = ((SR * 0.45 / freq) as usize).min(20);
            let saw: f32 = (1..=harmonics).map(|k| (TAU * phase * k as f32).sin() / k as f32).sum();
            let breath = noise.next() * 0.12;
            let a = 1.0 - (-TAU * (220.0 + 1600.0 * swell * swell) / SR).exp();
            low += a * ((saw + breath) * swell - low);
            lower += a * (low - lower);
            lower + body[0].tick(lower) * 0.8 + body[1].tick(lower) * 0.35
        })
        .collect();
    normalize(&mut out, 1.0);
    out
}

/// Gives a sound its room: `tail` seconds to ring in, `wet` of the hall,
/// and a common loudness.
fn finish(mut out: Vec<f32>, wet: f32, tail: f32) -> Vec<f32> {
    out.resize(out.len() + secs(tail), 0.0);
    reverb(&mut out, wet);
    normalize(&mut out, 0.9);
    fade_out(&mut out, secs(0.03));
    out
}

/// The samples of `cue`.
pub fn cue(cue: Cue) -> Vec<f32> {
    let n = &mut Noise(0x9e37_79b9 ^ (cue as u32 + 1).wrapping_mul(0x85eb_ca6b));
    let mut out = Vec::new();
    match cue {
        Cue::Tap => finish(wood(170.0, 0.45, 1.2, n), 0.0, 0.0),
        Cue::Dice => {
            // Two dice thrown on a table: each bounces, the gaps and the
            // blows shrinking, then tumbles edge over edge in quick light
            // ticks and settles. The second lands a moment after the first.
            for first in [true, false] {
                let (mut t, mut gap, mut hard) = if first { (0.0, 0.12, 1.0) } else { (0.035, 0.10, 0.85) };
                while gap > 0.03 {
                    add(&mut out, &die(hard, n), secs(t), hard);
                    t += gap * (1.0 + 0.15 * n.next());
                    gap *= 0.7;
                    hard *= 0.8;
                }
                let rolls = if first { 6 } else { 8 };
                for k in 0..rolls {
                    t += 0.04 + 0.012 * n.next();
                    let soft = hard * 0.7 * (1.0 - k as f32 / rolls as f32);
                    add(&mut out, &die(soft, n), secs(t), soft.max(0.06));
                }
            }
            finish(out, 0.03, 0.08)
        }
        Cue::Harvest => {
            add(&mut out, &pluck(note(0), 0.9, 0.3, n), 0, 1.0);
            add(&mut out, &pluck(note(4), 0.9, 0.3, n), secs(0.08), 0.8);
            finish(out, 0.3, 0.4)
        }
        Cue::Horn => finish(horn(&[(0.0, 0.3, D2 * 1.5), (0.3, 1.1, D2 * 2.0)], n), 0.6, 1.0),
        Cue::TurnOther => finish(skin(52.0, 3.0, 8.0, n), 0.5, 0.8),
        Cue::Build => {
            // Hammering timber, the blows a little uneven.
            for (t, gain, pitch) in [(0.0, 1.0, 1.0), (0.24, 0.85, 1.05), (0.45, 0.95, 0.97), (0.70, 0.75, 1.02)] {
                add(&mut out, &hammer(105.0 * pitch, n), secs(t), gain);
            }
            finish(out, 0.04, 0.1)
        }
        Cue::Coins => {
            // Heavy coins dropped on a pile.
            const COIN: [(f32, f32, f32); 3] = [(1.0, 1.0, 25.0), (2.29, 0.5, 35.0), (3.9, 0.25, 45.0)];
            for (t, f, gain) in [(0.0, 1500.0, 1.0), (0.08, 1750.0, 0.7), (0.17, 1350.0, 0.85)] {
                add(&mut out, &metal(f, &COIN, 0.5, n), secs(t), gain);
            }
            finish(out, 0.2, 0.3)
        }
        Cue::Clash => {
            const BLADE: [(f32, f32, f32); 6] = [
                (1.0, 1.0, 3.5),
                (2.32, 0.7, 5.0),
                (3.86, 0.5, 6.0),
                (5.21, 0.35, 8.0),
                (6.73, 0.2, 10.0),
                (8.1, 0.12, 13.0),
            ];
            let mut scrape: Vec<f32> = (0..secs(0.3)).map(|i| n.next() * (-(i as f32 / SR) * 25.0).exp()).collect();
            highpass(&mut scrape, 1200.0);
            lowpass(&mut scrape, 4000.0);
            normalize(&mut scrape, 1.0);
            add(&mut out, &metal(330.0, &BLADE, 1.2, n), 0, 1.0);
            add(&mut out, &scrape, 0, 0.3);
            add(&mut out, &metal(370.0, &BLADE, 1.0, n), secs(0.09), 0.45);
            finish(out, 0.35, 0.6)
        }
        Cue::Magic => {
            // Chimes rising over an octave and a half, on a rush of air.
            const CHIME: [(f32, f32, f32); 3] = [(1.0, 1.0, 3.0), (2.76, 0.3, 5.0), (5.40, 0.1, 8.0)];
            for (j, degree) in [0, 2, 4, 6, 7, 9, 11].into_iter().enumerate() {
                add(&mut out, &metal(note(degree), &CHIME, 1.2, n), secs(0.06 * j as f32), 0.6);
            }
            let len = secs(0.9);
            let mut low = 0.0;
            let air: Vec<f32> = (0..len)
                .map(|i| {
                    let p = i as f32 / len as f32;
                    let a = 1.0 - (-TAU * (300.0 + 1700.0 * p) / SR).exp();
                    low += a * (n.next() - low);
                    low * (PI * p).sin()
                })
                .collect();
            add(&mut out, &air, 0, 0.35);
            finish(out, 0.6, 0.8)
        }
        Cue::Item => {
            const CHIME: [(f32, f32, f32); 3] = [(1.0, 1.0, 4.0), (2.76, 0.3, 7.0), (5.40, 0.1, 10.0)];
            for (t, degree) in [(0.0, 0), (0.08, 4), (0.16, 7)] {
                add(&mut out, &metal(note(degree), &CHIME, 1.0, n), secs(t), 1.0);
            }
            finish(out, 0.4, 0.6)
        }
        Cue::Trap => {
            // A thud, and two low strings a semitone apart.
            add(&mut out, &skin(48.0, 5.0, 8.0, n), 0, 1.0);
            add(&mut out, &pluck(D2, 1.2, 0.2, n), 0, 0.6);
            add(&mut out, &pluck(D2 * 2f32.powf(1.0 / 12.0), 1.2, 0.2, n), 0, 0.6);
            finish(out, 0.3, 0.6)
        }
        Cue::Bandit => {
            // Low strings falling a fifth, the second with a drum.
            add(&mut out, &pluck(D2 * 1.5, 1.2, 0.2, n), 0, 0.8);
            add(&mut out, &pluck(D2, 1.6, 0.2, n), secs(0.3), 1.0);
            add(&mut out, &skin(48.0, 5.0, 8.0, n), secs(0.3), 0.7);
            finish(out, 0.4, 0.6)
        }
        Cue::March => {
            add(&mut out, &step(n), 0, 1.0);
            add(&mut out, &step(n), secs(0.32), 0.8);
            finish(out, 0.1, 0.2)
        }
        Cue::Fanfare => {
            let (d3, f3, a3, d4) = (note(-7), note(-5), note(-3), note(0));
            add(&mut out, &horn(&[(0.0, 0.28, d3), (0.28, 0.28, f3), (0.56, 0.28, a3), (0.84, 1.5, d4)], n), 0, 1.0);
            add(&mut out, &horn(&[(0.84, 1.5, a3)], n), 0, 0.5);
            for t in [0.0, 0.56, 0.84] {
                add(&mut out, &skin(55.0, 3.5, 7.0, n), secs(t), 0.7);
            }
            finish(out, 0.5, 1.0)
        }
    }
}

/// The music's players: each instrument's notes made once, then laid into
/// bar after bar.
pub struct Band {
    drone: Vec<f32>,
    boom: Vec<f32>,
    rim: Vec<f32>,
    root: Vec<f32>,
    fifth: Vec<f32>,
    strings: HashMap<i32, Vec<f32>>,
    noise: Noise,
}

impl Band {
    pub fn new() -> Self {
        let noise = &mut Noise(0x2545_f491);
        Band {
            drone: drone(),
            boom: skin(58.0, 3.5, 7.0, noise),
            rim: skin(140.0, 14.0, 2.0, noise),
            root: pluck(note(-14), 2.2, 0.3, noise),
            fifth: pluck(note(-17), 2.2, 0.3, noise),
            strings: HashMap::new(),
            noise: Noise(0x6c07_8965),
        }
    }

    /// The samples of bar `index` of [`TUNE`]: the bar and what rings on
    /// over the next one.
    pub fn bar(&mut self, index: usize) -> Vec<f32> {
        let mut out = vec![0.0; BAR + TAIL];
        for (i, s) in self.drone.iter().enumerate() {
            let fade = if i < SEAM {
                i as f32 / SEAM as f32
            } else if i >= BAR {
                1.0 - (i - BAR) as f32 / SEAM as f32
            } else {
                1.0
            };
            out[i] += s * fade * 0.11;
        }
        add(&mut out, &self.boom, 0, 0.45);
        add(&mut out, &self.boom, 4 * EIGHTH, 0.4);
        add(&mut out, &self.rim, 6 * EIGHTH, 0.15);
        add(&mut out, &self.rim, 7 * EIGHTH, 0.18);
        add(&mut out, &self.root, 0, 0.25);
        add(&mut out, &self.fifth, 4 * EIGHTH, 0.2);
        for &(eighth, degree) in TUNE[index] {
            let noise = &mut self.noise;
            let string = self.strings.entry(degree).or_insert_with(|| pluck(note(degree - 7), 1.6, 0.4, noise));
            add(&mut out, string, eighth * EIGHTH, 0.4);
        }
        out.truncate(BAR + TAIL);
        reverb(&mut out, 0.5);
        lowpass(&mut out, 4500.0);
        fade_out(&mut out, secs(0.15));
        // Gently rounds what peaks, nearly linear below.
        out.iter_mut().for_each(|s| *s = (*s * 1.1).tanh());
        out
    }
}

/// The drone: D and its fifth, bowed, swelling once a bar, one bar and a
/// seam long. Every pitch and the swell fit a bar in whole cycles, so its
/// seam matches its start.
fn drone() -> Vec<f32> {
    let mut out: Vec<f32> = (0..BAR + SEAM)
        .map(|i| {
            let t = i as f64 / RATE as f64;
            let swell = 0.85 + 0.15 * (std::f64::consts::TAU * t * RATE as f64 / BAR as f64).sin();
            let mut s = 0.0;
            for (f, g) in [(D2 as f64, 1.0), (D2 as f64 * 1.5, 0.7), (D2 as f64 * 2.0, 0.4)] {
                for k in 1..=6 {
                    let k = k as f64;
                    s += g * (std::f64::consts::TAU * f * k * t).sin() / (k * k.sqrt());
                }
            }
            (s * swell) as f32
        })
        .collect();
    normalize(&mut out, 1.0);
    out
}

/// `samples` as a mono 16-bit WAV file.
pub fn wav(samples: &[f32]) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    out
}
