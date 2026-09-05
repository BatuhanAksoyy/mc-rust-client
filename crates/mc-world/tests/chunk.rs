//! Synthetic conversion tests: `mc_protocol::chunk::LevelChunk` -> `mc_world::Chunk`.
//! Builds `LevelChunk` values directly (its fields are public); no network or
//! game data needed.

use mc_protocol::chunk::{ChunkSection, LevelChunk, LightData, Palette, PalettedContainer};
use mc_world::Chunk;

const fn single_valued(value: i32) -> PalettedContainer {
    PalettedContainer { bits_per_entry: 0, palette: Palette::Single(value), indices: Vec::new() }
}

const fn indirect(palette: Vec<i32>, indices: Vec<u32>) -> PalettedContainer {
    PalettedContainer { bits_per_entry: 4, palette: Palette::Indirect(palette), indices }
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

#[test]
fn resolves_section_count_and_defaults_to_air_outside_range() {
    let level = LevelChunk {
        x: 3,
        z: -2,
        heightmaps: Vec::new(),
        sections: vec![
            ChunkSection {
                block_count: 0,
                fluid_count: 0,
                block_states: single_valued(0),
                biomes: single_valued(4),
            },
            ChunkSection {
                block_count: 4096,
                fluid_count: 0,
                block_states: single_valued(7),
                biomes: single_valued(4),
            },
        ],
        block_entities: Vec::new(),
        light: no_light(),
    };
    let chunk = Chunk::from_level(&level);
    assert_eq!((chunk.position.x, chunk.position.z), (3, -2));
    assert_eq!(chunk.section_count(), 2);

    // Section 0 (y 0..16): every entry resolves to the single value 0.
    assert_eq!(chunk.block_at(0, 0, 0), Some(0));
    assert_eq!(chunk.block_at(15, 15, 15), Some(0));
    // Section 1 (y 16..32): single value 7.
    assert_eq!(chunk.block_at(0, 16, 0), Some(7));
    assert_eq!(chunk.block_at(15, 31, 15), Some(7));

    // Out of range: no third section, and x/z/negative-y are rejected.
    assert_eq!(chunk.block_at(0, 32, 0), None);
    assert_eq!(chunk.block_at(16, 0, 0), None);
    assert_eq!(chunk.block_at(0, 0, 16), None);
    assert_eq!(chunk.block_at(0, -1, 0), None);
}

#[test]
fn block_indexing_matches_x_fastest_then_z_then_y() {
    // Palette: [air, stone, dirt]. Place stone at (1,0,0) and dirt at (0,0,1)
    // within an otherwise-air section to confirm axis order and that x
    // increases fastest (wiki Chunk format § Data Array format).
    let mut indices = vec![0_u32; 4096];
    indices[1] = 1; // x=1, z=0, y=0
    indices[16] = 2; // x=0, z=1, y=0 (one full row of x, i.e. index 16 = z=1)
    let section = ChunkSection {
        block_count: 2,
        fluid_count: 0,
        block_states: indirect(vec![0, 5, 10], indices),
        biomes: single_valued(4),
    };
    let level = LevelChunk {
        x: 0,
        z: 0,
        heightmaps: Vec::new(),
        sections: vec![section],
        block_entities: Vec::new(),
        light: no_light(),
    };
    let chunk = Chunk::from_level(&level);
    assert_eq!(chunk.block_at(0, 0, 0), Some(0));
    assert_eq!(chunk.block_at(1, 0, 0), Some(5));
    assert_eq!(chunk.block_at(0, 0, 1), Some(10));
    assert_eq!(chunk.block_at(1, 0, 1), Some(0));
}
