//! Global block-state ID → name/color lookup, from an optional local cache.
//!
//! Block-state IDs are not sent as a synchronized registry (`docs/JOIN.md`
//! only covers what protocol 776 actually transmits, e.g. dimension type and
//! biome); the global ID space is fixed data baked into the game itself.
//! `docs/WORLD_PHYSICS_ASSETS.md` generates it locally, once, by running the
//! pinned server jar's own `--reports` flag — never bundled or committed,
//! and never required: an absent cache degrades to hashed placeholder colors.

use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
    sync::Arc,
};

/// One global block state, including the properties that select its resource-pack variant.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct BlockState {
    /// Namespaced block identifier.
    pub name: Box<str>,
    /// State properties such as `axis=x` or `facing=north`.
    #[serde(default)]
    pub properties: BTreeMap<Box<str>, Box<str>>,
}

/// Numeric block-state ID → name, loaded from an optional cache.
#[derive(Debug, Clone)]
pub struct BlockRegistry {
    /// Indexed by state ID; empty when no cache was found or it failed to parse.
    states: Arc<[BlockState]>,
    /// IDs with no collision box (see [`Self::with_non_solid`]); empty until
    /// a caller opts in, so [`Self::is_solid`] keeps its old "solid unless
    /// air" behavior by default.
    non_solid: Arc<HashSet<u32>>,
    /// IDs whose texture has real alpha variation (see
    /// [`Self::with_non_opaque`]); empty until a caller opts in, so
    /// [`Self::is_opaque`] defaults every ID to opaque.
    non_opaque: Arc<HashSet<u32>>,
}

#[derive(serde::Deserialize)]
struct CachedReport {
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    states: Vec<BlockState>,
}

impl BlockRegistry {
    /// An empty registry: every ID reports no name and a hashed color.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            states: Arc::from(Vec::new().into_boxed_slice()),
            non_solid: Arc::default(),
            non_opaque: Arc::default(),
        }
    }

    /// Build a registry directly from an ID-indexed name list (index = state
    /// ID), independent of the cache or JSON: useful for synthetic fixtures,
    /// and for a registry sourced some other way in the future.
    #[must_use]
    pub fn from_names(names: Vec<String>) -> Self {
        Self {
            states: names
                .into_iter()
                .map(|name| BlockState { name: name.into_boxed_str(), properties: BTreeMap::new() })
                .collect(),
            non_solid: Arc::default(),
            non_opaque: Arc::default(),
        }
    }

    /// Returns this registry with `ids` additionally marked as having no
    /// collision box, regardless of [`Self::is_air`] — the resource-pack
    /// walk-through decorations (cross-shaped plants, torches, redstone
    /// components, ...) `mc_render::atlas::Atlas` tells apart from
    /// structural partial shapes (fences, walls, stairs) via its own
    /// `is_solid`. IDs outside `ids` keep [`Self::is_solid`]'s old
    /// "solid unless air" default.
    #[must_use]
    pub fn with_non_solid(mut self, ids: impl IntoIterator<Item = u32>) -> Self {
        self.non_solid = Arc::new(ids.into_iter().collect());
        self
    }

    /// Whether a player's collision box is blocked by this ID: false for air
    /// and for any ID [`Self::with_non_solid`] marked walk-through. An ID
    /// this registry doesn't recognize at all defaults to solid — the safer
    /// choice, same reasoning as [`Self::is_air`].
    #[must_use]
    pub fn is_solid(&self, id: u32) -> bool {
        !self.is_air(id) && !self.non_solid.contains(&id)
    }

    /// Returns this registry with `ids` additionally marked as having a
    /// texture with real alpha variation — `mc_render::atlas::Atlas`'s own
    /// `is_opaque`, read straight off the resource pack's texture data (a
    /// full cube can be solid and still not opaque: leaves' cutout gaps,
    /// glass's transparency). IDs outside `ids` keep [`Self::is_opaque`]'s
    /// default of opaque.
    #[must_use]
    pub fn with_non_opaque(mut self, ids: impl IntoIterator<Item = u32>) -> Self {
        self.non_opaque = Arc::new(ids.into_iter().collect());
        self
    }

    /// Whether this ID's texture fully occludes whatever is behind it: false
    /// for any ID [`Self::with_non_opaque`] marked as having real alpha
    /// variation. An ID this registry doesn't recognize, or one no caller
    /// has marked non-opaque, defaults to opaque — a neighbor's shared face
    /// should only be culled by a real, fully-covering occluder.
    #[must_use]
    pub fn is_opaque(&self, id: u32) -> bool {
        !self.non_opaque.contains(&id)
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
        if report.states.is_empty() {
            (!report.names.is_empty()).then(|| Self::from_names(report.names))
        } else {
            Some(Self {
                states: report.states.into(),
                non_solid: Arc::default(),
                non_opaque: Arc::default(),
            })
        }
    }

    /// This numeric ID's full block state, if present in the local registry.
    #[must_use]
    pub fn state(&self, id: u32) -> Option<&BlockState> {
        self.states.get(id as usize)
    }

    /// This block-state ID's namespaced name, if the registry has it.
    #[must_use]
    pub fn name(&self, id: u32) -> Option<&str> {
        self.state(id).map(|state| state.name.as_ref())
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
    if name.ends_with("_leaves") {
        return Some([0.20, 0.45, 0.13]);
    }
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
        // Grass-tinted cross/lily-pad models (`atlas::model`'s `tinted_cross` and
        // `tinted_flower_pot_cross` parents) reuse `mesh::push_baked_quad`'s tint
        // slot for the real per-biome grass-color multiply Java samples from
        // a biome colormap; without biome data we have no such sample, so
        // these fell through to `hashed_color`'s per-ID placeholder instead —
        // a value with no relation to grass green, sometimes close enough to
        // the sky/water behind a block to look "see-through" instead of
        // wrong-colored. `minecraft:grass_block`'s own curated green above is
        // this client's one flat stand-in for the real biome sample; reuse it
        // here so every grass-family prop reads as grass instead of noise.
        "minecraft:short_grass"
        | "minecraft:tall_grass"
        | "minecraft:fern"
        | "minecraft:large_fern"
        | "minecraft:sugar_cane"
        | "minecraft:bamboo_sapling"
        | "minecraft:bush"
        | "minecraft:potted_fern" => [0.29, 0.62, 0.27],
        // `vine`/`lily_pad` are tinted from the foliage (not grass) colormap
        // in Java; reuse the same flat foliage green curated for `_leaves` above.
        "minecraft:vine" | "minecraft:lily_pad" => [0.20, 0.45, 0.13],
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
    fn is_solid_defaults_to_true_except_air_until_ids_are_marked_non_solid() {
        let registry = BlockRegistry::from_names(vec![
            "minecraft:air".into(),
            "minecraft:stone".into(),
            "minecraft:short_grass".into(),
        ]);
        assert!(!registry.is_solid(0)); // Air.
        assert!(registry.is_solid(1)); // Solid unless marked otherwise.
        assert!(registry.is_solid(2)); // Not yet marked non-solid.
        let registry = registry.with_non_solid([2]);
        assert!(registry.is_solid(1)); // Unaffected.
        assert!(!registry.is_solid(2)); // Now walk-through.
        assert!(!registry.is_solid(0)); // Air stays non-solid either way.
    }

    #[test]
    fn is_opaque_defaults_to_true_until_ids_are_marked_non_opaque() {
        let registry = BlockRegistry::from_names(vec![
            "minecraft:stone".into(),
            "minecraft:oak_leaves".into(),
        ]);
        assert!(registry.is_opaque(0)); // Solid and opaque, unmarked.
        assert!(registry.is_opaque(1)); // Not yet marked non-opaque.
        let registry = registry.with_non_opaque([1]);
        assert!(registry.is_opaque(0)); // Unaffected.
        assert!(!registry.is_opaque(1)); // Solid, but its cutout texture isn't opaque.
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

    #[test]
    fn grass_family_props_get_the_curated_grass_tint_not_a_hashed_placeholder() {
        let json = r#"{"protocol":776,"version":"26.2","names":[
            "minecraft:short_grass","minecraft:tall_grass","minecraft:fern",
            "minecraft:large_fern","minecraft:sugar_cane","minecraft:bamboo_sapling",
            "minecraft:bush","minecraft:potted_fern","minecraft:vine","minecraft:lily_pad"
        ]}"#;
        let registry = BlockRegistry::from_report_json(json).unwrap();
        let grass = [0.29, 0.62, 0.27].map(f32::to_bits);
        let foliage = [0.20, 0.45, 0.13].map(f32::to_bits);
        for id in 0..8 {
            assert_eq!(registry.color(id).map(f32::to_bits), grass, "id {id}");
        }
        for id in 8..10 {
            assert_eq!(registry.color(id).map(f32::to_bits), foliage, "id {id}");
        }
    }

    #[test]
    fn state_report_retains_variant_properties() {
        let json = r#"{"protocol":776,"version":"26.2","states":[{"name":"minecraft:birch_log","properties":{"axis":"x"}}]}"#;
        let registry = BlockRegistry::from_report_json(json).unwrap();
        let state = registry.state(0).unwrap();
        assert_eq!(state.name.as_ref(), "minecraft:birch_log");
        assert_eq!(state.properties.get("axis").map(AsRef::as_ref), Some("x"));
    }
}
