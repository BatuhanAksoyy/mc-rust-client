# AI-GUIDE.md — how AI implements the client, task by task

## Order (each = one PR, one test suite)

1. `mc-protocol` primitives: VarInt, String, NBT, Position. Property tests.
2. Packet framing + compression + encryption skeleton (mock keys).
3. Handshake + Status ping against local dummy server (integration test).
4. Login offline → Configuration (Registry Data parse from cache blob) → Play spawn (headless bot joins vanilla 26.2 server in LAN/offline mode).
5. `mc-auth` device-code + ownership gate (mock HTTP; no live calls in CI).
6. `mc-world` chunk types + mesher input structs.
7. `mc-render` cube → chunk → atlas (synthetic data first).
8. `mc-client` tick loop + movement physics tests.
9. Full join: auth → config → play → render first chunk → move.
10. Perf pass + cross-platform CI.

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
