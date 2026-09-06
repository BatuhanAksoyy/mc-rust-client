// SPDX-License-Identifier: MIT OR Apache-2.0
//! wgpu renderer shell. See `docs/RENDER.md`.

mod app;
pub mod atlas;
pub mod fluid;
pub mod mesh;
mod renderer;

pub use app::{Game, InputState, RunError, run};
pub use renderer::{Renderer, RendererError};

/// Rendering backend selection. `wgpu` picks the best available
/// (Vulkan/Metal/DX12/GL) — mirrors vanilla 26.2 Default/GL/Vulkan option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicsApi {
    /// Let the renderer choose a supported backend.
    Default,
    /// Prefer OpenGL when available.
    PreferGl,
    /// Prefer Vulkan when available.
    PreferVulkan,
}
