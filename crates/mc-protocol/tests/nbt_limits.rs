//! Hostile-input and cumulative-resource tests for the client NBT decoder.

use mc_protocol::{
    CodecError,
    nbt::{Limits, MAX_DEPTH, NbtError, decode_named, decode_network},
};
use proptest::prelude::*;

#[test]
fn invalid_types_lengths_and_end_lists_are_rejected() {
    for id in 13..=255 {
        assert_eq!(decode_network(&[id], Limits::default()), Err(NbtError::InvalidTag(id)));
        assert_eq!(
            decode_network(&[9, id, 0, 0, 0, 0], Limits::default()),
            Err(NbtError::InvalidTag(id))
        );
    }
    for id in [7, 9, 11, 12] {
        let mut bytes = vec![id];
        if id == 9 {
            bytes.push(1);
        }
        bytes.extend_from_slice(&(-1_i32).to_be_bytes());
        assert_eq!(
            decode_network(&bytes, Limits::default()),
            Err(NbtError::Codec(CodecError::InvalidLength))
        );
    }
    assert_eq!(decode_network(&[9, 0, 0, 0, 0, 1], Limits::default()), Err(NbtError::EndList));
}

#[test]
fn every_truncated_prefix_of_each_tag_fails() {
    let fixtures: &[&[u8]] = &[
        &[0],
        &[1, 0],
        &[2, 0, 0],
        &[3, 0, 0, 0, 0],
        &[4, 0, 0, 0, 0, 0, 0, 0, 0],
        &[5, 0, 0, 0, 0],
        &[6, 0, 0, 0, 0, 0, 0, 0, 0],
        &[7, 0, 0, 0, 2, 1, 2],
        &[8, 0, 2, b'h', b'i'],
        &[9, 1, 0, 0, 0, 1, 42],
        &[10, 1, 0, 1, b'x', 1, 0],
        &[11, 0, 0, 0, 1, 0, 0, 0, 1],
        &[12, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1],
    ];
    for fixture in fixtures {
        assert!(decode_network(fixture, Limits::default()).is_ok());
        for end in 0..fixture.len() {
            assert!(
                decode_network(&fixture[..end], Limits::default()).is_err(),
                "{fixture:?} at {end}"
            );
        }
        let mut named = vec![fixture[0]];
        if fixture[0] != 0 {
            named.extend_from_slice(&[0, 1, b'r']);
        }
        named.extend_from_slice(&fixture[1..]);
        assert!(decode_named(&named, Limits::default()).is_ok());
        for end in 0..named.len() {
            assert!(decode_named(&named[..end], Limits::default()).is_err());
        }
    }
}

#[test]
fn byte_budget_is_exact_and_ignores_trailing_packet_data() {
    let bytes = [3, 0, 0, 0, 1, 0xaa, 0xbb];
    for max_bytes in 0..5 {
        assert_eq!(
            decode_network(&bytes, Limits { max_bytes, ..Limits::default() }),
            Err(NbtError::ByteLimit)
        );
    }
    let (_, consumed) =
        decode_network(&bytes, Limits { max_bytes: 5, ..Limits::default() }).unwrap();
    assert_eq!(consumed, 5);
}

#[test]
fn declared_huge_counts_fail_without_backing_payloads() {
    for (id, expected) in [
        (7, NbtError::ByteLimit),
        (9, NbtError::ElementLimit),
        (11, NbtError::ElementLimit),
        (12, NbtError::ElementLimit),
    ] {
        let mut bytes = vec![id];
        if id == 9 {
            bytes.push(10);
        }
        bytes.extend_from_slice(&i32::MAX.to_be_bytes());
        assert_eq!(decode_network(&bytes, Limits::default()), Err(expected));
    }
}

#[test]
fn allocation_budget_counts_all_siblings_names_and_array_elements() {
    // Root + two IntArray children + names a,b + three array integers = 8 units.
    let bytes = [
        10, 11, 0, 1, b'a', 0, 0, 0, 1, 0, 0, 0, 0, 11, 0, 1, b'b', 0, 0, 0, 2, 0, 0, 0, 0, 0, 0,
        0, 0, 0,
    ];
    for max_elements in 0..8 {
        assert_eq!(
            decode_network(&bytes, Limits { max_elements, ..Limits::default() }),
            Err(NbtError::ElementLimit)
        );
    }
    assert!(decode_network(&bytes, Limits { max_elements: 8, ..Limits::default() }).is_ok());
    // UTF-16 units, not encoded bytes: root + a two-unit supplementary character.
    let string = [8, 0, 6, 0xed, 0xa0, 0xbd, 0xed, 0xb8, 0x80];
    assert_eq!(
        decode_network(&string, Limits { max_elements: 2, ..Limits::default() }),
        Err(NbtError::ElementLimit)
    );
    assert!(decode_network(&string, Limits { max_elements: 3, ..Limits::default() }).is_ok());
    // Byte arrays are borrowed and only charge their tag.
    assert!(
        decode_network(&[7, 0, 0, 0, 2, 1, 2], Limits { max_elements: 1, ..Limits::default() })
            .is_ok()
    );
}

fn nested_lists(depth: usize) -> Vec<u8> {
    let mut bytes = vec![9];
    for _ in 0..depth {
        bytes.extend_from_slice(&[9, 0, 0, 0, 1]);
    }
    bytes.extend_from_slice(&[0, 0, 0, 0, 0]);
    bytes
}

#[test]
fn recursion_is_bounded_even_with_custom_limits() {
    assert!(decode_network(&nested_lists(MAX_DEPTH), Limits::default()).is_ok());
    assert_eq!(
        decode_network(&nested_lists(MAX_DEPTH + 1), Limits::default()),
        Err(NbtError::DepthLimit)
    );
    assert_eq!(
        decode_network(&[0], Limits { max_depth: usize::MAX, ..Limits::default() }),
        Err(NbtError::DepthLimit)
    );
    let root_only = Limits { max_depth: 0, ..Limits::default() };
    assert!(decode_network(&[10, 0], root_only).is_ok());
    assert!(decode_network(&[9, 0, 0, 0, 0, 0], root_only).is_ok());
    assert_eq!(decode_network(&[10, 1, 0, 0, 1, 0], root_only), Err(NbtError::DepthLimit));
    assert_eq!(decode_network(&nested_lists(1), root_only), Err(NbtError::DepthLimit));
}

#[test]
fn malformed_modified_utf8_is_not_replaced_lossily() {
    for encoded in [
        &[0x80][..],
        &[0xc2],
        &[0xc2, b'a'],
        &[0xe1, 0x80],
        &[0xe1, 0x80, b'a'],
        &[0xf0, 0x9f, 0x98, 0x80],
    ] {
        let mut bytes = vec![8];
        bytes.extend_from_slice(&u16::try_from(encoded.len()).unwrap().to_be_bytes());
        bytes.extend_from_slice(encoded);
        assert_eq!(decode_network(&bytes, Limits::default()), Err(NbtError::InvalidString));
    }
}

proptest! {
    #[test]
    fn arbitrary_input_and_limits_do_not_panic(
        bytes in prop::collection::vec(any::<u8>(), 0..2048),
        max_bytes in 0_usize..2048,
        max_elements in 0_usize..256,
        max_depth in 0_usize..=MAX_DEPTH,
    ) {
        let limits = Limits { max_bytes, max_elements, max_depth };
        if let Ok((_, consumed)) = decode_network(&bytes, limits) {
            prop_assert!(consumed <= bytes.len() && consumed <= max_bytes);
        }
        if let Ok((_, consumed)) = decode_named(&bytes, limits) {
            prop_assert!(consumed <= bytes.len() && consumed <= max_bytes);
        }
    }
}
