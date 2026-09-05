# ROADMAP.md — phases (SP+MP, wgpu, YAGNI)

- [ ] P0 Repo + CI + docs (this snapshot). Acceptance: `fmt/clippy/test/deny` green on 3 OS.
- [ ] P1 `mc-protocol` 776 codec + tests. Acceptance: round-trip + fuzz clean, status ping works.
- [ ] P2 `mc-auth` ownership-gated login. Acceptance: mock tests + manual device-code login, offline flag works.
- [ ] P3 Headless join (offline server) → spawn. Acceptance: bot receives Login(play)+chunks, stays 60s.
- [ ] P4 `mc-render` chunk rendering. Acceptance: synthetic chunk 60 FPS debug HUD, no assets committed.
- [ ] P5 Tick/physics parity. Acceptance: movement tests within epsilon of vanilla observations.
- [ ] P6 Singleplayer (integrated server via local vanilla `server.jar` initially) + multiplayer hardening.
- [ ] P7 Perf: multithread meshing, caching, profiling docs. Acceptance: criterion reports in PR.
- [ ] Future (not now): Rust mod API (`cdylib` + sandbox). Now: only `ModHost` trait stub, no loader.

Non-goals v1: Bedrock support, server implementation, asset redistribution, Bevy migration.
