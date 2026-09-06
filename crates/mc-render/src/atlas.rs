//! Block name → texture resolution and atlas packing (`docs/RENDER.md`
//! milestone 3).
//!
//! Reads runtime-loaded Minecraft *resource* files only: blockstate
//! JSON, block-model JSON and 16×16 block-texture PNGs, all data/asset files
//! Mojang ships for any resource pack or mod loader to read — never Java
//! source, never committed (`docs/WORLD_PHYSICS_ASSETS.md`'s ASSETS policy).
//!
//! State properties select variants and multipart components. Parent models,
//! cuboid elements, authored/default UVs, element rotations, cull faces,
//! tints, and orthogonal block-state transforms are baked once per state.

mod blockstate;
mod model;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use mc_world::BlockState;
use model::{ModelRefs, resolve_block};

pub use image::RgbaImage;

/// One block texture is always 16×16 in vanilla's base resource pack. A
/// resource pack's own animated textures are taller (stacked frames); we
/// only ever read the first frame (see [`load_first_frame`]).
const TILE: u32 = 16;

/// One reusable, resource-model-authored block quad.
#[derive(Debug, Clone, Copy)]
pub struct BakedQuad {
    /// Block-local positions for the quad's four corners.
    pub positions: [[f32; 3]; 4],
    /// Normalized atlas UV at each corresponding corner.
    pub uv: [[f32; 2]; 4],
    /// Whether the resource model marks this face with a tint index.
    pub tinted: bool,
    /// Neighbor offset that may hide this quad; `None` means never cull it.
    pub cull: Option<(i32, i32, i32)>,
    /// Model-authored directional shading multiplier.
    pub brightness: f32,
}

/// All baked quads selected for one numeric block state.
#[derive(Debug, Clone)]
pub struct BakedModel {
    /// Quads from the selected variant or all matching multipart components.
    pub quads: Vec<BakedQuad>,
    /// Whether this state has a real collision box a player is blocked by
    /// (see [`Atlas::is_solid`]).
    pub solid: bool,
}

/// Block name → resolved face textures.
///
/// Also carries a reserved solid-white texel for blocks this atlas didn't
/// resolve (so `mesh.rs` can tint that texel with `BlockRegistry`'s debug
/// color instead, keeping one uniform vertex format for both paths).
#[derive(Debug, Clone)]
pub struct Atlas {
    models: HashMap<u32, BakedModel>,
    white_uv: [f32; 4],
}

impl Atlas {
    /// This numeric block state's resolved per-face texture information.
    #[must_use]
    pub fn lookup(&self, id: u32) -> Option<&BakedModel> {
        self.models.get(&id)
    }

    /// Whether `id` has a real collision box, if this atlas resolved it —
    /// `None` when it didn't (caller should keep its own solid-by-default
    /// fallback, same as an unresolved texture).
    #[must_use]
    pub fn is_solid(&self, id: u32) -> Option<bool> {
        self.models.get(&id).map(|model| model.solid)
    }

    /// A 1×1 solid-white texel's atlas rect, for tinting with a flat debug
    /// color when a block has no resolved texture.
    #[must_use]
    pub const fn white_uv(&self) -> [f32; 4] {
        self.white_uv
    }

    /// Resolve and pack textures for exactly `states`, reading resource files
    /// under `assets_root` (an extracted client jar's `assets/minecraft`,
    /// see [`assets_root`]). Names this atlas can't resolve are simply
    /// absent from `lookup` — never an error; the caller already has a
    /// solid-color fallback (`mc_world::BlockRegistry`).
    #[must_use]
    pub fn build<'a>(
        assets_root: &Path,
        states: impl IntoIterator<Item = (u32, &'a BlockState)>,
    ) -> (Self, RgbaImage) {
        let refs: HashMap<u32, ModelRefs> = states
            .into_iter()
            .filter_map(|(id, state)| Some((id, resolve_block(assets_root, state)?)))
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
fn pack(assets_root: &Path, refs: &HashMap<u32, ModelRefs>) -> (Atlas, RgbaImage) {
    let mut unique_paths: Vec<&str> = Vec::new();
    for model_refs in refs.values() {
        for path in model_refs.paths() {
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

    let mut models = HashMap::with_capacity(refs.len());
    for (id, model_refs) in refs {
        let Some(quads) = model_refs
            .quads
            .iter()
            .map(|quad| {
                let [u0, v0, u1, v1] = tile_index.get(quad.path.as_str()).copied().map(uv_of)?;
                let uv = quad.uv.map(|[u, v]| {
                    [(u / 16.0).mul_add(u1 - u0, u0), (v / 16.0).mul_add(v1 - v0, v0)]
                });
                Some(BakedQuad {
                    positions: quad.positions,
                    uv,
                    tinted: quad.tinted,
                    cull: quad.cull.map(model::Direction::offset),
                    brightness: quad.brightness,
                })
            })
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        models.insert(*id, BakedModel { quads, solid: model_refs.solid });
    }

    (Atlas { models, white_uv: uv_of(white_index) }, atlas_image)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use mc_world::BlockRegistry;

    use super::Atlas;

    #[test]
    fn assets_root_is_none_without_an_extracted_client() {
        // No real extraction is expected in CI; this just exercises the
        // absent-cache path without touching the developer's real cache.
        assert!(super::assets_root("no-such-version-marker").is_none());
    }

    #[test]
    fn build_with_no_names_yields_an_empty_atlas_with_a_white_texel() {
        let (atlas, image) = Atlas::build(Path::new("/nonexistent"), std::iter::empty());
        assert!(atlas.lookup(1).is_none());
        assert_eq!(image.width(), image.height());
        let [u0, v0, u1, v1] = atlas.white_uv();
        assert!(u1 > u0 && v1 > v0);
    }

    /// Developer coverage probe for the pinned external cache. It is ignored
    /// in CI because neither Mojang assets nor the derived state registry may
    /// be committed.
    #[test]
    #[ignore = "requires the external 26.2 asset and block-state caches"]
    fn cached_assets_bake_representative_model_families() {
        let root = super::assets_root("26.2").expect("extract the pinned client assets first");
        let registry = BlockRegistry::load_cached("26.2");
        let states: Vec<_> =
            (0..).map_while(|id| registry.state(id).map(|state| (id, state))).collect();
        let (atlas, _) = Atlas::build(&root, states.iter().copied());
        let resolved = states.iter().filter(|(id, _)| atlas.lookup(*id).is_some()).count();
        eprintln!("baked {resolved}/{} cached block states", states.len());
        let mut unresolved_names = states
            .iter()
            .filter(|(id, _)| atlas.lookup(*id).is_none())
            .map(|(_, state)| state.name.as_ref())
            .collect::<Vec<_>>();
        unresolved_names.sort_unstable();
        unresolved_names.dedup();
        eprintln!("unresolved block families: {}", unresolved_names.join(", "));
        for name in [
            "minecraft:birch_log",
            "minecraft:grass_block",
            "minecraft:oak_stairs",
            "minecraft:oak_fence",
            "minecraft:short_grass",
            "minecraft:torch",
        ] {
            let (id, _) = states.iter().find(|(_, state)| state.name.as_ref() == name).unwrap();
            assert!(atlas.lookup(*id).is_some(), "failed to bake {name}");
        }
    }
}
