//! Dense, render-ready block storage converted from a decoded network chunk.

use crate::ChunkPos;
use mc_protocol::chunk::LevelChunk;

/// One chunk's blocks as a dense, randomly indexable array.
///
/// `mc_protocol::chunk::LevelChunk` keeps each section's paletted containers
/// as received; this resolves every entry once, here, rather than on every
/// lookup a mesher makes.
#[derive(Debug, Clone)]
pub struct Chunk {
    /// Position in chunk coordinates.
    pub position: ChunkPos,
    /// Bottom-to-top, matching wire order (`docs/JOIN.md`); 4096 entries
    /// each, indexed by `(y * 16 + z) * 16 + x` within the section.
    sections: Vec<[u32; 4096]>,
}

impl Chunk {
    /// Resolve every section's block-states container into a dense array.
    #[must_use]
    pub fn from_level(level: &LevelChunk) -> Self {
        let sections = level
            .sections
            .iter()
            .map(|section| {
                let mut blocks = [0_u32; 4096];
                for (index, block) in blocks.iter_mut().enumerate() {
                    // A resolved chunk always has valid indices (`chunk::decode`
                    // rejects an out-of-range one); `unwrap_or(0)` only guards
                    // the type conversion, not a real failure mode.
                    *block = section
                        .block_states
                        .get(index)
                        .and_then(|id| u32::try_from(id).ok())
                        .unwrap_or(0);
                }
                blocks
            })
            .collect();
        Self { position: ChunkPos { x: level.x, z: level.z }, sections }
    }

    /// Number of 16-block-tall sections, bottom-to-top.
    #[must_use]
    pub const fn section_count(&self) -> usize {
        self.sections.len()
    }

    /// The block-state ID at `(x, y, z)`, relative to this chunk's own
    /// origin (`x`/`z` in `0..16`; `y` 0 is the first decoded section's
    /// bottom layer — this client does not yet resolve absolute world
    /// height, see `docs/JOIN.md`). Out-of-range coordinates return `None`.
    #[must_use]
    pub fn block_at(&self, x: i32, y: i32, z: i32) -> Option<u32> {
        if !(0..16).contains(&x) || !(0..16).contains(&z) || y < 0 {
            return None;
        }
        let section = self.sections.get(usize::try_from(y / 16).ok()?)?;
        let local_y = y % 16;
        // Wiki Chunk format § Data Array format: x increases fastest, then z, then y.
        let index = usize::try_from((local_y * 16 + z) * 16 + x).ok()?;
        section.get(index).copied()
    }

    /// Replace the block-state ID at chunk-local `(x, y, z)`.
    ///
    /// Returns `false` without changing the chunk when the coordinate is out
    /// of its decoded bounds. The client-side fluid fixture uses this seam;
    /// authoritative gameplay updates will eventually apply through the same
    /// dense storage after decoding server block-change packets.
    pub fn set_block(&mut self, x: i32, y: i32, z: i32, id: u32) -> bool {
        if !(0..16).contains(&x) || !(0..16).contains(&z) || y < 0 {
            return false;
        }
        let Some(section) =
            usize::try_from(y / 16).ok().and_then(|section| self.sections.get_mut(section))
        else {
            return false;
        };
        let local_y = y % 16;
        let Ok(index) = usize::try_from((local_y * 16 + z) * 16 + x) else {
            return false;
        };
        let Some(block) = section.get_mut(index) else { return false };
        *block = id;
        true
    }
}
