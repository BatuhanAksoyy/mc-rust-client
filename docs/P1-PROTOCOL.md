# P1-PROTOCOL.md — implementation contract for `mc-protocol` (next step)

Target: join an **offline** Pumpkin 26.2 server (headless, no auth, no encryption).
Vanilla is an optional compatibility reference; see `SINGLEPLAYER.md`.

## Reference inputs (all in `$CACHE/mc-rust-client/`, never in repo)

- `26.2/packets-776.csv` — 256 packets with IDs + official names (from wiki, 776).
- `26.2/client-extracted/version.json` — protocol 776, world_version 4903, pack versions.
- `ref-26.1/protocol-26.1.json` — minecraft-data structure reference (types + packet
  field layouts for 26.1; verify each field against wiki 776 before use).
- `26.2/client.jar` / `server.jar` — run the offline test server from cache.
- `26.2/client-extracted/assets|data/` — blockstates/models/lang for later phases.

## Task list (one PR each, smallest first)

1. **Primitives** (`types.rs`): Boolean/Byte/UByte/Short/UShort/Int/Long/Float/Double
   (big-endian), VarInt/VarLong (signed, ≤5/≤10 bytes, reject overlong and overflow;
   implemented with sample vectors and property tests in the foundation),
   String (VarInt-prefixed UTF-8, UTF-16 length semantics), Identifier, UUID, Position
   (26/26/12), Angle, BitSet/Fixed BitSet, Prefixed Array/Optional, ID-or-X, ID Set.
   Tests: wiki sample vectors (VarInt table incl. negatives) + proptest round-trips.
2. **NBT** (`nbt/`): all tags, network (uncompressed, unnamed root) vs named roots.
   The root type byte is retained. Bounded decoding is specified in `NBT.md`;
   encoding/round-trips remain follow-up work. Use synthetic binary fixtures;
   loot-table JSON is not a binary NBT fixture. Disk compression is out of scope.
3. **Framing** (`framing.rs`): length-prefixed packets, 2 MiB cap, compression
   (`login_compression` threshold, zlib) — encryption stubbed (offline skips it).
4. **Status ping** (`status.rs`): handshake(intent=1) + `status_request` → parse
   `status_response` JSON → ping/pong. Integration test vs local vanilla server
   (needs Java — see SOURCE_OF_TRUTH; if Java missing, test vs mock TCP server).
5. **Login→Config→Play minimal** (`login.rs`, `config.rs`): Login Start →
   Login Success → Ack → Registry Data (parse + store raw NBT for now) →
   Finish Configuration → Login (play) → Game Event + first chunk.
   Record a real Registry Data blob from the local server into cache as fixture.

## Acceptance

- `cargo fmt/clippy/test` green; fuzz VarInt/NBT/frame with `cargo-fuzz` (limit 2 MiB).
- Docs per module link wiki section + 776. No packet IDs from memory (CSV or generator).
