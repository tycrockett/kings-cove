//! Board topology: hex tiles in axial coordinates whose corners (vertices)
//! and sides (edges) are shared between neighbours, each with a stable id.
//!
//! Positions use an integer lattice for pointy-top hexes: a tile at axial
//! `(q, r)` has its centre at `(2q + r, 3r)`, where one x unit is half a hex's
//! width and one y unit is half its side. Corners land on exact integers, so
//! shared corners deduplicate without floating point.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::{
    balance::{self, MapShape, MapSize},
    rng::Rng,
    types::{EdgeId, Resource, TileId, VertexId},
};

/// Corner offsets of a pointy-top hex in lattice units, clockwise from the top.
const CORNERS: [(i32, i32); 6] = [(0, -2), (1, -1), (1, 1), (0, 2), (-1, 1), (-1, -1)];
const AXIAL_DIRECTIONS: [(i32, i32); 6] = [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tile {
    pub id: TileId,
    pub q: i32,
    pub r: i32,
    /// What it produces; meaningless on a barren tile.
    pub resource: Resource,
    /// Its production number, 2..=12 without 7; 0 on a barren tile (where a
    /// structure stands), which never produces.
    pub number: u8,
    /// Corners, clockwise from the top.
    pub vertices: [VertexId; 6],
    /// Sides: `edges[i]` joins `vertices[i]` and `vertices[(i + 1) % 6]`.
    pub edges: [EdgeId; 6],
    pub neighbors: Vec<TileId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vertex {
    pub id: VertexId,
    /// Lattice position.
    pub x: i32,
    pub y: i32,
    pub tiles: Vec<TileId>,
    pub edges: Vec<EdgeId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub id: EdgeId,
    pub vertices: [VertexId; 2],
    pub tiles: Vec<TileId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Board {
    pub tiles: Vec<Tile>,
    pub vertices: Vec<Vertex>,
    pub edges: Vec<Edge>,
}

impl Board {
    /// The topology of a hexagonal board of `radius` rings, every tile Wood 2;
    /// [`Board::generate`] deals resources and numbers onto it.
    pub fn hexagon(radius: i32) -> Board {
        Board::from_coords(hex_area(radius))
    }

    /// The topology of a board of tiles at the axial `coords`, every tile
    /// Wood 2. Tile ids follow the order of `coords`.
    pub fn from_coords(coords: Vec<(i32, i32)>) -> Board {
        // Stable vertex ids: sorted by lattice position (top to bottom, left to right).
        let mut vertex_keys: BTreeMap<(i32, i32), u16> = BTreeMap::new();
        for &(q, r) in &coords {
            let (cx, cy) = (2 * q + r, 3 * r);
            for (dx, dy) in CORNERS {
                vertex_keys.insert((cy + dy, cx + dx), 0);
            }
        }
        for (i, id) in vertex_keys.values_mut().enumerate() {
            *id = i as u16;
        }
        let mut vertices: Vec<Vertex> = vertex_keys
            .iter()
            .map(|(&(y, x), &id)| Vertex { id: VertexId(id), x, y, tiles: Vec::new(), edges: Vec::new() })
            .collect();

        let tile_vertices: Vec<[VertexId; 6]> = coords
            .iter()
            .map(|&(q, r)| {
                let (cx, cy) = (2 * q + r, 3 * r);
                CORNERS.map(|(dx, dy)| VertexId(vertex_keys[&(cy + dy, cx + dx)]))
            })
            .collect();

        // Stable edge ids: sorted by their two vertex ids.
        let mut edge_keys: BTreeMap<(VertexId, VertexId), u16> = BTreeMap::new();
        for corners in &tile_vertices {
            for i in 0..6 {
                let (a, b) = (corners[i], corners[(i + 1) % 6]);
                edge_keys.insert((a.min(b), a.max(b)), 0);
            }
        }
        for (i, id) in edge_keys.values_mut().enumerate() {
            *id = i as u16;
        }
        let mut edges: Vec<Edge> = edge_keys
            .iter()
            .map(|(&(a, b), &id)| Edge { id: EdgeId(id), vertices: [a, b], tiles: Vec::new() })
            .collect();
        for edge in &edges {
            for v in edge.vertices {
                vertices[v.index()].edges.push(edge.id);
            }
        }

        let index_of: BTreeMap<(i32, i32), usize> = coords.iter().enumerate().map(|(i, &c)| (c, i)).collect();
        let mut tiles = Vec::with_capacity(coords.len());
        for (i, &(q, r)) in coords.iter().enumerate() {
            let id = TileId(i as u16);
            let corners = tile_vertices[i];
            let sides: [EdgeId; 6] = std::array::from_fn(|k| {
                let (a, b) = (corners[k], corners[(k + 1) % 6]);
                EdgeId(edge_keys[&(a.min(b), a.max(b))])
            });
            for v in corners {
                vertices[v.index()].tiles.push(id);
            }
            for e in sides {
                edges[e.index()].tiles.push(id);
            }
            let neighbors = AXIAL_DIRECTIONS
                .iter()
                .filter_map(|&(dq, dr)| index_of.get(&(q + dq, r + dr)).map(|&n| TileId(n as u16)))
                .collect();
            tiles.push(Tile {
                id,
                q,
                r,
                resource: Resource::Wood,
                number: 2,
                vertices: corners,
                edges: sides,
                neighbors,
            });
        }
        Board { tiles, vertices, edges }
    }

    /// A playable island of `size` and `shape`: [`Board::layout`] with
    /// resources and numbers dealt onto every tile ([`Board::deal_out`]).
    pub fn generate(size: MapSize, shape: MapShape, rng: &mut Rng) -> Board {
        let mut board = Board::layout(size, shape, rng);
        board.deal_out(&[], rng);
        board
    }

    /// The tiles of an island of `size` and `shape`, every tile Wood 2.
    pub fn layout(size: MapSize, shape: MapShape, rng: &mut Rng) -> Board {
        match shape {
            MapShape::Classic => Board::hexagon(size.radius()),
            MapShape::Wild => Board::from_coords(wild_coords(size.tiles(), rng)),
        }
    }

    /// Deals the resources of [`balance::TILE_RESOURCES`] and the numbers of
    /// [`balance::PRODUCTION_NUMBERS`] (repeated for a bigger board) onto
    /// every tile but the `barren` ones, so no two hot numbers touch and no
    /// tile shares its number with a neighbour. Barren tiles (where a
    /// structure stands) get number 0: they never produce.
    pub fn deal_out(&mut self, barren: &[TileId], rng: &mut Rng) {
        let open: Vec<usize> = self.tile_ids().filter(|t| !barren.contains(t)).map(TileId::index).collect();
        let pool: Vec<Resource> =
            balance::TILE_RESOURCES.iter().flat_map(|&(r, count)| std::iter::repeat_n(r, count)).collect();
        let mut resources = deal(&pool, open.len(), rng);
        rng.shuffle(&mut resources);
        for (&i, resource) in open.iter().zip(resources) {
            self.tiles[i].resource = resource;
        }
        let mut numbers = deal(&balance::PRODUCTION_NUMBERS, open.len(), rng);
        loop {
            rng.shuffle(&mut numbers);
            if self.place_numbers(&numbers, &open, rng) {
                break;
            }
        }
    }

    /// Places `numbers` one per `open` tile (the rest get 0) so they are
    /// balanced, backtracking when a tile has no fitting number left; gives
    /// up (to be retried with another draw) when that takes too long.
    fn place_numbers(&mut self, numbers: &[u8], open: &[usize], rng: &mut Rng) -> bool {
        let mut counts: BTreeMap<u8, usize> = BTreeMap::new();
        for &n in numbers {
            *counts.entry(n).or_default() += 1;
        }
        for tile in &mut self.tiles {
            tile.number = 0;
        }
        let mut budget = 20_000;
        self.fill(open, &mut counts, rng, &mut budget)
    }

    /// Numbers the first of `open` and, recursively, the rest.
    fn fill(&mut self, open: &[usize], counts: &mut BTreeMap<u8, usize>, rng: &mut Rng, budget: &mut u32) -> bool {
        let Some((&i, rest)) = open.split_first() else { return true };
        if *budget == 0 {
            return false;
        }
        *budget -= 1;
        let mut options: Vec<u8> = counts.iter().filter(|&(_, &c)| c > 0).map(|(&n, _)| n).collect();
        rng.shuffle(&mut options);
        for n in options {
            if self.fits(TileId(i as u16), n) {
                self.tiles[i].number = n;
                *counts.get_mut(&n).expect("counted") -= 1;
                if self.fill(rest, counts, rng, budget) {
                    return true;
                }
                *counts.get_mut(&n).expect("counted") += 1;
                self.tiles[i].number = 0;
            }
        }
        false
    }

    /// Whether `number` may go on `tile` next to the numbers already placed.
    fn fits(&self, tile: TileId, number: u8) -> bool {
        let hot = |n: u8| balance::HOT_NUMBERS.contains(&n);
        self.tile(tile).neighbors.iter().all(|&nb| {
            let other = self.tile(nb).number;
            other == 0 || (other != number && !(hot(number) && hot(other)))
        })
    }

    /// No hot numbers side by side, and no neighbours sharing a number;
    /// barren tiles (number 0) aside.
    pub fn numbers_balanced(&self) -> bool {
        let hot = |n: u8| balance::HOT_NUMBERS.contains(&n);
        self.tiles.iter().filter(|t| t.number != 0).all(|t| {
            t.neighbors
                .iter()
                .map(|&n| self.tile(n).number)
                .filter(|&other| other != 0)
                .all(|other| other != t.number && !(hot(t.number) && hot(other)))
        })
    }

    pub fn tile(&self, id: TileId) -> &Tile {
        &self.tiles[id.index()]
    }

    pub fn vertex(&self, id: VertexId) -> &Vertex {
        &self.vertices[id.index()]
    }

    /// The corners one side away from `v`.
    pub fn vertex_neighbors(&self, v: VertexId) -> impl Iterator<Item = VertexId> + '_ {
        self.vertex(v).edges.iter().map(move |&e| {
            let [a, b] = self.edge(e).vertices;
            if a == v { b } else { a }
        })
    }

    pub fn edge(&self, id: EdgeId) -> &Edge {
        &self.edges[id.index()]
    }

    pub fn tile_ids(&self) -> impl Iterator<Item = TileId> + '_ {
        self.tiles.iter().map(|t| t.id)
    }

    pub fn are_adjacent(&self, a: TileId, b: TileId) -> bool {
        self.tile(a).neighbors.contains(&b)
    }

    /// Lattice centre of a tile.
    pub fn tile_center(&self, id: TileId) -> (i32, i32) {
        let t = self.tile(id);
        (2 * t.q + t.r, 3 * t.r)
    }
}

/// `n` items from whole copies of `set`, topped up with a random part of one
/// more, so every kind keeps its share.
fn deal<T: Copy>(set: &[T], n: usize, rng: &mut Rng) -> Vec<T> {
    let mut out: Vec<T> = set.iter().copied().cycle().take(n / set.len() * set.len()).collect();
    let mut extra = set.to_vec();
    rng.shuffle(&mut extra);
    out.extend_from_slice(&extra[..n % set.len()]);
    out
}

/// The axial coordinates within `radius` rings of the centre.
fn hex_area(radius: i32) -> Vec<(i32, i32)> {
    let mut coords = Vec::new();
    for q in -radius..=radius {
        for r in (-radius).max(-q - radius)..=radius.min(-q + radius) {
            coords.push((q, r));
        }
    }
    coords
}

fn hex_distance((q, r): (i32, i32)) -> i32 {
    (q.abs() + r.abs() + (q + r).abs()) / 2
}

fn neighbors_of((q, r): (i32, i32)) -> impl Iterator<Item = (i32, i32)> {
    AXIAL_DIRECTIONS.iter().map(move |&(dq, dr)| (q + dq, r + dr))
}

/// Twelve directions, as unit vectors: the axis a wild island stretches along.
const STRETCH_AXES: [(f64, f64); 12] = [
    (1.0, 0.0),
    (0.866_025_4, 0.5),
    (0.5, 0.866_025_4),
    (0.0, 1.0),
    (-0.5, 0.866_025_4),
    (-0.866_025_4, 0.5),
    (-1.0, 0.0),
    (-0.866_025_4, -0.5),
    (-0.5, -0.866_025_4),
    (0.0, -1.0),
    (0.5, -0.866_025_4),
    (0.866_025_4, -0.5),
];
/// How many tiles' worth of distance the landscape's noise outweighs: how
/// deep the bays and how long the capes of a wild island.
const WILD_ROUGHNESS: f64 = 3.0;

/// The axial coordinates of a wild island of `n` tiles, sorted, around the
/// centre. It is grown from the centre over a smoothed random landscape
/// that falls away with the distance from it, stretched along a random axis,
/// so its coast has capes and bays. Water it encloses is filled in and the
/// lowest coast taken away for it, keeping the island in one piece.
fn wild_coords(n: usize, rng: &mut Rng) -> Vec<(i32, i32)> {
    // Room for an island stretched to twice a hexagon's width.
    let reach = {
        let mut r = 0;
        while 3 * r * r + 3 * r + 1 < n as i32 {
            r += 1;
        }
        2 * r + 2
    };
    let area: BTreeSet<(i32, i32)> = hex_area(reach).into_iter().collect();

    // A landscape of random heights, smoothed twice, rescaled to 0..1.
    let mut height: BTreeMap<(i32, i32), f64> = area.iter().map(|&c| (c, rng.below(1000) as f64)).collect();
    for _ in 0..2 {
        height = area
            .iter()
            .map(|&c| {
                let around: Vec<f64> =
                    std::iter::once(c).chain(neighbors_of(c)).filter_map(|n| height.get(&n).copied()).collect();
                (c, around.iter().sum::<f64>() / around.len() as f64)
            })
            .collect();
    }
    let lo = height.values().copied().fold(f64::MAX, f64::min);
    let hi = height.values().copied().fold(f64::MIN, f64::max);
    let (axis_x, axis_y) = STRETCH_AXES[rng.below(STRETCH_AXES.len())];
    let stretch = 1.0 + rng.below(9) as f64 / 10.0;
    let ground: BTreeMap<(i32, i32), f64> = area
        .iter()
        .map(|&(q, r)| {
            // In tile widths: neighbours are 1 apart.
            let (x, y) = (q as f64 + r as f64 / 2.0, r as f64 * 0.866_025_4);
            let (along, across) = (x * axis_x + y * axis_y, x * axis_y - y * axis_x);
            let distance = ((along / stretch).powi(2) + across.powi(2)).sqrt();
            let noise = if hi > lo { (height[&(q, r)] - lo) / (hi - lo) } else { 0.0 };
            ((q, r), WILD_ROUGHNESS * noise - distance)
        })
        .collect();
    let higher = |a: &(i32, i32), b: &(i32, i32)| ground[a].total_cmp(&ground[b]).then(b.cmp(a));

    // Grow from the centre, always onto the highest ground along the coast.
    let mut island: BTreeSet<(i32, i32)> = BTreeSet::from([(0, 0)]);
    let mut coast: BTreeSet<(i32, i32)> = neighbors_of((0, 0)).collect();
    while island.len() < n {
        let next = *coast.iter().max_by(|a, b| higher(a, b)).expect("the area outgrows any island");
        coast.remove(&next);
        island.insert(next);
        coast.extend(neighbors_of(next).filter(|c| area.contains(c) && !island.contains(c)));
    }

    // Fill in enclosed water, then give up as many tiles of the lowest
    // coast, never one that would split the island.
    let open_sea = |island: &BTreeSet<(i32, i32)>| {
        let start = (reach + 1, 0);
        let mut sea = BTreeSet::from([start]);
        let mut queue = VecDeque::from([start]);
        while let Some(c) = queue.pop_front() {
            for nb in neighbors_of(c) {
                if hex_distance(nb) <= reach + 1 && !island.contains(&nb) && sea.insert(nb) {
                    queue.push_back(nb);
                }
            }
        }
        sea
    };
    let sea = open_sea(&island);
    let lakes: Vec<(i32, i32)> = area.iter().copied().filter(|c| !island.contains(c) && !sea.contains(c)).collect();
    island.extend(lakes);
    while island.len() > n {
        let sea = open_sea(&island);
        let mut shore: Vec<(i32, i32)> =
            island.iter().copied().filter(|&c| neighbors_of(c).any(|nb| sea.contains(&nb))).collect();
        shore.sort_by(|a, b| higher(a, b));
        let cut = shore
            .into_iter()
            .find(|c| {
                let mut rest = island.clone();
                rest.remove(c);
                connected(&rest)
            })
            .expect("a coast tile whose loss keeps the island whole");
        island.remove(&cut);
    }

    // Centre it on the world origin.
    let (sx, sy) = island.iter().fold((0, 0), |(sx, sy), &(q, r)| (sx + 2 * q + r, sy + 3 * r));
    let count = island.len() as f64;
    let dr = (-(sy as f64) / count / 3.0).round() as i32;
    let dq = ((-(sx as f64) / count - dr as f64) / 2.0).round() as i32;
    island.into_iter().map(|(q, r)| (q + dq, r + dr)).collect::<BTreeSet<_>>().into_iter().collect()
}

/// The tiles at `cells` form one piece.
fn connected(cells: &BTreeSet<(i32, i32)>) -> bool {
    let Some(&start) = cells.first() else { return true };
    let mut seen = BTreeSet::from([start]);
    let mut queue = VecDeque::from([start]);
    while let Some(c) = queue.pop_front() {
        for nb in neighbors_of(c) {
            if cells.contains(&nb) && seen.insert(nb) {
                queue.push_back(nb);
            }
        }
    }
    seen.len() == cells.len()
}
