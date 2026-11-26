//! The client's state: the session, the latest server view, what the player
//! is doing on the board, and the animations events start. The server is
//! the only authority; the client sends actions and shows what comes back.

use std::time::Duration;

use kings_cove_engine::{
    Action, Event, Game, Phase, PlayerId, RobberId, Spell, TileId, UnitId, UnitKind, balance::MapSize,
};
use kings_cove_protocol::{ActionRequest, CreateRequest, ErrorBody, JoinRequest, MatchView, Session, SessionRequest};
use nsh_core::{prelude::*, storage::Storage};
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    board::{self, Anim, AnimKind, Mood},
    fx::{self, Fx, FxKind},
    palette,
    rules::Item,
};

/// How far behind the game queued effects may fall before they are dropped.
const MAX_BACKLOG_MS: f64 = 4000.0;
const SERVER_KEY: &str = "server";
const SESSION_KEY: &str = "session";
const NAME_KEY: &str = "name";
/// The server a fresh install talks to: `KC_SERVER_URL` when the app was
/// built (the published web build sets it), else this machine.
pub const DEFAULT_SERVER: &str = match option_env!("KC_SERVER_URL") {
    Some(url) if !url.is_empty() => url,
    _ => "http://127.0.0.1:8787",
};

/// What a tap on the board does next.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    Idle,
    Road,
    Outpost,
    UpgradeTown,
    UpgradeCity,
    Deploy(UnitKind),
    /// Aiming a spell: for two-tile spells, the first tile once picked.
    Cast(Spell, Option<kings_cove_engine::TileId>),
    Unit(UnitId),
    Robber(RobberId),
}

/// Something the player tapped to look at.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Inspect {
    Tile(kings_cove_engine::TileId),
    Vertex(kings_cove_engine::VertexId),
    Edge(kings_cove_engine::EdgeId),
}

/// The game menu, and which choice it is asking the player to confirm.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Menu {
    Closed,
    Open,
    ConfirmLeave,
    ConfirmEnd,
}

#[derive(Copy, Clone)]
pub struct AppState {
    pub server: Signal<String>,
    pub name: Signal<String>,
    /// The victory points a match created here is played to.
    pub goal: Signal<u32>,
    /// The island size a match created here is played on.
    pub map_size: Signal<MapSize>,
    pub session: Signal<Option<Session>>,
    pub view: Signal<Option<MatchView>>,
    pub toast: Signal<Option<String>>,
    pub mode: Signal<Mode>,
    pub inspect: Signal<Option<Inspect>>,
    pub busy: Signal<bool>,
    pub connected: Signal<bool>,
    pub anims: Signal<Vec<Anim>>,
    /// Screen-wide effects: turn banners, the dice roll, confetti.
    pub fx: Signal<Vec<Fx>>,
    /// Milliseconds, ticking while animations run.
    pub clock: Signal<f64>,
    /// The HUD's dice hide while the rolled ones fly to them.
    pub dice_hidden: Signal<bool>,
    dice_until: Stored<f64>,
    /// When the last queued effect's beat ends (ms): later ones follow it.
    fx_cursor: Stored<f64>,
    /// When the badges of resources the player just collected rise, so the
    /// inventory can bounce as they do (ms).
    pub gains_at: Stored<f64>,
    pub trade_open: Signal<bool>,
    pub history_open: Signal<bool>,
    pub menu: Signal<Menu>,
    /// The build tray item whose card is open.
    pub item_card: Signal<Option<Item>>,
    pub rules_open: Signal<bool>,
    pub spellbook_open: Signal<bool>,
    /// An attack the player tapped, waiting for them to confirm its outcome.
    pub confirm_attack: Signal<Option<Action>>,
    /// The room code was just copied: the lobby says so for a moment.
    pub code_copied: Signal<bool>,
    last_seq: Stored<Option<u64>>,
    poll_generation: Stored<u64>,
    ticker: Stored<Option<Spawned<()>>>,
    /// The app's own scope: requests, polls and timers live as long as the
    /// app, not as long as the button that started them.
    root: Owner,
}

pub fn now_ms() -> f64 {
    nsh_core::time::unix_ms()
}

impl AppState {
    pub fn new() -> Self {
        let session = Storage::get_item(SESSION_KEY).and_then(|s| serde_json::from_str::<Session>(&s).ok());
        let state = AppState {
            server: signal(Storage::get_item(SERVER_KEY).unwrap_or_else(|| DEFAULT_SERVER.to_owned())),
            name: signal(Storage::get_item(NAME_KEY).unwrap_or_default()),
            goal: signal(kings_cove_engine::balance::DEFAULT_VICTORY_POINTS),
            map_size: signal(MapSize::default()),
            session: signal(session.clone()),
            view: signal(None),
            toast: signal(None),
            mode: signal(Mode::Idle),
            inspect: signal(None),
            busy: signal(false),
            connected: signal(true),
            anims: signal(Vec::new()),
            fx: signal(Vec::new()),
            clock: signal(now_ms()),
            dice_hidden: signal(false),
            dice_until: stored(0.0),
            fx_cursor: stored(0.0),
            gains_at: stored(0.0),
            trade_open: signal(false),
            history_open: signal(false),
            menu: signal(Menu::Closed),
            item_card: signal(None),
            rules_open: signal(false),
            spellbook_open: signal(false),
            confirm_attack: signal(None),
            code_copied: signal(false),
            last_seq: stored(None),
            poll_generation: stored(0),
            ticker: stored(None),
            root: Owner::current(),
        };
        if session.is_some() {
            state.poll();
        }
        state
    }

    pub fn game(&self) -> Option<Game> {
        self.view.with(|v| v.as_ref().and_then(|v| v.game.clone()))
    }

    pub fn seat(&self) -> Option<PlayerId> {
        self.view.with(|v| v.as_ref().map(|v| v.seat))
    }

    /// Puts the room code on the clipboard, to paste into a message.
    pub fn copy_code(&self, code: String) {
        set_clipboard(code);
        self.code_copied.set(true);
        let copied = self.code_copied;
        self.root.run(|| set_timeout(Duration::from_millis(1800), move || copied.set(false)));
    }

    pub fn show_error(&self, message: String) {
        self.toast.set(Some(message.clone()));
        let toast = self.toast;
        self.root.run(|| {
            set_timeout(Duration::from_millis(2800), move || {
                if toast.get_untracked().as_deref() == Some(message.as_str()) {
                    toast.set(None);
                }
            })
        });
    }

    fn base(&self) -> String {
        self.server.get_untracked().trim().trim_end_matches('/').to_owned()
    }

    // ----- Session -----

    pub fn create(&self, name: String, victory_points: u32, map_size: MapSize) {
        self.remember_lobby_inputs(&name);
        let (this, base) = (*self, self.base());
        let body = CreateRequest { name, victory_points: Some(victory_points), map_size: Some(map_size) };
        self.request(async move { post::<_, Session>(&base, "/create", &body).await }, move |s| this.enter(s));
    }

    pub fn join(&self, code: String, name: String) {
        self.remember_lobby_inputs(&name);
        let (this, base) = (*self, self.base());
        self.request(async move { post::<_, Session>(&base, "/join", &JoinRequest { code, name }).await }, move |s| {
            this.enter(s)
        });
    }

    pub fn start_match(&self) {
        let Some(s) = self.session.get_untracked() else {
            return;
        };
        let (this, base) = (*self, self.base());
        let body = SessionRequest { code: s.code, token: s.token };
        self.request(async move { post::<_, MatchView>(&base, "/start", &body).await }, move |v| this.receive(v));
    }

    /// The host ends the match for every player.
    pub fn end_match(&self) {
        let Some(s) = self.session.get_untracked() else {
            return;
        };
        let (this, base) = (*self, self.base());
        let body = SessionRequest { code: s.code, token: s.token };
        self.request(async move { post::<_, MatchView>(&base, "/end", &body).await }, move |v| {
            this.menu.set(Menu::Closed);
            this.receive(v)
        });
    }

    /// Gives up this device's seat and goes back home. The match goes on
    /// on the server; the host can end it.
    pub fn leave(&self) {
        Storage::remove_item(SESSION_KEY);
        self.poll_generation.with(|g| *g += 1);
        self.session.set(None);
        self.view.set(None);
        self.mode.set(Mode::Idle);
        self.inspect.set(None);
        self.trade_open.set(false);
        self.history_open.set(false);
        self.menu.set(Menu::Closed);
        self.item_card.set(None);
        self.confirm_attack.set(None);
        self.anims.set(Vec::new());
        self.fx.set(Vec::new());
        self.fx_cursor.replace(0.0);
        self.last_seq.replace(None);
    }

    fn remember_lobby_inputs(&self, name: &str) {
        let _ = Storage::set_item(NAME_KEY, name);
        let _ = Storage::set_item(SERVER_KEY, self.base());
    }

    fn enter(&self, session: Session) {
        let _ = Storage::set_item(SESSION_KEY, serde_json::to_string(&session).expect("json"));
        self.session.set(Some(session));
        self.poll();
    }

    // ----- Actions -----

    /// Sends an action; the server validates it and the answer replaces the view.
    pub fn send(&self, action: Action) {
        let Some(s) = self.session.get_untracked() else {
            return;
        };
        let (this, base) = (*self, self.base());
        let body = ActionRequest { code: s.code, token: s.token, action };
        self.request(async move { post::<_, MatchView>(&base, "/action", &body).await }, move |v| this.receive(v));
    }

    fn request<T: 'static>(
        &self,
        call: impl Future<Output = Result<T, String>> + 'static,
        done: impl FnOnce(T) + 'static,
    ) {
        if self.busy.get_untracked() {
            return;
        }
        self.busy.set(true);
        let this = *self;
        self.root.run(|| {
            spawn(async move {
                let result = call.await;
                this.busy.set(false);
                match result {
                    Ok(value) => done(value),
                    Err(e) => this.show_error(e),
                }
            })
        });
    }

    /// Long-polls the server for every new version of the match.
    fn poll(&self) {
        let generation = self.poll_generation.with(|g| {
            *g += 1;
            *g
        });
        let this = *self;
        self.root.run(|| {
            spawn(async move {
                loop {
                    if this.poll_generation.get() != generation {
                        return;
                    }
                    let Some(s) = this.session.get_untracked() else {
                        return;
                    };
                    let since = this.view.with_untracked(|v| v.as_ref().map_or(0, |v| v.version));
                    let url = format!("{}/poll?code={}&token={}&since={since}", this.base(), s.code, s.token);
                    match get::<MatchView>(&url).await {
                        Ok(view) if this.poll_generation.get() == generation => {
                            this.connected.set(true);
                            let ended = view.ended;
                            this.receive(view);
                            // Nothing more will change in an ended match.
                            if ended {
                                return;
                            }
                        }
                        Ok(_) => return,
                        Err(e) if e.contains("not seated") || e.contains("No match") => {
                            this.show_error(e);
                            this.leave();
                            return;
                        }
                        Err(_) => {
                            this.connected.set(false);
                            timer(Duration::from_secs(2)).await;
                        }
                    }
                }
            })
        });
    }

    /// Takes a view from the server if it is newer than ours, animating the
    /// events it brings.
    fn receive(&self, view: MatchView) {
        let current = self.view.with_untracked(|v| v.as_ref().map(|v| v.version));
        if current.is_some_and(|c| c >= view.version) {
            return;
        }
        if let Some(game) = &view.game {
            let latest = game.event_seq;
            match self.last_seq.get() {
                // First sight of this match: show it as it is, animate nothing
                // old, but say whose turn it is while the island is set up.
                None if matches!(game.phase, Phase::Setup { .. }) => {
                    self.animate(None, game, view.seat, &view.players, &[])
                }
                None => {}
                Some(seen) => {
                    let previous = self.game();
                    let fresh: Vec<Event> =
                        game.events.iter().filter(|e| e.seq > seen).map(|e| e.event.clone()).collect();
                    self.animate(previous.as_ref(), game, view.seat, &view.players, &fresh);
                }
            }
            self.last_seq.replace(Some(latest));
            if !game.is_active(view.seat) {
                self.mode.set(Mode::Idle);
            }
        }
        let enchanted = view.game.as_ref().is_some_and(Game::spells_on_board);
        self.view.set(Some(view));
        // Spells on the board shimmer for as long as they last.
        if enchanted {
            self.run_clock();
        }
    }

    /// Schedules what `events` show, one beat after another: a turn's
    /// banner, then the dice, then the tiles they hit and what they yield.
    fn animate(&self, before: Option<&Game>, after: &Game, seat: PlayerId, names: &[String], events: &[Event]) {
        let now = now_ms();
        // Catching up on several turns at once, only the latest is worth showing.
        let turns: Vec<usize> =
            events.iter().enumerate().filter(|(_, e)| matches!(e, Event::TurnStarted { .. })).map(|(i, _)| i).collect();
        let events = &events[turns.len().checked_sub(2).map_or(0, |i| turns[i] + 1)..];
        let name = |p: PlayerId| names.get(p.index()).cloned().unwrap_or_else(|| format!("Player {}", p.0 + 1));
        let banner = |player: PlayerId, round: u32, start: f64| {
            let mine = player == seat;
            let subtitle = match after.phase {
                Phase::Setup { step, .. } if mine => format!("Place your {}", after.setup_building_kind(step).name()),
                Phase::Setup { .. } => "Setting up the island".to_owned(),
                _ if mine => format!("Round {round} · roll the dice"),
                _ => format!("Round {round}"),
            };
            if mine {
                haptic(Haptic::Notification(HapticNotification::Success));
            }
            Fx {
                start,
                dur: if mine { fx::MY_BANNER_MS } else { fx::BANNER_MS },
                kind: FxKind::Banner {
                    title: if mine { "Your turn!".to_owned() } else { format!("{}'s turn", name(player)) },
                    subtitle,
                    color: palette::player(player),
                    mine,
                },
            }
        };

        let (mut anims, mut effects) = (Vec::new(), Vec::new());
        // Effects play one after another, after those already queued; when
        // the queue falls far behind the game, what is stale is dropped.
        let mut cursor = now.max(self.fx_cursor.get());
        if cursor - now > MAX_BACKLOG_MS {
            self.anims.update(|a| a.retain(|x| x.start <= now));
            self.fx.update(|f| f.retain(|x| x.start <= now));
            self.dice_until.replace(now);
            cursor = now;
        }
        // When this batch's tiles light up, and how many badges each has had.
        let mut harvest = cursor;
        let mut badges: Vec<(TileId, u32)> = Vec::new();
        for event in events {
            match *event {
                Event::Rolled { player, dice } => {
                    let sum = dice.0 + dice.1;
                    let mine = player == seat;
                    effects.push(Fx {
                        start: cursor,
                        dur: fx::DICE_MS,
                        kind: FxKind::Dice {
                            dice,
                            caption: if mine { "You rolled".to_owned() } else { format!("{} rolled", name(player)) },
                            note: (sum == 7).then(|| {
                                let place = after.robber_relocation && after.robbers.is_empty();
                                match (mine, place) {
                                    (true, true) => "Place a bandit!".to_owned(),
                                    (true, false) => "Move a bandit!".to_owned(),
                                    (false, true) => "A bandit is coming".to_owned(),
                                    (false, false) => "A bandit moves".to_owned(),
                                }
                            }),
                        },
                    });
                    self.dice_hidden.set(true);
                    self.dice_until.replace(cursor + fx::DICE_LANDED_MS);
                    harvest = cursor + fx::DICE_LANDED_MS - 100.0;
                    for t in after.board.tiles.iter().filter(|t| t.number == sum) {
                        let spell = before.and_then(|b| b.enchantment_at(t.id)).map(|e| e.spell);
                        let mood = match (after.robber_at(t.id).is_some() || after.blockaded(t.id), spell) {
                            (true, _) => Mood::Blocked,
                            (false, Some(s)) if s.stops() => Mood::Stopped(s),
                            (false, Some(s)) if s.doubles() => Mood::Doubled(s),
                            _ => Mood::Normal,
                        };
                        let kind = AnimKind::Jelly { tile: t.id, mood };
                        anims.push(Anim { start: harvest, dur: board::JELLY_MS, kind });
                    }
                    badges.clear();
                    cursor = harvest + 300.0;
                    if mine {
                        haptic(Haptic::Impact(HapticImpact::Medium));
                    }
                }
                Event::Produced { player, tile, resource, amount } => {
                    let of =
                        events.iter().filter(|e| matches!(e, Event::Produced { tile: t, .. } if *t == tile)).count();
                    let slot = match badges.iter_mut().find(|(t, _)| *t == tile) {
                        Some((_, n)) => {
                            *n += 1;
                            *n - 1
                        }
                        None => {
                            badges.push((tile, 1));
                            0
                        }
                    };
                    let start = harvest + 250.0 + slot as f64 * 120.0;
                    let mine = player == seat;
                    let color = palette::player(player);
                    let kind = AnimKind::Gain { tile, resource, amount, color, mine, slot, of: of as u32 };
                    anims.push(Anim { start, dur: board::GAIN_MS, kind });
                    if mine {
                        self.gains_at.replace(start + 500.0);
                    }
                }
                Event::TurnStarted { player, round } => {
                    let b = banner(player, round, cursor);
                    cursor = b.end() - 300.0;
                    effects.push(b);
                }
                Event::Won { .. } => effects.push(Fx { start: cursor, dur: fx::CONFETTI_MS, kind: FxKind::Confetti }),
                // What a roll's tiles did, shown with their wobble.
                Event::ProductionBlocked { tile }
                | Event::ProductionBlockaded { tile, .. }
                | Event::ProductionStopped { tile, .. }
                | Event::SpellFaded { tile, .. } => {
                    let (label, delay) = match *event {
                        Event::ProductionBlocked { .. } => ("Blocked", 250.0),
                        Event::ProductionBlockaded { .. } => ("Blockaded!", 250.0),
                        Event::ProductionStopped { .. } => ("Frozen!", 250.0),
                        Event::SpellFaded { spell, .. } => (
                            match spell {
                                Spell::Frost => "The frost melts",
                                Spell::Bounty => "Bounty spent",
                                Spell::Luck => "Luck spent",
                                _ => "The numbers swap back",
                            },
                            1100.0,
                        ),
                        _ => continue,
                    };
                    let color = match *event {
                        Event::ProductionStopped { spell, .. } | Event::SpellFaded { spell, .. } => {
                            crate::art::spell_colors(spell).1
                        }
                        _ => 0x2b2b2e,
                    };
                    let kind = AnimKind::Float { at: board::tile_pos(after, tile), color, label: label.to_owned() };
                    anims.push(Anim { start: harvest + delay, dur: 1300.0, kind });
                }
                Event::WindfallPaid { player, gained } => {
                    let label = format!("{} windfall: +{}", if player == seat { "Your" } else { "A" }, gained.total());
                    let color = crate::art::spell_colors(Spell::Windfall).1;
                    let kind = AnimKind::Float { at: (0.0, 0.0), color, label };
                    anims.push(Anim { start: harvest + 400.0, dur: 1800.0, kind });
                }
                Event::RingPaid { player, tile, resource, amount } => {
                    let color = palette::player(player);
                    let kind = AnimKind::Gain { tile, resource, amount, color, mine: player == seat, slot: 0, of: 1 };
                    anims.push(Anim { start: cursor, dur: board::GAIN_MS, kind });
                }
                _ => anims.extend(board::anims_for(after, event, cursor)),
            }
        }
        // Setup turns pass without a TurnStarted.
        if matches!(after.phase, Phase::Setup { .. }) && before.is_none_or(|b| b.current != after.current) {
            let b = banner(after.current, 0, cursor + 250.0);
            cursor = b.end() - 300.0;
            effects.push(b);
        }
        self.fx_cursor.replace(cursor);
        if anims.is_empty() && effects.is_empty() {
            return;
        }
        self.anims.update(|a| a.extend(anims));
        self.fx.update(|f| f.extend(effects));
        self.run_clock();
    }

    /// Ticks `clock` every frame while anything animates.
    fn run_clock(&self) {
        if self.ticker.with(|t| t.is_some()) {
            return;
        }
        let this = *self;
        let task = self.root.run(|| {
            set_interval(Duration::from_millis(16), move || {
                let now = now_ms();
                let dice_until = this.dice_until.get();
                if dice_until <= now && this.dice_hidden.get_untracked() {
                    this.dice_hidden.set(false);
                }
                let busy = this.anims.with_untracked(|a| a.iter().any(|x| x.end() > now))
                    || this.fx.with_untracked(|f| f.iter().any(|x| x.end() > now))
                    || dice_until > now;
                let ambient = this.view.with_untracked(|v| {
                    v.as_ref().and_then(|v| v.game.as_ref()).is_some_and(Game::spells_on_board)
                });
                // Spells alone shimmer at about 30 frames a second.
                if busy || now - this.clock.get_untracked() >= 30.0 {
                    this.clock.set(now);
                }
                if !busy {
                    if this.anims.with_untracked(|a| !a.is_empty()) {
                        this.anims.set(Vec::new());
                    }
                    if this.fx.with_untracked(|f| !f.is_empty()) {
                        this.fx.set(Vec::new());
                    }
                    if !ambient && let Some(t) = this.ticker.replace(None) {
                        t.cancel();
                    }
                }
            })
        });
        self.ticker.replace(Some(task));
    }
}

async fn post<B: Serialize, R: DeserializeOwned>(base: &str, path: &str, body: &B) -> Result<R, String> {
    let request = HttpRequest::post(format!("{base}{path}"), serde_json::to_vec(body).map_err(|e| e.to_string())?)
        .header("Content-Type", "application/json");
    decode(fetch(request).await.map_err(|_| "Cannot reach the server".to_owned())?)
}

async fn get<R: DeserializeOwned>(url: &str) -> Result<R, String> {
    decode(fetch(url.to_owned()).await?)
}

fn decode<R: DeserializeOwned>(response: HttpResponse) -> Result<R, String> {
    let text = response.text();
    if response.ok() {
        serde_json::from_str(&text).map_err(|e| format!("Unreadable answer: {e}"))
    } else {
        Err(serde_json::from_str::<ErrorBody>(&text)
            .map(|e| e.error)
            .unwrap_or_else(|_| format!("Server answered {}", response.status())))
    }
}
