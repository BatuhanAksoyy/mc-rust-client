# mc-rust-client — 1:1 Minecraft Java Edition 26.2 client in Rust (docs-first)

Target: **Minecraft Java Edition 26.2 "Chaos Cubed" (release 2026-06-16, protocol 776)**.
Goal: feel exactly like Java Edition, with better FPS, cross-platform, Rust modding later.

> **Legal hard rule:** this repo NEVER contains Mojang code, decompiled source,
> `client.jar`/`server.jar`, assets, or mappings dumps. See `docs/LEGAL.md`.
> Reference material is fetched at build time to the local machine cache only.

## Layout

```
docs/               # source of truth for humans + AI agents (read these first)
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

## Quick start (does not download game files into repo)

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask verify-manifest --version 26.2   # network, cache-only, verifies SHA1
cargo xtask fetch-reference --help           # downloads client.jar to $CACHE only
```

## Toolchain

- Stable Rust >= 1.85 (see `rust-toolchain.toml`), edition 2021.
- Targets: `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`, `aarch64-apple-darwin`.
- Renderer: `wgpu` (portable Vulkan/Metal/DX12/GL). No Bevy engine dependency in v1.

## License (ours)

Our Rust code: to be decided (MIT OR Apache-2.0 recommended). Mojang assets/code: proprietary, never redistributed.
