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

## In-jar `version.json` (extracted to cache, ground truth)

```json
{ "id": "26.2", "protocol_version": 776, "world_version": 4903,
  "pack_version": { "resource_major": 88, "resource_minor": 0, "data_major": 107, "data_minor": 1 },
  "java_component": "java-runtime-epsilon", "java_version": 25, "stable": true }
```

## Local reference cache (`~/.cache/mc-rust-client/`, never committed)

| path | contents |
|---|---|
| `26.2/26.2.json` | version manifest (libs, args, assets) |
| `26.2/client.jar` / `server.jar` | SHA1-verified game jars (study/run offline server only) |
| `26.2/32-assets.json` | asset index (5057 objects: ~4871 sounds, 143 lang) |
| `26.2/client-extracted/` | jar JSON+assets: `version.json`, `assets/minecraft/blockstates` (1199), `models/block` (2658), `textures/block` (1372), `data/` (9021 incl. worldgen), `lang`, `META-INF/LICENSE` |
| `26.2/packets-776.csv` | 256 packet IDs + official names (generated from wiki 776 page) |
| `ref-26.1/protocol-26.1.json` | minecraft-data 26.1 packet field layouts (structural reference; re-verify vs 776) |
| `ref-26.1/blocks-26.1.json` | minecraft-data 26.1 block shapes (structural reference) |
| `versions-minosoft.json` | minosoft version/protocol mapping (cross-check) |

Textures ship **inside** `client.jar` (extract for atlas tests from cache).
Sounds/lang live in asset objects — fetch on demand, not now (audio is P5+):

```sh
# example: fetch one object by hash prefix
h=<sha1>; curl -fsSL "https://resources.download.minecraft.net/${h:0:2}/$h" -o "$CACHE/$h"
```

## Offline test server (verified 2026-09-05, no auth needed)

- Runtime: Homebrew `openjdk` 26.0.2 runs the 26.2 server (wants Java 25) — boot verified,
  stops at EULA as expected. Non-interactive shells need
  `export PATH="/opt/homebrew/opt/openjdk/bin:$PATH"`.
- Recipe (run from a **scratch dir outside the repo** — the bundler unpacks
  `libraries/`+`versions/` into cwd, and the server writes world files):

```sh
export PATH="/opt/homebrew/opt/openjdk/bin:$PATH"
mkdir -p /tmp/mc-test && cd /tmp/mc-test
cp ~/.cache/mc-rust-client/26.2/server.jar .
java -jar server.jar --nogui   # first run: creates eula.txt, exits
echo "eula=true" > eula.txt
# server.properties: set online-mode=false, enable-status=true
java -jar server.jar --nogui   # boots 26.2 offline server on :25565
```

- Our client tests target this server: status ping → offline login → config → play.

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
