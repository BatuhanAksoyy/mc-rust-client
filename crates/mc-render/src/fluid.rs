//! Fluid (water/lava) surface geometry (`docs/RENDER.md` milestone 3's
//! fluid gap).
//!
//! Vanilla renders fluids with no resource-pack blockstate/model JSON at all
//! (`atlas::model`'s whole blockstate→model→element pipeline doesn't apply
//! here — there simply is no `blockstates/water.json`); fluid rendering is
//! Java-internal logic, not resource-pack data. This instead hand-implements
//! the fluid-height and corner-blending convention long publicly documented
//! and independently reimplemented by countless non-Mojang tools (wiki
//! articles, `prismarine-js`, other clean-room voxel engines) — never
//! decompiled source.
//!
//! The pure per-block math lives here, unit-tested without a `Chunk`;
//! `mesh.rs` supplies the actual neighbor lookups and turns the result into
//! `Vertex`es.

use mc_world::BlockState;

use crate::mesh::Vertex;

/// Which fluid a resolved block state is, if it's a fluid at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FluidKind {
    /// Tinted by `BlockRegistry::color` (its texture is grayscale, meant to
    /// be multiplied by a color — a biome average in Java, a single flat
    /// curated blue here, same simplification `named_color` already applies
    /// to grass/foliage).
    Water,
    /// Never tinted — its texture already carries real orange/red color.
    Lava,
}

impl FluidKind {
    /// Classify a resolved block state by name; `None` for anything that
    /// isn't water or lava.
    #[must_use]
    pub fn of(state: &BlockState) -> Option<Self> {
        match state.name.as_ref() {
            "minecraft:water" => Some(Self::Water),
            "minecraft:lava" => Some(Self::Lava),
            _ => None,
        }
    }

    /// Whether this fluid's texture needs `BlockRegistry`'s tint multiply.
    #[must_use]
    pub const fn tinted(self) -> bool {
        matches!(self, Self::Water)
    }

    /// Whether this fluid needs the depth-write-off translucent pass
    /// (`Mesh::translucent`): water's texture has real, non-cutout alpha
    /// (~0.7 everywhere); lava's is fully opaque (255), so it renders
    /// correctly through the ordinary opaque/cutout pass instead.
    #[must_use]
    pub const fn translucent(self) -> bool {
        matches!(self, Self::Water)
    }
}

/// A fluid state's `level` property (`"0"`..`"15"`).
///
/// 0 is a source, 1-7 are increasingly-empty flowing levels, 8-15 are
/// falling columns (the low 3 bits mirror the source feeding them; only
/// whether it's `>= 8` matters for rendering). Absent or unparseable
/// defaults to a source (0) — the safer default, same reasoning as
/// `BlockRegistry::is_air`: render a real, full fluid block rather than
/// guess at a sliver.
#[must_use]
pub fn level(state: &BlockState) -> u8 {
    state.properties.get("level").and_then(|value| value.parse().ok()).unwrap_or(0)
}

/// This level's own surface height (0..1), ignoring neighbors.
///
/// A source (`level == 0`) sits at 8/9 — vanilla's long-observed quirk that
/// a fluid source's top isn't quite flush with the block's own top face —
/// each flowing level below that is a further 1/9 shorter, and a falling
/// column (`level >= 8`, actively flowing straight down through this block)
/// is a full cube.
#[must_use]
pub fn own_height(level: u8) -> f32 {
    if level >= 8 { 1.0 } else { f32::from(8 - level) / 9.0 }
}

/// One of a fluid block's 4 top corners.
///
/// `cells` is the up-to-4 cells that meet at that corner (this block, the
/// two orthogonal neighbors in the corner's quadrant, and the diagonal
/// one), each `Some((level, fluid_above))` when that cell holds the *same*
/// fluid — a different block, air, or an unresolved chunk-edge cell
/// contributes nothing, same as `mesh_chunk`'s own face culling only ever
/// consulting a real neighbor — and `fluid_above` records whether that same
/// fluid also fills the cell directly above it.
///
/// A corner with any contributing cell that has fluid above it sits flush
/// with the block's top: there's no visible surface there at all (the fluid
/// continues upward, or this is the middle of a waterfall). Otherwise the
/// corner is the average `own_height` of however many cells actually
/// contributed (at least the block asking, in real use, so this never
/// divides by zero).
#[must_use]
#[allow(clippy::cast_precision_loss)] // `cells` has 4 entries; the count fits exactly in f32.
pub fn corner_height(cells: [Option<(u8, bool)>; 4]) -> f32 {
    let mut any_above = false;
    let mut total = 0.0_f32;
    let mut count: u32 = 0;
    for (own_level, above) in cells.into_iter().flatten() {
        any_above |= above;
        total += own_height(own_level);
        count += 1;
    }
    if any_above || count == 0 { 1.0 } else { total / count as f32 }
}

/// The block's 4 top-corner heights, in `(nw, ne, se, sw)` order (`x`/`z`
/// each 0 or 1 — `mesh.rs`'s own north/south/east/west = -z/+z/+x/-x
/// convention).
#[derive(Debug, Clone, Copy)]
pub struct Corners {
    /// North-west corner (`x=0, z=0`) height.
    pub nw: f32,
    /// North-east corner (`x=1, z=0`) height.
    pub ne: f32,
    /// South-east corner (`x=1, z=1`) height.
    pub se: f32,
    /// South-west corner (`x=0, z=1`) height.
    pub sw: f32,
}

/// Whether each of the block's 6 faces should render.
///
/// Follows the same "only a real, fully-covering occluder hides a face"
/// rule as solid blocks (`mesh::occludes`) — with one fluid-only exception:
/// a neighbor of the *same* fluid never gets a face either, since there's no
/// visible boundary between two parts of one continuous body of water/lava.
#[derive(Debug, Clone, Copy)]
#[allow(clippy::struct_excessive_bools)] // Six independent per-face flags, not a state machine.
pub struct Faces {
    /// Top face (sloped to `Corners`).
    pub up: bool,
    /// Bottom face (always flat).
    pub down: bool,
    /// `-z` face.
    pub north: bool,
    /// `+z` face.
    pub south: bool,
    /// `+x` face.
    pub east: bool,
    /// `-x` face.
    pub west: bool,
}

/// Emit this fluid block's visible faces as triangles into `vertices`.
///
/// Follows `corners`' sloped top and `faces`' visibility. The bottom face is
/// always flat (vanilla never slopes it); the four side faces follow the
/// top's slope on their shared edge and stay flat (`y = 0`) at the bottom.
#[allow(clippy::too_many_arguments)]
pub fn push_fluid_block(
    vertices: &mut Vec<Vertex>,
    block: [f32; 3],
    corners: Corners,
    faces: Faces,
    uv_rect: [f32; 4],
    tint: [f32; 3],
) {
    // Keep fluid surfaces just inside their cell. Adjacent block faces live
    // exactly on integer boundaries; sharing that plane makes the depth
    // winner unstable and produces dirt/water triangles while moving. This
    // also follows the clean-room-observed boundary bias in Java 26.2's
    // `net.minecraft.client.renderer.block.FluidRenderer#tesselate`.
    const INSET: f32 = 0.001;
    let Corners { nw, ne, se, sw } = corners;
    let [nw, ne, se, sw] = [nw, ne, se, sw].map(|height| (height - INSET).max(0.0));
    let bottom = if faces.down { INSET } else { 0.0 };
    let [u0, v0, u1, v1] = uv_rect;
    let uv = [[u0, v1], [u1, v1], [u1, v0], [u0, v0]];
    let mut push = |positions: [[f32; 3]; 4], brightness: f32| {
        let shaded = tint.map(|channel| channel * brightness);
        let world = positions.map(|[x, y, z]| [block[0] + x, block[1] + y, block[2] + z]);
        for &index in &[0, 1, 2, 0, 2, 3] {
            vertices.push(Vertex { position: world[index], uv: uv[index], tint: shaded });
        }
    };
    if faces.up {
        push([[0.0, nw, 0.0], [0.0, sw, 1.0], [1.0, se, 1.0], [1.0, ne, 0.0]], 1.0);
    }
    if faces.down {
        push([[0.0, bottom, 0.0], [1.0, bottom, 0.0], [1.0, bottom, 1.0], [0.0, bottom, 1.0]], 0.4);
    }
    if faces.north {
        push([[0.0, bottom, INSET], [0.0, nw, INSET], [1.0, ne, INSET], [1.0, bottom, INSET]], 0.8);
    }
    if faces.south {
        push(
            [
                [0.0, bottom, 1.0 - INSET],
                [1.0, bottom, 1.0 - INSET],
                [1.0, se, 1.0 - INSET],
                [0.0, sw, 1.0 - INSET],
            ],
            0.8,
        );
    }
    if faces.east {
        push(
            [
                [1.0 - INSET, bottom, 0.0],
                [1.0 - INSET, ne, 0.0],
                [1.0 - INSET, se, 1.0],
                [1.0 - INSET, bottom, 1.0],
            ],
            0.6,
        );
    }
    if faces.west {
        push([[INSET, bottom, 0.0], [INSET, bottom, 1.0], [INSET, sw, 1.0], [INSET, nw, 0.0]], 0.6);
    }
}

#[cfg(test)]
mod tests {
    use super::{FluidKind, corner_height, level, own_height};
    use mc_world::BlockState;
    use std::collections::BTreeMap;

    fn state(name: &str, level: &str) -> BlockState {
        let mut properties = BTreeMap::new();
        properties.insert("level".into(), level.into());
        BlockState { name: name.into(), properties }
    }

    #[test]
    fn of_classifies_water_and_lava_by_name_only() {
        assert_eq!(FluidKind::of(&state("minecraft:water", "0")), Some(FluidKind::Water));
        assert_eq!(FluidKind::of(&state("minecraft:lava", "0")), Some(FluidKind::Lava));
        assert_eq!(FluidKind::of(&state("minecraft:stone", "0")), None);
    }

    #[test]
    fn only_water_is_tinted() {
        assert!(FluidKind::Water.tinted());
        assert!(!FluidKind::Lava.tinted());
    }

    #[test]
    fn level_reads_the_property_and_defaults_to_a_source() {
        assert_eq!(level(&state("minecraft:water", "7")), 7);
        let sourceless = BlockState { name: "minecraft:water".into(), properties: BTreeMap::new() };
        assert_eq!(level(&sourceless), 0);
    }

    #[test]
    #[allow(clippy::float_cmp)] // Every value here is an exact small-integer fraction of 9.
    fn own_height_matches_the_source_flowing_falling_convention() {
        assert_eq!(own_height(0), 8.0 / 9.0); // Source: not quite flush with the top.
        assert_eq!(own_height(1), 7.0 / 9.0);
        assert_eq!(own_height(7), 1.0 / 9.0); // Nearly empty.
        assert_eq!(own_height(8), 1.0); // Falling: full cube.
        assert_eq!(own_height(15), 1.0);
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn corner_height_averages_only_contributing_cells() {
        // Just the block itself (source, no neighbors of the same fluid).
        assert_eq!(corner_height([Some((0, false)), None, None, None]), 8.0 / 9.0);
        // Two same-fluid cells at different levels average their own heights.
        let mixed = corner_height([Some((0, false)), Some((4, false)), None, None]);
        assert_eq!(mixed, f32::midpoint(8.0 / 9.0, 4.0 / 9.0));
    }

    #[test]
    #[allow(clippy::float_cmp)] // 1.0 is exact, not an accumulated computation.
    fn corner_height_is_flush_when_any_contributor_has_fluid_above() {
        // A nearly-empty cell that's fed from above is *not* sloped down to
        // its own low height — the real surface is higher up, or this is
        // mid-waterfall, either way not a visible slope at this corner.
        assert_eq!(corner_height([Some((7, true)), None, None, None]), 1.0);
    }

    #[test]
    #[allow(clippy::float_cmp)] // 1.0 is exact, not an accumulated computation.
    fn corner_height_defaults_to_full_with_no_contributing_cells() {
        // Shouldn't happen in real use (the block itself always contributes),
        // but a corner with nothing to average from should never divide by
        // zero or render as an invisible zero-height sliver.
        assert_eq!(corner_height([None, None, None, None]), 1.0);
    }
}
