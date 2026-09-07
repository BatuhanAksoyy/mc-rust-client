# mc-rust-client — 1:1 Minecraft Java Edition 26.2 client in Rust (docs-first)

Target: **Minecraft Java Edition 26.2 "Chaos Cubed" (release 2026-06-16, protocol 776)**.
Goal: feel exactly like Java Edition, with better FPS, cross-platform, Rust modding later.

> **Legal hard rule:** this repo NEVER contains Mojang code, decompiled source,
> `client.jar`/`server.jar`, assets, or mappings dumps. See `docs/LEGAL.md`.
> Reference material is fetched at build time to the local machine cache only.

## Layout

```
docs/               # source of truth for humans + AI agents (read these first)
                      # PACKETS-776.md (packet inventory), P1-PROTOCOL.md (next contract),
                      # AUTH.md + LEGAL.md (deferred to P6 — do not block implementation)
crates/
  mc-protocol/      # protocol 776 types, packets, NBT, compression, encryption
  mc-auth/          # Microsoft OAuth2 -> Xbox -> Minecraft auth + ownership gate
  mc-world/         # blocks, chunks, lighting, region format
  mc-render/        # wgpu renderer (Vulkan/Metal/DX12/GL via wgpu)
  mc-client/        # game loop, tick, input, audio, UI
  mc-launcher/      # version resolve, asset bootstrap (runtime only)
xtask/              # `cargo xtask ...` automation (fetch/verify, no vendoring)
scripts/            # shell helpers (fetch only to cache dir, never into repo)
```

## AI workflow

1. Read `AGENTS.md`, then `docs/AI-GUIDE.md`.
2. Pick the smallest task in `docs/ROADMAP.md`. Follow YAGNI/KISS/DRY.
3. Never paste Mojang source. Re-implement from specs + observed behavior.
4. Every push to `main` must pass `cargo fmt --check`, `clippy -D warnings`, `test`, `deny`, `audit`.

## Launch locally

Requires Python 3 and the Rust toolchain. In two terminals, from this directory:

```sh
./launch-server
# Wait for server readiness, then in the second terminal:
./launch-client
```

On Windows use `python launch-server` and `python launch-client`.
The server downloads and verifies the pinned Pumpkin executable into the current
working directory's `.data/bin/`, saving its world and logs in `.data/server/`.
Downloads are reused. Stop the server with Ctrl-C to save. No Java is required.
Both scripts build in release mode and accept `--help`; pass `--port 25566` to
both to change ports. `./launch-server --check` verifies startup and shutdown.

Run `./download-assets` once before launching for real block textures (Windows:
`python download-assets`). Setup requires Python 3 and Java matching Minecraft
26.2; it downloads verified archives and generates the block-state registry in
`.data`. Run it and `launch-client` from the same working directory. The client
launcher automatically uses the completed local asset cache.

The client opens the current initial-world renderer with movement. Continuous
chunk streaming and block interaction remain unfinished. Existing optional
texture caches are used; without them the client displays debug colors.
See [singleplayer setup](docs/SINGLEPLAYER.md) for details.

Implemented: bounded signed protocol primitives, incremental frames and zlib,
Handshake/Status/Ping, and a deterministic 20 TPS scheduler with bounded catch-up.
The remaining crates retain their data/API shells. See
[`docs/FOUNDATION.md`](docs/FOUNDATION.md) for scope and
[`docs/PERFORMANCE.md`](docs/PERFORMANCE.md) for the synthetic codec baseline.

```sh
cargo bench -p mc-protocol --bench codecs
```

The Rust launcher/fetch commands are placeholders; only `scripts/fetch-26.2.sh`
currently downloads references. See `docs/FOUNDATION.md` for the implementation
contract and remaining phases.

## Toolchain

- Stable Rust >= 1.97 (see `rust-toolchain.toml`), edition 2024.
- Targets: `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`, `aarch64-apple-darwin`.
- Renderer: `wgpu` (portable Vulkan/Metal/DX12/GL). No Bevy engine dependency in v1.

## License (ours)

Our Rust code: to be decided (MIT OR Apache-2.0 recommended). Mojang assets/code: proprietary, never redistributed.
