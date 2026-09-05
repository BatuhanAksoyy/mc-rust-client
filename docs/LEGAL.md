# LEGAL.md — what AI + humans must obey

Source: Mojang "Removing obfuscation in Java Edition" (2025-10-29), Minecraft EULA
(https://www.minecraft.net/en-us/eula), mappings license header (2020, updated 21w03a wording).

## TL;DR

- Code is **readable since 26.1** but still **proprietary**. EULA unchanged.
- You may **decompile, study, reference** for development. You may **not redistribute**
  decompiled source, jars, assets, or complete unmodified mappings.
- "Non-commercial + open-source + no assets + require ownership" **reduces risk but does
  not grant redistribution rights**. The only safe posture is clean-room.

## Allowed

- Downloading `client.jar`/`server.jar` from `piston-data.mojang.com` to your own machine
  via launcher/piston-meta for study.
- Decompiling locally with Vineflower/CFR for reading (see `SOURCE_OF_TRUTH.md`).
- Writing original Rust code that interoperates (protocol, auth) — like Prism Launcher,
  Fabric Loom, Paper do.
- Referencing official symbol names (e.g. `intention`, `login_finished`) in docs/specs.

## Forbidden in this repo

- `*.jar`, `*.class`, decompiled `*.java`, pasted Mojang methods, full mappings files.
- `assets/objects/**`, `sounds/**`, `textures/**`, `lang/**` from Mojang.
- Hardcoded Mojang secrets, client IDs you don't own (except clearly-marked dev placeholder).
- Instructions to bypass ownership/entitlement checks or to play without a license.

## How we stay compliant

1. **No vendoring.** Fetch scripts write to `$XDG_CACHE_HOME/mc-rust-client/` or
   `~/.cache/mc-rust-client/`, never into the repo. CI verifies absence via
   `git ls-files | grep -E '\.(jar|class)$'` guard.
2. **Ownership gate.** Launcher + client refuse online play unless
   `api.minecraftservices.com/entitlements` shows `game_minecraft`/`product_minecraft`
   or a valid profile exists (Game Pass still shows profile). See `AUTH.md`.
3. **Assets at runtime only.** Load from user's own `~/.minecraft` install or from
   Mojang CDN after auth, into cache dir. Never commit. See `ASSETS.md`.
4. **Clean-room log.** Each re-implemented behavior cites spec/test, not source paste.
   Example: "jump velocity 0.42, gravity 0.08 per tick — verified against vanilla 26.2
   observation + wiki, test in `mc-client/tests/physics.rs`".

## License headers

- Our code files: `// SPDX-License-Identifier: MIT OR Apache-2.0`.
- Each jar shipped by Mojang now includes a `LICENSE` file linking to EULA — do not strip
  or re-host it.

If in doubt: document behavior, write a test, ask a maintainer. Do not paste.
