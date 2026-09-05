# AI-GUIDE.md — how AI implements the client, task by task

> Priority decision (2026-09-05): **auth and legal are deferred** (see ROADMAP P6).
> Implement offline-first. Reference data lives in `$CACHE/mc-rust-client/26.2/`
> (jars, asset index, extracted JSON) — never in the repo.

## Order (each = one PR, one test suite)

The first usable foundation is scoped and verified by `FOUNDATION.md`; it delivers
the transport/status path before NBT and login. Follow-up work resumes the order
below. Declared future-phase dependencies are added when their code lands.

1. `mc-protocol` primitives: signed VarInt/VarLong, String, Position and fixed-width
   reads have property/vector tests. NBT and remaining P1 types are next.
2. Packet framing + bounded zlib implemented (no encryption yet).
3. Handshake + Status ping implemented against a local dummy TCP server; run
   `cargo run -p mc-client -- status localhost`.
4. Login offline → Configuration (Registry Data parse from cache blob) → Play spawn (headless bot joins vanilla 26.2 server in offline/LAN mode).
5. `mc-world` chunk types + mesher input structs (registries from cache fixtures).
6. `mc-render` cube → chunk → atlas (synthetic data first, cache textures later).
7. `mc-client` tick loop + movement physics tests.
8. Full join: config → play → render first chunk → move (still offline).
9. Perf pass + cross-platform CI.
10. LAST: `mc-auth` device-code + ownership gate (mock HTTP; no live calls in CI) + legal review.

## Definition of done per task

- Spec link (this docs/ + wiki URL + protocol 776).
- Unit + integration tests, `cargo clippy -D warnings` clean.
- No new warnings in `cargo doc`, no committed binaries/assets.
- Perf-sensitive: add `criterion` bench or explain why not.

## Prompts that work

- "Implement VarInt per PROTOCOL-776.md §Transport, with tests for 1/2/3-byte boundaries and overlong rejection."
- "Add Registry Data decoder using cached blob at $CACHE/26.2/registry.bin (do not commit), assert re-encode equality."

## Anti-patterns

- Pasting decompiled methods, hardcoding packet IDs from memory, adding Bevy/extra deps "just in case",
  committing `client.jar` or textures to make tests pass (use synthetic fixtures + cache).
