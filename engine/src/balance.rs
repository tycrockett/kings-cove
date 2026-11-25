//! Every balancing constant of the game, in one place.

use crate::types::{Resource, Resources};

/// Victory points needed to win.
pub const VICTORY_POINTS_TO_WIN: u32 = 10;
/// Victory points of a town.
pub const TOWN_POINTS: u32 = 1;
/// Victory points of a city.
pub const CITY_POINTS: u32 = 2;

pub const MIN_PLAYERS: usize = 2;
pub const MAX_PLAYERS: usize = 4;

/// Rings of tiles around the centre tile: 2 gives the classic 19 tiles.
pub const BOARD_RADIUS: i32 = 2;

/// How many tiles of each resource the board holds (sums to the tile count).
pub const TILE_RESOURCES: [(Resource, usize); 4] =
    [(Resource::Wood, 5), (Resource::Stone, 5), (Resource::Food, 5), (Resource::Iron, 4)];

/// Production numbers, one per tile: 2..=12 without 7, weighted toward the
/// middle like two dice are.
pub const PRODUCTION_NUMBERS: [u8; 19] = [2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 8, 8, 9, 9, 10, 10, 11, 11, 12];

/// Numbers that may not sit next to each other (the most frequent ones).
pub const HOT_NUMBERS: [u8; 2] = [6, 8];

/// The dice total that produces nothing and lets the roller move a robber
/// (or place one when the board has none).
pub const ROBBER_ROLL: u8 = 7;

/// The island always has at least this many robbers: one stands from the
/// start, and a removed one comes back (see `ROBBER_RESPAWN_ROUNDS`).
pub const MIN_ROBBERS: u32 = 1;
/// Above the minimum, up to `total_settlements / SETTLEMENTS_PER_ROBBER`.
pub const SETTLEMENTS_PER_ROBBER: u32 = 4;
/// Chance (numerator / denominator) an extra robber appears at the end of a
/// round while below capacity.
pub const ROBBER_SPAWN_CHANCE: (u64, u64) = (1, 4);
/// After a knight removes a robber, no robber appears for this many full
/// rounds; one appears at the end of the last of them if the island is
/// below its minimum.
pub const ROBBER_RESPAWN_ROUNDS: u32 = 2;

/// Spells, unlocked by a city, all sharing the caster's mana. Each is priced
/// in what fits it: the spells that stop or double wood and food are paid
/// in stone and iron or sown with food and wood, and the stone and iron
/// ones the other way round.
pub const FROST_COST: Resources = res(0, 1, 0, 2);
pub const BOUNTY_COST: Resources = res(1, 0, 2, 0);
/// Timber and grain to dig the slide.
pub const ROCKSLIDE_COST: Resources = res(2, 0, 1, 0);
/// Stone, iron and food to open the seam.
pub const MOTHERLODE_COST: Resources = res(0, 1, 1, 1);
/// A stone and iron wall.
pub const WARD_COST: Resources = res(0, 1, 0, 1);
/// Bait and a hook.
pub const LURE_COST: Resources = res(0, 0, 1, 1);
/// One of each, traded around.
pub const SWAP_COST: Resources = res(1, 1, 1, 1);
/// A feast for the troops.
pub const RALLY_COST: Resources = res(0, 0, 2, 0);
/// A cartload of goods.
pub const WINDFALL_COST: Resources = res(1, 1, 1, 0);
/// Rolls of its tile's number a lingering spell lasts (frost, bounty,
/// rockslide, motherlode, swap).
pub const SPELL_CHARGES: u32 = 2;
/// Rounds a ward lasts.
pub const WARD_ROUNDS: u32 = 2;
/// Rounds the caster's mana takes to refill after any spell.
pub const MANA_RECHARGE_ROUNDS: u32 = 4;

/// How many of one resource the bank takes for one of another.
pub const BANK_TRADE_RATE: u32 = 4;

/// How many events the game keeps for the history and animations.
pub const EVENT_HISTORY: usize = 80;

pub const fn res(wood: u32, stone: u32, food: u32, iron: u32) -> Resources {
    Resources { wood, stone, food, iron }
}

pub const STARTING_RESOURCES: Resources = res(2, 2, 2, 1);

/// Resources a building collects per matching adjacent tile.
pub const OUTPOST_YIELD: u32 = 1;
pub const TOWN_YIELD: u32 = 1;
pub const CITY_YIELD: u32 = 2;

pub const ROAD_COST: Resources = res(1, 1, 0, 0);
/// Every new building starts as an outpost.
pub const OUTPOST_COST: Resources = res(2, 1, 0, 0);
/// Outpost → town.
pub const TOWN_UPGRADE_COST: Resources = res(0, 1, 2, 0);
/// Town → city.
pub const CITY_UPGRADE_COST: Resources = res(0, 3, 2, 2);
pub const KNIGHT_CARD_COST: Resources = res(0, 0, 1, 1);
