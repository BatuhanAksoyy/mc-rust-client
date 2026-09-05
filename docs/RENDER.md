# RENDER.md — wgpu renderer plan (why wgpu, 26.2 Vulkan notes)

Decision: **`wgpu` + `winit`, no Bevy engine** (see README). Bevy uses wgpu internally;
we skip the engine overhead for chunk-meshing hot loops and exact timing control.

## 26.2 context (fetched)

- Vanilla 26.2 adds **experimental Vulkan renderer** + Friends List. Graphics API option:
  Default / Prefer OpenGL / Vulkan (fallback to OpenGL on bugs).
- Vanilla libs confirm both paths: `lwjgl-opengl`, `lwjgl-vulkan`, `shaderc`, `spvc`, `vma`.
- Our `wgpu` covers Vulkan (Linux/Windows), Metal (macOS), DX12 (Windows), GL (compat) from one API —
  closest to "support latest + cross-platform + better FPS".

## Architecture (`mc-render`, no game logic)

- `Renderer`: device/queue/surface, shader modules (WGSL), bind groups (atlas, lightmap, uniforms).
- `Atlas`: bakes block textures from runtime-loaded assets (cache only) into array texture; mipmaps; anisotropy.
- `Mesher`: chunk → greedy/culled mesh (opaque/cutout/translucent passes), face culling, ambient occlusion-lite, lightmap UVs. Multithread via `rayon` or worker pool; incremental remesh on block edit.
- `Culling`: frustum + distance + occlusion (Hi-Z later, YAGNI now).
- `Frame`: uniform buffers (view/proj, fog, time), draw indirect where measurable.

Milestones:
1. Triangle → textured cube (wgpu example parity).
2. Single chunk render from synthetic data (no assets needed).
3. Atlas + lighting + fog matching vanilla screenshots (visual diff test, assets from cache).
4. Entity/block-entity pass stub, UI (egui/wgpu) for debug HUD (FPS, ms, draw calls).
5. Perf: `criterion` benches for mesher; `tracy`/`puffin` scopes; target 60 FPS @ 12 chunks on M1/GTX 1060 class.

## Rules

- All GPU code behind `Renderer` trait for headless tests (`wgpu --features headless` or mock).
- Shaders in WGSL, formatted, reviewed. No `unsafe` except inside `wgpu` as required (workspace forbids `unsafe_code` — use `#[allow]` locally with SAFETY comment if ever needed).
- Screenshots as test artifacts only, never commit Mojang textures.
