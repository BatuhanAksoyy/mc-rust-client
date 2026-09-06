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
1. **Done.** Triangle → textured cube (wgpu example parity) — folded into milestone 2's
   pipeline directly rather than as a separate throwaway example.
2. **Done.** Single chunk render from synthetic data (no assets needed). `mesh::mesh_chunk`
   culls internal faces and bakes a fixed per-face brightness (no atlas, no real lighting
   yet); `renderer::Renderer` owns the wgpu device/surface/pipeline. `mc-client render <host>`
   wires it to a live join. Verified against a real local Pumpkin server (window registers
   as a foreground GUI app in the window server; 24 sections / 55,488 vertices meshed from
   a real chunk).
2b. **Done.** Real first-person movement/camera, still holding no game logic here:
   `app::Game`/`InputState` are the seam — `App` (this crate) owns the window, held
   keys, and accumulated mouse-look delta, and hands them to a caller-supplied `Game`
   once per frame; `mc-client`'s `play::RenderGame` is the concrete `Game`, owning
   physics (`WORLD_PHYSICS_ASSETS.md`) and the view-projection matrix. Mouse is
   captured (`CursorGrabMode::Locked`, falling back to `Confined`) for FPS-style look.
   The old `camera::OrbitCamera` is gone — nothing used it once real look/move input
   landed.
2c. **Done.** Render the complete initial chunk view returned by `join`, including
   post-spawn batches collected for the CLI's adjustable `--render-distance` (2–8,
   default 4), positioned
   relative to the chunk containing the server-confirmed spawn. Chunk X/Z are world
   coordinates in 16-block units; translating them around that nearby origin keeps GPU
   coordinates small. Cull faces across loaded chunk boundaries. If the spawn chunk was
   not part of the pre-spawn batch, fall back to the server's chunk-cache center and then
   the first received chunk. Player collision remains limited to the selected origin
   chunk until the gameplay network loop and a multi-chunk world store land; the camera
   therefore still uses a safe local spawn above that chunk rather than pretending the
   server teleport's absolute Y is relative to the first decoded section.
3. **Partly done.** `atlas::Atlas` resolves real block textures from a locally extracted
   client jar (`WORLD_PHYSICS_ASSETS.md`'s ASSETS section): blockstate → model → texture,
   following both `variants` (`atlas::blockstate`, incl. `OR`/`AND` multipart
   `when` conditions) and `multipart` selection against cached block-state
   properties, then the model `parent`/`#slot` chain. Bakes arbitrary
   authored elements per state (`atlas::model`) — not just single-element full
   cubes — so fences, walls, stairs, and other multi-element/non-cube models
   mesh with their real per-element cuboids, cull faces, and tint indices;
   orthogonal block-state `x`/`y` transforms and `uvlock` rotate both
   positions and UVs together. Coverage checked against the full cached
   26.2 block-state list (`atlas::tests::cached_assets_bake_representative_model_families`,
   `#[ignore]`d — needs the local asset/state caches): only blocks Java
   itself renders via a block-entity/BER (signs, banners, skulls, chests,
   shulker boxes, heads, decorated pots, the copper golem statue), non-model
   blocks (air variants, light, barrier, conduit, bubble column, moving
   piston, end portal/gateway, structure void — fluids are handled
   separately, below), and one hanging-sign rotation's compound (non
   `angle`+`axis`) element rotation stay unresolved — `mesh_chunk` falls back
   to `BlockRegistry`'s solid debug color for those.
   Each resolved state also carries a `solid` flag (`Atlas::is_solid`),
   following the resource model's own `ambientocclusion` (vanilla sets it
   `false` on exactly the walk-through decorations — cross-shaped plants,
   torches, redstone components, rails, ... — and leaves it at the default
   for structural partial shapes like fences/walls/stairs); `mc-client`
   feeds the resolved non-solid IDs into `BlockRegistry::with_non_solid` so
   collision matches what actually rendered instead of treating every
   non-air block as a full solid cube. Each resolved state also carries an
   `opaque` flag (`Atlas::is_opaque`), independent of `solid`: read directly
   off the resource pack's own texture pixels (fully alpha-255 or not), not
   from any model flag, since solidity and texture transparency are
   unrelated in Java (leaves and glass are full cubes with a real collision
   box — `solid` — whose texture still has alpha gaps or blend — not
   `opaque`). `mesh_chunk`'s own neighbor-face culling (`mesh.rs`'s
   `occludes`) requires *both* flags rather than `is_air` alone: a
   cross-shaped decoration is non-air but covers almost none of a
   neighboring face (fails `solid`), and leaves/glass are solid full cubes
   but don't fully cover the face either (fail `opaque`) — either way, a
   block standing next to or under one (a mushroom, a flower, tall grass, a
   log behind leaves, ...) keeps that face instead of having it culled away
   as if the neighbor were a real, fully-covering occluder. `mc-client`
   feeds both resolved ID sets into `BlockRegistry` (`with_non_solid` and
   `with_non_opaque`) *before* meshing, not just before physics — the mesher
   needs the same view collision uses, or the fix has no effect on what's
   actually drawn.
   The atlas is a real mip chain now (`Atlas::build`'s second return value:
   5 levels, 16×16 down to 1×1 — `atlas::MIP_LEVELS`), each tile's own chain
   built and packed in isolation *before* laying tiles into the shared
   atlas image, so a lower level never blends pixels across an unrelated
   neighboring tile's border (the standard atlas-mipmapping pitfall). The
   sampler stays nearest-filtered within a level (preserving the blocky
   look up close) but blends linearly between levels — without this, any
   textured surface more than a couple of tiles away, or at a grazing
   angle, aliased into shimmering noise; water's own thin, height-varying
   quads (the only geometry here that ever spans less than a full block
   vertically) suffered far worse than anything else, visible as flickering
   right at a shoreline. No anisotropic filtering yet; lighting/fog still
   fixed per-face brightness.
   Tinted faces (`tintindex`, real per-biome colormap sampling in Java)
   multiply by `BlockRegistry::color`'s flat curated-by-name/hashed-by-ID
   stand-in (no biome data yet) instead — grass-family cross props
   (`short_grass`, `fern`, `large_fern`, `sugar_cane`, `bamboo_sapling`,
   `bush`, `potted_fern`) and `vine`/`lily_pad` are now curated (grass/foliage
   green) so they read as green instead of an unrelated per-ID hash color
   that could read as washed-out/"see-through" against real terrain.
   Fluids (water/lava, `mesh::fluid`) have no blockstate/model JSON at all —
   Java renders them from internal logic, not resource-pack data — so they
   bypass `atlas::model` entirely: `fluid::level` reads the `level` property,
   `fluid::own_height`/`corner_height` follow the fluid-height and
   corner-blending convention long publicly documented and independently
   reimplemented by non-Mojang tools (never decompiled source), and
   `mesh_fluid_block` bakes a real sloped top (flat bottom, side faces
   following the slope) instead of a flat full cube. `Atlas::fluid_uv` packs
   the two fixed `water_still`/`lava_still` textures (their real vanilla
   paths, unconditionally — no model ever references them) into the same
   atlas; water is tinted with `BlockRegistry::color`'s flat blue (its
   texture is grayscale, meant for a multiply, same as grass), lava isn't
   (its texture already carries real color). `mc-client` also marks both
   fluids non-solid (`build_atlas`), so a player passes through rather than
   colliding with an invisible wall — real swimming physics (buoyancy,
   speed, breath) is separate, later work. Animation (both textures are
   32-frame flipbooks) and the flowing texture's directional alignment are
   not implemented yet — only the first frame of the *still* texture is used
   on every face, same simplification already applied to every other
   animated texture in this atlas. Water's real alpha (baked into
   `water_still` itself, ~0.7) draws through its own pass (`Mesh::translucent`,
   `Renderer`'s `translucent_pipeline`): same shader and bind group as
   everything else, drawn after the opaque/cutout pass in the same render
   pass. The translucent pipeline writes depth too: this gives overlapping
   water surfaces a deterministic nearest-surface winner instead of allowing
   camera motion to reorder coplanar alpha blends frame to frame. Lava draws
   through the ordinary opaque/cutout pass instead — its
   texture has no real alpha variation, so it needs no special treatment.
   The camera projection (`play.rs`'s `view_projection`) is reversed-Z
   (`perspective_infinite_reverse`, matching `Renderer`'s depth clear value
   0.0 and `GreaterEqual` compare) rather than a finite far plane: a
   standard depth buffer spends almost all of a float32 depth's precision
   within the first few meters of `near` no matter how far the chosen far
   plane actually is, so any two surfaces close to each other in world
   space but far from the camera — a water surface stretching toward the
   horizon, viewed at a grazing angle, most of all — were left fighting
   over what little precision remained, visible as flickering noise right
   at the surfaces' shared edge.
   Still needs a visual diff test against real screenshots.
4. Entity/block-entity pass stub, UI (egui/wgpu) for debug HUD (FPS, ms, draw calls).
5. Perf: `criterion` benches for mesher; `tracy`/`puffin` scopes; target 60 FPS @ 12 chunks on M1/GTX 1060 class.

## Rules

- All GPU code behind `Renderer` trait for headless tests (`wgpu --features headless` or mock).
- Shaders in WGSL, formatted, reviewed. No `unsafe` except inside `wgpu` as required (workspace forbids `unsafe_code` — use `#[allow]` locally with SAFETY comment if ever needed).
- Screenshots as test artifacts only, never commit Mojang textures.
