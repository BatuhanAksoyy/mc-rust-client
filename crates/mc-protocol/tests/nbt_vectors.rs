//! Synthetic protocol 776 NBT fixtures; no game data or Java runtime required.

use mc_protocol::nbt::{Limits, Tag, decode_named, decode_network};
use proptest::prelude::*;

fn decode(bytes: &[u8]) -> Tag<'_> {
    let (tag, consumed) = decode_network(bytes, Limits::default()).unwrap();
    assert_eq!(consumed, bytes.len());
    tag
}

#[test]
fn scalar_vectors_preserve_signed_values_and_float_bits() {
    assert_eq!(decode(&[0]), Tag::End);
    assert_eq!(decode(&[1, 0x80]), Tag::Byte(i8::MIN));
    assert_eq!(decode(&[2, 0xff, 0xfe]), Tag::Short(-2));
    assert_eq!(decode(&[3, 0x80, 0, 0, 0]), Tag::Int(i32::MIN));
    assert_eq!(decode(&[4, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe]), Tag::Long(-2));
    for bits in [0_u32, 0x8000_0000, 0x3fc0_0000, 0x7f80_0000, 0x7fc0_0123] {
        let bytes = [&[5][..], &bits.to_be_bytes()].concat();
        let Tag::Float(value) = decode(&bytes) else { panic!("expected Float") };
        assert_eq!(value.to_bits(), bits);
    }
    for bits in [0_u64, 0x8000_0000_0000_0000, 0x3ff8_0000_0000_0000, 0x7ff8_0000_0000_0123] {
        let bytes = [&[6][..], &bits.to_be_bytes()].concat();
        let Tag::Double(value) = decode(&bytes) else { panic!("expected Double") };
        assert_eq!(value.to_bits(), bits);
    }
}

#[test]
fn arrays_are_big_endian_and_byte_arrays_borrow() {
    let bytes = [7, 0, 0, 0, 3, 0, 0x80, 0xff];
    let Tag::ByteArray(value) = decode(&bytes) else { panic!("expected ByteArray") };
    assert_eq!(value, &[0, 0x80, 0xff]);
    assert_eq!(value.as_ptr(), bytes[5..].as_ptr());
    assert_eq!(
        decode(&[11, 0, 0, 0, 2, 0, 0, 0, 1, 0xff, 0xff, 0xff, 0xfe]),
        Tag::IntArray(vec![1, -2])
    );
    assert_eq!(
        decode(&[12, 0, 0, 0, 1, 0x80, 0, 0, 0, 0, 0, 0, 0]),
        Tag::LongArray(vec![i64::MIN])
    );
    for id in [7, 11, 12] {
        assert!(decode_network(&[id, 0, 0, 0, 0], Limits::default()).is_ok());
    }
}

#[test]
fn named_and_network_roots_have_different_headers() {
    let network = [10, 1, 0, 1, b'x', 42, 0, 0xaa];
    let named = [10, 0, 1, b'r', 1, 0, 1, b'x', 42, 0, 0xbb];
    let (root, consumed) = decode_network(&network, Limits::default()).unwrap();
    assert_eq!(consumed, 7);
    let (named_root, consumed) = decode_named(&named, Limits::default()).unwrap();
    assert_eq!(consumed, 10);
    assert_eq!(named_root.name.to_utf8().unwrap(), "r");
    assert_eq!(named_root.tag, root);
    let (end, consumed) = decode_named(&[0, 0xff], Limits::default()).unwrap();
    assert_eq!(end.tag, Tag::End);
    assert!(end.name.as_utf16().is_empty());
    assert_eq!(consumed, 1);
}

#[test]
fn compounds_preserve_duplicate_names_and_lists_retain_type() {
    let Tag::Compound(entries) = decode(&[10, 1, 0, 1, b'x', 1, 1, 0, 1, b'x', 2, 0]) else {
        panic!("expected Compound")
    };
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].name, entries[1].name);
    assert_eq!(entries[0].tag, Tag::Byte(1));
    assert_eq!(entries[1].tag, Tag::Byte(2));
    assert_eq!(
        decode(&[9, 2, 0, 0, 0, 2, 0, 1, 0xff, 0xff]),
        Tag::List { element_id: 2, elements: vec![Tag::Short(1), Tag::Short(-1)] }
    );
    assert_eq!(
        decode(&[9, 10, 0, 0, 0, 2, 0, 0]),
        Tag::List { element_id: 10, elements: vec![Tag::Compound(vec![]); 2] }
    );
    for id in 0..=12 {
        assert_eq!(decode(&[9, id, 0, 0, 0, 0]), Tag::List { element_id: id, elements: vec![] });
    }
}

#[test]
fn modified_utf8_preserves_nul_supplementary_and_isolated_surrogates() {
    // A, NUL, U+00E9, U+6C34, U+1F600 encoded through a surrogate pair.
    let bytes = [
        8, 0, 14, b'A', 0xc0, 0x80, 0xc3, 0xa9, 0xe6, 0xb0, 0xb4, 0xed, 0xa0, 0xbd, 0xed, 0xb8,
        0x80,
    ];
    let Tag::String(value) = decode(&bytes) else { panic!("expected String") };
    assert_eq!(value.to_utf8().unwrap(), "A\0é水😀");
    let Tag::String(value) = decode(&[8, 0, 3, 0xed, 0xa0, 0x80]) else {
        panic!("expected String")
    };
    assert_eq!(value.as_utf16(), &[0xd800]);
    assert!(value.to_utf8().is_err());
    // DataInput.readUTF also accepts literal NUL and noncanonical overlong forms.
    let Tag::String(value) = decode(&[8, 0, 6, 0, 0xc1, 0x81, 0xe0, 0x81, 0x81]) else {
        panic!("expected String")
    };
    assert_eq!(value.to_utf8().unwrap(), "\0AA");
}

proptest! {
    #[test]
    fn integer_array_values_survive_wire_decoding(values in prop::collection::vec(any::<i32>(), 0..256)) {
        let mut bytes = vec![11];
        bytes.extend_from_slice(&i32::try_from(values.len()).unwrap().to_be_bytes());
        for value in &values {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        prop_assert_eq!(decode(&bytes), Tag::IntArray(values));
    }

    #[test]
    fn all_java_utf16_units_survive_modified_utf8(units in prop::collection::vec(any::<u16>(), 0..128)) {
        let mut encoded = Vec::new();
        for &unit in &units {
            let [hi, lo] = unit.to_be_bytes();
            if (1..=0x7f).contains(&unit) {
                encoded.push(lo);
            } else if unit <= 0x7ff {
                encoded.extend_from_slice(&[0xc0 | (hi << 2) | (lo >> 6), 0x80 | (lo & 0x3f)]);
            } else {
                encoded.extend_from_slice(&[
                    0xe0 | (hi >> 4), 0x80 | ((hi & 0xf) << 2) | (lo >> 6), 0x80 | (lo & 0x3f)
                ]);
            }
        }
        let mut bytes = vec![8];
        bytes.extend_from_slice(&u16::try_from(encoded.len()).unwrap().to_be_bytes());
        bytes.extend_from_slice(&encoded);
        let Tag::String(decoded) = decode(&bytes) else { panic!("expected String") };
        prop_assert_eq!(decoded.as_utf16(), units);
    }
}
