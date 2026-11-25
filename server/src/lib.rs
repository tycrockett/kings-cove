//! Matches the server holds: lobbies, seats, games and their randomness.
//! Every client request comes through [`Store`], which checks the seat's
//! token and lets the engine validate the action.

use std::collections::BTreeMap;

use kings_cove_engine::{Action, Game, PlayerId, Rng, balance, balance::MapSize};
use kings_cove_protocol::{MatchView, Session};
use serde::{Deserialize, Serialize};

const CODE_LETTERS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
const ENDED: &str = "The host ended this match";
const CODE_LEN: usize = 5;
const MAX_NAME: usize = 16;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Seat {
    pub name: String,
    pub token: String,
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
}

/// The goal of matches saved before goals could be chosen.
fn classic_goal() -> u32 {
    balance::VICTORY_POINT_GOALS[0]
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

    pub fn create(
        &mut self,
        name: &str,
        victory_points: Option<u32>,
        map_size: Option<MapSize>,
    ) -> Result<Session, String> {
        let name = clean_name(name)?;
        let victory_points = victory_points.unwrap_or(balance::DEFAULT_VICTORY_POINTS);
        if !balance::VICTORY_POINT_GOALS.contains(&victory_points) {
            return Err(format!("Play to one of {:?} victory points", balance::VICTORY_POINT_GOALS));
        }
        let code = self.new_code();
        let token = self.token();
        let rng = Rng::new(self.rng.next_u64());
        self.matches.insert(
            code.clone(),
            Match {
                code: code.clone(),
                seats: vec![Seat { name, token: token.clone() }],
                game: None,
                rng,
                version: 1,
                ended: false,
                victory_points,
                map_size: map_size.unwrap_or_default(),
            },
        );
        Ok(Session { code, token, seat: PlayerId(0) })
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
        m.seats.push(Seat { name, token: token.clone() });
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
    fn create_join_start_and_act_with_seat_tokens() {
        let mut store = Store::new(7);
        assert!(store.create("Ada", Some(15), None).is_err(), "only the offered goals");
        let host = store.create("Ada", None, None).unwrap();
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
        let host = store.create("Ada", Some(10), None).unwrap();
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
    fn store_round_trips_through_json() {
        let mut store = Store::new(1);
        let host = store.create("Ada", Some(30), Some(MapSize::Small)).unwrap();
        store.join(&host.code, "Bo").unwrap();
        store.start(&host.code, &host.token).unwrap();
        let saved = serde_json::to_string(&store).unwrap();
        let restored: Store = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored.view(&host.code, &host.token).unwrap(), store.view(&host.code, &host.token).unwrap());
    }
}
