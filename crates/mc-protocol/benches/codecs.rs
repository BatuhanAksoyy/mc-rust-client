//! Synthetic protocol 776 baseline. Run with cargo bench -p mc-protocol.
use std::hint::black_box;

use bytes::BytesMut;
use criterion::{Criterion, criterion_group, criterion_main};
use mc_protocol::nbt::{Limits, decode_network};
use mc_protocol::{decode_varint, encode_varint, framing::FrameCodec};

fn codecs(c: &mut Criterion) {
    // Synthetic registry-shaped compound: 128 named integer-list entries.
    let mut nbt = vec![10];
    for _ in 0..128 {
        nbt.extend_from_slice(&[9, 0, 3, b'k', b'e', b'y', 3, 0, 0, 0, 16]);
        for value in 0_i32..16 {
            nbt.extend_from_slice(&value.to_be_bytes());
        }
    }
    nbt.push(0);
    c.bench_function("nbt_decode_compound_128_lists", |b| {
        b.iter(|| black_box(decode_network(black_box(&nbt), Limits::default()).unwrap()));
    });
    c.bench_function("signed_varint_roundtrip", |b| {
        let mut output = Vec::with_capacity(5);
        b.iter(|| {
            output.clear();
            encode_varint(black_box(-1), &mut output);
            black_box(decode_varint(black_box(&output)).unwrap())
        });
    });
    for threshold in [-1, 256] {
        let mut codec = FrameCodec::default();
        codec.set_compression(threshold);
        let payload = vec![0x42; 4096];
        let frame = codec.encode(0, &payload).unwrap();
        c.bench_function(&format!("frame_decode_4k_threshold_{threshold}"), |b| {
            b.iter_batched(
                || BytesMut::from(frame.as_ref()),
                |mut input| black_box(codec.decode(&mut input).unwrap()),
                criterion::BatchSize::SmallInput,
            );
        });
    }
}

criterion_group!(benches, codecs);
criterion_main!(benches);
