//! The JSON messages between clients and the authoritative server.
//!
//! | Request | Body | Answer |
//! |---|---|---|
//! | `POST /create` | [`CreateRequest`] | [`Session`] |
//! | `POST /join` | [`JoinRequest`] | [`Session`] |
//! | `POST /start` | [`SessionRequest`] | [`MatchView`] |
//! | `POST /action` | [`ActionRequest`] | [`MatchView`] |
//! | `POST /end` | [`SessionRequest`] (the host's) | [`MatchView`], now `ended` |
//! | `GET /poll?code=&token=&since=` | | [`MatchView`] once its version passes `since` (long poll) |
//!
//! Failures answer a 4xx status with an [`ErrorBody`].

use kings_cove_engine::{Action, Game, PlayerId};
use serde::{Deserialize, Serialize};

/// How long `/poll` holds a request open with no change, in seconds.
pub const POLL_SECONDS: u64 = 20;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateRequest {
    pub name: String,
    /// One of `balance::VICTORY_POINT_GOALS`; the default when absent.
    #[serde(default)]
    pub victory_points: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JoinRequest {
    pub code: String,
    pub name: String,
}

/// A seat in a match: what a client keeps to reconnect.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub code: String,
    pub token: String,
    pub seat: PlayerId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionRequest {
    pub code: String,
    pub token: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActionRequest {
    pub code: String,
    pub token: String,
    pub action: Action,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: String,
}

/// A match as one seat sees it. The server's random generator never leaves it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MatchView {
    pub code: String,
    /// Grows with every change.
    pub version: u64,
    pub seat: PlayerId,
    /// Names in seat order; seat 0 hosts and starts the match.
    pub players: Vec<String>,
    /// `None` while in the lobby.
    pub game: Option<Game>,
    /// The host ended the match; it takes no more actions.
    #[serde(default)]
    pub ended: bool,
    /// Victory points that win the match, known from the lobby on.
    #[serde(default)]
    pub victory_points: u32,
}
