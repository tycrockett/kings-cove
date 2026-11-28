//! The client's state: the session, the latest server view, what the player
//! is doing on the board, and the animations events start. The server is
//! the only authority; the client sends actions and shows what comes back.

use std::time::Duration;

use kings_cove_engine::{
    Action, Event, Game, Phase, PlayerId, Resources, RobberId, Spell, TileId, UnitId, UnitKind, bot::Style,
};
use kings_cove_protocol::{
    ActionRequest, AddBotRequest, ConfigureRequest, CreateRequest, ErrorBody, JoinRequest, MatchView, ReactRequest,
    Reaction, SentReaction, Session, SessionRequest,
};
use nsh_core::{prelude::*, share::ShareRequest, storage::Storage};
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    board::{self, Anim, AnimKind, Mood},
    fx::{self, Fx, FxKind},
    nudge::{self, Anchors, Nudge, Spots},
    palette,
    rules::Item,
    sound::{self, Cue},
};
use nsh_core::platform::Lifecycle;

/// How far behind the game queued effects may fall before they are dropped:
/// room for a turn passing, played move by move, and the next one's banner.
const MAX_BACKLOG_MS: f64 = 10_000.0;
/// The longest one of another player's moves (or a bandit's, as the round
/// ends) holds the board before the next one plays (ms).
const MOVE_STEP_MS: f64 = 450.0;
/// The pause between the board settling and the next turn's banner (ms).
const SETTLE_MS: f64 = 250.0;
/// A roll's tiles light up one after another in an order drawn for the roll:
/// this far apart (ms), each up to `WAVE_JITTER_MS` late, so their badges
/// and pops never land together.
const WAVE_STEP_MS: f64 = 55.0;
const WAVE_JITTER_MS: f64 = 10.0;
/// How soon after a tile starts to wobble its badges (or its "Blocked")
/// show, and it pops (ms).
const YIELD_MS: f64 = 100.0;
/// The clock's period while something animates: every display frame, up to
/// 60 a second (a 120 Hz screen gets every other one).
const FRAME_MS: u64 = 16;
/// Its period while only spells shimmer on the board: 30 frames a second
/// keep the shimmer smooth while working a phone half as hard.
const SHIMMER_MS: u64 = 33;
/// How long a reaction stays in the middle of the screen.
const REACTION_SHOW_MS: u64 = 2600;
const SERVER_KEY: &str = "server";
const SESSION_KEY: &str = "session";
const NAME_KEY: &str = "name";
const EFFECTS_KEY: &str = "sound-effects";
const MUSIC_KEY: &str = "music";
const TIPS_KEY: &str = "tips";
/// The server a fresh install talks to: `KC_SERVER_URL` when the app was
/// built (the published web build sets it), else this machine.
pub const DEFAULT_SERVER: &str = match option_env!("KC_SERVER_URL") {
    Some(url) if !url.is_empty() => url,
    _ => "http://127.0.0.1:8787",
};

/// Where the web client is published: room links from the other builds
/// open it. `KC_WEB_URL` when the app was built, else the GitHub Pages site.
pub const DEFAULT_WEB_URL: &str = match option_env!("KC_WEB_URL") {
    Some(url) if !url.is_empty() => url,
    _ => "https://tycrockett.github.io/kings-cove/",
};
/// The query parameter of a room link that names its room.
const ROOM_PARAM: &str = "room";

/// A link into room `code`: the web client's own address (`page`, when it
/// is a web page) or [`DEFAULT_WEB_URL`], asking for the room.
pub fn room_link(page: Option<&str>, code: &str) -> String {
    let page = page.filter(|p| p.starts_with("https://") || p.starts_with("http://")).unwrap_or(DEFAULT_WEB_URL);
    let base = page.split(['?', '#']).next().unwrap_or(page);
    format!("{base}?{ROOM_PARAM}={code}")
}

/// The room a link asks for, if any.
pub fn room_in_link(url: &str) -> Option<String> {
    let query = url.split('#').next()?.split_once('?')?.1;
    query
        .split('&')
        .find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == ROOM_PARAM))
        .map(|(_, code)| code.trim().to_ascii_uppercase())
        .filter(|code| !code.is_empty() && code.chars().all(|c| c.is_ascii_alphanumeric()))
}

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

/// How much of the screen the HUD covers, in points, as laid out: the top
/// bar from the top edge, and the action strip and the dock above the
/// bottom edge. Zero until measured.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Hud {
    pub top: f32,
    pub strip: f32,
    pub dock: f32,
}

impl Hud {
    pub fn measured(&self) -> bool {
        self.top > 0.0 && self.dock > 0.0
    }
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
    pub session: Signal<Option<Session>>,
    pub view: Signal<Option<MatchView>>,
    pub toast: Signal<Option<String>>,
    pub mode: Signal<Mode>,
    pub inspect: Signal<Option<Inspect>>,
    /// How much of the screen the HUD covers, as laid out.
    pub hud: Signal<Hud>,
    /// The tile the board is zoomed in on; `None` shows the whole island.
    /// Write it with `look_at`.
    pub focus: Signal<Option<TileId>>,
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
    /// The offer being put together, kept while the composer is closed so a
    /// declined offer can be sweetened and sent again.
    pub trade_give: Signal<Resources>,
    pub trade_want: Signal<Resources>,
    pub trade_to: Signal<Option<PlayerId>>,
    /// A counter being put together to the open offer, as the maker would see
    /// it: what they give and what they get.
    pub counter: Signal<Option<(Resources, Resources)>>,
    pub history_open: Signal<bool>,
    /// Everyone's chips, dropped below the active player's.
    pub players_open: Signal<bool>,
    /// The row of reactions to pick from.
    pub reactions_open: Signal<bool>,
    /// Reactions showing beside their senders' chips right now.
    pub reactions_shown: Signal<Vec<SentReaction>>,
    /// The `seq` of the latest reaction seen; `None` before the first view.
    last_reaction: Stored<Option<u64>>,
    pub menu: Signal<Menu>,
    /// The build tray item whose card is open.
    pub item_card: Signal<Option<Item>>,
    pub rules_open: Signal<bool>,
    pub spellbook_open: Signal<bool>,
    /// An attack the player tapped, waiting for them to confirm its outcome.
    pub confirm_attack: Signal<Option<Action>>,
    /// The room code was just copied: the lobby says so for a moment.
    pub code_copied: Signal<bool>,
    /// The room link was just copied, where there is no share sheet.
    pub link_copied: Signal<bool>,
    /// The room code typed on the home screen, or a room link's.
    pub join_code: Signal<String>,
    /// The player's sound settings: effects, and music during a match.
    pub effects_on: Signal<bool>,
    pub music_on: Signal<bool>,
    /// Whether tips pop up over the dock.
    pub tips_on: Signal<bool>,
    /// The tip showing over the dock, if any.
    pub nudge: Signal<Option<Nudge>>,
    /// Where the tips' anchors were laid out.
    pub spots: Signal<Spots>,
    /// The elements tips point at, for their overlays to follow.
    pub anchors: Anchors,
    /// The tips already shown this turn.
    seen_nudges: Stored<Vec<Nudge>>,
    /// Tips about something that just happened, waiting for the showing one
    /// to go, highest priority first.
    pending_nudges: Stored<Vec<Nudge>>,
    last_seq: Stored<Option<u64>>,
    poll_generation: Stored<u64>,
    /// The interval ticking `clock`, with its period (ms).
    ticker: Stored<Option<(u64, Spawned<()>)>>,
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
            session: signal(session.clone()),
            view: signal(None),
            toast: signal(None),
            mode: signal(Mode::Idle),
            inspect: signal(None),
            hud: signal(Hud::default()),
            focus: signal(None),
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
            trade_give: signal(Resources::ZERO),
            trade_want: signal(Resources::ZERO),
            trade_to: signal(None),
            counter: signal(None),
            history_open: signal(false),
            players_open: signal(false),
            reactions_open: signal(false),
            reactions_shown: signal(Vec::new()),
            last_reaction: stored(None),
            menu: signal(Menu::Closed),
            item_card: signal(None),
            rules_open: signal(false),
            spellbook_open: signal(false),
            confirm_attack: signal(None),
            code_copied: signal(false),
            link_copied: signal(false),
            join_code: signal(String::new()),
            effects_on: signal(Storage::get_item(EFFECTS_KEY).as_deref() != Some("off")),
            music_on: signal(Storage::get_item(MUSIC_KEY).as_deref() != Some("off")),
            tips_on: signal(Storage::get_item(TIPS_KEY).as_deref() != Some("off")),
            nudge: signal(None),
            spots: signal(Spots::default()),
            anchors: Anchors::new(),
            seen_nudges: stored(Vec::new()),
            pending_nudges: stored(Vec::new()),
            last_seq: stored(None),
            poll_generation: stored(0),
            ticker: stored(None),
            root: Owner::current(),
        };
        if session.is_some() {
            state.poll();
        }
        // A room link (the web client's address) fills in the code to join.
        let join_code = state.join_code;
        state.root.run(|| {
            spawn(async move {
                if let Some(code) = app_url().await.as_deref().and_then(room_in_link) {
                    join_code.set(code);
                }
            })
        });
        sound::set_effects(state.effects_on.get_untracked());
        state.root.run(|| spawn(sound::load()));
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

    /// Shares a link into room `code` through the system's share sheet (a
    /// message, mail, another app); where there is none, or it fails, puts
    /// the link on the clipboard.
    pub fn share_room(&self, code: String) {
        let this = *self;
        self.root.run(|| {
            spawn(async move {
                let link = room_link(app_url().await.as_deref(), &code);
                if share_available().await {
                    let request = ShareRequest::new()
                        .title("King's Cove")
                        .text(format!("Join my King's Cove match! Room code: {code}"))
                        .url(link.clone());
                    if share(request).await.is_ok() {
                        return;
                    }
                }
                set_clipboard(link);
                this.link_copied.set(true);
                let copied = this.link_copied;
                this.root.run(|| set_timeout(Duration::from_millis(1800), move || copied.set(false)));
            })
        });
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

    pub fn create(&self, name: String) {
        self.remember_lobby_inputs(&name);
        let (this, base) = (*self, self.base());
        let body = CreateRequest { name };
        self.request(async move { post::<_, Session>(&base, "/create", &body).await }, move |s| this.enter(s));
    }

    /// A started match against a rival the server plays, set up mid-game:
    /// for trying the client alone. Debug servers only.
    pub fn practice(&self, name: String) {
        self.remember_lobby_inputs(&name);
        let (this, base) = (*self, self.base());
        let body = CreateRequest { name };
        self.request(async move { post::<_, Session>(&base, "/practice", &body).await }, move |s| this.enter(s));
    }

    /// The host changes the match's settings in the lobby: those `edit` sets.
    pub fn configure(&self, edit: impl FnOnce(&mut ConfigureRequest)) {
        let Some(s) = self.session.get_untracked() else {
            return;
        };
        let (this, base) = (*self, self.base());
        let mut body = ConfigureRequest::new(s.code, s.token);
        edit(&mut body);
        self.request(async move { post::<_, MatchView>(&base, "/configure", &body).await }, move |v| this.receive(v));
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

    /// The host seats a bot of `style` the server plays.
    pub fn add_bot(&self, style: Style) {
        let Some(s) = self.session.get_untracked() else {
            return;
        };
        let (this, base) = (*self, self.base());
        let body = AddBotRequest { code: s.code, token: s.token, style };
        self.request(async move { post::<_, MatchView>(&base, "/add-bot", &body).await }, move |v| this.receive(v));
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
        self.focus.set(None);
        self.trade_open.set(false);
        self.trade_give.set(Resources::ZERO);
        self.trade_want.set(Resources::ZERO);
        self.trade_to.set(None);
        self.counter.set(None);
        self.history_open.set(false);
        self.players_open.set(false);
        self.reactions_open.set(false);
        self.reactions_shown.set(Vec::new());
        self.last_reaction.replace(None);
        self.menu.set(Menu::Closed);
        self.item_card.set(None);
        self.confirm_attack.set(None);
        self.nudge.set(None);
        self.seen_nudges.replace(Vec::new());
        self.pending_nudges.replace(Vec::new());
        self.anims.set(Vec::new());
        self.fx.set(Vec::new());
        self.fx_cursor.replace(0.0);
        self.last_seq.replace(None);
        sound::hush();
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

    // ----- Sound -----

    pub fn set_effects(&self, on: bool) {
        self.effects_on.set(on);
        let _ = Storage::set_item(EFFECTS_KEY, if on { "on" } else { "off" });
        sound::set_effects(on);
        sound::play(Cue::Tap, 0.3, 1.0);
    }

    pub fn set_music(&self, on: bool) {
        self.music_on.set(on);
        let _ = Storage::set_item(MUSIC_KEY, if on { "on" } else { "off" });
    }

    pub fn set_tips(&self, on: bool) {
        self.tips_on.set(on);
        if !on {
            self.nudge.set(None);
            self.pending_nudges.replace(Vec::new());
        }
        let _ = Storage::set_item(TIPS_KEY, if on { "on" } else { "off" });
    }

    /// Starts or stops the music, in the app's scope so that it outlives
    /// what asked.
    pub fn play_music(&self, on: bool) {
        self.root.run(|| sound::music(on));
    }

    // ----- Actions -----

    /// Sends an action; the server validates it and the answer replaces the view.
    /// Ending the turn takes the board back to the whole island.
    pub fn send(&self, action: Action) {
        let Some(s) = self.session.get_untracked() else {
            return;
        };
        if action == Action::EndTurn {
            self.look_at(None);
        }
        let (this, base) = (*self, self.base());
        let body = ActionRequest { code: s.code, token: s.token, action };
        self.request(async move { post::<_, MatchView>(&base, "/action", &body).await }, move |v| this.receive(v));
    }

    /// Takes back the player's last action: the server answers with the view
    /// from before it.
    pub fn undo(&self) {
        let Some(s) = self.session.get_untracked() else {
            return;
        };
        self.mode.set(Mode::Idle);
        self.inspect.set(None);
        self.trade_open.set(false);
        let (this, base) = (*self, self.base());
        let body = SessionRequest { code: s.code, token: s.token };
        self.request(async move { post::<_, MatchView>(&base, "/undo", &body).await }, move |v| this.receive(v));
    }

    /// Points the board at `tile`, or at the whole island. The camera flies
    /// there even when it is already the focus, as the player may have
    /// panned away.
    pub fn look_at(&self, tile: Option<TileId>) {
        self.focus.update(|f| *f = tile);
    }

    /// Sends a reaction everyone sees. Neither held back by an action in
    /// flight nor holding one back.
    pub fn react(&self, reaction: Reaction) {
        let Some(s) = self.session.get_untracked() else {
            return;
        };
        self.reactions_open.set(false);
        let (this, base) = (*self, self.base());
        let body = ReactRequest { code: s.code, token: s.token, reaction };
        self.root.run(|| {
            spawn(async move {
                match post::<_, MatchView>(&base, "/react", &body).await {
                    Ok(v) => this.receive(v),
                    Err(e) => this.show_error(e),
                }
            })
        });
    }

    /// Shows reactions newer than the last seen beside their senders' chips
    /// for a moment; ones sent before this device arrived are not replayed.
    fn show_reactions(&self, reactions: &[SentReaction]) {
        let latest = reactions.last().map_or(0, |r| r.seq);
        let Some(seen) = self.last_reaction.replace(Some(latest)) else {
            return;
        };
        let fresh: Vec<SentReaction> = reactions.iter().filter(|r| r.seq > seen).copied().collect();
        if fresh.is_empty() {
            return;
        }
        self.reactions_shown.update(|shown| shown.extend(fresh.iter().copied()));
        let shown = self.reactions_shown;
        self.root.run(|| {
            set_timeout(Duration::from_millis(REACTION_SHOW_MS), move || {
                shown.update(|s| s.retain(|r| !fresh.iter().any(|f| f.seq == r.seq)))
            })
        });
    }

    /// Shows `nudge` over the dock until the next press.
    fn show_nudge(&self, nudge: Nudge) {
        self.seen_nudges.with(|s| s.push(nudge));
        self.nudge.set(Some(nudge));
    }

    /// Hides the showing tip, and shows the next one waiting, if any.
    pub fn hide_nudge(&self) {
        self.nudge.set(None);
        let next = self.pending_nudges.with(|p| (!p.is_empty()).then(|| p.remove(0)));
        if let Some(n) = next {
            self.show_nudge(n);
        }
    }

    /// Hides the showing tip when it is no longer on offer, and shows what
    /// just became so. Each shows once a turn, which starts afresh when the
    /// player's turn does. Tips about something that just happened wait for
    /// the showing one to go; the rest show only when nothing else does.
    fn follow_nudges(&self, before: Option<&Game>, after: &Game, seat: PlayerId, events: &[Event]) {
        if events.iter().any(|e| matches!(e, Event::TurnStarted { player, .. } if *player == seat)) {
            self.seen_nudges.replace(Vec::new());
            self.pending_nudges.replace(Vec::new());
        }
        if !self.tips_on.get_untracked() {
            return;
        }
        let on_offer = nudge::available(after, seat);
        self.pending_nudges.with(|p| p.retain(|n| on_offer.contains(n)));
        if self.nudge.get_untracked().is_some_and(|n| !on_offer.contains(&n)) {
            self.hide_nudge();
        }
        let Some(before) = before else {
            return;
        };
        let (seen, pending) = (self.seen_nudges.get(), self.pending_nudges.get());
        let showing = self.nudge.get_untracked();
        let fresh: Vec<Nudge> = nudge::fresh(before, after, seat, &seen)
            .into_iter()
            .filter(|n| !pending.contains(n) && showing != Some(*n))
            .collect();
        self.pending_nudges.with(|p| {
            p.extend(fresh.iter().copied().filter(|n| n.queues()));
            // `available` lists them by priority.
            p.sort_by_key(|n| on_offer.iter().position(|o| o == n));
        });
        if showing.is_some() {
            return;
        }
        let next = self.pending_nudges.with(|p| (!p.is_empty()).then(|| p.remove(0)));
        if let Some(n) = next.or_else(|| fresh.first().copied()) {
            self.show_nudge(n);
        }
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
            let previous = self.game();
            self.follow_trade(previous.as_ref(), game);
            let latest = game.event_seq;
            match self.last_seq.get() {
                // First sight of this match: show it as it is, animate nothing
                // old, but say whose turn it is while the island is set up.
                None if matches!(game.phase, Phase::Setup { .. }) => {
                    self.animate(None, game, view.seat, &view.players, &[])
                }
                None => {}
                Some(seen) => {
                    let fresh: Vec<Event> =
                        game.events.iter().filter(|e| e.seq > seen).map(|e| e.event.clone()).collect();
                    self.animate(previous.as_ref(), game, view.seat, &view.players, &fresh);
                    self.follow_trade_events(previous.as_ref(), game, view.seat, &fresh);
                    self.follow_nudges(previous.as_ref(), game, view.seat, &fresh);
                }
            }
            self.last_seq.replace(Some(latest));
            if !game.is_active(view.seat) {
                self.mode.set(Mode::Idle);
            }
        }
        let enchanted = view.game.as_ref().is_some_and(Game::spells_on_board);
        self.show_reactions(&view.reactions);
        self.view.set(Some(view));
        // Spells on the board shimmer for as long as they last.
        if enchanted {
            self.shimmer();
        }
    }

    /// Clears a counter draft when the offer it answers changes or closes.
    fn follow_trade(&self, before: Option<&Game>, after: &Game) {
        let same = match (before.and_then(|g| g.trade.as_ref()), after.trade.as_ref()) {
            (Some(a), Some(b)) => (a.from, a.to, a.give, a.want) == (b.from, b.to, b.give, b.want),
            _ => false,
        };
        if !same {
            self.counter.set(None);
        }
    }

    /// Follows the player's own offer: a declined one reopens the composer
    /// with the draft intact; a completed one clears the draft. Bots answer
    /// within the same view, so the offer may open and close in `events`.
    fn follow_trade_events(&self, before: Option<&Game>, after: &Game, seat: PlayerId, events: &[Event]) {
        let was_mine = before.and_then(|g| g.trade.as_ref()).is_some_and(|o| o.from == seat)
            || events.iter().any(|e| matches!(e, Event::TradeProposed { from, .. } if *from == seat));
        let traded = events.iter().any(|e| matches!(e, Event::Traded { from, .. } if *from == seat));
        if traded {
            self.trade_give.set(Resources::ZERO);
            self.trade_want.set(Resources::ZERO);
            self.trade_to.set(None);
        } else if was_mine
            && after.trade.is_none()
            && after.is_active(seat)
            && events.iter().any(|e| matches!(e, Event::TradeDeclined { .. }))
            && !events.iter().any(|e| matches!(e, Event::TradeCancelled { .. }))
        {
            self.trade_open.set(true);
        }
    }

    /// Schedules what `events` show, one beat after another: the dice, the
    /// tiles they hit and what they yield, each move on the board, and only
    /// once the board has settled, the next turn's banner.
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
        // The sounds that go with them, and when.
        let mut cues: Vec<(f64, (Cue, f32, f32))> = Vec::new();
        let turn_cue = |player: PlayerId| {
            if player == seat { (Cue::Horn, 0.25, 1.0) } else { (Cue::TurnOther, 0.5, 1.0) }
        };
        // Effects play one after another, after those already queued; when
        // the queue falls far behind the game, what is stale is dropped.
        let mut cursor = now.max(self.fx_cursor.get());
        if cursor - now > MAX_BACKLOG_MS {
            self.anims.update(|a| a.retain(|x| x.start <= now));
            self.fx.update(|f| f.retain(|x| x.start <= now));
            self.dice_until.replace(now);
            cursor = now;
        }
        // The player's own moves show at once. Anyone else's, and what the
        // round's end does (bandits wander, items come and go), play one
        // after another so each can be followed.
        let own = before.is_some_and(|b| b.current == seat)
            && after.current == seat
            && !events.iter().any(|e| matches!(e, Event::TurnStarted { .. }));
        // A turn's banner waits for the board to settle, so it hides none of it.
        let queued = self.anims.with_untracked(|a| a.iter().map(Anim::end).fold(0.0, f64::max));
        let calm = |anims: &[Anim]| anims.iter().map(Anim::end).fold(queued, f64::max) + SETTLE_MS;
        // When this batch's tiles light up, each one's place in the wave, and
        // how many badges each has had.
        let mut harvest = cursor;
        let mut wave: Vec<(TileId, f64)> = Vec::new();
        let lag = |wave: &[(TileId, f64)], tile: TileId| wave.iter().find(|w| w.0 == tile).map_or(0.0, |w| w.1);
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
                    cues.push((cursor, (Cue::Dice, 0.45, 1.0)));
                    let seed = now as u64 as u32;
                    let mut hit: Vec<(TileId, f32)> = after
                        .board
                        .tiles
                        .iter()
                        .filter(|t| t.number == sum)
                        .map(|t| (t.id, crate::art::hash01(seed.wrapping_add(t.id.0 as u32 * 7919))))
                        .collect();
                    hit.sort_by(|a, b| a.1.total_cmp(&b.1));
                    wave = hit
                        .iter()
                        .enumerate()
                        .map(|(i, &(tile, r))| (tile, i as f64 * WAVE_STEP_MS + r as f64 * WAVE_JITTER_MS))
                        .collect();
                    for t in after.board.tiles.iter().filter(|t| t.number == sum) {
                        let spell = before.and_then(|b| b.enchantment_at(t.id)).map(|e| e.spell);
                        let mood = match (after.robber_at(t.id).is_some() || after.blockaded(t.id), spell) {
                            (true, _) => Mood::Blocked,
                            (false, Some(s)) if s.stops() => Mood::Stopped(s),
                            (false, Some(s)) if s.doubles() => Mood::Doubled(s),
                            _ => Mood::Normal,
                        };
                        let kind = AnimKind::Jelly { tile: t.id, mood };
                        let start = harvest + lag(&wave, t.id);
                        anims.push(Anim { start, dur: board::JELLY_MS, kind });
                        // Every tile hit pops as its badges (or its "Blocked")
                        // show, each a step up a major pentatonic on the last.
                        const STEPS: [f32; 6] = [0.0, 2.0, 4.0, 7.0, 9.0, 12.0];
                        let place = wave.iter().position(|w| w.0 == t.id).unwrap_or(0).min(STEPS.len() - 1);
                        let rate = 2f32.powf(STEPS[place] / 12.0);
                        cues.push((start + YIELD_MS, (Cue::Harvest, 1.0, rate)));
                    }
                    badges.clear();
                    cursor = harvest + 300.0 + wave.last().map_or(0.0, |w| w.1);
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
                    let start = harvest + lag(&wave, tile) + YIELD_MS + slot as f64 * 120.0;
                    let mine = player == seat;
                    let color = palette::player(player);
                    let kind = AnimKind::Gain { tile, resource, amount, color, mine, slot, of: of as u32 };
                    anims.push(Anim { start, dur: board::GAIN_MS, kind });
                    if mine {
                        // The wave lands out of order: bounce after the last.
                        self.gains_at.replace(self.gains_at.get().max(start + 500.0));
                    }
                }
                Event::TurnStarted { player, round } => {
                    let b = banner(player, round, cursor.max(calm(&anims)));
                    cursor = b.end() - 300.0;
                    cues.push((b.start, turn_cue(player)));
                    effects.push(b);
                }
                Event::Won { .. } => {
                    effects.push(Fx { start: cursor, dur: fx::CONFETTI_MS, kind: FxKind::Confetti });
                    cues.push((cursor, (Cue::Fanfare, 0.8, 1.0)));
                }
                // What a roll's tiles did, shown with their wobble.
                Event::ProductionBlocked { tile }
                | Event::ProductionBlockaded { tile, .. }
                | Event::ProductionStopped { tile, .. }
                | Event::SpellFaded { tile, .. } => {
                    let (label, delay) = match *event {
                        Event::ProductionBlocked { .. } => ("Blocked", YIELD_MS),
                        Event::ProductionBlockaded { .. } => ("Blockaded!", YIELD_MS),
                        Event::ProductionStopped { .. } => ("Frozen!", YIELD_MS),
                        Event::SpellFaded { spell, .. } => (
                            match spell {
                                Spell::Frost => "The frost melts",
                                Spell::Bounty => "Bounty spent",
                                Spell::Luck => "Luck spent",
                                Spell::Ward => "The ward fades",
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
                    anims.push(Anim { start: harvest + lag(&wave, tile) + delay, dur: 1300.0, kind });
                }
                Event::WindfallPaid { player, gained } => {
                    let label = format!("{} windfall: +{}", if player == seat { "Your" } else { "A" }, gained.total());
                    let color = crate::art::spell_colors(Spell::Windfall).1;
                    let kind = AnimKind::Float { at: (0.0, 0.0), color, label };
                    anims.push(Anim { start: harvest + 400.0, dur: 1800.0, kind });
                }
                Event::RingPaid { player, tile, resource, amount }
                | Event::StructurePaid { player, tile, resource, amount, .. } => {
                    let color = palette::player(player);
                    let mine = player == seat;
                    let kind = AnimKind::Gain { tile, resource, amount, color, mine, slot: 0, of: 1 };
                    anims.push(Anim { start: cursor, dur: board::GAIN_MS, kind });
                    if mine {
                        cues.push((cursor, (Cue::Harvest, 0.5, 1.1)));
                    }
                }
                _ => {
                    let shown = board::anims_for(after, event, cursor);
                    if let Some(cue) = sound::cue_for(event) {
                        cues.push((shown.first().map_or(cursor, |a| a.start), cue));
                    }
                    if !own && let Some(lead) = shown.first() {
                        cursor += lead.dur.min(MOVE_STEP_MS);
                    }
                    anims.extend(shown);
                }
            }
        }
        // Setup turns pass without a TurnStarted.
        if matches!(after.phase, Phase::Setup { .. }) && before.is_none_or(|b| b.current != after.current) {
            let b = banner(after.current, 0, (cursor + 250.0).max(calm(&anims)));
            cursor = b.end() - 300.0;
            cues.push((b.start, turn_cue(after.current)));
            effects.push(b);
        }
        self.fx_cursor.replace(cursor);
        let now = now_ms();
        self.root.run(|| {
            for (start, (cue, volume, rate)) in cues {
                sound::play_in(start - now, cue, volume, rate);
            }
        });
        if anims.is_empty() && effects.is_empty() {
            return;
        }
        self.anims.update(|a| a.extend(anims));
        self.fx.update(|f| f.extend(effects));
        self.run_clock();
    }

    /// Ticks `clock` every frame while anything animates.
    fn run_clock(&self) {
        self.tick_every(FRAME_MS);
    }

    /// Ticks `clock` slowly for spells on the board, unless it already ticks.
    fn shimmer(&self) {
        if self.ticker.with(|t| t.is_none()) {
            self.tick_every(SHIMMER_MS);
        }
    }

    /// Ticks on display frames, at most once every `ms`, unless the clock
    /// already ticks at least as often. In step with the screen, so motion
    /// does not judder as a free-running timer drifts against it.
    fn tick_every(&self, ms: u64) {
        if self.ticker.with(|t| t.as_ref().is_some_and(|(period, _)| *period <= ms)) {
            return;
        }
        self.stop_clock();
        let this = *self;
        let task = self.root.run(|| {
            spawn(async move {
                let mut last = f64::MIN;
                loop {
                    next_frame().await;
                    let now = now_ms();
                    // A frame's slack, so a 60 Hz screen meets a 16 ms period.
                    if now - last + 2.0 >= ms as f64 {
                        last = now;
                        this.tick(ms);
                    }
                }
            })
        });
        self.ticker.replace(Some((ms, task)));
    }

    fn stop_clock(&self) {
        if let Some((_, task)) = self.ticker.replace(None) {
            task.cancel();
        }
    }

    /// Starts the clock again, back in the foreground, if anything moves.
    pub fn resume_clock(&self) {
        let now = now_ms();
        let pending = self.anims.with_untracked(|a| !a.is_empty())
            || self.fx.with_untracked(|f| !f.is_empty())
            || self.dice_until.get() > now;
        if pending {
            self.run_clock();
        } else if self
            .view
            .with_untracked(|v| v.as_ref().and_then(|v| v.game.as_ref()).is_some_and(Game::spells_on_board))
        {
            self.shimmer();
        }
    }

    fn tick(&self, period: u64) {
        // Nothing is seen in the background: rest until the app is back.
        if lifecycle() == Lifecycle::Background {
            self.stop_clock();
            return;
        }
        let now = now_ms();
        let dice_until = self.dice_until.get();
        if dice_until <= now && self.dice_hidden.get_untracked() {
            self.dice_hidden.set(false);
        }
        let busy = self.anims.with_untracked(|a| a.iter().any(|x| x.end() > now))
            || self.fx.with_untracked(|f| f.iter().any(|x| x.end() > now))
            || dice_until > now;
        self.clock.set(now);
        if busy {
            return;
        }
        if self.anims.with_untracked(|a| !a.is_empty()) {
            self.anims.set(Vec::new());
        }
        if self.fx.with_untracked(|f| !f.is_empty()) {
            self.fx.set(Vec::new());
        }
        let ambient =
            self.view.with_untracked(|v| v.as_ref().and_then(|v| v.game.as_ref()).is_some_and(Game::spells_on_board));
        // Spells alone shimmer at a few frames a second.
        if !ambient {
            self.stop_clock();
        } else if period != SHIMMER_MS {
            self.stop_clock();
            self.tick_every(SHIMMER_MS);
        }
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
