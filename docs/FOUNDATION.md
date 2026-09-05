# Foundation implementation contract

This first usable basis follows `AI-GUIDE.md` and protocol **776**. It is not
the full P1–P5 client: NBT/registries, offline login, world storage, rendering,
physics, auth, and the real launcher remain separate follow-up work.

## Repository findings

The initial workspace contains six crate shells, an unsigned-only VarInt codec,
and placeholder fetch commands. Workspace lints are declared but not inherited;
the Windows documentation step uses POSIX environment assignment. Most declared
dependencies are unused. No `.codegraph/` index is present.

## Deliverables and acceptance

1. All crates inherit Rust version, license, and workspace lints. Keep only
   dependencies used by implemented code. Preserve the three-OS CI matrix and
   make its documentation command portable. Provide the documented xtask alias.
2. Pure, bounded protocol building blocks: signed VarInt/VarLong, a borrowing
   cursor for big-endian fields and UTF-8 strings, UUID bytes, and packed Position.
   Property tests cover signed boundaries, malformed inputs, and truncation.
3. Incremental packet framing with optional zlib compression. An incomplete frame
   consumes nothing. Reject invalid lengths/IDs, oversized data, threshold
   violations, corrupt streams, and inflated-size mismatches. Synthetic tests
   cover fragmented/coalesced frames and compression negotiation.
4. Typed Handshake/Status/Ping codecs and a runnable `mc-client status` command.
   The protocol crate owns serialization; the client owns TCP, JSON interpretation,
   timeouts, and round-trip timing. Test the whole exchange against a local mock
   TCP server, including malformed replies and disconnects. No game files or
   account are required.
5. A deterministic fixed-step scheduler in `mc-client`: 20 TPS, fractional render
   interpolation, and explicit bounded catch-up. Tests use supplied durations;
   no renderer, wall-clock sleeping, or unverified movement constants.
6. Run the repository fmt/clippy/test/doc/deny/audit gates. Add a Criterion
   baseline for codec hot paths; make no speedup claim without before/after data.
   Remote Linux/Windows results require CI and are not implied by local tests.

## SPEC: implementation policies

Reference: [protocol definitions and packets](https://minecraft.wiki/w/Java_Edition_protocol/Packets),
`PROTOCOL-776.md`, and `PACKETS-776.md`. Packet IDs are checked against the local
26.2 CSV, never inferred from older versions.

- VarInts use signed two's complement, not ZigZag. Reject values exceeding the
  32/64-bit width and continuations beyond 5/10 bytes. Accept nonminimal encodings
  within that width, as the wire format permits them. Encoders emit minimal forms.
- String limits count UTF-16 code units. Reject negative lengths, invalid UTF-8,
  and byte lengths above three times the declared character limit before reading.
- Position uses x:26, z:26, y:12 bits. Encoding rejects out-of-range coordinates.
- The wire frame limit is **2^21 - 1**, with a maximum three-byte prefix. The
  decompressed ID + payload limit is **2^23** (separate from the wire limit).
  Validate size before reserving or inflating. Frame errors are terminal for a
  connection; callers discard it rather than attempting resynchronization.
- Compression is disabled initially; negative thresholds disable it, zero
  compresses every packet. Compressed packets must meet the threshold and expand
  to exactly the declared size, with exactly one complete zlib stream.
- Status host limit is 255 UTF-16 units, status JSON limit is 32767. Preserve
  unknown JSON fields; check packet IDs, trailing bytes, and the ping echo.
- A status timeout covers DNS, connect, request, and pong as one operation. A
  cancelled/failed exchange drops the connection. No retries or background tasks.
- Scheduler catch-up limit is supplied by the caller and must be nonzero.
  Excess whole ticks are reported as dropped time while preserving the sub-tick
  remainder. This is local overload policy, not a claim of vanilla physics parity.

## Follow-up order

Complete the remaining P1 primitives and bounded NBT, then implement offline
Login → Configuration → Play against a vanilla server. Keep registry data and
assets in the external cache. Add world/render dependencies when those phases
have working code; keep network and graphics out of the data-only crates.
