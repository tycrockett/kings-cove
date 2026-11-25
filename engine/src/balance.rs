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

/// `robber_capacity = total_settlements / SETTLEMENTS_PER_ROBBER`.
pub const SETTLEMENTS_PER_ROBBER: u32 = 4;
/// Chance (numerator / denominator) a robber spawns at the end of a round
/// while below capacity.
pub const ROBBER_SPAWN_CHANCE: (u64, u64) = (1, 4);

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
pub const RAM_CARD_COST: Resources = res(2, 0, 0, 2);
