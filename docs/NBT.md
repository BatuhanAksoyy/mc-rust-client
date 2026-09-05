# NBT decoding — protocol 776 / Java Edition 26.2

## SPEC: first client-facing increment

Decode registry/chunk metadata without server or rendering dependencies. Support
all payload IDs 1–12, plus root End (0) as an absent value. Network roots retain
their type byte but omit the name; named roots retain the unsigned-short-prefixed
name. Both APIs return the consumed byte count, leaving subsequent packet fields
untouched. Named-root decoding accepts any tag type; callers requiring a compound
must enforce that constraint. Compression, encoding and packet-specific validation
are separate work, not part of this increment.

Integers and IEEE floats are big-endian. Byte arrays borrow the input; integer
arrays own decoded values. Compounds preserve entry order and duplicate names.
Lists retain their element ID, including empty lists. Negative list/array lengths,
unknown IDs (even for empty lists), and positive-length End lists are rejected.
This strict length policy deliberately excludes legacy negative-length empty lists.

Names and strings use Java modified UTF-8 with an unsigned 16-bit byte length,
not the protocol String codec. Decode to UTF-16 units so isolated surrogates are
preserved; conversion to Rust UTF-8 is explicit and fallible, never lossy. Accept
the single-byte NUL and overlong two/three-byte sequences that `DataInput.readUTF`
also accepts; reject malformed continuations and four-byte UTF-8. No Java runtime
or additional dependency is needed.

## Resource policy (client limits, not wire constants)

Default limits: 2 MiB consumed bytes, 65,536 allocation units, depth 64. The root
is depth zero; each list/compound child increments depth. Callers can lower these
limits; requested depth above 64 is rejected even for shallow input. Allocation
units count each tag, each UTF-16 code unit, and each integer/long array element.
Compound terminators do not count as tags. Borrowed byte-array bytes are covered
by the byte limit. Limits apply cumulatively across the whole root, not per list.
Counts are checked before allocating; arrays require all source bytes before
allocation, and lists grow only as elements are successfully decoded. The byte
limit covers this NBT value, not unrelated trailing packet fields.

## Verification and references

Synthetic fixtures cover every tag, named/network roots, strings, floating-point
bits, truncation, hostile lengths, nesting and cumulative limits. Property tests
exercise integer arrays and arbitrary bytes; these are not a substitute for the
future P1 `cargo-fuzz` campaign or a live Registry Data fixture. Criterion records
a synthetic registry-shaped decode baseline; no optimization claim is made.

- [Original NBT specification, archived](https://github.com/acfoltzer/nbt/blob/master/NBT-spec.txt)
- [Protocol NBT reference, including unnamed network roots](https://wikivg.booky.dev/NBT)
- [Java DataInput modified UTF-8 and readUTF contract](https://docs.oracle.com/en/java/javase/25/docs/api/java.base/java/io/DataInput.html#modified-utf-8)
- `PROTOCOL-776.md`, `P1-PROTOCOL.md`, `AI-GUIDE.md`
