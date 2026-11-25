//! The tabletop look: muted terrain, distinct player colours.

use kings_cove_engine::{PlayerId, Resource};
use nsh_core::prelude::*;

pub const TABLE: u32 = 0x31505c;
pub const INK: u32 = 0x1f2328;
pub const PAPER: u32 = 0xf6f1e4;
pub const PAPER_DIM: u32 = 0xe7dfcb;
pub const MUTED: u32 = 0x6b6658;
pub const GOLD: u32 = 0xf2c14e;
pub const DANGER: u32 = 0xd8453b;
pub const GO: u32 = 0x3f8f5a;
pub const HOT: u32 = 0xb0302a;

pub const PLAYERS: [u32; 4] = [0xc8423a, 0x2f6db5, 0xf2eee3, 0x8a4fb0];

pub fn hex(c: u32) -> Rgba {
    rgb(c)
}

/// `c` at `alpha` (0..1).
pub fn hexa(c: u32, alpha: f32) -> Rgba {
    rgba((c << 8) | (alpha.clamp(0.0, 1.0) * 255.0).round() as u32)
}

pub fn player(p: PlayerId) -> u32 {
    PLAYERS[p.index() % PLAYERS.len()]
}

pub fn resource(r: Resource) -> u32 {
    match r {
        Resource::Wood => 0x5f8a4a,
        Resource::Stone => 0x9b978c,
        Resource::Food => 0xd9b75a,
        Resource::Iron => 0xa5674b,
    }
}

/// A darker shade for glyphs and edges.
pub fn shade(c: u32, f: f32) -> u32 {
    let ch = |s: u32| (((c >> s) & 0xff) as f32 * f).clamp(0.0, 255.0) as u32;
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

/// `c` mixed toward white by `f` (0..1).
pub fn lighten(c: u32, f: f32) -> u32 {
    let ch = |s: u32| {
        let v = ((c >> s) & 0xff) as f32;
        (v + (255.0 - v) * f).clamp(0.0, 255.0) as u32
    };
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

/// Text that reads on a fill of `c`.
pub fn ink_on(c: u32) -> u32 {
    let luma = 0.299 * ((c >> 16) & 0xff) as f32 + 0.587 * ((c >> 8) & 0xff) as f32 + 0.114 * (c & 0xff) as f32;
    if luma > 170.0 { INK } else { 0xffffff }
}
