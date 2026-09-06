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
    /// Whether every texture this state uses is fully opaque (see
    /// [`Atlas::is_opaque`]).
    pub opaque: bool,
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
    /// `block/water_still`'s atlas rect, if the resource pack had it (see
    /// [`Self::fluid_uv`]).
    water_uv: Option<[f32; 4]>,
    /// `block/lava_still`'s atlas rect, same fallback rule as `water_uv`.
    lava_uv: Option<[f32; 4]>,
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

    /// Whether every texture `id` resolved to is fully opaque, if this atlas
    /// resolved it — `None` when it didn't, same fallback rule as
    /// [`Self::is_solid`]. A block can be solid (has a collision box) and
    /// still not opaque: leaves and glass are full cubes a player collides
    /// with, but their textures have real alpha gaps (leaves' cutout,
    /// glass's transparency), so a neighbor sitting behind one should still
    /// render its shared face instead of being culled as if the leaves/glass
    /// were a real, fully-covering occluder.
    #[must_use]
    pub fn is_opaque(&self, id: u32) -> Option<bool> {
        self.models.get(&id).map(|model| model.opaque)
    }

    /// `kind`'s still texture's atlas rect, `None` when the resource pack
    /// didn't have it (same absent-asset fallback as everything else here).
    #[must_use]
    pub const fn fluid_uv(&self, kind: crate::fluid::FluidKind) -> Option<[f32; 4]> {
        match kind {
            crate::fluid::FluidKind::Water => self.water_uv,
            crate::fluid::FluidKind::Lava => self.lava_uv,
        }
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

/// Whether every pixel in `image` is fully opaque (alpha 255) — a plain
/// stone/dirt-style texture, versus a cutout (leaves, saplings) or
/// translucent (glass, ice) one with real alpha variation.
fn texture_is_opaque(image: &RgbaImage) -> bool {
    image.pixels().all(|pixel| pixel.0[3] == 255)
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
    // Fluids (water/lava) have no blockstate/model JSON at all (`fluid.rs`'s
    // own doc comment) — their two textures aren't referenced by any
    // `ModelRefs`, so pack them into the same atlas unconditionally instead.
    for path in FLUID_TEXTURES {
        if !unique_paths.contains(&path) {
            unique_paths.push(path);
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
    // Whether each loaded tile is fully opaque (no pixel with alpha < 255) —
    // read straight off the resource pack's own texture data, the same
    // signal Java's block-render-layer assignment (opaque/cutout/translucent)
    // ultimately reflects, without needing that assignment itself (compiled
    // into the Java client, not resource-pack data we can read).
    let tile_opaque: Vec<bool> = tiles.iter().map(texture_is_opaque).collect();
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
        let mut opaque = true;
        let Some(quads) = model_refs
            .quads
            .iter()
            .map(|quad| {
                let index = *tile_index.get(quad.path.as_str())?;
                opaque &= tile_opaque[index];
                let [u0, v0, u1, v1] = uv_of(index);
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
        models.insert(*id, BakedModel { quads, solid: model_refs.solid, opaque });
    }

    let water_uv = tile_index.get(FLUID_TEXTURES[0]).copied().map(uv_of);
    let lava_uv = tile_index.get(FLUID_TEXTURES[1]).copied().map(uv_of);
    (Atlas { models, white_uv: uv_of(white_index), water_uv, lava_uv }, atlas_image)
}

/// Fixed vanilla asset paths for the two fluids' still texture (`fluid.rs`).
/// No resource-pack data points at these — there's no model to reference
/// them from — so `pack` loads them by this hardcoded path instead, the same
/// way it already reserves a fixed white texel for the debug-color fallback.
const FLUID_TEXTURES: [&str; 2] = ["block/water_still", "block/lava_still"];

#[cfg(test)]
mod tests {
    use std::path::Path;

    use mc_world::{BlockRegistry, BlockState};

    use super::Atlas;

    /// A block's collision shape (`Atlas::is_solid`) and its texture's alpha
    /// (`Atlas::is_opaque`) are independent: a full cube with a cutout
    /// texture (leaves, in effect) is solid but not opaque, and should not
    /// be conflated with a plain fully-opaque cube of the same shape.
    #[test]
    fn is_opaque_reflects_the_textures_actual_alpha_not_the_models_shape() {
        let root = std::env::temp_dir().join(format!(
            "mc-rust-client-atlas-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let write_json = |relative: &str, contents: &str| {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        };
        let write_texture = |relative: &str, image: &super::RgbaImage| {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            image.save(path).unwrap();
        };
        let full_cube = r##"{"textures":{"all":"block/#NAME#"},
            "elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"north":{"texture":"#all"}}}]}"##;

        write_json("models/block/test_cutout.json", &full_cube.replace("#NAME#", "test_cutout"));
        write_json(
            "blockstates/test_cutout.json",
            r#"{"variants":{"":{"model":"block/test_cutout"}}}"#,
        );
        let mut cutout = super::RgbaImage::from_pixel(16, 16, image::Rgba([0, 200, 0, 255]));
        cutout.put_pixel(0, 0, image::Rgba([0, 0, 0, 0])); // One transparent texel: a real cutout gap.
        write_texture("textures/block/test_cutout.png", &cutout);

        write_json("models/block/test_opaque.json", &full_cube.replace("#NAME#", "test_opaque"));
        write_json(
            "blockstates/test_opaque.json",
            r#"{"variants":{"":{"model":"block/test_opaque"}}}"#,
        );
        let opaque = super::RgbaImage::from_pixel(16, 16, image::Rgba([120, 90, 60, 255]));
        write_texture("textures/block/test_opaque.png", &opaque);

        let states = [
            BlockState {
                name: "test_cutout".into(),
                properties: std::collections::BTreeMap::new(),
            },
            BlockState {
                name: "test_opaque".into(),
                properties: std::collections::BTreeMap::new(),
            },
        ];
        let (atlas, _) = Atlas::build(&root, [(0, &states[0]), (1, &states[1])]);
        std::fs::remove_dir_all(&root).ok();

        assert_eq!(
            atlas.is_opaque(0),
            Some(false),
            "a texture with any alpha<255 pixel isn't opaque"
        );
        assert_eq!(atlas.is_opaque(1), Some(true), "a fully alpha=255 texture is opaque");
    }

    /// Fluids have no blockstate/model JSON at all (`fluid.rs`), so `pack`
    /// must load their two fixed texture paths unconditionally — not as a
    /// side effect of resolving any block state.
    #[test]
    fn fluid_uv_resolves_the_fixed_texture_paths_when_present() {
        let root = std::env::temp_dir().join(format!(
            "mc-rust-client-atlas-fluid-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let write_texture = |relative: &str| {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            super::RgbaImage::from_pixel(16, 16, image::Rgba([180, 180, 180, 180]))
                .save(path)
                .unwrap();
        };
        write_texture("textures/block/water_still.png"); // No lava_still: exercises the `None` side too.

        let (atlas, _) = Atlas::build(&root, std::iter::empty());
        std::fs::remove_dir_all(&root).ok();

        assert!(atlas.fluid_uv(crate::fluid::FluidKind::Water).is_some());
        assert!(atlas.fluid_uv(crate::fluid::FluidKind::Lava).is_none());
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
        // Fluids never resolve via `lookup` (no blockstate/model JSON), but
        // the real `water_still`/`lava_still` textures should still load.
        assert!(
            atlas.fluid_uv(crate::fluid::FluidKind::Water).is_some(),
            "failed to bake water_still"
        );
        assert!(
            atlas.fluid_uv(crate::fluid::FluidKind::Lava).is_some(),
            "failed to bake lava_still"
        );
    }
}
