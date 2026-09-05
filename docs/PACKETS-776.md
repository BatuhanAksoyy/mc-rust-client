# PACKETS-776.md — packet inventory for 26.2 (protocol 776)

Source: `minecraft.wiki/w/Java_Edition_protocol/Packets` (CC BY-SA 3.0 — the wiki
explicitly permits using its contents to create clients/servers/bots; full table
reproduction lives in **cache only**, see below).

## Counts (verified 2026-09-05)

| state | clientbound | serverbound |
|---|---|---|
| handshaking | — | 1 (`intention`) |
| status | 2 | 2 |
| login | 6 | 5 |
| configuration | 20 | 10 |
| play | 141 | 69 |
| **total** | | **256** |

Machine-readable full table (id, wiki name, official name per state/dir):
`$CACHE/mc-rust-client/26.2/packets-776.csv` — generated from the wiki page, do not commit.

## Key packets for P1–P2 (offline-first)

- Handshake: sb `0x00 intention` (protocol=776, host, port, intent 1=status/2=login).
- Status: sb `status_request`, cb `status_response` (JSON), sb `ping_request` + cb `pong_response`.
- Login: sb `hello` (Login Start: name + UUID), cb `login_finished` (Login Success),
  sb `login_acknowledged`, cb `login_compression` (threshold; offline servers often skip),
  `custom_query`/`custom_query_answer`, `cookie_request`/`cookie_response`.
- Configuration: cb `registry_data` (id 7, NBT-heavy — hardest packet),
  cb `finish_configuration` → sb `finish_configuration` (ack),
  `select_known_packs`, `update_enabled_features`, `update_tags`, keep-alive/ping.
- Play (first needed): Login (play), `Game Event` 13 (start waiting for chunks),
  `Chunk Data and Update Light`, `Synchronize Player Position`, `keep_alive`.

## Rules

- Never hardcode IDs from memory — look them up in the cache CSV or confirm via
  data generators once Java lands (see `docs/SOURCE_OF_TRUTH.md`).
- In code, cite per packet: `// 26.2 cb play 0x?? official_name (wiki Packets 776)`.
- Data types reference: same wiki page §§ Definitions (big-endian, VarInt/VarLong ≤5/10
  bytes, Position 26/26/12 bits, BitSet, ID-or-X, Light Data, Game Profile, LpVec3).
