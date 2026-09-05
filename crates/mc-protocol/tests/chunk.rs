//! Synthetic protocol 776 `level_chunk_with_light`/chunk-batch fixtures.
//! No game data is vendored; every fixture is hand-encoded here.

use bytes::Bytes;
use mc_protocol::{CodecError, chunk, encode_varint, framing::RawPacket};

fn varint(value: i32) -> Vec<u8> {
    let mut out = Vec::new();
    encode_varint(value, &mut out);
    out
}

/// Pack `bits`-wide entries into big-endian longs (wiki Chunk format § Data
/// Array format): no entry spans two longs, padding sits at the top.
fn pack_bits(values: &[u32], bits: u32) -> Vec<u8> {
    let entries_per_long = 64 / bits;
    let mut body = Vec::new();
    for chunk in values.chunks(entries_per_long as usize) {
        let mut long: u64 = 0;
        for (slot, &value) in chunk.iter().enumerate() {
            long |= u64::from(value) << (u32::try_from(slot).unwrap() * bits);
        }
        body.extend(long.to_be_bytes());
    }
    body
}

fn single_valued(value: i32) -> Vec<u8> {
    let mut body = vec![0];
    body.extend(varint(value));
    body
}

fn indirect(bits: u32, palette: &[i32], entries: &[u32], entries_len: usize) -> Vec<u8> {
    let mut body = vec![u8::try_from(bits).unwrap()];
    body.extend(varint(i32::try_from(palette.len()).unwrap()));
    for &id in palette {
        body.extend(varint(id));
    }
    let mut padded = entries.to_vec();
    padded.resize(entries_len, 0);
    body.extend(pack_bits(&padded, bits));
    body
}

fn direct(bits: u32, entries: &[u32], entries_len: usize) -> Vec<u8> {
    let mut body = vec![u8::try_from(bits).unwrap()];
    let mut padded = entries.to_vec();
    padded.resize(entries_len, 0);
    body.extend(pack_bits(&padded, bits));
    body
}

fn section(block_count: i16, fluid_count: i16, blocks: Vec<u8>, biomes: Vec<u8>) -> Vec<u8> {
    let mut body = block_count.to_be_bytes().to_vec();
    body.extend(fluid_count.to_be_bytes());
    body.extend(blocks);
    body.extend(biomes);
    body
}

fn data_field(sections: &[Vec<u8>]) -> Vec<u8> {
    let joined = sections.concat();
    let mut body = varint(i32::try_from(joined.len()).unwrap());
    body.extend(joined);
    body
}

fn heightmaps(entries: &[(i32, &[u64])]) -> Vec<u8> {
    let mut body = varint(i32::try_from(entries.len()).unwrap());
    for (kind, longs) in entries {
        body.extend(varint(*kind));
        body.extend(varint(i32::try_from(longs.len()).unwrap()));
        for long in *longs {
            body.extend(long.to_be_bytes());
        }
    }
    body
}

fn block_entities(entries: &[(u8, u8, i16, i32, &[u8])]) -> Vec<u8> {
    let mut body = varint(i32::try_from(entries.len()).unwrap());
    for (x, z, y, kind, nbt) in entries {
        body.push(((x & 0x0f) << 4) | (z & 0x0f));
        body.extend(y.to_be_bytes());
        body.extend(varint(*kind));
        body.extend_from_slice(nbt);
    }
    body
}

fn bitset(longs: &[u64]) -> Vec<u8> {
    let mut body = varint(i32::try_from(longs.len()).unwrap());
    for long in longs {
        body.extend(long.to_be_bytes());
    }
    body
}

fn light_arrays(arrays: &[&[u8]]) -> Vec<u8> {
    let mut body = varint(i32::try_from(arrays.len()).unwrap());
    for array in arrays {
        body.extend(varint(i32::try_from(array.len()).unwrap()));
        body.extend_from_slice(array);
    }
    body
}

#[allow(clippy::too_many_arguments)]
fn light(
    sky_mask: &[u64],
    block_mask: &[u64],
    empty_sky_mask: &[u64],
    empty_block_mask: &[u64],
    sky_arrays: &[&[u8]],
    block_arrays: &[&[u8]],
) -> Vec<u8> {
    let mut body = bitset(sky_mask);
    body.extend(bitset(block_mask));
    body.extend(bitset(empty_sky_mask));
    body.extend(bitset(empty_block_mask));
    body.extend(light_arrays(sky_arrays));
    body.extend(light_arrays(block_arrays));
    body
}

fn no_light() -> Vec<u8> {
    light(&[], &[], &[], &[], &[], &[])
}

fn level_chunk(
    x: i32,
    z: i32,
    heightmaps: Vec<u8>,
    sections: &[Vec<u8>],
    block_entities: Vec<u8>,
    light: Vec<u8>,
) -> RawPacket {
    let mut body = x.to_be_bytes().to_vec();
    body.extend(z.to_be_bytes());
    body.extend(heightmaps);
    body.extend(data_field(sections));
    body.extend(block_entities);
    body.extend(light);
    RawPacket { id: chunk::LEVEL_CHUNK_WITH_LIGHT_ID, payload: Bytes::from(body) }
}

fn empty_section() -> Vec<u8> {
    section(0, 0, single_valued(0), single_valued(0))
}

#[test]
fn minimal_chunk_with_single_valued_containers_decodes() {
    let packet = level_chunk(
        1,
        -3,
        heightmaps(&[]),
        &[section(0, 0, single_valued(0), single_valued(4))],
        block_entities(&[]),
        no_light(),
    );
    let decoded = chunk::decode(&packet).unwrap();
    assert_eq!(decoded.x, 1);
    assert_eq!(decoded.z, -3);
    assert!(decoded.heightmaps.is_empty());
    assert_eq!(decoded.sections.len(), 1);
    assert_eq!(decoded.sections[0].block_states.get(0), Some(0));
    assert_eq!(decoded.sections[0].block_states.get(4095), Some(0));
    assert_eq!(decoded.sections[0].biomes.get(0), Some(4));
    assert!(decoded.block_entities.is_empty());
    assert!(decoded.light.sky_light.is_empty());
}

#[test]
fn indirect_and_direct_palettes_resolve_expected_values() {
    let blocks = indirect(4, &[10, 20, 30], &[0, 1, 2], 4096);
    let biomes = direct(4, &[5, 9], 64); // 4 bits > MAX_INDIRECT_BIOME_BITS (3): Direct.
    let packet = level_chunk(
        0,
        0,
        heightmaps(&[]),
        &[section(100, 0, blocks, biomes)],
        block_entities(&[]),
        no_light(),
    );
    let decoded = chunk::decode(&packet).unwrap();
    let section = &decoded.sections[0];
    assert_eq!(section.block_states.get(0), Some(10));
    assert_eq!(section.block_states.get(1), Some(20));
    assert_eq!(section.block_states.get(2), Some(30));
    assert_eq!(section.block_states.get(3), Some(10)); // Unset entries default to palette index 0.
    assert_eq!(section.biomes.get(0), Some(5));
    assert_eq!(section.biomes.get(1), Some(9));
    assert_eq!(section.biomes.get(2), Some(0));
}

#[test]
fn heightmaps_and_block_entities_are_retained() {
    let hm = heightmaps(&[(4, &[0x1122_3344_5566_7788, 0])]);
    let be = block_entities(&[(3, 5, 64, 7, &[10, 0])]); // Empty compound NBT.
    let packet = level_chunk(2, 2, hm, &[empty_section()], be, no_light());
    let decoded = chunk::decode(&packet).unwrap();
    assert_eq!(decoded.heightmaps.len(), 1);
    assert_eq!(decoded.heightmaps[0].kind, 4);
    assert_eq!(decoded.heightmaps[0].data, vec![0x1122_3344_5566_7788, 0]);
    assert_eq!(decoded.block_entities.len(), 1);
    let entity = &decoded.block_entities[0];
    assert_eq!((entity.x, entity.z, entity.y, entity.kind), (3, 5, 64, 7));
    assert_eq!(entity.data.as_ref(), &[10, 0]);
}

#[test]
fn light_arrays_match_their_masks() {
    let sky = [7_u8; 2048];
    let packet = level_chunk(
        0,
        0,
        heightmaps(&[]),
        &[empty_section()],
        block_entities(&[]),
        light(&[0b1], &[], &[], &[], &[&sky], &[]),
    );
    let decoded = chunk::decode(&packet).unwrap();
    assert_eq!(decoded.light.sky_light.len(), 1);
    assert_eq!(decoded.light.sky_light[0].as_ref(), &sky[..]);
    assert!(decoded.light.block_light.is_empty());
}

#[test]
fn malformed_paletted_containers_are_rejected() {
    let excessive_bits = section(0, 0, vec![250, 0], single_valued(0));
    let packet =
        level_chunk(0, 0, heightmaps(&[]), &[excessive_bits], block_entities(&[]), no_light());
    assert!(matches!(
        chunk::decode(&packet),
        Err(chunk::Error::Codec(CodecError::InvalidValue(_)))
    ));

    let out_of_range = section(0, 0, indirect(4, &[10], &[5], 4096), single_valued(0));
    let packet =
        level_chunk(0, 0, heightmaps(&[]), &[out_of_range], block_entities(&[]), no_light());
    assert!(matches!(
        chunk::decode(&packet),
        Err(chunk::Error::Codec(CodecError::InvalidValue(_)))
    ));
}

#[test]
fn malformed_light_data_is_rejected() {
    let wrong_length = light(&[0b1], &[], &[], &[], &[&[0; 10]], &[]);
    let packet =
        level_chunk(0, 0, heightmaps(&[]), &[empty_section()], block_entities(&[]), wrong_length);
    assert!(matches!(
        chunk::decode(&packet),
        Err(chunk::Error::Codec(CodecError::InvalidValue(_)))
    ));

    let mismatched_count = light(&[0b1], &[], &[], &[], &[], &[]); // Mask has 1 bit; 0 arrays given.
    let packet = level_chunk(
        0,
        0,
        heightmaps(&[]),
        &[empty_section()],
        block_entities(&[]),
        mismatched_count,
    );
    assert!(matches!(
        chunk::decode(&packet),
        Err(chunk::Error::Codec(CodecError::InvalidValue(_)))
    ));
}

#[test]
fn trailing_data_after_chunk_is_rejected() {
    let mut packet =
        level_chunk(0, 0, heightmaps(&[]), &[empty_section()], block_entities(&[]), no_light());
    let mut body = packet.payload.to_vec();
    body.push(0xff);
    packet.payload = Bytes::from(body);
    assert_eq!(chunk::decode(&packet), Err(chunk::Error::Codec(CodecError::TrailingData)));
}

#[test]
fn excessive_section_count_is_rejected() {
    let sections: Vec<Vec<u8>> = std::iter::repeat_with(empty_section).take(385).collect();
    let packet = level_chunk(0, 0, heightmaps(&[]), &sections, block_entities(&[]), no_light());
    assert!(matches!(
        chunk::decode(&packet),
        Err(chunk::Error::Codec(CodecError::InvalidValue(_)))
    ));
}

#[test]
fn packet_id_is_validated() {
    let mut packet =
        level_chunk(0, 0, heightmaps(&[]), &[empty_section()], block_entities(&[]), no_light());
    packet.id = 99;
    assert_eq!(chunk::decode(&packet), Err(chunk::Error::Codec(CodecError::InvalidPacketId(99))));
}

#[test]
fn batch_finished_decodes_and_validates() {
    let packet = RawPacket { id: chunk::BATCH_FINISHED_ID, payload: Bytes::from(varint(5)) };
    assert_eq!(chunk::decode_batch_finished(&packet), Ok(5));

    let wrong_id = RawPacket { id: 1, payload: Bytes::new() };
    assert_eq!(chunk::decode_batch_finished(&wrong_id), Err(CodecError::InvalidPacketId(1)));
}

#[test]
fn batch_received_encodes_a_big_endian_float() {
    assert_eq!(chunk::batch_received(2.5), 2.5_f32.to_be_bytes().to_vec());
}
