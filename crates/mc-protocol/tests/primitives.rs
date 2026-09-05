//! Protocol 776 synthetic wire vectors and property tests.
use mc_protocol::{
    CodecError, decode_varint, decode_varlong, encode_varint, encode_varlong,
    types::{Position, Reader, encode_string},
};
use proptest::prelude::*;

#[test]
fn signed_varint_wire_vectors() {
    let vectors: &[(i32, &[u8])] = &[
        (0, &[0]),
        (1, &[1]),
        (127, &[127]),
        (128, &[0x80, 1]),
        (255, &[0xff, 1]),
        (2_097_151, &[0xff, 0xff, 0x7f]),
        (i32::MAX, &[0xff, 0xff, 0xff, 0xff, 7]),
        (-1, &[0xff, 0xff, 0xff, 0xff, 0x0f]),
        (i32::MIN, &[0x80, 0x80, 0x80, 0x80, 8]),
    ];
    for &(value, expected) in vectors {
        let mut encoded = Vec::new();
        encode_varint(value, &mut encoded);
        assert_eq!(encoded, expected);
        assert_eq!(decode_varint(expected), Ok((value, expected.len())));
        for end in 0..expected.len() {
            assert_eq!(decode_varint(&expected[..end]), Err(CodecError::UnexpectedEof));
        }
    }
}

#[test]
fn variable_integer_width_and_nonminimal_encodings() {
    assert_eq!(decode_varint(&[0x80, 0]), Ok((0, 2)));
    assert_eq!(decode_varint(&[0x80; 5]), Err(CodecError::VarIntTooLong));
    assert_eq!(decode_varlong(&[0x80; 10]), Err(CodecError::VarLongTooLong));
    assert_eq!(decode_varint(&[0xff, 0xff, 0xff, 0xff, 0x10]), Err(CodecError::IntegerOverflow));
    let mut invalid = [0xff; 10];
    invalid[9] = 2;
    assert_eq!(decode_varlong(&invalid), Err(CodecError::IntegerOverflow));
    invalid[9] = 1;
    assert_eq!(decode_varlong(&invalid), Ok((-1, 10)));
    let mut minimum = [0x80; 10];
    minimum[9] = 1;
    assert_eq!(decode_varlong(&minimum), Ok((i64::MIN, 10)));
    let mut encoded = Vec::new();
    encode_varlong(i64::MAX, &mut encoded);
    assert_eq!(encoded, [0xff; 8].into_iter().chain([0x7f]).collect::<Vec<_>>());
}

#[test]
fn strings_use_utf16_units_and_reject_malformed_data() {
    let mut output = Vec::new();
    encode_string("😀", 2, &mut output).unwrap();
    assert_eq!(output, [4, 0xf0, 0x9f, 0x98, 0x80]);
    assert_eq!(Reader::new(&output).string(1), Err(CodecError::StringTooLong));
    assert_eq!(Reader::new(&output).string(2), Ok("😀"));
    let before = output.clone();
    assert_eq!(encode_string("😀", 1, &mut output), Err(CodecError::StringTooLong));
    assert_eq!(output, before);
    assert_eq!(Reader::new(&[1, 0xff]).string(1), Err(CodecError::InvalidUtf8));
    assert_eq!(
        Reader::new(&[0xff, 0xff, 0xff, 0xff, 15]).string(1),
        Err(CodecError::InvalidLength)
    );
    assert_eq!(Reader::new(&[4]).string(1), Err(CodecError::StringTooLong));
    assert_eq!(Reader::new(&[2, b'a']).string(2), Err(CodecError::UnexpectedEof));
    assert_eq!(Reader::new(&[0]).string(0), Ok(""));
    assert_eq!(Reader::new(&[1]).finish(), Err(CodecError::TrailingData));
}

#[test]
fn fixed_fields_have_network_byte_order() {
    let mut reader = Reader::new(&[
        1, 0, 0xff, 0xfe, 0x12, 0x34, 0xff, 0xff, 0xff, 0xff, 0x3f, 0x80, 0, 0, 0x3f, 0xf0, 0, 0,
        0, 0, 0, 0,
    ]);
    assert!(reader.boolean().unwrap());
    assert!(!reader.boolean().unwrap());
    assert_eq!(reader.i16(), Ok(-2));
    assert_eq!(reader.u16(), Ok(0x1234));
    assert_eq!(reader.i32(), Ok(-1));
    assert_eq!(reader.f32().unwrap().to_bits(), 1_f32.to_bits());
    assert_eq!(reader.f64().unwrap().to_bits(), 1_f64.to_bits());
    reader.finish().unwrap();
    assert_eq!(Reader::new(&[2]).boolean(), Err(CodecError::InvalidBoolean(2)));
    let uuid = [0x42; 16];
    assert_eq!(Reader::new(&uuid).array::<16>(), Ok(uuid));
    let mut short = Reader::new(&uuid[..15]);
    assert_eq!(short.array::<16>(), Err(CodecError::UnexpectedEof));
    assert_eq!(short.remaining().len(), 15);
}

#[test]
fn position_layout_and_bounds() {
    let position = Position { x: 1, y: 3, z: 2 };
    assert_eq!(position.packed(), Ok((1 << 38) | (2 << 12) | 3));
    assert_eq!(Position::from_packed(u64::MAX), Position { x: -1, y: -1, z: -1 });
    for position in [
        Position { x: 1 << 25, y: 0, z: 0 },
        Position { x: -(1 << 25) - 1, y: 0, z: 0 },
        Position { x: 0, y: 2048, z: 0 },
        Position { x: 0, y: -2049, z: 0 },
        Position { x: 0, y: 0, z: 1 << 25 },
    ] {
        assert_eq!(position.packed(), Err(CodecError::PositionOutOfRange));
    }
}

proptest! {
    #[test]
    fn all_signed_integers_roundtrip(int in any::<i32>(), long in any::<i64>()) {
        let mut bytes = Vec::new();
        encode_varint(int, &mut bytes);
        let int_len = bytes.len();
        encode_varlong(long, &mut bytes);
        prop_assert_eq!(decode_varint(&bytes), Ok((int, int_len)));
        prop_assert_eq!(decode_varlong(&bytes[int_len..]), Ok((long, bytes.len() - int_len)));
        let mut reader = Reader::new(&bytes);
        prop_assert_eq!(reader.varint(), Ok(int));
        prop_assert_eq!(reader.varlong(), Ok(long));
        prop_assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn strings_roundtrip(chars in proptest::collection::vec(any::<char>(), 0..100)) {
        let value: String = chars.into_iter().collect();
        let max = value.encode_utf16().count();
        let mut bytes = Vec::new();
        encode_string(&value, max, &mut bytes).unwrap();
        prop_assert_eq!(Reader::new(&bytes).string(max), Ok(value.as_str()));
        for end in 0..bytes.len() {
            prop_assert!(Reader::new(&bytes[..end]).string(max).is_err());
        }
    }

    #[test]
    fn packed_bits_roundtrip(bits in any::<u64>()) {
        prop_assert_eq!(Position::from_packed(bits).packed(), Ok(bits));
    }

    #[test]
    fn arbitrary_bytes_do_not_panic(bytes in proptest::collection::vec(any::<u8>(), 0..128)) {
        let _ = decode_varint(&bytes);
        let _ = decode_varlong(&bytes);
        let _ = Reader::new(&bytes).string(32);
    }
}
