//! Chunk → vertex buffer, culling internal faces.
//!
//! No greedy meshing yet (`docs/RENDER.md`): one quad per visible face,
//! textured from `atlas` where a block resolved a real texture, or tinted
//! `registry` debug color on the atlas's reserved white texel otherwise.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use mc_world::{BlockRegistry, Chunk, ChunkPos};

use crate::atlas::{Atlas, BakedQuad};
use crate::fluid::{self, FluidKind};

/// One mesh vertex: chunk-local position, an atlas UV, and a pre-shaded tint.
///
/// The tint is a flat debug color for untextured blocks, or white for
/// textured ones — either way multiplied by a fixed per-face brightness, a
/// cheap stand-in for real lighting until block/sky light data is wired in.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    /// Chunk-local block-space position (not yet offset by chunk X/Z).
    pub position: [f32; 3],
    /// Atlas texture coordinates, `[0, 1]` normalized.
    pub uv: [f32; 2],
    /// Linear RGB, already shaded.
    pub tint: [f32; 3],
}

impl Vertex {
    /// `wgpu` vertex-buffer attribute layout matching this struct's fields.
    pub const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x3];
}

/// Triangle-list vertex data in block coordinates relative to a nearby chunk origin.
#[derive(Debug, Clone, Default)]
pub struct Mesh {
    /// Non-indexed triangle list (6 vertices per visible face): everything
    /// opaque or cutout-alpha-tested (real texture or debug color alike),
    /// lava included — its texture has no real alpha variation, so it draws
    /// correctly through the same depth-write-on pass as everything else.
    pub vertices: Vec<Vertex>,
    /// Water's own non-indexed triangle list, drawn in a separate pass with
    /// depth writes off (`Renderer`): water's alpha is real (baked into
    /// `water_still`, ~0.7, not just cutout on/off), so drawing it through
    /// the same depth-write-on pass as everything else let one water quad's
    /// write block another translucent surface's blend behind it — visible
    /// as noisy, moiré-like overdraw exactly where several water quads
    /// overlap in screen space (a shoreline's many differently-sloped
    /// blocks, an underwater drop-off's stacked side faces).
    pub translucent: Vec<Vertex>,
}

/// `(dx, dy, dz, brightness)` per face: a cheap directional-light stand-in,
/// matching vanilla's classic per-face shading (top brightest, bottom darkest).
const FACES: [(i32, i32, i32, f32); 6] = [
    (0, 1, 0, 1.0),
    (0, -1, 0, 0.4),
    (0, 0, 1, 0.8),
    (0, 0, -1, 0.8),
    (1, 0, 0, 0.6),
    (-1, 0, 0, 0.6),
];

/// Mesh every visible face of every non-air block in `chunk`.
///
/// A face is visible when its neighbor is missing (chunk edge/top/bottom —
/// there is no neighbor chunk to consult yet) or doesn't `occludes` it —
/// air, same as ever, but also any walk-through decoration (a cross-shaped
/// plant standing on a block is non-air, but nowhere near covering that
/// block's top face) and any solid-but-transparent block (leaves' cutout
/// gaps, glass) that fails to fully cover the face even though it's a real,
/// solid full cube (`RENDER.md` milestone 3). Either one culled away like a
/// real occluder just exposes a hole down into, or a solid-color patch over,
/// whatever actually sat behind it.
#[must_use]
pub fn mesh_chunk(chunk: &Chunk, registry: &BlockRegistry, atlas: &Atlas) -> Mesh {
    mesh_chunks(std::slice::from_ref(chunk), chunk.position, registry, atlas)
}

/// Mesh a loaded chunk batch into one scene, translating chunk coordinates
/// relative to `origin` and culling faces shared across chunk boundaries.
///
/// Keeping the origin near the player avoids feeding large absolute world
/// coordinates to the GPU while preserving each chunk's spatial relationship.
#[must_use]
#[allow(clippy::cast_precision_loss)]
// Local x/z are 0..16 and decoded chunk height is bounded to 6,144 blocks.
pub fn mesh_chunks(
    chunks: &[Chunk],
    origin: ChunkPos,
    registry: &BlockRegistry,
    atlas: &Atlas,
) -> Mesh {
    let mut vertices = Vec::new();
    let mut translucent = Vec::new();
    let by_position: HashMap<ChunkPos, &Chunk> =
        chunks.iter().map(|chunk| (chunk.position, chunk)).collect();
    for chunk in chunks {
        let Ok(height) = i32::try_from(chunk.section_count() * 16) else { continue };
        let [offset_x, offset_z] = relative_offset(chunk.position, origin);
        for y in 0..height {
            for z in 0..16 {
                for x in 0..16 {
                    let Some(id) = chunk.block_at(x, y, z) else { continue };
                    if registry.is_air(id) {
                        continue;
                    }
                    let block = [offset_x + x as f32, y as f32, offset_z + z as f32];
                    if let Some(kind) = registry.state(id).and_then(FluidKind::of)
                        && let Some(uv) = atlas.fluid_uv(kind)
                    {
                        let tint = if kind.tinted() { registry.color(id) } else { [1.0; 3] };
                        let target =
                            if kind.translucent() { &mut translucent } else { &mut vertices };
                        mesh_fluid_block(
                            target,
                            &by_position,
                            chunk,
                            registry,
                            atlas,
                            kind,
                            x,
                            y,
                            z,
                            block,
                            uv,
                            tint,
                        );
                        continue;
                        // Else: no resource pack has `water_still`/
                        // `lava_still` (an absent asset cache, `RENDER.md`
                        // milestone 3) — fall through to the debug-color
                        // path below, same as any other unresolved block.
                    }
                    if let Some(model) = atlas.lookup(id) {
                        for quad in &model.quads {
                            let visible = quad.cull.is_none_or(|(dx, dy, dz)| {
                                neighbor_block(&by_position, chunk, x + dx, y + dy, z + dz)
                                    .is_none_or(|neighbor| {
                                        !occludes(registry, atlas, neighbor, (-dx, -dy, -dz))
                                    })
                            });
                            if visible {
                                push_baked_quad(&mut vertices, block, quad, registry.color(id));
                            }
                        }
                        continue;
                    }
                    for &(dx, dy, dz, brightness) in &FACES {
                        let visible = neighbor_block(&by_position, chunk, x + dx, y + dy, z + dz)
                            .is_none_or(|neighbor| {
                                !occludes(registry, atlas, neighbor, (-dx, -dy, -dz))
                            });
                        if visible {
                            push_face(
                                &mut vertices,
                                block,
                                (dx, dy, dz),
                                atlas.white_uv(),
                                uv_corners((dx, dy, dz)),
                                registry.color(id),
                                brightness,
                            );
                        }
                    }
                }
            }
        }
    }
    Mesh { vertices, translucent }
}

/// Mesh one fluid block: sample the up-to-9 same-fluid neighbors `fluid`'s
/// corner-blending and face-visibility rules need, then hand the results to
/// `fluid::push_fluid_block`.
#[allow(clippy::too_many_arguments)]
fn mesh_fluid_block(
    vertices: &mut Vec<Vertex>,
    by_position: &HashMap<ChunkPos, &Chunk>,
    chunk: &Chunk,
    registry: &BlockRegistry,
    atlas: &Atlas,
    kind: FluidKind,
    x: i32,
    y: i32,
    z: i32,
    block: [f32; 3],
    uvs: crate::atlas::FluidUvs,
    tint: [f32; 3],
) {
    let is_same = |dx: i32, dy: i32, dz: i32| -> bool {
        neighbor_block(by_position, chunk, x + dx, y + dy, z + dz)
            .and_then(|id| registry.state(id))
            .is_some_and(|state| FluidKind::of(state) == Some(kind))
    };
    // Java's corner blend distinguishes a same-fluid height, replaceable
    // empty space (zero), and solid/missing space (no contribution).
    let height = |dx: i32, dz: i32| -> Option<f32> {
        let id = neighbor_block(by_position, chunk, x + dx, y, z + dz)?;
        let state = registry.state(id)?;
        if FluidKind::of(state) == Some(kind) {
            Some(if is_same(dx, 1, dz) { 1.0 } else { fluid::own_height(fluid::level(state)) })
        } else if registry.is_solid(id) {
            None
        } else {
            Some(0.0)
        }
    };
    let self_height = height(0, 0).unwrap_or(1.0);
    let corners = fluid::Corners {
        nw: fluid::corner_height(self_height, height(-1, 0), height(0, -1), height(-1, -1)),
        ne: fluid::corner_height(self_height, height(1, 0), height(0, -1), height(1, -1)),
        se: fluid::corner_height(self_height, height(1, 0), height(0, 1), height(1, 1)),
        sw: fluid::corner_height(self_height, height(-1, 0), height(0, 1), height(-1, 1)),
    };
    // Accumulate the horizontal surface flow from the four cardinal cells.
    // Empty space only contributes when this fluid continues one block below
    // it; solid blocks and different fluids do not affect the vector.
    let own_height = neighbor_block(by_position, chunk, x, y, z)
        .and_then(|id| registry.state(id))
        .map_or(1.0, |state| fluid::own_height(fluid::level(state)));
    let mut flow_x = 0.0;
    let mut flow_z = 0.0;
    for (dx, dz, step_x, step_z) in
        [(0, -1, 0.0_f32, -1.0_f32), (0, 1, 0.0, 1.0), (-1, 0, -1.0, 0.0), (1, 0, 1.0, 0.0)]
    {
        let Some(neighbor) = neighbor_block(by_position, chunk, x + dx, y, z + dz) else {
            continue;
        };
        let Some(state) = registry.state(neighbor) else { continue };
        let neighbor_kind = FluidKind::of(state);
        let distance = if neighbor_kind == Some(kind) {
            own_height - fluid::own_height(fluid::level(state))
        } else if neighbor_kind.is_none() && !registry.is_solid(neighbor) {
            neighbor_block(by_position, chunk, x + dx, y - 1, z + dz)
                .and_then(|id| registry.state(id))
                .filter(|state| FluidKind::of(state) == Some(kind))
                .map_or(0.0, |state| {
                    own_height - (fluid::own_height(fluid::level(state)) - 8.0 / 9.0)
                })
        } else {
            0.0
        };
        flow_x = step_x.mul_add(distance, flow_x);
        flow_z = step_z.mul_add(distance, flow_z);
    }
    let flow = fluid::flow_direction(flow_x, flow_z);
    // A face is visible against a real, fully-covering occluder just like a
    // solid block's own faces (`occludes`) — with one fluid-only addition:
    // never against the *same* fluid either, since two parts of one
    // continuous body of water/lava share no visible boundary.
    let visible = |dx: i32, dy: i32, dz: i32| -> bool {
        if is_same(dx, dy, dz) {
            return false;
        }
        neighbor_block(by_position, chunk, x + dx, y + dy, z + dz)
            .is_none_or(|neighbor| !occludes(registry, atlas, neighbor, (-dx, -dy, -dz)))
    };
    let minimum_top = corners.nw.min(corners.ne).min(corners.se).min(corners.sw);
    let faces = fluid::Faces {
        // A full block above only hides a flush surface. Source water's
        // 8/9-height top remains visible in the small gap below that block.
        up: !is_same(0, 1, 0) && (minimum_top < 1.0 || visible(0, 1, 0)),
        down: visible(0, -1, 0),
        north: visible(0, 0, -1),
        south: visible(0, 0, 1),
        east: visible(1, 0, 0),
        west: visible(-1, 0, 0),
    };
    fluid::push_fluid_block(vertices, block, corners, faces, flow, uvs, tint);
}

fn push_baked_quad(
    vertices: &mut Vec<Vertex>,
    block: [f32; 3],
    quad: &BakedQuad,
    block_tint: [f32; 3],
) {
    let tint = if quad.tinted { block_tint } else { [1.0; 3] };
    let shaded = tint.map(|channel| channel * quad.brightness);
    let positions = quad
        .positions
        .map(|position| [block[0] + position[0], block[1] + position[1], block[2] + position[2]]);
    for &index in &[0, 1, 2, 0, 2, 3] {
        vertices.push(Vertex { position: positions[index], uv: quad.uv[index], tint: shaded });
    }
}

/// Whether a neighbor at `id` fully covers the face it shares with the block
/// asking — the only case a face may be culled for. Solidity (a collision
/// box) and whole-model opacity are independent. Resolved resource models use
/// their actual opaque, full-boundary quads; unresolved states retain the
/// registry's conservative solid-and-opaque fallback.
fn occludes(registry: &BlockRegistry, atlas: &Atlas, id: u32, face: (i32, i32, i32)) -> bool {
    atlas.occludes_face(id, face).unwrap_or_else(|| registry.is_solid(id) && registry.is_opaque(id))
}

fn neighbor_block(
    chunks: &HashMap<ChunkPos, &Chunk>,
    current: &Chunk,
    x: i32,
    y: i32,
    z: i32,
) -> Option<u32> {
    if (0..16).contains(&x) && (0..16).contains(&z) {
        return current.block_at(x, y, z);
    }
    let chunk_x = current.position.x.checked_add(x.div_euclid(16))?;
    let chunk_z = current.position.z.checked_add(z.div_euclid(16))?;
    chunks.get(&ChunkPos { x: chunk_x, z: chunk_z })?.block_at(
        x.rem_euclid(16),
        y,
        z.rem_euclid(16),
    )
}

#[allow(clippy::cast_precision_loss)]
// Relative offsets are kept near the player's origin; protocol coordinates
// use i32, and converting through i64 avoids subtraction overflow first.
fn relative_offset(position: ChunkPos, origin: ChunkPos) -> [f32; 2] {
    [
        ((i64::from(position.x) - i64::from(origin.x)) * 16) as f32,
        ((i64::from(position.z) - i64::from(origin.z)) * 16) as f32,
    ]
}

/// `(u, v)` offsets (in `[0, 1]` tile-local space) for each face's 4 corners,
/// in the same winding order as `push_face`'s position corners.
const fn uv_corners(face: (i32, i32, i32)) -> [[f32; 2]; 4] {
    match face {
        (0, 1 | -1, 0) | (0, 0, 1) => [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
        _ => [[1.0, 1.0], [0.0, 1.0], [0.0, 0.0], [1.0, 0.0]],
    }
}

#[allow(clippy::cast_precision_loss)] // Relative scene and cube-corner coordinates stay small.
fn push_face(
    vertices: &mut Vec<Vertex>,
    block: [f32; 3],
    face: (i32, i32, i32),
    uv_rect: [f32; 4],
    tile_uv: [[f32; 2]; 4],
    tint: [f32; 3],
    brightness: f32,
) {
    // Corners of the unit cube face for this normal, in `[x, y, z]` offsets
    // from the block's minimum corner. Winding is not load-bearing: the
    // pipeline disables backface culling (one chunk's worth of geometry is
    // cheap; getting six winding orders right by hand is not worth the risk).
    let corners: [[i32; 3]; 4] = match face {
        (0, 1, 0) => [[0, 1, 0], [0, 1, 1], [1, 1, 1], [1, 1, 0]],
        (0, -1, 0) => [[0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]],
        (0, 0, 1) => [[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]],
        (0, 0, -1) => [[0, 0, 0], [0, 1, 0], [1, 1, 0], [1, 0, 0]],
        (1, 0, 0) => [[1, 0, 0], [1, 1, 0], [1, 1, 1], [1, 0, 1]],
        (-1, 0, 0) => [[0, 0, 0], [0, 0, 1], [0, 1, 1], [0, 1, 0]],
        _ => return, // FACES only ever supplies unit axis directions.
    };
    let shaded = [tint[0] * brightness, tint[1] * brightness, tint[2] * brightness];
    let [u0, v0, u1, v1] = uv_rect;
    let positions = corners
        .map(|[ox, oy, oz]| [block[0] + ox as f32, block[1] + oy as f32, block[2] + oz as f32]);
    let uvs = tile_uv.map(|[u, v]| [u.mul_add(u1 - u0, u0), v.mul_add(v1 - v0, v0)]);
    for &index in &[0, 1, 2, 0, 2, 3] {
        vertices.push(Vertex { position: positions[index], uv: uvs[index], tint: shaded });
    }
}
