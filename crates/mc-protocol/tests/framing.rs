//! Synthetic frame fixtures. No game files or network required.
use std::io::Write;

use bytes::BytesMut;
use flate2::{Compression, write::ZlibEncoder};
use mc_protocol::{
    CodecError, MAX_PACKET_SIZE, MAX_UNCOMPRESSED_SIZE, encode_varint, framing::FrameCodec,
};
use proptest::prelude::*;

fn codec(threshold: i32) -> FrameCodec {
    let mut codec = FrameCodec::default();
    codec.set_compression(threshold);
    codec
}

fn wrap(body: &[u8]) -> BytesMut {
    let mut bytes = Vec::new();
    encode_varint(i32::try_from(body.len()).unwrap(), &mut bytes);
    bytes.extend_from_slice(body);
    BytesMut::from(bytes.as_slice())
}

fn compressed(declared: i32, packet: &[u8], trailing: &[u8]) -> BytesMut {
    let mut body = Vec::new();
    encode_varint(declared, &mut body);
    let mut zlib = ZlibEncoder::new(body, Compression::default());
    zlib.write_all(packet).unwrap();
    body = zlib.finish().unwrap();
    body.extend_from_slice(trailing);
    wrap(&body)
}

#[test]
fn wire_vector_and_zero_copy_payload() {
    let codec = codec(-1);
    assert_eq!(codec.encode(1, &[0x42]).unwrap().as_ref(), &[2, 1, 0x42]);
    let mut input = BytesMut::from(&[2, 1, 0x42][..]);
    let pointer = input[2..].as_ptr();
    let packet = codec.decode(&mut input).unwrap().unwrap();
    assert_eq!(packet.id, 1);
    assert_eq!(packet.payload.as_ref(), &[0x42]);
    assert_eq!(packet.payload.as_ptr(), pointer);
    assert!(input.is_empty());
}

#[test]
fn incomplete_frames_never_consume_and_coalesced_frames_decode_separately() {
    for threshold in [-1, 0, 256] {
        let codec = codec(threshold);
        let frame = codec.encode(128, &[0x42; 256]).unwrap();
        for split in 0..frame.len() {
            let mut input = BytesMut::from(&frame[..split]);
            let before = input.clone();
            assert_eq!(codec.decode(&mut input), Ok(None));
            assert_eq!(input, before);
        }
        let mut input = BytesMut::new();
        input.extend_from_slice(&frame);
        input.extend_from_slice(&frame);
        assert!(codec.decode(&mut input).unwrap().is_some());
        assert_eq!(input.as_ref(), frame.as_ref());
        assert!(codec.decode(&mut input).unwrap().is_some());
        assert!(input.is_empty());
    }
}

#[test]
fn rejects_bad_lengths_ids_and_terminal_errors_without_consuming() {
    for (bytes, expected) in [
        (vec![0], CodecError::InvalidLength),
        (vec![0x80, 0x80, 0x80], CodecError::InvalidLength),
        (vec![1, 0x80], CodecError::UnexpectedEof),
        (vec![5, 0xff, 0xff, 0xff, 0xff, 0x0f], CodecError::InvalidPacketId(-1)),
    ] {
        let mut input = BytesMut::from(bytes.as_slice());
        assert_eq!(codec(-1).decode(&mut input), Err(expected));
        assert_eq!(input.as_ref(), bytes);
    }
    assert_eq!(codec(-1).encode(-1, &[]), Err(CodecError::InvalidPacketId(-1)));
}

#[test]
fn compression_threshold_includes_the_packet_id() {
    let codec = codec(4);
    assert_eq!(codec.encode(0, &[1, 2]).unwrap().as_ref(), &[4, 0, 0, 1, 2]);
    let compressed = codec.encode(0, &[1, 2, 3]).unwrap();
    assert_eq!(compressed[1], 4);
    assert_eq!(codec.decode(&mut wrap(&[0, 0, 1, 2, 3])), Err(CodecError::CompressionThreshold));
    assert_eq!(
        codec.decode(&mut self::compressed(3, &[0, 1, 2], &[])),
        Err(CodecError::CompressionThreshold)
    );
    assert_eq!(self::codec(0).decode(&mut wrap(&[0, 0])), Err(CodecError::CompressionThreshold));
}

#[test]
fn decompression_is_exact_bounded_and_rejects_extra_streams() {
    let codec = codec(0);
    for mut input in [
        compressed(2, &[0, 1, 2], &[]),
        compressed(4, &[0, 1, 2], &[]),
        compressed(3, &[0, 1, 2], &[0]),
        wrap(&[3, 0xff]),
        compressed(3, &[], &[]),
    ] {
        assert_eq!(codec.decode(&mut input), Err(CodecError::InvalidCompression));
    }
    let mut oversized = Vec::new();
    encode_varint(i32::try_from(MAX_UNCOMPRESSED_SIZE + 1).unwrap(), &mut oversized);
    assert_eq!(
        codec.decode(&mut wrap(&oversized)),
        Err(CodecError::PacketTooLarge(MAX_UNCOMPRESSED_SIZE + 1))
    );
    let mut negative = Vec::new();
    encode_varint(-1, &mut negative);
    assert_eq!(codec.decode(&mut wrap(&negative)), Err(CodecError::InvalidLength));
    let frame = compressed(3, &[0, 1, 2], &[]);
    let mut body = frame[1..].to_vec();
    body.pop(); // Truncated checksum, with a complete outer frame.
    assert_eq!(codec.decode(&mut wrap(&body)), Err(CodecError::InvalidCompression));
    body = frame[1..].to_vec();
    body.extend_from_slice(&frame[2..]); // Concatenated zlib streams.
    assert_eq!(codec.decode(&mut wrap(&body)), Err(CodecError::InvalidCompression));
}

#[test]
fn wire_and_inflated_limits_are_distinct() {
    let plain = codec(-1);
    let payload = vec![0; MAX_PACKET_SIZE - 1];
    let mut frame = BytesMut::from(plain.encode(0, &payload).unwrap().as_ref());
    assert_eq!(plain.decode(&mut frame).unwrap().unwrap().payload.len(), payload.len());
    assert_eq!(
        plain.encode(0, &vec![0; MAX_PACKET_SIZE]),
        Err(CodecError::PacketTooLarge(MAX_PACKET_SIZE + 1))
    );
    let zipped = codec(0);
    let payload = vec![0; MAX_UNCOMPRESSED_SIZE - 1];
    let mut frame = BytesMut::from(zipped.encode(0, &payload).unwrap().as_ref());
    assert!(frame.len() < MAX_PACKET_SIZE);
    assert_eq!(zipped.decode(&mut frame).unwrap().unwrap().payload.len(), payload.len());
    assert_eq!(
        zipped.encode(0, &vec![0; MAX_UNCOMPRESSED_SIZE]),
        Err(CodecError::PacketTooLarge(MAX_UNCOMPRESSED_SIZE + 1))
    );
}

#[test]
fn compression_can_change_between_buffered_frames() {
    let mut codec = codec(-1);
    let first = codec.encode(3, &[0]).unwrap();
    let second = self::codec(0).encode(0, &[0x42; 128]).unwrap();
    let mut input = BytesMut::from(first.as_ref());
    input.extend_from_slice(&second);
    assert_eq!(codec.decode(&mut input).unwrap().unwrap().id, 3);
    codec.set_compression(0);
    assert_eq!(codec.decode(&mut input).unwrap().unwrap().payload.as_ref(), &[0x42; 128]);
    codec.set_compression(-5);
    assert_eq!(codec.encode(0, &[]).unwrap().as_ref(), &[1, 0]);
}

proptest! {
    #[test]
    fn packets_roundtrip(id in 0..i32::MAX, payload in proptest::collection::vec(any::<u8>(), 0..4096), threshold in -1..512) {
        let codec = codec(threshold);
        let frame = codec.encode(id, &payload).unwrap();
        let mut input = BytesMut::from(frame.as_ref());
        let packet = codec.decode(&mut input).unwrap().unwrap();
        prop_assert_eq!(packet.id, id);
        prop_assert_eq!(packet.payload.as_ref(), payload);
        prop_assert!(input.is_empty());
    }

    #[test]
    fn arbitrary_frames_do_not_panic(bytes in proptest::collection::vec(any::<u8>(), 0..256), threshold in -1..512) {
        let _ = codec(threshold).decode(&mut BytesMut::from(bytes.as_slice()));
    }
}
