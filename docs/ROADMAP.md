# ROADMAP.md — phases (SP+MP, wgpu, YAGNI)

> Priority decision (2026-09-05): **implementation first**. Auth (MSA/OAuth) and
> legal polish are deferred to P6. Early testing uses offline mode
> (`online-mode=false` LAN server, `--offline <name>`) + cache-only reference data.
> `docs/AUTH.md` and `docs/LEGAL.md` stay as-is for later; they must not block P1–P5.

- [ ] P0 Repo + CI + docs (this snapshot). Acceptance: `fmt/clippy/test/deny` green on 3 OS.
- [ ] P1 `mc-protocol` 776 codec + tests. Acceptance: round-trip + fuzz clean, status ping works.
  Foundation complete: signed integers, bounded strings/positions, frames/zlib,
  and runnable status ping. Remaining: NBT, other P1 types, and coverage-guided fuzzing.
- [ ] P2 Headless join (offline server) → spawn. Acceptance: bot receives Login(play)+chunks, stays 60s. No auth.
  Local runtime: pinned Pumpkin process (`SINGLEPLAYER.md`), no custom server.
- [ ] P3 `mc-world` chunk types + registries from cache fixtures. Acceptance: parse recorded chunks.
- [ ] P4 `mc-render` chunk rendering. Acceptance: synthetic chunk 60 FPS debug HUD, no assets committed.
- [ ] P5 Tick/physics parity. Acceptance: movement tests within epsilon of vanilla observations.
- [ ] P6 Auth ownership-gated login + legal review (deferred; see `docs/AUTH.md`, `docs/LEGAL.md`).
- [ ] P7 Perf: multithread meshing, caching, profiling docs. Acceptance: criterion reports in PR.
- [ ] Future (not now): Rust mod API (`cdylib` + sandbox). Now: only `ModHost` trait stub, no loader.

Non-goals v1: Bedrock support, server implementation, asset redistribution, Bevy migration.

Singleplayer uses Pumpkin for simulation. First playable client milestone:
movement + breaking/placing blocks + save/reload. Pause is deferred.
