//! Matches the server holds: lobbies, seats, games and their randomness.
//! Every client request comes through [`Store`], which checks the seat's
//! token and lets the engine validate the action.

pub mod practice;

use std::collections::BTreeMap;

use kings_cove_engine::{
    Action, Event, Game, Phase, PlayerId, Rng, balance,
    balance::{Amount, MapShape, MapSize, Settings},
    bot::{self, Style},
};
use kings_cove_protocol::{ConfigureRequest, MatchView, RECENT_REACTIONS, Reaction, SentReaction, Session};
use serde::{Deserialize, Serialize};

const CODE_LETTERS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
const ENDED: &str = "The host ended this match";
const CODE_LEN: usize = 5;
const MAX_NAME: usize = 16;
/// How long a bot waits on a person to answer its trade offer before it
/// withdraws it and plays on.
pub const BOT_OFFER_MS: u64 = 30_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Seat {
    pub name: String,
    pub token: String,
    /// The server plays this seat: a bot the host added, or a practice
    /// match's rival.
    #[serde(default)]
    pub bot: bool,
    /// A practice rival: it only rolls and passes.
    #[serde(default)]
    pub passive: bool,
    /// How a bot plays.
    #[serde(default)]
    pub style: Style,
}

impl Seat {
    fn person(name: String, token: String) -> Seat {
        Seat { name, token, bot: false, passive: false, style: Style::default() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Match {
    pub code: String,
    pub seats: Vec<Seat>,
    pub game: Option<Game>,
    /// The match's own dice, robbers and board: never sent to clients.
    pub rng: Rng,
    pub version: u64,
    /// The host ended it: nothing more happens in it.
    #[serde(default)]
    pub ended: bool,
    /// The goal its game is played to.
    #[serde(default = "classic_goal")]
    pub victory_points: u32,
    /// The size of the island its game is played on.
    #[serde(default)]
    pub map_size: MapSize,
    #[serde(default)]
    pub map_shape: MapShape,
    #[serde(default)]
    pub shrines: Amount,
    #[serde(default)]
    pub mines: Amount,
    #[serde(default)]
    pub mills: Amount,
    /// Its settings are drawn as it starts, to suit its players.
    #[serde(default)]
    pub random: bool,
    /// The latest reactions, oldest first: not saved.
    #[serde(skip)]
    reactions: Vec<SentReaction>,
    /// The `seq` of the latest reaction; 0 before the first. Saved, unlike
    /// the reactions, so a restarted server keeps counting up and clients
    /// still tell the next reaction from ones they saw.
    #[serde(default)]
    reaction_seq: u64,
    /// The bot trade offer waiting on a person: the `seq` of its proposal
    /// and when the server first saw it, in Unix milliseconds. Not saved: a
    /// restarted server gives the offer its full time again.
    #[serde(skip)]
    bot_offer: Option<(u64, u64)>,
    /// The game and dice as they were before the last action, while its
    /// seat may still take it back. Not saved: a restart forgets it.
    #[serde(skip)]
    undo: Option<Undo>,
}

/// What `POST /undo` puts back.
#[derive(Clone, Debug)]
struct Undo {
    seat: PlayerId,
    game: Game,
    rng: Rng,
}

/// The goal of matches saved before goals could be chosen.
fn classic_goal() -> u32 {
    10
}

impl Match {
    fn seat_of(&self, token: &str) -> Result<PlayerId, String> {
        self.seats
            .iter()
            .position(|s| s.token == token)
            .map(|i| PlayerId(i as u8))
            .ok_or_else(|| "You are not seated in this match".to_owned())
    }

    pub fn view(&self, seat: PlayerId) -> MatchView {
        MatchView {
            code: self.code.clone(),
            version: self.version,
            seat,
            players: self.seats.iter().map(|s| s.name.clone()).collect(),
            // What an item is stays secret until a knight finds it.
            game: self.game.clone().map(|mut g| {
                g.hide_items();
                g
            }),
            ended: self.ended,
            victory_points: self.victory_points,
            map_size: self.map_size,
            map_shape: self.map_shape,
            shrines: self.shrines,
            mines: self.mines,
            mills: self.mills,
            random: self.random,
            reactions: self.reactions.clone(),
            bots: self.seats.iter().map(|s| (s.bot && !s.passive).then_some(s.style)).collect(),
            can_undo: self.undo.as_ref().is_some_and(|u| u.seat == seat),
        }
    }

    /// The seats the server plays answer the open trade offer, then take
    /// their turns, until it is a person's turn, the bot whose turn it is
    /// waits on a person's answer to its offer, or the game is over.
    fn play_bots(&mut self) {
        let Match { seats, game, rng, .. } = self;
        let Some(game) = game.as_mut() else {
            return;
        };
        loop {
            for (i, seat) in seats.iter().enumerate().filter(|(_, s)| s.bot) {
                let p = PlayerId(i as u8);
                if seat.passive {
                    let _ = game.apply(p, Action::DeclineTrade, rng);
                } else {
                    bot::answer_trade(game, p, seat.style, rng);
                }
            }
            let p = game.current;
            let Some(seat) = seats.get(p.index()).filter(|s| s.bot) else {
                break;
            };
            // Its own offer, until every person it was made to answers it.
            let players = seats.len();
            let awaits_person = game.trade.as_ref().is_some_and(|t| {
                t.from == p && t.recipients(players).any(|r| t.answer(r).is_none() && !seats[r.index()].bot)
            });
            if game.winner().is_some() || awaits_person {
                break;
            }
            let played = if seat.passive {
                practice::pass_turn(game, p, rng)
            } else {
                bot::play_turn(game, p, seat.style, rng).map(|_| ())
            };
            if played.is_err() {
                break;
            }
        }
    }

    /// The open trade offer of a bot, and the `seq` of its proposal.
    fn open_bot_offer(&self) -> Option<(PlayerId, u64)> {
        let game = self.game.as_ref().filter(|_| !self.ended)?;
        let from = game.trade.as_ref()?.from;
        self.seats.get(from.index()).filter(|s| s.bot)?;
        let proposed = game.events.iter().rev().find(|e| matches!(e.event, Event::TradeProposed { .. }))?;
        Some((from, proposed.seq))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Store {
    pub matches: BTreeMap<String, Match>,
    rng: Rng,
}

impl Store {
    pub fn new(seed: u64) -> Self {
        Store { matches: BTreeMap::new(), rng: Rng::new(seed) }
    }

    fn token(&mut self) -> String {
        format!("{:016x}{:016x}", self.rng.next_u64(), self.rng.next_u64())
    }

    fn new_code(&mut self) -> String {
        loop {
            let code: String =
                (0..CODE_LEN).map(|_| CODE_LETTERS[self.rng.below(CODE_LETTERS.len())] as char).collect();
            if !self.matches.contains_key(&code) {
                return code;
            }
        }
    }

    pub fn create(&mut self, name: &str) -> Result<Session, String> {
        let name = clean_name(name)?;
        let token = self.token();
        let code = self.open(vec![Seat::person(name, token.clone())], None);
        Ok(Session { code, token, seat: PlayerId(0) })
    }

    /// A started match against [`practice::RIVAL`], whom the server plays,
    /// dealt into the middle of a game: for trying the client alone.
    pub fn practice(&mut self, name: &str) -> Result<Session, String> {
        let name = clean_name(name)?;
        let token = self.token();
        let mut rng = Rng::new(self.rng.next_u64());
        let game = practice::game(&name, &mut rng);
        let rival = Seat {
            name: practice::RIVAL.to_owned(),
            token: self.token(),
            bot: true,
            passive: true,
            style: Style::default(),
        };
        let me = Seat::person(name, token.clone());
        let code = self.open(vec![me, rival], Some((game, rng)));
        Ok(Session { code, token, seat: PlayerId(0) })
    }

    /// A new match of `seats` under a fresh code, its game already dealt
    /// with the randomness it goes on with, or still in the lobby.
    fn open(&mut self, seats: Vec<Seat>, started: Option<(Game, Rng)>) -> String {
        let code = self.new_code();
        let (game, rng) = match started {
            Some((game, rng)) => (Some(game), rng),
            None => (None, Rng::new(self.rng.next_u64())),
        };
        self.matches.insert(
            code.clone(),
            Match {
                code: code.clone(),
                seats,
                game,
                rng,
                version: 1,
                ended: false,
                victory_points: balance::DEFAULT_VICTORY_POINTS,
                map_size: MapSize::default(),
                map_shape: MapShape::default(),
                shrines: Amount::Off,
                mines: Amount::Off,
                mills: Amount::Off,
                random: false,
                reactions: Vec::new(),
                reaction_seq: 0,
                bot_offer: None,
                undo: None,
            },
        );
        code
    }

    pub fn join(&mut self, code: &str, name: &str) -> Result<Session, String> {
        let name = clean_name(name)?;
        let code = code.trim().to_ascii_uppercase();
        let token = self.token();
        let m = self.matches.get_mut(&code).ok_or("No match with that code")?;
        if m.ended {
            return Err(ENDED.to_owned());
        }
        if m.game.is_some() {
            return Err("That match has already started".to_owned());
        }
        if m.seats.len() >= balance::MAX_PLAYERS {
            return Err("That match is full".to_owned());
        }
        if m.seats.iter().any(|s| s.name.eq_ignore_ascii_case(&name)) {
            return Err("That name is taken in this match".to_owned());
        }
        m.seats.push(Seat::person(name, token.clone()));
        m.version += 1;
        Ok(Session { code, token, seat: PlayerId(m.seats.len() as u8 - 1) })
    }

    /// The host seats a bot the server plays, of `style`, before the match
    /// starts. Bots are named by their style and numbered (`Bot 1`,
    /// `Friendly Bot 2`, …), by the first free name.
    pub fn add_bot(&mut self, code: &str, token: &str, style: Style) -> Result<MatchView, String> {
        let bot_token = self.token();
        let m = self.matches.get_mut(code).ok_or("No match with that code")?;
        let seat = m.seat_of(token)?;
        if seat != PlayerId(0) {
            return Err("Only the host can add a bot".to_owned());
        }
        if m.ended {
            return Err(ENDED.to_owned());
        }
        if m.game.is_some() {
            return Err("The match has already started".to_owned());
        }
        if m.seats.len() >= balance::MAX_PLAYERS {
            return Err("The match is full".to_owned());
        }
        let kind = match style {
            Style::Friendly => "Friendly Bot",
            Style::Balanced => "Bot",
            Style::Ruthless => "Ruthless Bot",
        };
        let name = (1..)
            .map(|i| format!("{kind} {i}"))
            .find(|n| !m.seats.iter().any(|s| s.name.eq_ignore_ascii_case(n)))
            .expect("a free name among more names than seats");
        m.seats.push(Seat { name, token: bot_token, bot: true, passive: false, style });
        m.version += 1;
        Ok(m.view(seat))
    }

    pub fn start(&mut self, code: &str, token: &str) -> Result<MatchView, String> {
        let m = self.matches.get_mut(code).ok_or("No match with that code")?;
        let seat = m.seat_of(token)?;
        if seat != PlayerId(0) {
            return Err("Only the host can start the match".to_owned());
        }
        if m.ended {
            return Err(ENDED.to_owned());
        }
        if m.game.is_some() {
            return Err("The match has already started".to_owned());
        }
        if m.seats.len() < balance::MIN_PLAYERS {
            return Err(format!("At least {} players are needed", balance::MIN_PLAYERS));
        }
        let names: Vec<String> = m.seats.iter().map(|s| s.name.clone()).collect();
        let settings = if m.random {
            Settings::random(names.len(), &mut m.rng)
        } else {
            Settings {
                victory_points: m.victory_points,
                map_size: m.map_size,
                map_shape: m.map_shape,
                shrines: m.shrines,
                mines: m.mines,
                mills: m.mills,
            }
        };
        let Settings { victory_points, map_size, map_shape, shrines, mines, mills } = settings;
        (m.victory_points, m.map_size, m.map_shape, m.shrines, m.mines, m.mills) =
            (victory_points, map_size, map_shape, shrines, mines, mills);
        m.game = Some(Game::new(&names, &settings, &mut m.rng));
        m.undo = None;
        m.version += 1;
        Ok(m.view(seat))
    }

    pub fn act(&mut self, code: &str, token: &str, action: Action) -> Result<MatchView, String> {
        let m = self.matches.get_mut(code).ok_or("No match with that code")?;
        let seat = m.seat_of(token)?;
        if m.ended {
            return Err(ENDED.to_owned());
        }
        let game = m.game.as_mut().ok_or("The match has not started")?;
        let (before, before_rng, before_seq) = (game.clone(), m.rng.clone(), game.event_seq);
        let taken = action.clone();
        game.apply(seat, action, &mut m.rng).map_err(|e| e.to_string())?;
        let after_apply = game.event_seq;
        let events: Vec<Event> = game.events.iter().filter(|e| e.seq > before_seq).map(|e| e.event.clone()).collect();
        m.play_bots();
        let game = m.game.as_ref().expect("a game was just played");
        let quiet = game.event_seq == after_apply;
        let mine = matches!(game.phase, Phase::Playing) && game.current == seat;
        m.undo = (taken.undoable(&events) && quiet && mine).then_some(Undo { seat, game: before, rng: before_rng });
        m.version += 1;
        Ok(m.view(seat))
    }

    /// A seat takes back its own last action, while nothing else has happened.
    /// The event counter keeps climbing so clients animate what follows.
    pub fn undo(&mut self, code: &str, token: &str) -> Result<MatchView, String> {
        let m = self.matches.get_mut(code).ok_or("No match with that code")?;
        let seat = m.seat_of(token)?;
        if m.ended {
            return Err(ENDED.to_owned());
        }
        let undo = match m.undo.take() {
            Some(u) if u.seat == seat => u,
            other => {
                m.undo = other;
                return Err("Nothing to undo".to_owned());
            }
        };
        let seq = m.game.as_ref().map_or(0, |g| g.event_seq);
        let Undo { game, rng, .. } = undo;
        m.game = Some(Game { event_seq: seq, ..game });
        m.rng = rng;
        m.version += 1;
        Ok(m.view(seat))
    }

    /// The host changes the settings before the game starts; absent fields stay.
    pub fn configure(&mut self, change: &ConfigureRequest) -> Result<MatchView, String> {
        let m = self.matches.get_mut(&change.code).ok_or("No match with that code")?;
        let seat = m.seat_of(&change.token)?;
        if seat != PlayerId(0) {
            return Err("Only the host can change the settings".to_owned());
        }
        if m.ended {
            return Err(ENDED.to_owned());
        }
        if m.game.is_some() {
            return Err("The match has already started".to_owned());
        }
        if let Some(goal) = change.victory_points {
            if !balance::VICTORY_POINT_GOALS.contains(&goal) {
                return Err(format!("Play to one of {:?} victory points", balance::VICTORY_POINT_GOALS));
            }
            m.victory_points = goal;
        }
        if let Some(size) = change.map_size {
            m.map_size = size;
        }
        if let Some(shape) = change.map_shape {
            m.map_shape = shape;
        }
        if let Some(shrines) = change.shrines {
            m.shrines = shrines;
        }
        if let Some(mines) = change.mines {
            m.mines = mines;
        }
        if let Some(mills) = change.mills {
            m.mills = mills;
        }
        if let Some(random) = change.random {
            m.random = random;
        }
        m.undo = None;
        m.version += 1;
        Ok(m.view(seat))
    }

    /// A seat sends a quick reaction, lobby or game.
    pub fn react(&mut self, code: &str, token: &str, reaction: Reaction) -> Result<MatchView, String> {
        let m = self.matches.get_mut(code).ok_or("No match with that code")?;
        let seat = m.seat_of(token)?;
        if m.ended {
            return Err(ENDED.to_owned());
        }
        m.reaction_seq += 1;
        m.reactions.push(SentReaction { seq: m.reaction_seq, seat, reaction });
        if m.reactions.len() > RECENT_REACTIONS {
            m.reactions.remove(0);
        }
        m.version += 1;
        Ok(m.view(seat))
    }

    /// The host ends the match for everyone, in the lobby or mid-game.
    pub fn end(&mut self, code: &str, token: &str) -> Result<MatchView, String> {
        let m = self.matches.get_mut(code).ok_or("No match with that code")?;
        let seat = m.seat_of(token)?;
        if seat != PlayerId(0) {
            return Err("Only the host can end the match".to_owned());
        }
        if !m.ended {
            m.ended = true;
            m.undo = None;
            m.version += 1;
        }
        Ok(m.view(seat))
    }

    /// Withdraws bot trade offers left unanswered for [`BOT_OFFER_MS`],
    /// and lets those bots play on. `now_ms` is Unix time in milliseconds;
    /// true when a match changed.
    pub fn expire_bot_offers(&mut self, now_ms: u64) -> bool {
        let mut changed = false;
        for m in self.matches.values_mut() {
            let Some((from, seq)) = m.open_bot_offer() else {
                m.bot_offer = None;
                continue;
            };
            match m.bot_offer {
                Some((seen, since)) if seen == seq => {
                    if now_ms.saturating_sub(since) < BOT_OFFER_MS {
                        continue;
                    }
                    let game = m.game.as_mut().expect("an open offer is in a game");
                    let _ = game.apply(from, Action::CancelTrade, &mut m.rng);
                    m.bot_offer = None;
                    m.undo = None;
                    m.play_bots();
                    m.version += 1;
                    changed = true;
                }
                _ => m.bot_offer = Some((seq, now_ms)),
            }
        }
        changed
    }

    pub fn view(&self, code: &str, token: &str) -> Result<MatchView, String> {
        let m = self.matches.get(code).ok_or("No match with that code")?;
        Ok(m.view(m.seat_of(token)?))
    }

    pub fn version(&self, code: &str) -> Option<u64> {
        self.matches.get(code).map(|m| m.version)
    }
}

fn clean_name(name: &str) -> Result<String, String> {
    let name: String = name.trim().chars().filter(|c| !c.is_control()).take(MAX_NAME).collect();
    if name.is_empty() { Err("Enter a name".to_owned()) } else { Ok(name) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kings_cove_engine::{Phase, SetupPlacing, VertexId};

    #[test]
    fn saved_island_sizes_keep_their_tiles_across_the_rename() {
        let read = |tag: &str| serde_json::from_str::<MapSize>(&format!("\"{tag}\"")).unwrap();
        let old: Vec<usize> = ["small", "medium", "large"].map(|t| read(t).tiles()).into();
        assert_eq!(old, [19, 37, 61], "names saved before the rename");
        for size in MapSize::ALL {
            assert_eq!(read(serde_json::to_string(&size).unwrap().trim_matches('"')), size);
        }
    }

    #[test]
    fn create_join_start_and_act_with_seat_tokens() {
        let mut store = Store::new(7);
        let host = store.create("Ada").unwrap();
        let lobby = store.view(&host.code, &host.token).unwrap();
        assert_eq!((lobby.victory_points, lobby.map_size), (balance::DEFAULT_VICTORY_POINTS, MapSize::default()));
        let guest = store.join(&host.code.to_lowercase(), "Bo").unwrap();
        assert_eq!(guest.seat, PlayerId(1));
        assert!(store.start(&host.code, &guest.token).is_err(), "only the host starts");
        assert!(store.act(&host.code, &host.token, Action::Roll).is_err(), "no game before start");
        let view = store.start(&host.code, &host.token).unwrap();
        let game = view.game.unwrap();
        assert!(matches!(game.phase, Phase::Setup { .. }));
        assert_eq!(game.victory_points, balance::DEFAULT_VICTORY_POINTS);
        assert_eq!(game.board.tiles.len(), MapSize::default().tiles());
        assert!(store.join(&host.code, "Cy").is_err(), "no joining a started match");

        // The guest may not act on the host's turn; a forged token is refused.
        assert!(store.act(&host.code, &guest.token, Action::SetupBuilding(VertexId(0))).is_err());
        assert!(store.act(&host.code, "forged", Action::SetupBuilding(VertexId(0))).is_err());
        let before = store.version(&host.code).unwrap();
        store.act(&host.code, &host.token, Action::SetupBuilding(VertexId(0))).unwrap();
        assert_eq!(store.version(&host.code).unwrap(), before + 1);

        // Reconnecting with the token gives the same seat and state.
        let resumed = store.view(&host.code, &guest.token).unwrap();
        assert_eq!(resumed.seat, PlayerId(1));
        assert_eq!(resumed.players, vec!["Ada".to_owned(), "Bo".to_owned()]);
    }

    #[test]
    fn only_the_host_ends_a_match_and_it_stays_ended() {
        let mut store = Store::new(3);
        let host = store.create("Ada").unwrap();
        let guest = store.join(&host.code, "Bo").unwrap();
        store.start(&host.code, &host.token).unwrap();
        assert!(store.end(&host.code, &guest.token).is_err(), "only the host ends it");
        let before = store.version(&host.code).unwrap();
        assert!(store.end(&host.code, &host.token).unwrap().ended);
        assert_eq!(store.version(&host.code).unwrap(), before + 1, "waiting polls wake");
        assert!(store.view(&host.code, &guest.token).unwrap().ended);
        assert!(store.act(&host.code, &host.token, Action::SetupBuilding(VertexId(0))).is_err());
        assert!(store.join(&host.code, "Cy").is_err());
    }

    /// A configure request of `token`'s seat in `code` changing what `edit` sets.
    fn change(code: &str, token: &str, edit: impl FnOnce(&mut ConfigureRequest)) -> ConfigureRequest {
        let mut request = ConfigureRequest::new(code.to_owned(), token.to_owned());
        edit(&mut request);
        request
    }

    #[test]
    fn only_the_host_configures_an_unstarted_match() {
        let mut store = Store::new(5);
        let host = store.create("Ada").unwrap();
        let guest = store.join(&host.code, "Bo").unwrap();
        let goal = |token: &str, goal: u32| change(&host.code, token, |c| c.victory_points = Some(goal));
        assert!(store.configure(&goal(&guest.token, 30)).is_err(), "only the host");
        assert!(store.configure(&goal(&host.token, 12)).is_err(), "only the offered goals");
        let before = store.version(&host.code).unwrap();
        let view = store.configure(&goal(&host.token, 30)).unwrap();
        assert_eq!((view.victory_points, view.map_size), (30, MapSize::default()), "absent fields stay");
        assert_eq!(store.version(&host.code).unwrap(), before + 1, "waiting polls wake");
        let view = store
            .configure(&change(&host.code, &host.token, |c| {
                c.map_size = Some(MapSize::Large);
                c.map_shape = Some(MapShape::Wild);
                c.shrines = Some(Amount::Many);
                c.mines = Some(Amount::Few);
            }))
            .unwrap();
        assert_eq!((view.victory_points, view.map_size), (30, MapSize::Large));
        let seen = store.view(&host.code, &guest.token).unwrap();
        assert_eq!(
            (seen.victory_points, seen.map_size, seen.map_shape, seen.shrines, seen.mines, seen.mills, seen.random),
            (30, MapSize::Large, MapShape::Wild, Amount::Many, Amount::Few, Amount::Off, false)
        );
        let started = store.start(&host.code, &host.token).unwrap().game.unwrap();
        assert_eq!((started.victory_points, started.board.tiles.len()), (30, MapSize::Large.tiles()));
        let tiles = MapSize::Large.tiles();
        assert_eq!(started.structures.len(), Amount::Many.count(tiles) + Amount::Few.count(tiles));
        assert!(store.configure(&goal(&host.token, 10)).is_err(), "not after the start");

        let other = store.create("Cy").unwrap();
        store.end(&other.code, &other.token).unwrap();
        let late = change(&other.code, &other.token, |c| c.victory_points = Some(10));
        assert!(store.configure(&late).is_err(), "not once ended");
    }

    #[test]
    fn random_settings_are_drawn_at_the_start_to_suit_the_players() {
        for players in balance::MIN_PLAYERS..=balance::MAX_PLAYERS {
            for seed in 0..12 {
                let mut store = Store::new(seed);
                let host = store.create("Ada").unwrap();
                for _ in 1..players {
                    store.add_bot(&host.code, &host.token, Style::Balanced).unwrap();
                }
                store.configure(&change(&host.code, &host.token, |c| c.random = Some(true))).unwrap();
                let view = store.start(&host.code, &host.token).unwrap();
                let game = view.game.as_ref().unwrap();
                assert!(view.random);
                assert!(MapSize::for_players(players).contains(&view.map_size), "{players} players");
                assert!(view.map_size.fitting_goals().contains(&view.victory_points));
                assert_eq!((game.victory_points, game.board.tiles.len()), (view.victory_points, view.map_size.tiles()));
                let tiles = game.board.tiles.len();
                let structures = view.shrines.count(tiles) + view.mines.count(tiles) + view.mills.count(tiles);
                assert_eq!(game.structures.len(), structures);
            }
        }
    }

    #[test]
    fn reactions_are_numbered_and_trimmed() {
        let mut store = Store::new(9);
        let host = store.create("Ada").unwrap();
        let guest = store.join(&host.code, "Bo").unwrap();
        let first = store.react(&host.code, &host.token, Reaction::ALL[0]).unwrap();
        assert_eq!(first.reactions, vec![SentReaction { seq: 1, seat: PlayerId(0), reaction: Reaction::ALL[0] }]);
        assert!(store.react(&host.code, "forged", Reaction::ALL[1]).is_err());
        let before = store.version(&host.code).unwrap();
        let seen = store.react(&host.code, &guest.token, Reaction::ALL[1]).unwrap();
        assert_eq!(store.version(&host.code).unwrap(), before + 1, "waiting polls wake");
        assert_eq!(
            seen.reactions.iter().map(|r| (r.seq, r.seat)).collect::<Vec<_>>(),
            [(1, PlayerId(0)), (2, PlayerId(1))]
        );
        assert_eq!(store.view(&host.code, &host.token).unwrap().reactions, seen.reactions);
        // A seat may react again at once; only the latest are kept.
        for _ in 0..RECENT_REACTIONS + 2 {
            store.react(&host.code, &host.token, Reaction::ALL[2]).unwrap();
        }
        let kept = store.view(&host.code, &guest.token).unwrap().reactions;
        assert_eq!(kept.len(), RECENT_REACTIONS);
        assert_eq!(kept.last().unwrap().seq, RECENT_REACTIONS as u64 + 4);
        assert!(kept.windows(2).all(|w| w[1].seq == w[0].seq + 1), "oldest first");
        store.end(&host.code, &host.token).unwrap();
        assert!(store.react(&host.code, &guest.token, Reaction::ALL[0]).is_err(), "not once ended");
    }

    #[test]
    fn store_round_trips_through_json() {
        let mut store = Store::new(1);
        let host = store.create("Ada").unwrap();
        store
            .configure(&change(&host.code, &host.token, |c| {
                c.victory_points = Some(30);
                c.map_size = Some(MapSize::Small);
                c.map_shape = Some(MapShape::Wild);
                c.shrines = Some(Amount::Few);
                c.mines = Some(Amount::Many);
                c.mills = Some(Amount::Few);
            }))
            .unwrap();
        store.join(&host.code, "Bo").unwrap();
        store.start(&host.code, &host.token).unwrap();
        let saved = serde_json::to_string(&store).unwrap();
        let restored: Store = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored.view(&host.code, &host.token).unwrap(), store.view(&host.code, &host.token).unwrap());
    }

    #[test]
    fn the_host_adds_bots_of_any_style_that_play_their_own_setup() {
        let mut store = Store::new(11);
        let host = store.create("Ada").unwrap();
        let guest = store.join(&host.code, "Bot 1").unwrap();
        assert!(store.add_bot(&host.code, &guest.token, Style::Balanced).is_err(), "only the host");
        let before = store.version(&host.code).unwrap();
        let view = store.add_bot(&host.code, &host.token, Style::Balanced).unwrap();
        assert_eq!(view.players, ["Ada", "Bot 1", "Bot 2"], "the first free name");
        assert_eq!(store.version(&host.code).unwrap(), before + 1, "waiting polls wake");
        let view = store.add_bot(&host.code, &host.token, Style::Ruthless).unwrap();
        assert_eq!(view.players[3], "Ruthless Bot 1");
        assert!(store.add_bot(&host.code, &host.token, Style::Friendly).is_err(), "no more than four seats");

        let other = store.create("Cy").unwrap();
        store.add_bot(&other.code, &other.token, Style::Friendly).unwrap();
        let setup = store.start(&other.code, &other.token).unwrap().game.unwrap();
        let v = setup.setup_vertices(PlayerId(0))[0];
        store.act(&other.code, &other.token, Action::SetupBuilding(v)).unwrap();
        let e = setup.board.vertex(v).edges[0];
        let after = store.act(&other.code, &other.token, Action::SetupRoad(e)).unwrap().game.unwrap();
        // Snake order 0, 1, 1, 0: the bot placed both its buildings at once.
        assert_eq!(after.phase, Phase::Setup { step: 3, placing: SetupPlacing::Building });
        assert_eq!(after.current, PlayerId(0));
        assert_eq!(after.buildings.iter().flatten().filter(|b| b.owner == PlayerId(1)).count(), 2);
        assert!(store.add_bot(&other.code, &other.token, Style::Balanced).is_err(), "not after the start");
    }

    /// A match of a person and a bot, set up, on the bot's turn, rolled:
    /// the bot has only wood, short of everything it would build, and has
    /// just offered the person some for what it lacks.
    fn bot_offers_the_host(seed: u64) -> (Store, Session) {
        let mut store = Store::new(seed);
        let host = store.create("Ada").unwrap();
        store.add_bot(&host.code, &host.token, Style::Balanced).unwrap();
        let mut game = store.start(&host.code, &host.token).unwrap().game.unwrap();
        while matches!(game.phase, Phase::Setup { .. }) {
            let v = game.setup_vertices(PlayerId(0))[0];
            game = store.act(&host.code, &host.token, Action::SetupBuilding(v)).unwrap().game.unwrap();
            let e = game.setup_edges(PlayerId(0))[0];
            game = store.act(&host.code, &host.token, Action::SetupRoad(e)).unwrap().game.unwrap();
        }
        let m = store.matches.get_mut(&host.code).unwrap();
        let g = m.game.as_mut().unwrap();
        g.current = PlayerId(1);
        g.dice = Some((1, 2));
        g.players[0].resources = balance::res(5, 5, 5, 5);
        g.players[1].resources = balance::res(3, 0, 0, 0);
        m.play_bots();
        let offer = m.game.as_ref().unwrap().trade.clone().expect("the bot offers a trade");
        assert_eq!((offer.from, offer.to), (PlayerId(1), Some(PlayerId(0))));
        (store, host)
    }

    #[test]
    fn a_bot_waits_on_its_offer_and_plays_on_once_it_is_answered() {
        let (mut store, host) = bot_offers_the_host(4);
        assert_eq!(store.view(&host.code, &host.token).unwrap().game.unwrap().current, PlayerId(1), "it waits");
        let after = store.act(&host.code, &host.token, Action::AcceptTrade).unwrap().game.unwrap();
        assert!(after.events.iter().any(|e| matches!(e.event, Event::Traded { from: PlayerId(1), .. })), "traded");
        assert_eq!((after.current, after.trade), (PlayerId(0), None), "the bot finished its turn");
    }

    #[test]
    fn a_bot_takes_a_counter_to_its_offer_it_finds_worth_it_and_plays_on() {
        let (mut store, host) = bot_offers_the_host(4);
        let offer = store.view(&host.code, &host.token).unwrap().game.unwrap().trade.unwrap();
        assert_eq!(offer.give, balance::res(1, 0, 0, 0), "it offers one wood");
        let (give, want) = (balance::res(2, 0, 0, 0), offer.want);
        let after = store.act(&host.code, &host.token, Action::CounterTrade { give, want }).unwrap().game.unwrap();
        let traded = Event::Traded { from: PlayerId(1), to: PlayerId(0), give, want };
        assert!(after.events.iter().any(|e| e.event == traded), "traded on the counter");
        assert_eq!((after.current, after.trade), (PlayerId(0), None), "the bot finished its turn");
    }

    #[test]
    fn a_bot_withdraws_an_offer_left_unanswered_and_plays_on() {
        let (mut store, host) = bot_offers_the_host(4);
        let before = store.version(&host.code).unwrap();
        assert!(!store.expire_bot_offers(1_000), "the server first sees the offer");
        assert!(!store.expire_bot_offers(1_000 + BOT_OFFER_MS - 1));
        assert_eq!(store.version(&host.code).unwrap(), before);
        assert!(store.expire_bot_offers(1_000 + BOT_OFFER_MS));
        assert_eq!(store.version(&host.code).unwrap(), before + 1, "waiting polls wake");
        let after = store.view(&host.code, &host.token).unwrap().game.unwrap();
        assert_eq!((after.current, after.trade), (PlayerId(0), None), "withdrawn, and the bot finished its turn");
        assert_eq!(after.players[1].resources.wood, 3);
        assert!(!store.expire_bot_offers(10 * BOT_OFFER_MS), "nothing left to withdraw");
    }

    #[test]
    fn a_practice_match_starts_mid_game_and_its_rival_passes_straight_back() {
        for seed in 0..20 {
            let mut store = Store::new(seed);
            let me = store.practice("Ada").unwrap();
            let game = store.view(&me.code, &me.token).unwrap().game.unwrap();
            assert_eq!((game.phase, game.current, game.dice), (Phase::Playing, PlayerId(0), None));
            assert!(game.has_city(PlayerId(0)));
            for p in [PlayerId(0), PlayerId(1)] {
                let knights: Vec<_> = game.units.iter().filter(|u| u.owner == p).collect();
                assert_eq!(knights.len(), 2, "seed {seed}");
                assert!(knights.iter().all(|u| u.ready == (p == PlayerId(0))));
            }
            let mine = game.units.iter().find(|u| u.owner == PlayerId(0)).unwrap().tile;
            let theirs = game.units.iter().find(|u| u.owner == PlayerId(1)).unwrap().tile;
            assert!(game.board.are_adjacent(mine, theirs), "seed {seed}: a rival knight to attack");

            store.act(&me.code, &me.token, Action::Roll).unwrap();
            let passed = store.act(&me.code, &me.token, Action::EndTurn).unwrap().game.unwrap();
            assert_eq!((passed.current, passed.round, passed.dice), (PlayerId(0), 2, None), "seed {seed}");
        }
    }

    /// A practice match after the roll, with a road the host can build.
    fn rolled_practice(seed: u64) -> (Store, Session, kings_cove_engine::EdgeId) {
        let mut store = Store::new(seed);
        let me = store.practice("Ada").unwrap();
        store.act(&me.code, &me.token, Action::Roll).unwrap();
        let game = store.view(&me.code, &me.token).unwrap().game.unwrap();
        let edge = (0..game.roads.len())
            .map(|i| kings_cove_engine::EdgeId(i as _))
            .find(|&e| game.can_build_road(PlayerId(0), e).is_ok())
            .expect("a road to build");
        (store, me, edge)
    }

    #[test]
    fn a_built_road_can_be_taken_back_once() {
        let (mut store, me, edge) = rolled_practice(1);
        let before = store.view(&me.code, &me.token).unwrap();
        assert!(!before.can_undo, "a roll cannot be undone");
        let built = store.act(&me.code, &me.token, Action::BuildRoad(edge)).unwrap();
        assert!(built.can_undo);
        assert_ne!(
            built.game.as_ref().unwrap().players[0].resources,
            before.game.as_ref().unwrap().players[0].resources
        );
        let back = store.undo(&me.code, &me.token).unwrap();
        assert!(!back.can_undo);
        let (g, b) = (back.game.unwrap(), before.game.unwrap());
        assert_eq!(g.players[0].resources, b.players[0].resources);
        assert_eq!(g.roads, b.roads);
        assert!(g.event_seq >= built.game.unwrap().event_seq, "the counter never runs backwards");
        assert!(store.undo(&me.code, &me.token).is_err(), "only once");
        let again = store.act(&me.code, &me.token, Action::BuildRoad(edge)).unwrap().game.unwrap();
        assert!(again.events.last().unwrap().seq > g.event_seq, "fresh events get fresh numbers");
    }

    #[test]
    fn only_the_latest_plain_action_is_undoable() {
        let (mut store, me, edge) = rolled_practice(1);
        store.act(&me.code, &me.token, Action::BuildRoad(edge)).unwrap();
        let bought = store.act(&me.code, &me.token, Action::BuyCard(kings_cove_engine::UnitKind::Knight)).unwrap();
        assert!(bought.can_undo);
        let back = store.undo(&me.code, &me.token).unwrap().game.unwrap();
        assert!(back.roads[edge.index()].is_some(), "the road before it stays");
        assert!(!store.view(&me.code, &me.token).unwrap().can_undo);
        let ended = store.act(&me.code, &me.token, Action::EndTurn).unwrap();
        assert!(!ended.can_undo);
        assert!(store.undo(&me.code, &me.token).is_err());
    }

    #[test]
    fn another_seat_cannot_undo() {
        let (mut store, me, edge) = rolled_practice(1);
        let rival = store.matches[&me.code].seats[1].token.clone();
        store.act(&me.code, &me.token, Action::BuildRoad(edge)).unwrap();
        assert!(!store.view(&me.code, &rival).unwrap().can_undo);
        assert!(store.undo(&me.code, &rival).is_err());
        assert!(store.undo(&me.code, "forged").is_err());
        assert!(store.undo(&me.code, &me.token).is_ok(), "still the host's to take back");
    }
}
