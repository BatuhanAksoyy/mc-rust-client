// SPDX-License-Identifier: MIT OR Apache-2.0
//! Launcher: resolve piston-meta, verify SHA1, bootstrap cache. Never vendors jars.

pub mod pumpkin;

/// Piston endpoints (see `docs/SOURCE_OF_TRUTH.md`).
pub const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
/// Pinned 26.2 version JSON (release 2026-06-16).
pub const VERSION_26_2_URL: &str =
    "https://piston-meta.mojang.com/v1/packages/3592ebc61c6b6c33bb8228fe5a9e90221df0be68/26.2.json";
/// Expected SHA1 of the cached client archive.
pub const CLIENT_SHA1_26_2: &str = "2dc72797acbc1b63fc16a11c4ac393605f453754";
/// Expected SHA1 of the cached server archive.
pub const SERVER_SHA1_26_2: &str = "823e2250d24b3ddac457a60c92a6a941943fcd6a";
