//! Global block-state ID → name/color lookup, from an optional local cache.
//!
//! Block-state IDs are not sent as a synchronized registry (`docs/JOIN.md`
//! only covers what protocol 776 actually transmits, e.g. dimension type and
//! biome); the global ID space is fixed data baked into the game itself.
//! `docs/WORLD_PHYSICS_ASSETS.md` generates it locally, once, by running the
//! pinned server jar's own `--reports` flag — never bundled or committed,
//! and never required: an absent cache degrades to hashed placeholder colors.

use std::{path::PathBuf, sync::Arc};

/// Numeric block-state ID → name, loaded from an optional cache.
#[derive(Debug, Clone)]
pub struct BlockRegistry {
    /// Indexed by state ID; empty when no cache was found or it failed to parse.
    names: Arc<[Box<str>]>,
}

#[derive(serde::Deserialize)]
struct CachedReport {
    names: Vec<String>,
}

impl BlockRegistry {
    /// An empty registry: every ID reports no name and a hashed color.
    #[must_use]
    pub fn empty() -> Self {
        Self { names: Arc::from(Vec::new().into_boxed_slice()) }
    }

    /// Build a registry directly from an ID-indexed name list (index = state
    /// ID), independent of the cache or JSON: useful for synthetic fixtures,
    /// and for a registry sourced some other way in the future.
    #[must_use]
    pub fn from_names(names: Vec<String>) -> Self {
        Self { names: names.into_iter().map(String::into_boxed_str).collect() }
    }

    /// Load `<cache>/mc-rust-client/<version>/block-states.json`
    /// (`docs/WORLD_PHYSICS_ASSETS.md`). Never fails: an absent or malformed
    /// cache falls back to [`Self::empty`], since this client does not
    /// require real block names to render.
    #[must_use]
    pub fn load_cached(version: &str) -> Self {
        Self::try_load_cached(version).unwrap_or_else(Self::empty)
    }

    fn try_load_cached(version: &str) -> Option<Self> {
        let path = cache_dir()?.join(version).join("block-states.json");
        let text = std::fs::read_to_string(path).ok()?;
        Self::from_report_json(&text)
    }

    /// Parse a cached report's contents directly, independent of the cache
    /// directory or filesystem — the seam integration tests exercise.
    fn from_report_json(text: &str) -> Option<Self> {
        let report: CachedReport = serde_json::from_str(text).ok()?;
        Some(Self::from_names(report.names))
    }

    /// This block-state ID's namespaced name, if the registry has it.
    #[must_use]
    pub fn name(&self, id: u32) -> Option<&str> {
        self.names.get(id as usize).map(AsRef::as_ref)
    }

    /// Whether `id` is one of the three air variants. Unknown IDs (registry
    /// absent, or ID outside its range) are never air: the safer default is
    /// to render a block this client cannot identify, not a hole in the world.
    #[must_use]
    pub fn is_air(&self, id: u32) -> bool {
        matches!(self.name(id), Some("minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"))
    }

    /// A debug color for this block: curated for common blocks by name,
    /// otherwise hashed from the ID. Not a texture — see `docs/RENDER.md`
    /// milestone 2 (synthetic data; a texture atlas from cached assets is
    /// later work).
    #[must_use]
    pub fn color(&self, id: u32) -> [f32; 3] {
        self.name(id).and_then(named_color).unwrap_or_else(|| hashed_color(id))
    }
}

fn cache_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CACHE_HOME")
        .or_else(|| std::env::var_os("LOCALAPPDATA"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .map(|base| base.join("mc-rust-client"))
}

/// A small, dependency-free FNV-1a-style hash: deterministic per ID, not
/// cryptographic, purely for spreading unrecognized IDs across the color cube.
fn hashed_color(id: u32) -> [f32; 3] {
    let mut hash = 0x811c_9dc5_u32;
    for byte in id.to_le_bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    let channel = |shift: u32| f32::from(u8::try_from((hash >> shift) & 0xff).unwrap()) / 255.0;
    // Mid-brightness band: an unrecognized ID should read as "unknown block", not noise.
    [0.3 + channel(0) * 0.5, 0.3 + channel(8) * 0.5, 0.3 + channel(16) * 0.5]
}

#[allow(clippy::match_same_arms)]
fn named_color(name: &str) -> Option<[f32; 3]> {
    Some(match name {
        "minecraft:grass_block" => [0.29, 0.62, 0.27],
        "minecraft:dirt"
        | "minecraft:coarse_dirt"
        | "minecraft:rooted_dirt"
        | "minecraft:podzol" => [0.40, 0.28, 0.16],
        "minecraft:stone" | "minecraft:andesite" | "minecraft:tuff" => [0.5, 0.5, 0.5],
        "minecraft:deepslate" | "minecraft:cobbled_deepslate" | "minecraft:deepslate_bricks" => {
            [0.29, 0.29, 0.31]
        }
        "minecraft:deepslate_iron_ore" | "minecraft:iron_ore" | "minecraft:raw_iron_block" => {
            [0.66, 0.61, 0.55]
        }
        "minecraft:deepslate_coal_ore" | "minecraft:coal_ore" | "minecraft:coal_block" => {
            [0.15, 0.15, 0.15]
        }
        "minecraft:deepslate_gold_ore" | "minecraft:gold_ore" | "minecraft:raw_gold_block" => {
            [0.83, 0.71, 0.24]
        }
        "minecraft:deepslate_diamond_ore" | "minecraft:diamond_ore" | "minecraft:diamond_block" => {
            [0.44, 0.85, 0.82]
        }
        "minecraft:sand" | "minecraft:suspicious_sand" => [0.87, 0.80, 0.55],
        "minecraft:red_sand" => [0.73, 0.40, 0.16],
        "minecraft:sandstone" | "minecraft:smooth_sandstone" => [0.80, 0.74, 0.53],
        "minecraft:gravel" => [0.55, 0.53, 0.52],
        "minecraft:bedrock" => [0.1, 0.1, 0.1],
        "minecraft:water" => [0.16, 0.35, 0.86],
        "minecraft:lava" => [0.86, 0.35, 0.05],
        "minecraft:oak_log" | "minecraft:oak_wood" | "minecraft:oak_planks" => [0.45, 0.34, 0.19],
        "minecraft:oak_leaves" | "minecraft:azalea_leaves" => [0.20, 0.45, 0.13],
        "minecraft:snow" | "minecraft:snow_block" | "minecraft:powder_snow" => [0.95, 0.95, 0.97],
        "minecraft:ice" | "minecraft:packed_ice" | "minecraft:blue_ice" => [0.68, 0.80, 0.93],
        "minecraft:granite" | "minecraft:polished_granite" => [0.60, 0.39, 0.34],
        "minecraft:diorite" | "minecraft:polished_diorite" | "minecraft:calcite" => {
            [0.78, 0.78, 0.78]
        }
        "minecraft:clay" => [0.55, 0.56, 0.60],
        "minecraft:mycelium" => [0.45, 0.38, 0.42],
        "minecraft:moss_block" | "minecraft:moss_carpet" => [0.32, 0.47, 0.19],
        "minecraft:netherrack" => [0.44, 0.20, 0.20],
        "minecraft:end_stone" => [0.87, 0.85, 0.60],
        "minecraft:obsidian" | "minecraft:crying_obsidian" => [0.08, 0.06, 0.13],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::BlockRegistry;

    #[test]
    fn empty_registry_has_no_names_but_still_colors_every_id() {
        let registry = BlockRegistry::empty();
        assert_eq!(registry.name(0), None);
        assert!(!registry.is_air(0));
        let first = registry.color(42);
        let second = registry.color(42);
        assert_eq!(first.map(f32::to_bits), second.map(f32::to_bits)); // Deterministic.
        assert_ne!(
            registry.color(42).map(f32::to_bits),
            registry.color(43).map(f32::to_bits) // Spreads across IDs.
        );
    }

    #[test]
    fn malformed_report_json_falls_back_to_none() {
        assert!(BlockRegistry::from_report_json("not json").is_none());
        assert!(BlockRegistry::from_report_json(r#"{"wrong_field":[]}"#).is_none());
    }

    #[test]
    fn parsed_report_resolves_names_and_air() {
        let json =
            r#"{"protocol":776,"version":"26.2","names":["minecraft:air","minecraft:stone"]}"#;
        let registry = BlockRegistry::from_report_json(json).unwrap();
        assert_eq!(registry.name(0), Some("minecraft:air"));
        assert_eq!(registry.name(1), Some("minecraft:stone"));
        assert_eq!(registry.name(2), None); // Out of range.
        assert!(registry.is_air(0));
        assert!(!registry.is_air(1));
        assert!(!registry.is_air(2)); // Unknown, not air.
        assert_eq!(registry.color(1).map(f32::to_bits), [0.5, 0.5, 0.5].map(f32::to_bits)); // Stone.
    }
}
