// SPDX-License-Identifier: MIT OR Apache-2.0
//! wgpu renderer shell. See `docs/RENDER.md`.

/// Rendering backend selection. wgpu picks the best available
/// (Vulkan/Metal/DX12/GL) — mirrors vanilla 26.2 Default/GL/Vulkan option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicsApi {
    Default,
    PreferGl,
    PreferVulkan,
}
