//! Client-only fluid ramp used to inspect renderer behavior before item use
//! and live server block updates exist (`docs/RENDER.md`).

use mc_world::{BlockRegistry, Chunk, ChunkPos};

/// Which temporary fluid ramp(s) to add to the initial chunk snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Water levels 0 through 7.
    Water,
    /// Lava levels 0 through 7.
    Lava,
    /// Separate water and lava ramps.
    Both,
}

/// A requested fixture cannot be represented by the local cached data.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    /// The selected origin chunk was not in the received view.
    #[error("fluid demo origin chunk is missing")]
    MissingChunk,
    /// The chunk has no room for a support row and fluid above it.
    #[error("fluid demo origin chunk is too short")]
    ShortChunk,
    /// A required 26.2 block state was absent from the local registry cache.
    #[error("fluid demo requires cached block state {0}")]
    MissingState(String),
}

/// Inject deterministic level-0-through-7 ramps into the client-side chunk.
///
/// Returns the chunk-local Y coordinate of the fluid surfaces. No network or
/// disk state is touched; callers mesh this modified snapshot once.
pub fn inject(
    chunks: &mut [Chunk],
    origin: ChunkPos,
    registry: &BlockRegistry,
    kind: Kind,
) -> Result<i32, Error> {
    let chunk =
        chunks.iter_mut().find(|chunk| chunk.position == origin).ok_or(Error::MissingChunk)?;
    let height = i32::try_from(chunk.section_count() * 16).map_err(|_| Error::ShortChunk)?;
    if height < 3 {
        return Err(Error::ShortChunk);
    }
    let air = find_state(registry, "minecraft:air", None)?;
    let support = find_state(registry, "minecraft:stone", None)?;
    let ground = (0..height)
        .rev()
        .find(|&y| chunk.block_at(8, y, 8).is_some_and(|id| !registry.is_air(id)))
        .unwrap_or(height / 2);
    let support_y = (ground + 1).min(height - 2);
    let fluid_y = support_y + 1;
    let rows: &[(&str, i32)] = match kind {
        Kind::Water => &[("minecraft:water", 5)],
        Kind::Lava => &[("minecraft:lava", 5)],
        Kind::Both => &[("minecraft:water", 5), ("minecraft:lava", 2)],
    };

    for &(name, z) in rows {
        let levels = (0..=7)
            .map(|level| find_state(registry, name, Some(level)))
            .collect::<Result<Vec<_>, _>>()?;
        // Clear a one-block border so every slope and side remains visible
        // even when the selected terrain column is uneven.
        for clear_z in z - 1..=z + 1 {
            for x in 3..=12 {
                chunk.set_block(x, fluid_y, clear_z, air);
                chunk.set_block(x, fluid_y + 1, clear_z, air);
            }
        }
        for (index, id) in levels.into_iter().enumerate() {
            let x = 4 + i32::try_from(index).expect("eight fluid levels fit in i32");
            chunk.set_block(x, support_y, z, support);
            chunk.set_block(x, fluid_y, z, id);
        }
    }
    Ok(fluid_y)
}

fn find_state(registry: &BlockRegistry, name: &str, level: Option<u8>) -> Result<u32, Error> {
    (0_u32..)
        .map_while(|id| registry.state(id).map(|state| (id, state)))
        .find(|(_, state)| {
            state.name.as_ref() == name
                && level.is_none_or(|expected| {
                    state.properties.get("level").and_then(|value| value.parse().ok())
                        == Some(expected)
                })
        })
        .map(|(id, _)| id)
        .ok_or_else(|| {
            Error::MissingState(
                level.map_or_else(|| name.to_owned(), |level| format!("{name}[level={level}]")),
            )
        })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use mc_protocol::chunk::{ChunkSection, LevelChunk, LightData, Palette, PalettedContainer};
    use mc_world::BlockState;

    use super::{Error, Kind, inject};

    fn fixture() -> (Vec<mc_world::Chunk>, mc_world::BlockRegistry) {
        let mut states = vec![
            BlockState { name: "minecraft:air".into(), properties: BTreeMap::new() },
            BlockState { name: "minecraft:stone".into(), properties: BTreeMap::new() },
        ];
        for name in ["minecraft:water", "minecraft:lava"] {
            for level in 0..=7 {
                let mut properties = BTreeMap::new();
                properties.insert("level".into(), level.to_string().into());
                states.push(BlockState { name: name.into(), properties });
            }
        }
        let registry = mc_world::BlockRegistry::from_states(states);
        let level = LevelChunk {
            x: 2,
            z: -3,
            heightmaps: Vec::new(),
            sections: vec![ChunkSection {
                block_count: 0,
                fluid_count: 0,
                block_states: PalettedContainer {
                    bits_per_entry: 0,
                    palette: Palette::Single(0),
                    indices: Vec::new(),
                },
                biomes: PalettedContainer {
                    bits_per_entry: 0,
                    palette: Palette::Single(0),
                    indices: Vec::new(),
                },
            }],
            block_entities: Vec::new(),
            light: LightData {
                sky_light_mask: Vec::new(),
                block_light_mask: Vec::new(),
                empty_sky_light_mask: Vec::new(),
                empty_block_light_mask: Vec::new(),
                sky_light: Vec::new(),
                block_light: Vec::new(),
            },
        };
        (vec![mc_world::Chunk::from_level(&level)], registry)
    }

    #[test]
    fn both_ramps_use_real_level_states_and_stone_support() {
        let (mut chunks, registry) = fixture();
        let origin = chunks[0].position;
        chunks[0].set_block(8, 0, 8, 1);
        assert_eq!(inject(&mut chunks, origin, &registry, Kind::Both), Ok(2));
        for (offset, expected) in (2..=9).enumerate() {
            let x = 4 + i32::try_from(offset).unwrap();
            assert_eq!(chunks[0].block_at(x, 2, 5), Some(expected));
            assert_eq!(chunks[0].block_at(x, 1, 5), Some(1));
            assert_eq!(chunks[0].block_at(x, 2, 2), Some(expected + 8));
        }
    }

    #[test]
    fn missing_cached_fluid_states_are_reported() {
        let (mut chunks, _) = fixture();
        let registry = mc_world::BlockRegistry::from_names(vec![
            "minecraft:air".into(),
            "minecraft:stone".into(),
        ]);
        let origin = chunks[0].position;
        assert!(matches!(
            inject(&mut chunks, origin, &registry, Kind::Water),
            Err(Error::MissingState(state)) if state == "minecraft:water[level=0]"
        ));
    }
}
