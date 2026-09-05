// SPDX-License-Identifier: MIT OR Apache-2.0
//! World data structures. See `docs/WORLD_PHYSICS_ASSETS.md`.

/// Chunk position in chunk coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkPos {
    pub x: i32,
    pub z: i32,
}
