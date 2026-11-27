//! Every balancing constant of the game, in one place.

use serde::{Deserialize, Serialize};

use crate::{
    game::ItemKind,
    types::{Resource, Resources},
};

/// The victory points a match may be played to, chosen in the lobby.
pub const VICTORY_POINT_GOALS: [u32; 7] = [5, 10, 15, 20, 30, 40, 50];
pub const DEFAULT_VICTORY_POINTS: u32 = 20;
/// Victory points of a town.
pub const TOWN_POINTS: u32 = 1;
/// Victory points of a city.
pub const CITY_POINTS: u32 = 2;

pub const MIN_PLAYERS: usize = 2;
pub const MAX_PLAYERS: usize = 4;

/// The island a match is played on, chosen in the lobby. Saved before the
/// sizes were renamed, "small", "medium" and "large" meant 19, 37 and 61
/// tiles: they still read as those.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MapSize {
    /// The classic 19 tiles.
    #[serde(rename = "xs", alias = "small")]
    ExtraSmall,
    /// 37 tiles.
    #[serde(rename = "s", alias = "medium")]
    Small,
    /// 61 tiles.
    #[default]
    #[serde(rename = "m", alias = "large")]
    Medium,
    /// 91 tiles.
    #[serde(rename = "l")]
    Large,
    /// 127 tiles.
    #[serde(rename = "xl")]
    ExtraLarge,
}

impl MapSize {
    pub const ALL: [MapSize; 5] =
        [MapSize::ExtraSmall, MapSize::Small, MapSize::Medium, MapSize::Large, MapSize::ExtraLarge];

    /// Rings of tiles around the centre tile.
    pub const fn radius(self) -> i32 {
        match self {
            MapSize::ExtraSmall => 2,
            MapSize::Small => 3,
            MapSize::Medium => 4,
            MapSize::Large => 5,
            MapSize::ExtraLarge => 6,
        }
    }

    pub const fn tiles(self) -> usize {
        let r = self.radius() as usize;
        3 * r * r + 3 * r + 1
    }

    pub const fn label(self) -> &'static str {
        match self {
            MapSize::ExtraSmall => "Extra small",
            MapSize::Small => "Small",
            MapSize::Medium => "Medium",
            MapSize::Large => "Large",
            MapSize::ExtraLarge => "Extra large",
        }
    }

    /// For a row of choices too narrow for [`MapSize::label`].
    pub const fn short_label(self) -> &'static str {
        match self {
            MapSize::ExtraSmall => "XS",
            MapSize::Small => "S",
            MapSize::Medium => "M",
            MapSize::Large => "L",
            MapSize::ExtraLarge => "XL",
        }
    }
}

/// The resources of every 19 tiles (a bigger board repeats them).
pub const TILE_RESOURCES: [(Resource, usize); 4] =
    [(Resource::Wood, 5), (Resource::Stone, 5), (Resource::Food, 5), (Resource::Iron, 4)];

/// The production numbers of every 19 tiles: 2..=12 without 7, weighted
/// toward the middle like two dice are (a bigger board repeats them).
pub const PRODUCTION_NUMBERS: [u8; 19] = [2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 8, 8, 9, 9, 10, 10, 11, 11, 12];

/// Numbers that may not sit next to each other (the most frequent ones).
pub const HOT_NUMBERS: [u8; 2] = [6, 8];

/// The dice total that produces nothing and lets the roller move a robber
/// (or place one when the board has none).
pub const ROBBER_ROLL: u8 = 7;

/// The island always has at least this many robbers: one stands from the
/// start, and a removed one comes back (see `ROBBER_RESPAWN_ROUNDS`).
pub const MIN_ROBBERS: u32 = 1;
/// Above the minimum, up to `total_settlements / SETTLEMENTS_PER_ROBBER`
/// plus `tiles / TILES_PER_EXTRA_ROBBER`.
pub const SETTLEMENTS_PER_ROBBER: u32 = 4;
/// Bigger islands hold a few more robbers: one more per this many tiles
/// (none extra on 19 or 37 tiles, 1 on 61, 2 on 91, 3 on 127).
pub const TILES_PER_EXTRA_ROBBER: u32 = 40;
/// Chance (numerator / denominator) an extra robber appears at the end of a
/// round while below capacity.
pub const ROBBER_SPAWN_CHANCE: (u64, u64) = (1, 2);
/// After a knight removes a robber, no robber appears for this many full
/// rounds; one appears at the end of the last of them if the island is
/// below its minimum.
pub const ROBBER_RESPAWN_ROUNDS: u32 = 2;

// Spells, all sharing the caster's mana; free to a caster with a city. Iron
// is the heart of every one of them.

/// Iron and stone to freeze a field or a mine.
pub const FROST_COST: Resources = res(0, 1, 0, 3);
/// Timber, grain and iron tools for a double harvest.
pub const BOUNTY_COST: Resources = res(1, 0, 1, 3);
/// Bait and a hook.
pub const LURE_COST: Resources = res(0, 0, 1, 3);
/// One of each, traded around, with iron to bind it.
pub const SWAP_COST: Resources = res(1, 1, 1, 3);
/// A cartload of goods on iron wheels.
pub const WINDFALL_COST: Resources = res(1, 0, 1, 1);
/// An iron horseshoe nailed to a post.
pub const LUCK_COST: Resources = res(1, 0, 0, 3);
/// Chance (numerator / denominator) the roll after a Luck comes up as its
/// tile's number; otherwise any other total does.
pub const LUCK_CHANCE: (u64, u64) = (1, 2);
/// Rolls of its tile's number a lingering spell lasts (frost, bounty, swap).
pub const SPELL_CHARGES: u32 = 2;
/// Rounds the caster's mana takes to refill after any spell.
pub const MANA_RECHARGE_ROUNDS: u32 = 4;

// Items turn up on the island for knights to walk onto.

/// The island is topped up to this many hidden items per player.
pub const ITEMS_PER_PLAYER: u32 = 2;
/// Rounds an item lies unclaimed before it vanishes.
pub const ITEM_ROUNDS: u32 = 4;
/// How often each item turns up, against the others.
pub const ITEM_WEIGHTS: [(ItemKind, u32); 8] = [
    (ItemKind::IronOre, 3),
    (ItemKind::Supplies, 3),
    (ItemKind::ManaCrystal, 2),
    (ItemKind::Scroll, 2),
    (ItemKind::Recruit, 2),
    (ItemKind::Relic, 1),
    (ItemKind::Trap, 2),
    (ItemKind::Hoard, 1),
];
pub const IRON_ORE_FIND: Resources = res(0, 0, 0, 2);
/// Of one resource, drawn when found.
pub const SUPPLIES_FIND: u32 = 2;
pub const HOARD_FIND: Resources = res(2, 2, 2, 2);
/// Victory points of each relic a player's knights carry.
pub const RELIC_POINTS: u32 = 1;

/// Full rounds a knight's blockade holds after it starts packing up.
pub const BLOCKADE_LIFT_ROUNDS: u32 = 2;
/// What a tile pays each turn to the player ringing it with roads and
/// buildings, a city among them.
pub const RING_INCOME: u32 = 1;

/// How many of one resource the bank takes for one of another.
pub const BANK_TRADE_RATE: u32 = 4;

/// How many events the game keeps for the history and animations.
pub const EVENT_HISTORY: usize = 80;

pub const fn res(wood: u32, stone: u32, food: u32, iron: u32) -> Resources {
    Resources { wood, stone, food, iron }
}

pub const STARTING_RESOURCES: Resources = res(2, 2, 2, 1);
/// Unit tokens of knights every player starts with.
pub const STARTING_KNIGHTS: u32 = 1;

/// Resources a building collects per matching adjacent tile.
pub const OUTPOST_YIELD: u32 = 1;
pub const TOWN_YIELD: u32 = 1;
pub const CITY_YIELD: u32 = 2;

pub const ROAD_COST: Resources = res(1, 1, 0, 0);
/// Every new building starts as an outpost.
pub const OUTPOST_COST: Resources = res(2, 1, 0, 0);
/// Outpost → town.
pub const TOWN_UPGRADE_COST: Resources = res(0, 1, 2, 1);
/// Town → city.
pub const CITY_UPGRADE_COST: Resources = res(0, 3, 2, 2);
pub const KNIGHT_CARD_COST: Resources = res(0, 0, 1, 1);
