//! Synthetic protocol 776 baseline. Run with cargo bench -p mc-protocol.
use std::hint::black_box;

use bytes::BytesMut;
use criterion::{Criterion, criterion_group, criterion_main};
use mc_protocol::{decode_varint, encode_varint, framing::FrameCodec};

fn codecs(c: &mut Criterion) {
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
