//! Matches the server holds: lobbies, seats, games and their randomness.
//! Every client request comes through [`Store`], which checks the seat's
//! token and lets the engine validate the action.

pub mod practice;

use std::collections::BTreeMap;

use kings_cove_engine::{Action, Game, PlayerId, Rng, balance, balance::MapSize};
use kings_cove_protocol::{MatchView, RECENT_REACTIONS, Reaction, SentReaction, Session};
use serde::{Deserialize, Serialize};

const CODE_LETTERS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
const ENDED: &str = "The host ended this match";
const CODE_LEN: usize = 5;
const MAX_NAME: usize = 16;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Seat {
    pub name: String,
    pub token: String,
    /// The server plays this seat: a practice match's rival.
    #[serde(default)]
    pub bot: bool,
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
    /// The latest reactions, oldest first: not saved.
    #[serde(skip)]
    reactions: Vec<SentReaction>,
    /// The `seq` of the latest reaction; 0 before the first. Saved, unlike
    /// the reactions, so a restarted server keeps counting up and clients
    /// still tell the next reaction from ones they saw.
    #[serde(default)]
    reaction_seq: u64,
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
            reactions: self.reactions.clone(),
        }
    }

    /// The seats the server plays take their turns.
    fn play_bots(&mut self) {
        let bots: Vec<PlayerId> =
            self.seats.iter().enumerate().filter(|(_, s)| s.bot).map(|(i, _)| PlayerId(i as u8)).collect();
        if let Some(game) = self.game.as_mut().filter(|_| !bots.is_empty()) {
            practice::play_rivals(game, &bots, &mut self.rng);
        }
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
        let code = self.open(vec![Seat { name, token: token.clone(), bot: false }], None);
        Ok(Session { code, token, seat: PlayerId(0) })
    }

    /// A started match against [`practice::RIVAL`], whom the server plays,
    /// dealt into the middle of a game: for trying the client alone.
    pub fn practice(&mut self, name: &str) -> Result<Session, String> {
        let name = clean_name(name)?;
        let token = self.token();
        let mut rng = Rng::new(self.rng.next_u64());
        let game = practice::game(&name, &mut rng);
        let rival = Seat { name: practice::RIVAL.to_owned(), token: self.token(), bot: true };
        let code = self.open(vec![Seat { name, token: token.clone(), bot: false }, rival], Some((game, rng)));
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
                reactions: Vec::new(),
                reaction_seq: 0,
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
        m.seats.push(Seat { name, token: token.clone(), bot: false });
        m.version += 1;
        Ok(Session { code, token, seat: PlayerId(m.seats.len() as u8 - 1) })
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
        m.game = Some(Game::new(&names, m.victory_points, m.map_size, &mut m.rng));
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
        game.apply(seat, action, &mut m.rng).map_err(|e| e.to_string())?;
        m.play_bots();
        m.version += 1;
        Ok(m.view(seat))
    }

    /// The host changes the goal and island size before the game starts; absent fields stay.
    pub fn configure(
        &mut self,
        code: &str,
        token: &str,
        victory_points: Option<u32>,
        map_size: Option<MapSize>,
    ) -> Result<MatchView, String> {
        let m = self.matches.get_mut(code).ok_or("No match with that code")?;
        let seat = m.seat_of(token)?;
        if seat != PlayerId(0) {
            return Err("Only the host can change the settings".to_owned());
        }
        if m.ended {
            return Err(ENDED.to_owned());
        }
        if m.game.is_some() {
            return Err("The match has already started".to_owned());
        }
        if let Some(goal) = victory_points {
            if !balance::VICTORY_POINT_GOALS.contains(&goal) {
                return Err(format!("Play to one of {:?} victory points", balance::VICTORY_POINT_GOALS));
            }
            m.victory_points = goal;
        }
        if let Some(size) = map_size {
            m.map_size = size;
        }
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
            m.version += 1;
        }
        Ok(m.view(seat))
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
    use kings_cove_engine::{Phase, VertexId};

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

    #[test]
    fn only_the_host_configures_an_unstarted_match() {
        let mut store = Store::new(5);
        let host = store.create("Ada").unwrap();
        let guest = store.join(&host.code, "Bo").unwrap();
        assert!(store.configure(&host.code, &guest.token, Some(30), None).is_err(), "only the host");
        assert!(store.configure(&host.code, &host.token, Some(12), None).is_err(), "only the offered goals");
        let before = store.version(&host.code).unwrap();
        let view = store.configure(&host.code, &host.token, Some(30), None).unwrap();
        assert_eq!((view.victory_points, view.map_size), (30, MapSize::default()), "absent fields stay");
        assert_eq!(store.version(&host.code).unwrap(), before + 1, "waiting polls wake");
        let view = store.configure(&host.code, &host.token, None, Some(MapSize::Large)).unwrap();
        assert_eq!((view.victory_points, view.map_size), (30, MapSize::Large));
        let seen = store.view(&host.code, &guest.token).unwrap();
        assert_eq!((seen.victory_points, seen.map_size), (30, MapSize::Large));
        let started = store.start(&host.code, &host.token).unwrap().game.unwrap();
        assert_eq!((started.victory_points, started.board.tiles.len()), (30, MapSize::Large.tiles()));
        assert!(store.configure(&host.code, &host.token, Some(10), None).is_err(), "not after the start");

        let other = store.create("Cy").unwrap();
        store.end(&other.code, &other.token).unwrap();
        assert!(store.configure(&other.code, &other.token, Some(10), None).is_err(), "not once ended");
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
        store.configure(&host.code, &host.token, Some(30), Some(MapSize::Small)).unwrap();
        store.join(&host.code, "Bo").unwrap();
        store.start(&host.code, &host.token).unwrap();
        let saved = serde_json::to_string(&store).unwrap();
        let restored: Store = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored.view(&host.code, &host.token).unwrap(), store.view(&host.code, &host.token).unwrap());
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
}
