//! Sound effects and music: what plays, when, and whether the player wants
//! it. Every sound is synthesized ([`crate::synth`]) and loaded once as the
//! app starts. Effects play as the board shows what they go with; the music
//! loops only while a match is on screen ([`music`]).

use std::{cell::RefCell, time::Duration};

use kings_cove_engine::{Event, ItemKind};
use nsh_core::{platform::Lifecycle, prelude::*};

use crate::synth;

/// How loud the music plays, under the effects.
const MUSIC_VOLUME: f32 = 0.06;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Cue {
    /// A button pressed.
    Tap,
    Dice,
    /// Resources of the player's own coming in.
    Harvest,
    /// The player's own turn starting.
    Horn,
    /// Someone else's turn starting.
    TurnOther,
    Build,
    Coins,
    Clash,
    Magic,
    Item,
    Trap,
    Bandit,
    /// A knight deployed or marching.
    March,
    /// The match won.
    Fanfare,
}

impl Cue {
    const COUNT: usize = 14;
    const ALL: [Cue; Self::COUNT] = [
        Cue::Tap,
        Cue::Dice,
        Cue::Harvest,
        Cue::Horn,
        Cue::TurnOther,
        Cue::Build,
        Cue::Coins,
        Cue::Clash,
        Cue::Magic,
        Cue::Item,
        Cue::Trap,
        Cue::Bandit,
        Cue::March,
        Cue::Fanfare,
    ];
}

/// The loaded sounds, and what plays them.
struct Bank {
    effects: [Option<Sound>; Cue::COUNT],
    bars: Vec<Sound>,
    effects_on: bool,
    /// Bumped to drop the effects waiting to play ([`play_in`]).
    generation: u64,
    /// The music's loop, while it plays.
    music: Option<Spawned<()>>,
}

thread_local! {
    static BANK: RefCell<Bank> = RefCell::new(Bank {
        effects: Default::default(),
        bars: Vec::new(),
        effects_on: true,
        generation: 0,
        music: None,
    });
}

/// Synthesizes every sound and loads it, the effects first. Where nothing
/// plays sound, or a sound fails to load, the game goes on without it.
pub async fn load() {
    if !audio_available().await {
        return;
    }
    for cue in Cue::ALL {
        match load_sound(synth::wav(&synth::cue(cue))).await {
            Ok(sound) => BANK.with(|b| b.borrow_mut().effects[cue as usize] = Some(sound)),
            Err(e) => log::warn!("sound {cue:?} did not load: {}", e.message),
        }
    }
    let mut band = synth::Band::new();
    let mut bars = Vec::with_capacity(synth::BARS);
    for i in 0..synth::BARS {
        match load_sound(synth::wav(&band.bar(i))).await {
            Ok(sound) => bars.push(sound),
            Err(e) => {
                log::warn!("music bar {i} did not load: {}", e.message);
                return;
            }
        }
    }
    BANK.with(|b| b.borrow_mut().bars = bars);
}

/// Turns the effects on or off; off also silences those playing or waiting.
pub fn set_effects(on: bool) {
    BANK.with(|b| b.borrow_mut().effects_on = on);
    if !on {
        hush();
    }
}

/// Stops every effect playing or waiting to play: leaving a match.
pub fn hush() {
    BANK.with(|b| {
        let mut b = b.borrow_mut();
        b.generation += 1;
        b.effects.iter().flatten().for_each(Sound::stop);
    });
}

/// Plays `cue` at `volume` (0 to 1) and `rate` (0.5 to 2, the pitch with
/// it), unless the effects are off or the app is in the background.
pub fn play(cue: Cue, volume: f32, rate: f32) {
    if lifecycle() == Lifecycle::Background {
        return;
    }
    BANK.with(|b| {
        let b = b.borrow();
        if b.effects_on
            && let Some(sound) = &b.effects[cue as usize]
        {
            sound.play(volume, rate);
        }
    });
}

/// Plays `cue` in `delay` ms, when what it goes with shows ([`play`]); not
/// once [`hush`]ed. The wait belongs to the current owner.
pub fn play_in(delay: f64, cue: Cue, volume: f32, rate: f32) {
    let (on, generation) = BANK.with(|b| {
        let b = b.borrow();
        (b.effects_on, b.generation)
    });
    if !on {
        return;
    }
    if delay < 1.0 {
        play(cue, volume, rate);
        return;
    }
    set_timeout(Duration::from_secs_f64(delay / 1000.0), move || {
        if BANK.with(|b| b.borrow().generation) == generation {
            play(cue, volume, rate);
        }
    });
}

/// The effect a move on the board plays, with its volume and rate. Rolls,
/// harvests, turns and wins are cued where they are shown.
pub fn cue_for(event: &Event) -> Option<(Cue, f32, f32)> {
    Some(match event {
        Event::BuiltRoad { .. } => (Cue::Build, 0.3, 1.1),
        Event::Built { .. } | Event::Upgraded { .. } => (Cue::Build, 0.4, 1.0),
        Event::BlockadeLaid { .. } => (Cue::Build, 0.35, 0.8),
        Event::Deployed { .. } | Event::UnitMoved { .. } => (Cue::March, 0.5, 1.0),
        Event::KnightsClashed { .. } | Event::KnightKilledRobber { .. } => (Cue::Clash, 0.7, 1.0),
        Event::SpellCast { .. } => (Cue::Magic, 0.6, 1.0),
        Event::ItemFound { kind: ItemKind::Trap, .. } => (Cue::Trap, 0.7, 1.0),
        Event::ItemFound { .. } | Event::StructureTaken { .. } => (Cue::Item, 0.6, 1.0),
        Event::RobberPlaced { .. } | Event::RobberRelocated { .. } | Event::RobberSpawned { .. } => {
            (Cue::Bandit, 0.6, 1.0)
        }
        Event::Traded { .. } | Event::BankTraded { .. } | Event::CardBought { .. } => (Cue::Coins, 0.6, 1.0),
        _ => return None,
    })
}

/// Starts the music looping, or stops it. The loop belongs to the current
/// owner, which must outlive it (the app's).
pub fn music(on: bool) {
    if on == BANK.with(|b| b.borrow().music.is_some()) {
        return;
    }
    if !on {
        let (task, bars) = BANK.with(|b| {
            let mut b = b.borrow_mut();
            (b.music.take(), b.bars.clone())
        });
        if let Some(task) = task {
            task.cancel();
        }
        bars.iter().for_each(Sound::stop);
        return;
    }
    let task = spawn(async {
        // The bars may still be loading as the match opens.
        let bars = loop {
            let bars = BANK.with(|b| b.borrow().bars.clone());
            if !bars.is_empty() {
                break bars;
            }
            timer(Duration::from_millis(250)).await;
        };
        let bar = Duration::from_secs_f64(synth::BAR as f64 / synth::RATE as f64);
        let mut next = nsh_core::time::now();
        for &i in synth::SCORE.iter().cycle() {
            timer_until(next).await;
            // Fallen behind (a busy or throttled moment): go on from now
            // rather than rush the bars to catch up.
            let now = nsh_core::time::now();
            if now > next + bar / 4 {
                next = now;
            }
            bars[i].play(MUSIC_VOLUME, 1.0);
            next = next + bar;
        }
    });
    BANK.with(|b| b.borrow_mut().music = Some(task));
}
