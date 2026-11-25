//! Board topology: hex tiles in axial coordinates whose corners (vertices)
//! and sides (edges) are shared between neighbours, each with a stable id.
//!
//! Positions use an integer lattice for pointy-top hexes: a tile at axial
//! `(q, r)` has its centre at `(2q + r, 3r)`, where one x unit is half a hex's
//! width and one y unit is half its side. Corners land on exact integers, so
//! shared corners deduplicate without floating point.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    balance,
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
    pub resource: Resource,
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
        let mut coords = Vec::new();
        for q in -radius..=radius {
            for r in (-radius).max(-q - radius)..=radius.min(-q + radius) {
                coords.push((q, r));
            }
        }
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

    /// A playable board: the hexagon of [`balance::BOARD_RADIUS`] with the
    /// resources of [`balance::TILE_RESOURCES`] shuffled onto it and the
    /// numbers of [`balance::PRODUCTION_NUMBERS`] placed so no two hot
    /// numbers touch and no tile shares its number with a neighbour.
    pub fn generate(rng: &mut Rng) -> Board {
        let mut board = Board::hexagon(balance::BOARD_RADIUS);
        let mut resources: Vec<Resource> =
            balance::TILE_RESOURCES.iter().flat_map(|&(r, n)| std::iter::repeat_n(r, n)).collect();
        assert_eq!(resources.len(), board.tiles.len(), "TILE_RESOURCES must cover every tile");
        assert_eq!(balance::PRODUCTION_NUMBERS.len(), board.tiles.len(), "PRODUCTION_NUMBERS must cover every tile");
        rng.shuffle(&mut resources);
        for (tile, resource) in board.tiles.iter_mut().zip(resources) {
            tile.resource = resource;
        }
        let mut numbers = balance::PRODUCTION_NUMBERS.to_vec();
        loop {
            rng.shuffle(&mut numbers);
            for (tile, &n) in board.tiles.iter_mut().zip(&numbers) {
                tile.number = n;
            }
            if board.numbers_balanced() {
                break;
            }
        }
        board
    }

    /// No hot numbers side by side, and no neighbours sharing a number.
    pub fn numbers_balanced(&self) -> bool {
        self.tiles.iter().all(|t| {
            t.neighbors.iter().all(|&n| {
                let other = self.tile(n).number;
                other != t.number
                    && !(balance::HOT_NUMBERS.contains(&t.number) && balance::HOT_NUMBERS.contains(&other))
            })
        })
    }

    pub fn tile(&self, id: TileId) -> &Tile {
        &self.tiles[id.index()]
    }

    pub fn vertex(&self, id: VertexId) -> &Vertex {
        &self.vertices[id.index()]
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
