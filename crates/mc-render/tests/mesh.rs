//! Synthetic face-culling tests for `mesh_chunk`. Builds `LevelChunk`/`Chunk`
//! values directly; no network, GPU or window needed.

use mc_protocol::chunk::{ChunkSection, LevelChunk, LightData, Palette, PalettedContainer};
use mc_render::atlas::Atlas;
use mc_render::mesh::{mesh_chunk, mesh_chunks};
use mc_world::{BlockRegistry, Chunk, ChunkPos};

fn registry() -> BlockRegistry {
    BlockRegistry::from_names(vec!["minecraft:air".to_owned(), "minecraft:stone".to_owned()])
}

/// No extracted assets in these synthetic tests: every block falls back to
/// `BlockRegistry`'s solid debug color, same as before atlas support landed.
fn empty_atlas() -> Atlas {
    Atlas::build(std::path::Path::new(""), std::iter::empty()).0
}

const fn no_light() -> LightData {
    LightData {
        sky_light_mask: Vec::new(),
        block_light_mask: Vec::new(),
        empty_sky_light_mask: Vec::new(),
        empty_block_light_mask: Vec::new(),
        sky_light: Vec::new(),
        block_light: Vec::new(),
    }
}

/// One section with every entry set from `indices` (palette `[air, stone,
/// short_grass, oak_leaves]` — only the first two are used outside this
/// file's own non-solid/non-opaque-neighbor tests).
fn section_from(indices: Vec<u32>) -> ChunkSection {
    ChunkSection {
        block_count: 0,
        fluid_count: 0,
        block_states: PalettedContainer {
            bits_per_entry: 4,
            palette: Palette::Indirect(vec![0, 1, 2, 3]),
            indices,
        },
        biomes: PalettedContainer {
            bits_per_entry: 0,
            palette: Palette::Single(0),
            indices: Vec::new(),
        },
    }
}

fn chunk_at(position: ChunkPos, indices: Vec<u32>) -> Chunk {
    let level = LevelChunk {
        x: position.x,
        z: position.z,
        heightmaps: Vec::new(),
        sections: vec![section_from(indices)],
        block_entities: Vec::new(),
        light: no_light(),
    };
    Chunk::from_level(&level)
}

fn chunk_with(indices: Vec<u32>) -> Chunk {
    chunk_at(ChunkPos { x: 0, z: 0 }, indices)
}

#[test]
fn an_all_air_chunk_meshes_to_nothing() {
    let chunk = chunk_with(vec![0; 4096]);
    let mesh = mesh_chunk(&chunk, &registry(), &empty_atlas());
    assert!(mesh.vertices.is_empty());
}

#[test]
fn one_isolated_block_emits_all_six_faces() {
    // Block 1 (stone) at chunk-local (0, 0, 0); the section's chunk-edge and
    // top/bottom neighbors are all "missing" (no adjacent chunk decoded),
    // which counts as visible, same as air would.
    let mut indices = vec![0; 4096];
    indices[0] = 1; // (x=0, z=0, y=0) per x-fastest-then-z-then-y ordering.
    let chunk = chunk_with(indices);
    let mesh = mesh_chunk(&chunk, &registry(), &empty_atlas());
    assert_eq!(mesh.vertices.len(), 6 * 6); // 6 faces, 2 triangles (6 vertices) each.
}

#[test]
fn two_adjacent_blocks_hide_their_shared_face() {
    // Stone at (0,0,0) and (1,0,0): touching along X, so each loses exactly
    // one face (the one facing the other block) versus being isolated.
    let mut indices = vec![0; 4096];
    indices[0] = 1;
    indices[1] = 1;
    let chunk = chunk_with(indices);
    let mesh = mesh_chunk(&chunk, &registry(), &empty_atlas());
    assert_eq!(mesh.vertices.len(), 2 * 5 * 6); // 5 visible faces each, not 6.
}

#[test]
fn a_fully_enclosed_block_is_never_meshed() {
    // A stone core at (1,1,1), surrounded on all 6 sides by more stone: the
    // core contributes zero faces (every neighbor is solid).
    let mut indices = vec![0; 4096];
    let index = |x: i32, y: i32, z: i32| usize::try_from((y * 16 + z) * 16 + x).unwrap();
    for &(x, y, z) in &[(1, 1, 1), (0, 1, 1), (2, 1, 1), (1, 0, 1), (1, 2, 1), (1, 1, 0), (1, 1, 2)]
    {
        indices[index(x, y, z)] = 1;
    }
    let chunk = chunk_with(indices);
    let mesh = mesh_chunk(&chunk, &registry(), &empty_atlas());
    // The core is fully enclosed (0 faces); each of its 6 neighbors is
    // exposed on its other 5 sides (the 6th touches the core).
    assert_eq!(mesh.vertices.len(), 6 * 5 * 6);
}

#[test]
fn a_non_solid_neighbor_does_not_hide_the_solid_face_beside_it() {
    // Stone at (0,0,0), a walk-through decoration (`with_non_solid`, standing
    // in for a cross-shaped plant like a mushroom or flower) directly above
    // it at (0,1,0). The decoration is non-air, but its cross shape covers
    // almost none of the stone's top face — the real bug this guards
    // (`RENDER.md` milestone 3): culling on `is_air` alone treated any
    // non-air neighbor as a full occluder, hiding the stone's top face and
    // exposing whatever sat below it through the "hole" instead.
    let registry = BlockRegistry::from_names(vec![
        "minecraft:air".to_owned(),
        "minecraft:stone".to_owned(),
        "minecraft:short_grass".to_owned(),
    ])
    .with_non_solid([2]);
    let mut indices = vec![0; 4096];
    indices[0] = 1; // stone at (0,0,0)
    indices[256] = 2; // short_grass at (0,1,0): (y*16+z)*16+x = (1*16+0)*16+0.
    let chunk = chunk_with(indices);
    let mesh = mesh_chunk(&chunk, &registry, &empty_atlas());
    // Stone keeps all 6 faces (its "up" neighbor doesn't occlude); the
    // decoration keeps 5 (its "down" face is still hidden by the real, solid
    // stone beneath it).
    assert_eq!(mesh.vertices.len(), (6 + 5) * 6);
}

#[test]
fn a_solid_but_non_opaque_neighbor_does_not_hide_the_solid_face_beside_it() {
    // Stone at (0,0,0), a solid-but-transparent block (`with_non_opaque`,
    // standing in for leaves — a real full cube, but with a cutout texture)
    // directly above it at (0,1,0). Before `is_opaque` existed, culling on
    // `is_solid` alone treated any solid neighbor as a full occluder — a log
    // (or, here, stone) standing right next to leaves would have its shared
    // face hidden as if the leaves were plain opaque wood, even though the
    // leaves' real texture has alpha gaps a player can see straight through.
    let registry = BlockRegistry::from_names(vec![
        "minecraft:air".to_owned(),
        "minecraft:stone".to_owned(),
        "minecraft:short_grass".to_owned(),
        "minecraft:oak_leaves".to_owned(),
    ])
    .with_non_opaque([3]);
    let mut indices = vec![0; 4096];
    indices[0] = 1; // stone at (0,0,0)
    indices[256] = 3; // oak_leaves at (0,1,0): (y*16+z)*16+x = (1*16+0)*16+0.
    let chunk = chunk_with(indices);
    let mesh = mesh_chunk(&chunk, &registry, &empty_atlas());
    // Stone keeps all 6 faces (its "up" neighbor is solid but doesn't fully
    // occlude); the leaves keep only 5 (its "down" face is still hidden by
    // the real, solid *and* opaque stone beneath it).
    assert_eq!(mesh.vertices.len(), (6 + 5) * 6);
}

#[test]
fn chunk_batch_uses_positions_relative_to_the_requested_origin() {
    let mut indices = vec![0; 4096];
    indices[0] = 1;
    let chunk = chunk_at(ChunkPos { x: 3, z: -1 }, indices);
    let mesh = mesh_chunks(&[chunk], ChunkPos { x: 2, z: -2 }, &registry(), &empty_atlas());

    let xs = mesh.vertices.iter().map(|vertex| vertex.position[0]);
    let zs = mesh.vertices.iter().map(|vertex| vertex.position[2]);
    assert_eq!(xs.clone().reduce(f32::min), Some(16.0));
    assert_eq!(xs.reduce(f32::max), Some(17.0));
    assert_eq!(zs.clone().reduce(f32::min), Some(16.0));
    assert_eq!(zs.reduce(f32::max), Some(17.0));
}

#[test]
fn chunk_batch_culls_faces_across_chunk_boundaries() {
    let mut west = vec![0; 4096];
    west[15] = 1; // (15, 0, 0)
    let mut east = vec![0; 4096];
    east[0] = 1; // (0, 0, 0)
    let chunks = [chunk_at(ChunkPos { x: 0, z: 0 }, west), chunk_at(ChunkPos { x: 1, z: 0 }, east)];

    let mesh = mesh_chunks(&chunks, ChunkPos { x: 0, z: 0 }, &registry(), &empty_atlas());
    assert_eq!(mesh.vertices.len(), 2 * 5 * 6);
}
