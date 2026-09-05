# Codec baseline

Protocol 776 foundation, 2026-09-05, aarch64 macOS, Rust 1.97.1, release profile
(thin LTO, one codegen unit). Synthetic data; no game assets or recorded packets.

```sh
cargo bench -p mc-protocol --bench codecs -- --sample-size 20 --warm-up-time 1 --measurement-time 1
```

| Operation | Criterion estimate interval |
|---|---|
| Encode + decode signed VarInt -1, reused output buffer | 4.49–4.50 ns |
| Decode 4 KiB uncompressed packet | 52.68–54.62 ns |
| Decode 4 KiB repeated-byte packet, zlib threshold 256 | 4.21–4.24 µs |

These are initial measurements, not before/after speedups or network throughput.
Frame input-buffer creation is outside the measured batch; decoding includes
payload ownership and drop. Compressed input is deliberately highly compressible,
so it is not representative of all chunk data. Repeat on the same machine and
settings before changing hot paths. Full machine-local results are under
`target/criterion/` and are not committed.

Current design avoids copies for decoded uncompressed payloads and borrows string
fields. Both are tested invariants. Allocation and decompression limits protect
against oversized input; boundedness is independent of benchmark timings.

## NBT client-decoder baseline

2026-09-06, same platform/toolchain/profile. Run the command above with the
`nbt_decode` filter. A synthetic 9,602-byte compound contains 128 duplicate-named
entries, each a list of 16 integers (2,561 allocation units in total).

| Operation | Criterion estimate interval |
|---|---|
| Decode compound with 128 integer lists, including owned tree drop | 28.49–28.57 µs |

Fixture construction is outside timing. This measures bounded tree decoding and
allocation, not real registry/chunk throughput. There is no previous NBT decoder
to compare against; this is not an optimization or a Rust-versus-Java comparison.
