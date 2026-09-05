//! Chunk → vertex buffer, culling internal faces. No greedy meshing yet
//! (`docs/RENDER.md`): one quad per visible face, synthetic per-block colors.

use bytemuck::{Pod, Zeroable};
use mc_world::{BlockRegistry, Chunk};

/// One mesh vertex: chunk-local position and a pre-shaded RGB color (the
/// block's color × a fixed per-face brightness, a cheap stand-in for real
/// lighting until block/sky light data is wired in).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    /// Chunk-local block-space position (not yet offset by chunk X/Z).
    pub position: [f32; 3],
    /// Linear RGB, already shaded.
    pub color: [f32; 3],
}

impl Vertex {
    /// `wgpu` vertex-buffer attribute layout matching this struct's fields.
    pub const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
}

/// Triangle-list vertex data for one chunk, in chunk-local block coordinates.
#[derive(Debug, Clone, Default)]
pub struct Mesh {
    /// Non-indexed triangle list (6 vertices per visible face).
    pub vertices: Vec<Vertex>,
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

/// Mesh every visible face of every non-air block in `chunk`. A face is
/// visible when its neighbor is missing (chunk edge/top/bottom — there is no
/// neighbor chunk to consult yet) or is air.
#[must_use]
pub fn mesh_chunk(chunk: &Chunk, registry: &BlockRegistry) -> Mesh {
    let mut vertices = Vec::new();
    let Ok(height) = i32::try_from(chunk.section_count() * 16) else { return Mesh { vertices } };
    for y in 0..height {
        for z in 0..16 {
            for x in 0..16 {
                let Some(id) = chunk.block_at(x, y, z) else { continue };
                if registry.is_air(id) {
                    continue;
                }
                let color = registry.color(id);
                for &(dx, dy, dz, brightness) in &FACES {
                    let visible = chunk
                        .block_at(x + dx, y + dy, z + dz)
                        .is_none_or(|neighbor| registry.is_air(neighbor));
                    if visible {
                        push_face(&mut vertices, [x, y, z], (dx, dy, dz), color, brightness);
                    }
                }
            }
        }
    }
    Mesh { vertices }
}

#[allow(clippy::cast_precision_loss)] // Chunk-local coordinates are tiny (<= a few hundred).
fn push_face(
    vertices: &mut Vec<Vertex>,
    block: [i32; 3],
    face: (i32, i32, i32),
    color: [f32; 3],
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
    let shaded = [color[0] * brightness, color[1] * brightness, color[2] * brightness];
    let positions = corners.map(|[ox, oy, oz]| {
        [
            (block[0] + ox) as f32, // Chunk-local coordinates are tiny; see the fn-level allow.
            (block[1] + oy) as f32,
            (block[2] + oz) as f32,
        ]
    });
    for &index in &[0, 1, 2, 0, 2, 3] {
        vertices.push(Vertex { position: positions[index], color: shaded });
    }
}
