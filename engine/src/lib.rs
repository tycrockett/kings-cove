//! King's Cove: the rules, independent of rendering, networking and UI.

pub mod balance;
pub mod board;
pub mod bot;
pub mod game;
pub mod rng;
pub mod types;

pub use board::{Board, Edge, Tile, Vertex};
pub use game::{
    Action, Answer, Blockade, Building, BuildingKind, Cards, Enchantment, Event, Game, Item, ItemKind, LoggedEvent,
    Market, Phase, Player, Robber, RuleError, SetupPlacing, Spell, Structure, StructureKind, Target, TradeOffer, Unit,
    UnitKind, suggest_offer,
};
pub use rng::Rng;
pub use types::{EdgeId, PlayerId, Resource, Resources, RobberId, TileId, UnitId, VertexId};
