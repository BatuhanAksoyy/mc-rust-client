//! Block name → texture resolution and atlas packing (`docs/RENDER.md`
//! milestone 3).
//!
//! Reads runtime-loaded Minecraft *resource* files only: blockstate
//! JSON, block-model JSON and 16×16 block-texture PNGs, all data/asset files
//! Mojang ships for any resource pack or mod loader to read — never Java
//! source, never committed (`docs/WORLD_PHYSICS_ASSETS.md`'s ASSETS policy).
//!
//! State properties select a variant, and orthogonal blockstate/model UV
//! rotations are preserved for single-element full cubes. Multipart and
//! non-cube models remain on the debug-color fallback.

mod model;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use mc_world::BlockState;
use model::{FaceRef, FaceRefs, resolve_block};

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
pub struct Face {
    /// `[u0, v0, u1, v1]` normalized atlas rectangle.
    pub rect: [f32; 4],
    /// Tile-local UV at each of the mesher face's four geometry corners.
    pub uv: [[f32; 2]; 4],
    /// Whether the resource model marks this face with a tint index.
    pub tinted: bool,
}

/// A resolved block state's texture, one entry per cube face.
#[derive(Debug, Clone, Copy)]
pub struct BlockFaces {
    /// `+Y` face.
    pub up: Face,
    /// `+Y`'s opposite.
    pub down: Face,
    /// `-Z`.
    pub north: Face,
    /// `+Z`.
    pub south: Face,
    /// `+X`.
    pub east: Face,
    /// `-X`.
    pub west: Face,
}

/// Block name → resolved face textures.
///
/// Also carries a reserved solid-white texel for blocks this atlas didn't
/// resolve (so `mesh.rs` can tint that texel with `BlockRegistry`'s debug
/// color instead, keeping one uniform vertex format for both paths).
#[derive(Debug, Clone)]
pub struct Atlas {
    faces: HashMap<u32, BlockFaces>,
    white_uv: [f32; 4],
}

impl Atlas {
    /// This numeric block state's resolved per-face texture information.
    #[must_use]
    pub fn lookup(&self, id: u32) -> Option<&BlockFaces> {
        self.faces.get(&id)
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
        let refs: HashMap<u32, FaceRefs> = states
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
fn pack(assets_root: &Path, refs: &HashMap<u32, FaceRefs>) -> (Atlas, RgbaImage) {
    let mut unique_paths: Vec<&str> = Vec::new();
    for face_refs in refs.values() {
        for path in face_refs.paths() {
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
    for (id, face_refs) in refs {
        let face = |face: &FaceRef| {
            Some(Face {
                rect: tile_index.get(face.path.as_str()).copied().map(uv_of)?,
                uv: face.uv,
                tinted: face.tinted,
            })
        };
        let Some(resolved) = (|| {
            Some(BlockFaces {
                up: face(&face_refs.0[0])?,
                down: face(&face_refs.0[1])?,
                north: face(&face_refs.0[2])?,
                south: face(&face_refs.0[3])?,
                east: face(&face_refs.0[4])?,
                west: face(&face_refs.0[5])?,
            })
        })() else {
            continue; // A referenced texture file was missing; skip this block.
        };
        faces.insert(*id, resolved);
    }

    (Atlas { faces, white_uv: uv_of(white_index) }, atlas_image)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

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
}
