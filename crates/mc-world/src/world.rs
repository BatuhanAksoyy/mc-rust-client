//! Immutable index over the chunks currently loaded by the client.

use std::collections::HashMap;

use crate::{Chunk, ChunkPos};

/// A loaded chunk view addressed in block coordinates relative to `origin`.
///
/// Keeping the render origin out of each physics query lets the player use
/// small, precise coordinates while still crossing chunk boundaries.
#[derive(Debug, Clone)]
pub struct World {
    origin: ChunkPos,
    chunks: HashMap<ChunkPos, Chunk>,
}

impl World {
    /// Build an immutable lookup table for a received chunk batch.
    #[must_use]
    pub fn new(origin: ChunkPos, chunks: impl IntoIterator<Item = Chunk>) -> Self {
        Self { origin, chunks: chunks.into_iter().map(|chunk| (chunk.position, chunk)).collect() }
    }

    /// Return a state ID at coordinates relative to the origin chunk.
    /// Missing chunks and out-of-height positions return `None`.
    #[must_use]
    pub fn block_at(&self, x: i32, y: i32, z: i32) -> Option<u32> {
        let position = ChunkPos {
            x: self.origin.x.checked_add(x.div_euclid(16))?,
            z: self.origin.z.checked_add(z.div_euclid(16))?,
        };
        self.chunks.get(&position)?.block_at(x.rem_euclid(16), y, z.rem_euclid(16))
    }

    /// The chunk chosen as coordinate origin for this view.
    #[must_use]
    pub const fn origin(&self) -> ChunkPos {
        self.origin
    }
}
