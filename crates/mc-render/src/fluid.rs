//! Fluid (water/lava) surface geometry (`docs/RENDER.md` milestone 3's
//! fluid gap).
//!
//! Vanilla renders fluids with no resource-pack blockstate/model JSON at all
//! (`atlas::model`'s whole blockstate→model→element pipeline doesn't apply
//! here — there simply is no `blockstates/water.json`); fluid rendering is
//! Java-internal logic, not resource-pack data. This instead hand-implements
//! the fluid-height and corner-blending convention long publicly documented
//! and independently reimplemented by non-Mojang tools. Exact observable
//! behavior is clean-room cross-checked against the 26.2 symbols named in
//! `docs/RENDER.md`; no Mojang code or mappings are copied here.
//!
//! The pure per-block math lives here, unit-tested without a `Chunk`;
//! `mesh.rs` supplies the actual neighbor lookups and turns the result into
//! `Vertex`es.

use mc_world::BlockState;

use crate::{atlas::FluidUvs, mesh::Vertex};

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

/// Blend the four cell heights meeting at one fluid-surface corner.
///
/// `self_height` is always this fluid cell. Each side/diagonal is `Some(0)`
/// for replaceable empty space, `Some(height)` for the same fluid, or `None`
/// for a solid/missing cell. Near-full samples receive extra weight so a
/// source surface stays broad and only rolls down near its edge. The diagonal
/// participates only when at least one adjoining side contains fluid.
#[must_use]
pub fn corner_height(
    self_height: f32,
    side_a: Option<f32>,
    side_b: Option<f32>,
    diagonal: Option<f32>,
) -> f32 {
    if [side_a, side_b].into_iter().flatten().any(|height| height >= 1.0) {
        return 1.0;
    }
    let mut weighted_height = 0.0;
    let mut weight = 0.0;
    let mut add = |height: f32| {
        if height < 0.0 {
            return;
        }
        let sample_weight = if height >= 0.8 { 10.0 } else { 1.0 };
        weighted_height = height.mul_add(sample_weight, weighted_height);
        weight += sample_weight;
    };
    if side_a.is_some_and(|height| height > 0.0) || side_b.is_some_and(|height| height > 0.0) {
        if diagonal.is_some_and(|height| height >= 1.0) {
            return 1.0;
        }
        if let Some(height) = diagonal {
            add(height);
        }
    }
    add(self_height);
    for height in [side_a, side_b].into_iter().flatten() {
        add(height);
    }
    if weight == 0.0 { 1.0 } else { weighted_height / weight }
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

/// Normalize an accumulated horizontal fluid-flow vector, or return `None`
/// when it has no horizontal component and the top should use the still
/// sprite.
#[must_use]
pub fn flow_direction(x: f32, z: f32) -> Option<[f32; 2]> {
    let length = x.hypot(z);
    (length > 1e-6).then_some([x / length, z / length])
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
    flow: Option<[f32; 2]>,
    uvs: FluidUvs,
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
    let map_uv = |rect: [f32; 4], [u, v]: [f32; 2]| {
        let [u0, v0, u1, v1] = rect;
        [u.mul_add(u1 - u0, u0), v.mul_add(v1 - v0, v0)]
    };
    let still = [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]].map(|uv| map_uv(uvs.still, uv));
    let top_uv = if let Some([flow_x, flow_z]) = flow {
        let angle = flow_z.atan2(flow_x) - std::f32::consts::FRAC_PI_2;
        let (sin, cos) = angle.sin_cos();
        let (s, c) = (sin * 0.25, cos * 0.25);
        [
            [0.5 - c - s, 0.5 - c + s],
            [0.5 - c + s, 0.5 + c + s],
            [0.5 + c + s, 0.5 + c - s],
            [0.5 + c - s, 0.5 - c - s],
        ]
        .map(|uv| map_uv(uvs.flowing, uv))
    } else {
        still
    };
    let side_uv = |left_height: f32, right_height: f32| {
        [
            map_uv(uvs.flowing, [0.0, (1.0 - left_height) * 0.5]),
            map_uv(uvs.flowing, [0.5, (1.0 - right_height) * 0.5]),
            map_uv(uvs.flowing, [0.5, 0.5]),
            map_uv(uvs.flowing, [0.0, 0.5]),
        ]
    };
    let mut push = |positions: [[f32; 3]; 4], uv: [[f32; 2]; 4], brightness: f32| {
        let shaded = tint.map(|channel| channel * brightness);
        let world = positions.map(|[x, y, z]| [block[0] + x, block[1] + y, block[2] + z]);
        for &index in &[0, 1, 2, 0, 2, 3] {
            vertices.push(Vertex { position: world[index], uv: uv[index], tint: shaded });
        }
    };
    if faces.up {
        push([[0.0, nw, 0.0], [0.0, sw, 1.0], [1.0, se, 1.0], [1.0, ne, 0.0]], top_uv, 1.0);
    }
    if faces.down {
        push(
            [[0.0, bottom, 0.0], [1.0, bottom, 0.0], [1.0, bottom, 1.0], [0.0, bottom, 1.0]],
            still,
            0.4,
        );
    }
    if faces.north {
        push(
            [[0.0, nw, INSET], [1.0, ne, INSET], [1.0, bottom, INSET], [0.0, bottom, INSET]],
            side_uv(nw, ne),
            0.8,
        );
    }
    if faces.south {
        push(
            [
                [1.0, se, 1.0 - INSET],
                [0.0, sw, 1.0 - INSET],
                [0.0, bottom, 1.0 - INSET],
                [1.0, bottom, 1.0 - INSET],
            ],
            side_uv(se, sw),
            0.8,
        );
    }
    if faces.east {
        push(
            [
                [1.0 - INSET, ne, 0.0],
                [1.0 - INSET, se, 1.0],
                [1.0 - INSET, bottom, 1.0],
                [1.0 - INSET, bottom, 0.0],
            ],
            side_uv(ne, se),
            0.6,
        );
    }
    if faces.west {
        push(
            [[INSET, sw, 1.0], [INSET, nw, 0.0], [INSET, bottom, 0.0], [INSET, bottom, 1.0]],
            side_uv(sw, nw),
            0.6,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Corners, Faces, FluidKind, corner_height, flow_direction, level, own_height,
        push_fluid_block,
    };
    use crate::atlas::FluidUvs;
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
    fn corner_height_weights_near_full_fluid_and_classifies_neighbors() {
        let close = |actual: f32, expected: f32| assert!((actual - expected).abs() < 1e-6);
        let source = 8.0 / 9.0;
        close(corner_height(source, None, None, None), source);
        // The source receives weight 10; the lower neighbor receives 1.
        close(corner_height(source, Some(4.0 / 9.0), None, None), 28.0 / 33.0);
        // Replaceable empty space contributes zero; solid space contributes nothing.
        close(corner_height(source, Some(0.0), None, None), 80.0 / 99.0);
        close(corner_height(source, None, None, None), source);
    }

    #[test]
    #[allow(clippy::float_cmp)] // 1.0 is exact, not an accumulated computation.
    fn corner_height_is_flush_when_an_adjacent_column_has_fluid_above() {
        assert_eq!(corner_height(1.0 / 9.0, Some(1.0), None, None), 1.0);
        assert_eq!(corner_height(1.0 / 9.0, Some(0.5), None, Some(1.0)), 1.0);
    }

    #[test]
    fn diagonal_only_contributes_when_reachable_from_a_fluid_side() {
        let close = |actual: f32, expected: f32| assert!((actual - expected).abs() < 1e-6);
        let self_height = 4.0 / 9.0;
        close(corner_height(self_height, None, None, Some(8.0 / 9.0)), self_height);
        close(corner_height(self_height, Some(2.0 / 9.0), None, Some(8.0 / 9.0)), 86.0 / 108.0);
    }

    #[test]
    fn horizontal_flow_is_normalized_and_zero_flow_is_still() {
        assert_eq!(flow_direction(0.0, 0.0), None);
        let [x, z] = flow_direction(2.0, 0.0).unwrap();
        assert!((x - 1.0).abs() < 1e-6);
        assert!(z.abs() < 1e-6);
    }

    #[test]
    fn level_top_uses_still_uvs_and_sloped_top_uses_flowing_uvs() {
        let uvs = FluidUvs { still: [0.0, 0.0, 0.25, 0.25], flowing: [0.5, 0.5, 1.0, 1.0] };
        let faces =
            Faces { up: true, down: false, north: false, south: false, east: false, west: false };
        let mut vertices = Vec::new();
        push_fluid_block(
            &mut vertices,
            [0.0; 3],
            Corners { nw: 0.5, ne: 0.5, se: 0.5, sw: 0.5 },
            faces,
            None,
            uvs,
            [1.0; 3],
        );
        assert!(vertices.iter().all(|vertex| vertex.uv[0] <= 0.25 && vertex.uv[1] <= 0.25));

        vertices.clear();
        push_fluid_block(
            &mut vertices,
            [0.0; 3],
            Corners { nw: 1.0, sw: 1.0, ne: 0.5, se: 0.5 },
            faces,
            Some([1.0, 0.0]),
            uvs,
            [1.0; 3],
        );
        assert!(vertices.iter().all(|vertex| vertex.uv[0] >= 0.5 && vertex.uv[1] >= 0.5));
    }
}
