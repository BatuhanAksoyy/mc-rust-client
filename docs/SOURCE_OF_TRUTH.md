# SOURCE_OF_TRUTH.md — Minecraft 26.2 reference (fetched 2026-09-05)

> This file contains **facts + URLs + hashes**, never code. Use it to fetch locally.
> Do not commit fetched jars/assets.

## Identity

- Version: `26.2` (release, "Chaos Cubed"), released `2026-06-16T12:03:33Z`.
- Latest at fetch time: release `26.2`, snapshot `26.3-pre-2` (from `version_manifest_v2.json`).
- Version manifest entry SHA1: `3592ebc61c6b6c33bb8228fe5a9e90221df0be68`
  URL: `https://piston-meta.mojang.com/v1/packages/3592ebc61c6b6c33bb8228fe5a9e90221df0be68/26.2.json`
- Manifest root: `https://piston-meta.mojang.com/mc/game/version_manifest_v2.json`
- `mainClass`: `net.minecraft.client.main.Main`
- `minimumLauncherVersion`: 21, `complianceLevel`: 1
- Java runtime: `java-runtime-epsilon`, major 25 (launcher installs from
  `https://launchermeta.mojang.com/v1/products/java-runtime/.../all.json`).
- Obfuscation: **none**. No `client_mappings` in `downloads` (confirmed for 26.1/26.2).
  Deobfuscation step is skip; decompilation still needed for reading.

## Downloads (verify SHA1 before use)

| side   | sha1                                     | size     | url                                                        |
|--------|------------------------------------------|----------|------------------------------------------------------------|
| client | `2dc72797acbc1b63fc16a11c4ac393605f453754` | 39193383 | `https://piston-data.mojang.com/v1/objects/2dc72797.../client.jar` |
| server | `823e2250d24b3ddac457a60c92a6a941943fcd6a` | 60894273 | `https://piston-data.mojang.com/v1/objects/823e2250.../server.jar` |

Full URLs:
- client: `https://piston-data.mojang.com/v1/objects/2dc72797acbc1b63fc16a11c4ac393605f453754/client.jar`
- server: `https://piston-data.mojang.com/v1/objects/823e2250d24b3ddac457a60c92a6a941943fcd6a/server.jar`

## Assets

- `assets`: `32`, index id `32`, sha1 `795a52d29f7f6b1e51d3a65e60ca46ad62aaddec`, size 586366, total ~480 MB.
- Index URL: `https://piston-meta.mojang.com/v1/packages/795a52d29f7f6b1e51d3a65e60ca46ad62aaddec/32.json`
- Objects: 5057 entries, CDN: `https://resources.download.minecraft.net/<2-char-hash>/<hash>`.
- `mcmeta` mirror for metadata (not assets): `https://github.com/misode/mcmeta`.

## Libraries (131 entries)

- LWJGL `3.4.1` across freetype/glfw/jemalloc/openal/opengl/shaderc/spvc/stb/tinyfd/vma/vulkan + `lwjgl:unsafe`.
- Natives for: linux, macos, macos-arm64, windows, windows-arm64, windows-x86.
- Netty `netty-transport-native-epoll/kqueue`, `lz4-java 1.10.1`, `gson`, `oshi-core`, `azure-json`, `jtracy`.
- Full list: see version JSON `libraries[]`. Rust side does NOT need these (we use `wgpu`/`tokio`),
  but they document what vanilla links (GLFW, OpenAL, Vulkan path).

## Protocol

- Java protocol for 26.2 = **776** (source: `minecraft.wiki/w/Java_Edition_protocol/Packets`).
- Note: `26.2.json` no longer carries `protocolVersion`/`dataVersion` keys (only
  arguments/assetIndex/assets/complianceLevel/downloads/id/javaVersion/libraries/logging/
  mainClass/minimumLauncherVersion/times/type). Get protocol from wiki + data generators.
- Packet IDs are version-specific — never hardcode without generator. Get official names
  via built-in data generators (`intention`, `login_finished`, `login_compression`,
  `custom_query`, `cookie_request`, ...).

## How to fetch locally (cache only)

```sh
# verifies manifest + client SHA1, downloads to cache dir (default ~/.cache/mc-rust-client)
cargo xtask verify-manifest --version 26.2
cargo xtask fetch-reference --version 26.2 --side client
# then decompile locally for READING ONLY (requires local Java + Vineflower):
java -jar vineflower.jar ~/.cache/mc-rust-client/26.2/client.jar ~/.cache/mc-rust-client/26.2/decompiled/
# or: mcsrc.dev (in-browser, no redistribution)
```

See `scripts/fetch-26.2.sh` for the curl+sha1 equivalent.
