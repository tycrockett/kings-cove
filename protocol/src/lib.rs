//! The JSON messages between clients and the authoritative server.
//!
//! | Request | Body | Answer |
//! |---|---|---|
//! | `POST /create` | [`CreateRequest`] | [`Session`] |
//! | `POST /practice` (debug servers only) | [`CreateRequest`] | [`Session`] of a started match against a rival the server plays |
//! | `POST /join` | [`JoinRequest`] | [`Session`] |
//! | `POST /configure` | [`ConfigureRequest`] (the host's, in the lobby) | [`MatchView`] |
//! | `POST /add-bot` | [`AddBotRequest`] (the host's, in the lobby) | [`MatchView`], a bot the server plays seated |
//! | `POST /start` | [`SessionRequest`] | [`MatchView`] |
//! | `POST /action` | [`ActionRequest`] | [`MatchView`] |
//! | `POST /react` | [`ReactRequest`] | [`MatchView`] |
//! | `POST /end` | [`SessionRequest`] (the host's) | [`MatchView`], now `ended` |
//! | `GET /poll?code=&token=&since=` | | [`MatchView`] once its version passes `since` (long poll) |
//!
//! Failures answer a 4xx status with an [`ErrorBody`].

use kings_cove_engine::{
    Action, Game, PlayerId,
    balance::{Amount, MapShape, MapSize},
    bot::Style,
};
use serde::{Deserialize, Serialize};

/// How long `/poll` holds a request open with no change, in seconds.
pub const POLL_SECONDS: u64 = 20;
/// How many recent reactions a [`MatchView`] carries.
pub const RECENT_REACTIONS: usize = 8;

/// A match starts with the default settings; the host picks others in the
/// lobby with `/configure`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateRequest {
    pub name: String,
}

/// The host changes the match's settings before starting it; an absent
/// field stays as it is.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigureRequest {
    pub code: String,
    pub token: String,
    /// One of `balance::VICTORY_POINT_GOALS`.
    #[serde(default)]
    pub victory_points: Option<u32>,
    #[serde(default)]
    pub map_size: Option<MapSize>,
    #[serde(default)]
    pub map_shape: Option<MapShape>,
    #[serde(default)]
    pub shrines: Option<Amount>,
    #[serde(default)]
    pub mines: Option<Amount>,
    #[serde(default)]
    pub mills: Option<Amount>,
    /// The server draws every setting when the match starts, to suit its
    /// players, and nobody knows them before.
    #[serde(default)]
    pub random: Option<bool>,
}

impl ConfigureRequest {
    /// A request of `code`'s seat `token` that changes nothing yet.
    pub fn new(code: String, token: String) -> Self {
        ConfigureRequest {
            code,
            token,
            victory_points: None,
            map_size: None,
            map_shape: None,
            shrines: None,
            mines: None,
            mills: None,
            random: None,
        }
    }
}

/// The host seats a bot of `style` before starting the match.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AddBotRequest {
    pub code: String,
    pub token: String,
    #[serde(default)]
    pub style: Style,
}

/// The fixed set of reactions a player may send.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reaction {
    Wave,
    Sad,
    Wow,
    Angry,
    Heart,
    Eyes,
}

impl Reaction {
    pub const ALL: [Reaction; 6] =
        [Reaction::Wave, Reaction::Sad, Reaction::Wow, Reaction::Angry, Reaction::Heart, Reaction::Eyes];

    pub const fn emoji(self) -> &'static str {
        match self {
            Reaction::Wave => "👋",
            Reaction::Sad => "😢",
            Reaction::Wow => "😮",
            Reaction::Angry => "😡",
            Reaction::Heart => "💖",
            Reaction::Eyes => "👀",
        }
    }
}

/// A seat reacts.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReactRequest {
    pub code: String,
    pub token: String,
    pub reaction: Reaction,
}

/// A reaction a seat sent; `seq` counts the match's reactions from 1.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SentReaction {
    pub seq: u64,
    pub seat: PlayerId,
    pub reaction: Reaction,
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
    /// Victory points that win the match, known from the lobby on (drawn
    /// at the start when `random`).
    #[serde(default)]
    pub victory_points: u32,
    /// The island's size, known from the lobby on.
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
    /// The settings are drawn when the match starts; until then the ones
    /// above mean nothing.
    #[serde(default)]
    pub random: bool,
    /// The latest reactions, oldest first, at most [`RECENT_REACTIONS`].
    #[serde(default)]
    pub reactions: Vec<SentReaction>,
    /// Per seat, how the server plays it: a bot's style, `None` for people
    /// (and a practice rival, which only passes).
    #[serde(default)]
    pub bots: Vec<Option<Style>>,
}
