# PROTOCOL-776.md — Java Edition protocol for 26.2

Canonical pages:
- `https://minecraft.wiki/w/Java_Edition_protocol/Packets` (26.2, protocol **776**)
- `https://minecraft.wiki/w/Java_Edition_protocol` (auth/systems index)
- `wiki.vg/Protocol`, `wiki.vg/Protocol_FAQ`, `wiki.vg/Protocol_version_numbers` (legacy, 1.21=767; useful for concepts, not IDs)

## Transport

- TCP, packets = VarInt length + VarInt packet ID + data. Max 2^21-1 (2097151) bytes, length field ≤ 3 bytes.
- States: Handshaking → Login → Configuration → Play (+ Status parallel). Intent in `intention` (Handshake 0x00): 1=Status, 2=Login, 3=Transfer.
- Compression: `login_compression` (Set Compression) threshold; Data Length 0 = uncompressed. Negative disables. Uncompressed (ID+data) ≤ 2^23 (8388608).
- Encryption: AES/CFB8 after `hello` (Encryption Request) + `key` (Encryption Response); RSA-1024 verify token + shared secret, then `login_finished` (Login Success).

## Key packets (official names in backticks, IDs shift per version — use generators)

- Handshaking serverbound: `intention` (protocol version=776, host, port, intent).
- Login clientbound: `login_disconnect`, `hello`, `login_finished`, `login_compression`, `custom_query`, `cookie_request`.
- Login serverbound: Login Start, Encryption Response, Login Acknowledged, Cookie Response, Custom Query Answer.
- Configuration: Registry Data (NBT-heavy — hardest part), Finish Configuration + Ack.
- Play: `Login (play)`, `Game Event` (13 = start waiting for chunks), `Chunk Data and Update Light`, `Synchronize Player Position`.

## Minimal spawn sequence (from Protocol FAQ, adapt to 776)

1. C→S Handshake + Login Start
2. S→C (optional) Encryption Request; do auth digest; C→S Encryption Response
3. S→C Login Success → C→S Login Acknowledged
4. S→C Registry Data (replay recorded NBT or `PrismarineJS/minecraft-data loginPacket.json` shape for 776) → Finish Configuration
5. C→S Acknowledge Finish Configuration
6. S→C Login (play) + Game Event + Chunk Data / Sync Player Position

## Rust implementation order (`mc-protocol`)

1. Primitives: VarInt/VarLong, String (max length prefixed), UUID, NBT (all tags), Position, BitSet, registries.
2. Codec: `Encoder/Decoder` traits + `Packet` enum per state with `#[cfg(test)]` round-trips.
3. Compression (flate2/zlib) + encryption (aes + rsa) as Vegas-style feature flags.
4. Snapshot tests: recorded vanilla Registry Data blob (kept in cache, NOT repo) → parse → re-encode byte-identical.
5. Fuzz: `cargo fuzz` on VarInt/NBT/packet framing (limit 2 MiB).

## Rules for AI

- Never hardcode IDs from memory. Generate from server data (`versions.json` style mapping or packet-generator dump for 776) and store mapping as test fixture in cache.
- Document each packet: wiki link + official name + fields + test vector source.
- Max packet size guards mandatory (DoS-safe).
