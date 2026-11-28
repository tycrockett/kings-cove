//! Identifiers and resource bundles.

use std::fmt;

use serde::{Deserialize, Serialize};

macro_rules! id {
    ($(#[$doc:meta])* $name:ident($inner:ty)) => {
        $(#[$doc])*
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub $inner);

        impl $name {
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

id!(
    /// A hex tile: holds at most one unit or robber.
    TileId(u16)
);
id!(
    /// A tile corner, shared by up to three tiles: holds at most one building.
    VertexId(u16)
);
id!(
    /// A tile side, shared by up to two tiles: holds at most one road.
    EdgeId(u16)
);
id!(
    /// A seat at the table, in turn order.
    PlayerId(u8)
);
id!(
    /// A deployed knight or ram.
    UnitId(u32)
);
id!(
    /// A neutral robber.
    RobberId(u32)
);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Resource {
    Wood,
    Stone,
    Food,
    Iron,
}

impl Resource {
    pub const ALL: [Resource; 4] = [Resource::Wood, Resource::Stone, Resource::Food, Resource::Iron];

    pub fn name(self) -> &'static str {
        match self {
            Resource::Wood => "Wood",
            Resource::Stone => "Stone",
            Resource::Food => "Food",
            Resource::Iron => "Iron",
        }
    }
}

/// A bundle of resources: an inventory, a cost or one side of a trade.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Resources {
    pub wood: u32,
    pub stone: u32,
    pub food: u32,
    pub iron: u32,
}

impl Resources {
    pub const ZERO: Resources = Resources { wood: 0, stone: 0, food: 0, iron: 0 };

    pub fn one(resource: Resource) -> Self {
        let mut r = Self::ZERO;
        *r.get_mut(resource) = 1;
        r
    }

    pub fn get(&self, resource: Resource) -> u32 {
        match resource {
            Resource::Wood => self.wood,
            Resource::Stone => self.stone,
            Resource::Food => self.food,
            Resource::Iron => self.iron,
        }
    }

    pub fn get_mut(&mut self, resource: Resource) -> &mut u32 {
        match resource {
            Resource::Wood => &mut self.wood,
            Resource::Stone => &mut self.stone,
            Resource::Food => &mut self.food,
            Resource::Iron => &mut self.iron,
        }
    }

    pub fn total(&self) -> u32 {
        self.wood + self.stone + self.food + self.iron
    }

    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }

    /// Holds at least `other` of every resource.
    pub fn covers(&self, other: &Resources) -> bool {
        Resource::ALL.iter().all(|&r| self.get(r) >= other.get(r))
    }

    pub fn add(&mut self, other: &Resources) {
        for r in Resource::ALL {
            *self.get_mut(r) += other.get(r);
        }
    }

    /// Takes `other` away; the caller checked [`Self::covers`].
    pub fn remove(&mut self, other: &Resources) {
        debug_assert!(self.covers(other));
        for r in Resource::ALL {
            *self.get_mut(r) -= other.get(r);
        }
    }

    /// What `self` lacks of `cost`.
    pub fn shortfall(&self, cost: &Resources) -> Resources {
        let mut r = Resources::ZERO;
        for k in Resource::ALL {
            *r.get_mut(k) = cost.get(k).saturating_sub(self.get(k));
        }
        r
    }

    /// What `self` holds beyond `cost`.
    pub fn beyond(&self, cost: &Resources) -> Resources {
        cost.shortfall(self)
    }
}

impl fmt::Display for Resources {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = Resource::ALL
            .iter()
            .filter(|&&r| self.get(r) > 0)
            .map(|&r| format!("{} {}", self.get(r), r.name()))
            .collect();
        if parts.is_empty() { f.write_str("nothing") } else { f.write_str(&parts.join(", ")) }
    }
}
