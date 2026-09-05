# Offline client join — protocol 776 / 26.2

## SPEC: reach a confirmed spawn with its chunk data, without gameplay yet

Use the pinned external Pumpkin from `SINGLEPLAYER.md`. Implement a headless
`join` probe: Handshake(intent 2) → Login Start → Login Success → Login Ack →
Configuration → Finish Configuration Ack → decode Play Login → receive any
chunk batch Pumpkin sends before spawn → confirm the initial spawn teleport
(Confirm Teleportation + Player Loaded).
The probe reports the assigned player/entity/dimension, registry counts,
confirmed spawn position and received chunk count, then disconnects. The
library returns an owned live connection and buffered transport state for
subsequent chunk/entity work. This does not render, simulate movement, decode
block/biome global IDs into named blocks (no block-state registry exists
yet), or implement a server. Account authentication remains deferred.

Client policy: offline development only; usernames are 1–16 ASCII letters,
digits or underscores. Login Start sends a nil UUID; the offline server assigns
the authoritative identity returned by Login Success. Never try to bypass an
Encryption Request: return an explicit unsupported-authentication error.

## Wire contract

IDs verified from the external `26.2/packets-776.csv`; field layouts checked
against the current wiki protocol-776 page (2026-09-06), not the older 26.1 JSON.
In particular Login Success contains Game Profile **and a session UUID**; Play
Login ends with online-mode and secure-chat booleans. These must not be silently
accepted as older layouts.

- Login: compression changes apply immediately after the negotiation packet,
  including already-buffered next frames. Validate profile properties and exact
  field consumption. Reply to plugin queries as unsupported, cookies as absent.
- Configuration: send Client Information (en_US, view 4, hidden chat, colors on,
  all skin parts, right hand, filtering off, listings off, all particles).
- Reply to Known Packs with an empty list: no bundled registry definitions are
  claimed. Reject omitted entry data; do not invent registry content. Preserve
  registry and entry identifiers, numeric entry ordering, and raw validated NBT.
  Registry roots must be compounds. Duplicate registries/entries are rejected.
- Echo configuration keep-alive and ping. Validate and retain feature flags and
  tag updates; only structural tag validation is possible before static block
  registries exist. Ignore well-framed plugin payloads after channel validation.
- Unimplemented configuration packets fail explicitly. In particular do not
  automatically accept codes of conduct, download resource packs, follow server
  links/transfers, or persist cookies. Configuration disconnects are errors.
- Require a dimension-type registry and resolve Play Login's dimension-type
  index into it before reporting success. Other registry semantics are later
  client work. A finish ACK alone is not successful joining.
- Play (pre-spawn): unlike Login/Configuration, an unrecognized Play packet ID
  is skipped unread rather than treated as an error — Play has far more packet
  variety than this step needs to understand, and entity data, block updates
  and the rest of Play are later work. Only Disconnect, Game Event, Keep Alive,
  Set Center Chunk, Synchronize Player Position and the chunk-batch packets are
  decoded; Disconnect is an error, Keep Alive is echoed. Synchronize Player
  Position must be absolute (Teleport Flags zero) — there is no prior position
  yet to apply relative deltas to; a nonzero flags value is rejected rather
  than approximated. Reaching spawn means replying Confirm Teleportation with
  the given ID, then Player Loaded.
- Chunk data: decode Level Chunk With Light fully (heightmaps, paletted block
  and biome containers, block entities' NBT, light data), per the wiki's
  Chunk format page, verified against a live capture decoded byte-for-byte,
  not assumed. The `Data` field's own byte length ends the section loop, so
  this client never needs the dimension's world height to parse it; heightmap
  longs are retained unparsed for the same reason. A chunk batch must end with
  Chunk Batch Received (`25 / millisPerChunk`, one-shot, no 15-batch smoothing);
  Chunk Batch Finished without a preceding Start is rejected. Global block/biome
  IDs are retained as opaque integers — resolving them into named blocks needs
  a registry this client does not have yet.

## Bounds and verification

One caller-supplied deadline spans DNS, connection and all join phases; timeout
or cancellation drops the socket. Shared transport bounds the receive buffer
and retains coalesced frames across state/compression transitions. Join policy:
at most 16,384 inbound packets, 32 MiB cumulative decoded payload, 128 registries
and 65,536 total registry entries, 256 chunks. Individual collections are
bounded before allocation, including per-chunk sections (384), palette sizes
(4096 blocks/64 biomes) and bits-per-entry (32, so `64 / bits` never divides by
zero). Each NBT root uses `NBT.md` limits. These are development-client
limits, not protocol constants. No additional dependencies are required.

Synthetic TCP tests must cover compression/coalescing, state acknowledgements,
full registry retention, ping/cookie/plugin replies, malformed packets, missing
registry data, encryption rejection, timeout/EOF, an unrecognized Play packet
being skipped rather than failing, a rejected relative initial teleport, a
Play disconnect, and a rejected Chunk Batch Finished without a Start. Chunk
decoding itself (single/indirect/direct palettes, out-of-range palette index,
malformed light data, excessive section count) has its own synthetic suite
independent of the join state machine. Opt-in pinned-Pumpkin test must reach
confirmed spawn with at least one chunk whose every block/biome entry
resolves, not just Play Login. No game fixtures are vendored.

No hot-path optimization is proposed: this is a bounded startup transaction.
The existing codec/NBT Criterion benchmarks remain the performance baselines.

References: [packets](https://minecraft.wiki/w/Java_Edition_protocol/Packets),
[data types](https://minecraft.wiki/w/Java_Edition_protocol/Data_types),
[chunk format](https://minecraft.wiki/w/Java_Edition_protocol/Chunk_format),
`NBT.md`, `PROTOCOL-776.md`, `PACKETS-776.md`, `AI-GUIDE.md`.
