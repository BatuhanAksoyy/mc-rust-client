# WORLD.md + PHYSICS.md (combined stub — split when >300 lines)

## WORLD (`mc-world`)

- Sources: Anvil region format (`Region Files` spec on wiki.vg), chunk NBT, heightmaps, biomes, lighting (block+sky 0-15).
- 26.2 additions: sulfur caves biome, sulfur/cinnabar block variants, sulfur cube mob (behavior later).
- Implementation: `BlockId` registry (from Registry Data packet, not hardcoded), `Chunk { sections[16³], biome, light }`, `World { chunks: HashMap<ChunkPos, Chunk> }`.
- No I/O in core types; `mc-launcher`/`mc-client` handle region file loading.
- Tests: synthetic chunks + recorded vanilla chunk blob from cache (never committed).

## PHYSICS / TICK (`mc-client`)

- Tick 20 TPS fixed timestep, render interpolation; network on `tokio`.
- SPEC: the foundation scheduler accepts elapsed durations and a nonzero maximum
  catch-up count. It returns whole ticks to execute, interpolation in [0, 1), and
  dropped whole-tick time when overloaded. It preserves fractional time exactly,
  owns no wall clock, and never sleeps. This policy is tested separately from
  future movement/physics parity. See `FOUNDATION.md`.
- Re-implement from observation + wiki, verified by tests:
  - Movement: sprint 5.6 m/s, walk 4.3, sneak 1.3, jump vY 0.42, gravity 0.08/tick, drag 0.98/0.91.
  - Collision: AABB vs voxel grid, step, fluids, ladders (add per-behavior tests).
- "Feels like Java": input handling (GLFW codes documented, we use `winit` codes mapped identically), FOV, sensitivity, particles/sounds timing.
- Keep `tick()` deterministic + unit-testable without renderer/network.

## ASSETS (runtime only)

- Load from `~/.minecraft` install or Mojang CDN post-auth into `~/.cache/mc-rust-client/26.2/`.
- Index `32.json` (5057 objects, ~480 MB total). Fetch on demand (atlas textures, lang, sounds), verify SHA1.
- Never commit. `.gitignore` + CI guard enforce.
