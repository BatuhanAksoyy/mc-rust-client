//! Block name → texture resolution and atlas packing (`docs/RENDER.md`
//! milestone 3).
//!
//! Reads runtime-loaded Minecraft *resource* files only: blockstate
//! JSON, block-model JSON and 16×16 block-texture PNGs, all data/asset files
//! Mojang ships for any resource pack or mod loader to read — never Java
//! source, never committed (`docs/WORLD_PHYSICS_ASSETS.md`'s ASSETS policy).
//!
//! Only single-variant, unrotated full-cube models are resolved: the parent
//! chain must bottom out at one of vanilla's cube base models
//! (`cube_all`, `cube_column`, `cube_bottom_top`, `cube`). That covers most terrain blocks (stone, dirt,
//! ores, logs, planks, sand, wool, concrete, ...). Anything else (multipart
//! blockstates — fences/walls/stairs — liquids, or a model that isn't
//! cube-based) is left unresolved; `mesh.rs` falls back to
//! `BlockRegistry`'s solid debug color for those, same as before this
//! milestone.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub use image::RgbaImage;

/// One block texture is always 16×16 in vanilla's base resource pack. A
/// resource pack's own animated textures are taller (stacked frames); we
/// only ever read the first frame (see [`load_first_frame`]).
const TILE: u32 = 16;

/// A resolved block's texture, one entry per cube face.
///
/// All six are always populated (duplicated across faces when the model
/// only distinguished "all" or "top/bottom vs. side"), so callers never
/// need to special-case how a block model shaped its texture variables.
#[derive(Debug, Clone, Copy)]
pub struct BlockFaces {
    /// `[u0, v0, u1, v1]` normalized atlas rect for the `+Y` face.
    pub up: [f32; 4],
    /// `+Y`'s opposite.
    pub down: [f32; 4],
    /// `-Z`.
    pub north: [f32; 4],
    /// `+Z`.
    pub south: [f32; 4],
    /// `+X`.
    pub east: [f32; 4],
    /// `-X`.
    pub west: [f32; 4],
}

/// Block name → resolved face textures.
///
/// Also carries a reserved solid-white texel for blocks this atlas didn't
/// resolve (so `mesh.rs` can tint that texel with `BlockRegistry`'s debug
/// color instead, keeping one uniform vertex format for both paths).
#[derive(Debug, Clone)]
pub struct Atlas {
    faces: HashMap<String, BlockFaces>,
    white_uv: [f32; 4],
}

impl Atlas {
    /// This block's resolved per-face texture rects, if vanilla's resource
    /// files described it as a plain, unrotated full cube.
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<&BlockFaces> {
        self.faces.get(name)
    }

    /// A 1×1 solid-white texel's atlas rect, for tinting with a flat debug
    /// color when a block has no resolved texture.
    #[must_use]
    pub const fn white_uv(&self) -> [f32; 4] {
        self.white_uv
    }

    /// Resolve and pack textures for exactly `names`, reading resource files
    /// under `assets_root` (an extracted client jar's `assets/minecraft`,
    /// see [`assets_root`]). Names this atlas can't resolve are simply
    /// absent from `lookup` — never an error; the caller already has a
    /// solid-color fallback (`mc_world::BlockRegistry`).
    #[must_use]
    pub fn build<'a>(
        assets_root: &Path,
        names: impl IntoIterator<Item = &'a str>,
    ) -> (Self, RgbaImage) {
        let refs: HashMap<String, FaceRefs> = names
            .into_iter()
            .filter_map(|name| Some((name.to_string(), resolve_block(assets_root, name)?)))
            .collect();
        pack(assets_root, &refs)
    }
}

/// `<cache>/mc-rust-client/<version>/client-extracted/assets/minecraft`.
///
/// A plain `unzip` of the pinned `client.jar` (never the jar's compiled
/// classes — just its `assets/` resource tree), produced once outside this
/// client (`docs/WORLD_PHYSICS_ASSETS.md`). Absent when the extraction
/// hasn't been done; callers degrade to no textures, same as an absent
/// `block-states.json`.
#[must_use]
pub fn assets_root(version: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .or_else(|| std::env::var_os("LOCALAPPDATA"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    let root = base.join("mc-rust-client").join(version).join("client-extracted/assets/minecraft");
    root.is_dir().then_some(root)
}

/// One resolved block's six face texture *references* (resource paths like
/// `"minecraft:block/stone"`), before atlas packing has assigned UV rects.
#[derive(Debug, Clone)]
struct FaceRefs {
    up: String,
    down: String,
    north: String,
    south: String,
    east: String,
    west: String,
}

impl FaceRefs {
    /// All six face references, in no particular order — not an [`Iterator`]
    /// itself (there's no lazy work to do over just six fields), so this
    /// returns a plain array rather than naming itself `iter`.
    fn as_paths(&self) -> [&str; 6] {
        [&self.up, &self.down, &self.north, &self.south, &self.east, &self.west]
    }
}

/// Vanilla base models a resolved parent chain must reach to count as a
/// plain full cube. All of these ship in the base resource pack with a
/// `textures` map keyed by generic slots (`all`, or `end`/`side`, or
/// `bottom`/`top`/`side`) that leaf models fill in via `#slot` references —
/// resolved generically by [`resolve_model`], not hardcoded per key here.
const CUBE_BASE_MODELS: [&str; 5] =
    ["cube_all", "cube_column", "cube_column_horizontal", "cube_bottom_top", "cube"];

fn resolve_block(assets_root: &Path, name: &str) -> Option<FaceRefs> {
    let short = name.strip_prefix("minecraft:").unwrap_or(name);
    let blockstate = read_json(&assets_root.join("blockstates").join(format!("{short}.json")))?;
    let model_ref = first_variant_model(&blockstate)?;

    let (textures, is_cube) = resolve_model(assets_root, model_ref)?;
    if !is_cube {
        return None;
    }
    let texture_of = |keys: &[&str]| -> Option<String> {
        keys.iter().find_map(|key| resolve_final(&textures, key))
    };
    let all = texture_of(&["all"]);
    let side = texture_of(&["side"]).or_else(|| all.clone());
    let top = texture_of(&["up", "top", "end"]).or_else(|| all.clone());
    let bottom = texture_of(&["down", "bottom", "end"]).or_else(|| all.clone());
    Some(FaceRefs {
        up: top?,
        down: bottom?,
        north: texture_of(&["north"]).or_else(|| side.clone())?,
        south: texture_of(&["south"]).or_else(|| side.clone())?,
        east: texture_of(&["east"]).or_else(|| side.clone())?,
        west: texture_of(&["west"]).or(side)?,
    })
}

/// The model reference for one representative variant. Prefers a variant
/// with no `x`/`y` rotation (e.g. an axis-property block's upright
/// orientation, like `oak_log`'s `axis=y`) over a rotated one, since this
/// resolver never applies blockstate rotation — an unrotated variant's
/// texture assignment (top/bottom vs. side) is the one that will actually
/// look right on the cube this client renders. Falls back to whichever
/// variant `serde_json`'s (unordered) map iteration yields first when every
/// variant is rotated (rotation only ever misplaces a few axis-property
/// blocks' side/end assignment, not the whole texture).
fn first_variant_model(blockstate: &serde_json::Value) -> Option<&str> {
    let variants = blockstate.get("variants")?.as_object()?;
    let unrotated = variants.values().find(|value| !is_rotated(value));
    let chosen = unrotated.or_else(|| variants.values().next())?;
    let entry = chosen.as_array().and_then(|list| list.first()).unwrap_or(chosen);
    entry.get("model")?.as_str()
}

fn is_rotated(variant: &serde_json::Value) -> bool {
    let entry = variant.as_array().and_then(|list| list.first()).unwrap_or(variant);
    entry.get("x").is_some() || entry.get("y").is_some()
}

/// Merge this model's own `textures` map over its resolved parent's (a
/// child's slot fill wins), following `parent` up to the root. Returns
/// whether the chain reached a [`CUBE_BASE_MODELS`] entry.
fn resolve_model(assets_root: &Path, model_ref: &str) -> Option<(HashMap<String, String>, bool)> {
    let short = model_ref.strip_prefix("minecraft:").unwrap_or(model_ref);
    let short = short.strip_prefix("block/").unwrap_or(short);
    let model = read_json(&assets_root.join("models/block").join(format!("{short}.json")))?;

    let (mut textures, mut is_cube) = match model.get("parent").and_then(serde_json::Value::as_str)
    {
        Some(parent) => resolve_model(assets_root, parent)?,
        None => (HashMap::new(), false),
    };
    is_cube |= CUBE_BASE_MODELS.contains(&short);

    if let Some(own) = model.get("textures").and_then(serde_json::Value::as_object) {
        for (key, value) in own {
            if let Some(text) = value.as_str() {
                // Stored raw (not resolved yet): a `#slot` reference here may
                // point at a texture variable a *later* (more-child) model in
                // the chain hasn't filled in yet — e.g. `cube_all.json` maps
                // every face to `#all`, but only a leaf model like
                // `stone.json` actually defines `all`. Resolving eagerly here
                // would freeze in the not-yet-defined reference; `resolve_final`
                // chases the final chain once the whole merge is done.
                textures.insert(key.clone(), text.to_string());
            }
        }
    }
    Some((textures, is_cube))
}

/// Follow a texture map entry's `#slot` chain (a child model's own value can
/// point at a slot only a more-parent model defined, or vice versa — see the
/// merge comment above) to a real path, breaking on a cycle or dead end.
fn resolve_final(textures: &HashMap<String, String>, key: &str) -> Option<String> {
    let mut current = textures.get(key)?;
    for _ in 0..8 {
        // Generous bound: real model chains nest at most a handful deep.
        match current.strip_prefix('#') {
            Some(slot) => current = textures.get(slot)?,
            None => return Some(current.clone()),
        }
    }
    None
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn load_texture(assets_root: &Path, texture: &str) -> Option<RgbaImage> {
    let short = texture.strip_prefix("minecraft:").unwrap_or(texture);
    let path = assets_root.join("textures").join(format!("{short}.png"));
    load_first_frame(&path)
}

fn load_first_frame(path: &Path) -> Option<RgbaImage> {
    let image = image::open(path).ok()?.to_rgba8();
    if image.width() != TILE || image.height() < TILE {
        return None;
    }
    Some(image::imageops::crop_imm(&image, 0, 0, TILE, TILE).to_image())
}

/// Load every texture `refs` points to (deduplicated — most blocks reuse a
/// handful of the same textures across faces), lay them out in a fixed-tile
/// grid plus one reserved white texel, and resolve each block's six
/// `FaceRefs` paths into real atlas UV rects. A block with any face texture
/// that fails to load (missing file — resource files are optional/partial)
/// is dropped entirely, same as an unresolved block.
fn pack(assets_root: &Path, refs: &HashMap<String, FaceRefs>) -> (Atlas, RgbaImage) {
    let mut unique_paths: Vec<&str> = Vec::new();
    for face_refs in refs.values() {
        for path in face_refs.as_paths() {
            if !unique_paths.contains(&path) {
                unique_paths.push(path);
            }
        }
    }

    let mut tiles: Vec<RgbaImage> = Vec::with_capacity(unique_paths.len() + 1);
    let mut tile_index: HashMap<&str, usize> = HashMap::new();
    for path in unique_paths {
        if let Some(image) = load_texture(assets_root, path) {
            tile_index.insert(path, tiles.len());
            tiles.push(image);
        }
    }
    let white_index = tiles.len();
    tiles.push(RgbaImage::from_pixel(TILE, TILE, image::Rgba([255, 255, 255, 255])));

    let tile_count = u32::try_from(tiles.len()).unwrap_or(u32::MAX);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    // Tile counts here are at most a few thousand (distinct block textures),
    // so both the square root and its ceiling fit comfortably in a u32.
    let columns = f64::from(tile_count).sqrt().ceil() as u32;
    let rows = tile_count.div_ceil(columns.max(1));
    let mut atlas_image = RgbaImage::new((columns * TILE).max(TILE), (rows * TILE).max(TILE));
    for (index, tile) in tiles.iter().enumerate() {
        let index = u32::try_from(index).unwrap_or(u32::MAX);
        let (col, row) = (index % columns, index / columns);
        image::imageops::replace(
            &mut atlas_image,
            tile,
            i64::from(col * TILE),
            i64::from(row * TILE),
        );
    }

    let (atlas_w, atlas_h) = (atlas_image.width(), atlas_image.height());
    #[allow(clippy::cast_precision_loss)] // Atlas dimensions are tiny (well under f32's limit).
    let uv_of = |index: usize| -> [f32; 4] {
        let index = u32::try_from(index).unwrap_or(u32::MAX);
        let (col, row) = (index % columns, index / columns);
        [
            (col * TILE) as f32 / atlas_w as f32,
            (row * TILE) as f32 / atlas_h as f32,
            ((col + 1) * TILE) as f32 / atlas_w as f32,
            ((row + 1) * TILE) as f32 / atlas_h as f32,
        ]
    };

    let mut faces = HashMap::with_capacity(refs.len());
    for (name, face_refs) in refs {
        let uv = |path: &str| tile_index.get(path).copied().map(uv_of);
        let Some(resolved) = (|| {
            Some(BlockFaces {
                up: uv(&face_refs.up)?,
                down: uv(&face_refs.down)?,
                north: uv(&face_refs.north)?,
                south: uv(&face_refs.south)?,
                east: uv(&face_refs.east)?,
                west: uv(&face_refs.west)?,
            })
        })() else {
            continue; // A referenced texture file was missing; skip this block.
        };
        faces.insert(name.clone(), resolved);
    }

    (Atlas { faces, white_uv: uv_of(white_index) }, atlas_image)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Atlas, first_variant_model, resolve_final};

    #[test]
    fn first_variant_model_reads_the_simple_single_variant_form() {
        let blockstate: serde_json::Value =
            serde_json::from_str(r#"{"variants":{"":{"model":"minecraft:block/stone"}}}"#).unwrap();
        assert_eq!(first_variant_model(&blockstate), Some("minecraft:block/stone"));
    }

    #[test]
    fn first_variant_model_reads_the_weighted_list_form() {
        let blockstate: serde_json::Value = serde_json::from_str(
            r#"{"variants":{"":[{"model":"minecraft:block/stone"},{"model":"minecraft:block/stone_mirrored"}]}}"#,
        )
        .unwrap();
        assert_eq!(first_variant_model(&blockstate), Some("minecraft:block/stone"));
    }

    #[test]
    fn first_variant_model_rejects_multipart_blockstates() {
        let blockstate: serde_json::Value =
            serde_json::from_str(r#"{"multipart":[{"apply":{"model":"x"}}]}"#).unwrap();
        assert_eq!(first_variant_model(&blockstate), None);
    }

    #[test]
    fn resolve_final_chases_a_slot_chain_and_passes_through_a_real_path() {
        let mut slots = std::collections::HashMap::new();
        slots.insert("up".to_string(), "#all".to_string());
        slots.insert("all".to_string(), "minecraft:block/stone".to_string());
        slots.insert("down".to_string(), "minecraft:block/dirt".to_string());
        assert_eq!(resolve_final(&slots, "up").as_deref(), Some("minecraft:block/stone"));
        assert_eq!(resolve_final(&slots, "down").as_deref(), Some("minecraft:block/dirt"));
        assert_eq!(resolve_final(&slots, "missing"), None);
    }

    #[test]
    fn resolve_final_breaks_on_a_reference_cycle_instead_of_looping() {
        let mut slots = std::collections::HashMap::new();
        slots.insert("a".to_string(), "#b".to_string());
        slots.insert("b".to_string(), "#a".to_string());
        assert_eq!(resolve_final(&slots, "a"), None);
    }

    #[test]
    fn assets_root_is_none_without_an_extracted_client() {
        // No real extraction is expected in CI; this just exercises the
        // absent-cache path without touching the developer's real cache.
        assert!(super::assets_root("no-such-version-marker").is_none());
    }

    #[test]
    fn build_with_no_names_yields_an_empty_atlas_with_a_white_texel() {
        let (atlas, image) = Atlas::build(Path::new("/nonexistent"), std::iter::empty());
        assert!(atlas.lookup("minecraft:stone").is_none());
        assert_eq!(image.width(), image.height());
        let [u0, v0, u1, v1] = atlas.white_uv();
        assert!(u1 > u0 && v1 > v0);
    }
}
