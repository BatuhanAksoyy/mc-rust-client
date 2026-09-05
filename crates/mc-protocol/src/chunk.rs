//! Level Chunk With Light and chunk-batch bookkeeping for protocol 776.
//!
//! [Packets reference](https://minecraft.wiki/w/Java_Edition_protocol/Packets#Chunk_Data_and_Update_Light),
//! [chunk format](https://minecraft.wiki/w/Java_Edition_protocol/Chunk_format).
//!
//! Field layouts, palette-format thresholds and the Data Array bit-packing
//! were verified against a live Pumpkin connection (a captured payload was
//! decoded byte-for-byte, consuming the packet exactly) before this module
//! was written, not assumed from the wiki alone.

use crate::{
    CodecError,
    framing::RawPacket,
    nbt::{self, NbtError},
    types::Reader,
};
use bytes::Bytes;

/// 26.2 cb play 0x0b `chunk_batch_finished` (wiki Packets 776).
pub const BATCH_FINISHED_ID: i32 = 11;
/// 26.2 cb play 0x0c `chunk_batch_start` (wiki Packets 776).
pub const BATCH_START_ID: i32 = 12;
/// 26.2 sb play 0x0b `chunk_batch_received` (wiki Packets 776).
pub const BATCH_RECEIVED_ID: i32 = 11;
/// 26.2 cb play 0x2d `level_chunk_with_light` (wiki Packets 776).
pub const LEVEL_CHUNK_WITH_LIGHT_ID: i32 = 45;

/// Client policy bound on decoded sections per chunk; a `Data` byte length
/// crafted to imply an implausible section count is rejected rather than
/// trusted. Comfortably above any real dimension height (16 blocks/section).
const MAX_SECTIONS: usize = 384;
/// Client policy bound on a block-states local palette.
const MAX_BLOCK_PALETTE: usize = 4096;
/// Client policy bound on a biomes local palette.
const MAX_BIOME_PALETTE: usize = 64;
/// Bits-per-entry values above this are rejected. `64 / bits` must not
/// divide by zero, and the wiki notes even heavily modded servers stay
/// under this (up to 31 bits for an extreme block-ID count).
const MAX_BITS_PER_ENTRY: u32 = 32;
/// Bits-per-entry at or below this uses an indirect (locally palettized)
/// block-states container; above it, Direct (wiki Chunk format § Palette formats).
const MAX_INDIRECT_BLOCK_BITS: u8 = 8;
/// As above, for biomes.
const MAX_INDIRECT_BIOME_BITS: u8 = 3;
const MAX_HEIGHTMAPS: usize = 16;
const MAX_HEIGHTMAP_LONGS: usize = 1024;
const MAX_BLOCK_ENTITIES: usize = 4096;
const MAX_MASK_LONGS: usize = 64;
const MAX_LIGHT_ARRAYS: usize = MAX_SECTIONS + 2;
const LIGHT_ARRAY_LENGTH: usize = 2048;

/// Chunk field or embedded NBT validation failure.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    /// Invalid packet field.
    #[error(transparent)]
    Codec(#[from] CodecError),
    /// Invalid or over-budget block-entity NBT.
    #[error(transparent)]
    Nbt(#[from] NbtError),
}

/// A block-states or biomes paletted container (wiki Chunk format § Paletted
/// Container structure).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Palette {
    /// Every entry is this one global ID; no data array was sent.
    Single(i32),
    /// Local palette; `PalettedContainer::indices` holds indices into it.
    Indirect(Vec<i32>),
    /// No local palette; `PalettedContainer::indices` holds global IDs directly.
    Direct,
}

/// One resolved paletted container. Semantic interpretation of global IDs
/// (which block/biome an ID names) needs a block-state registry this client
/// does not have yet; that mapping is later work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PalettedContainer {
    /// As transmitted; determines both the palette format and the entries' width.
    pub bits_per_entry: u8,
    /// See [`Palette`].
    pub palette: Palette,
    /// Per-entry values in section order (x fastest, then z, then y).
    /// Empty when `palette` is `Single`, since every entry is that one value.
    pub indices: Vec<u32>,
}

impl PalettedContainer {
    /// Resolve one entry to its global ID, or `None` if `index` is out of range.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<i32> {
        match &self.palette {
            Palette::Single(value) => Some(*value),
            Palette::Indirect(palette) => {
                palette.get(usize::try_from(*self.indices.get(index)?).ok()?).copied()
            }
            Palette::Direct => i32::try_from(*self.indices.get(index)?).ok(),
        }
    }
}

/// One 16×16×16 chunk section (wiki Chunk format § Chunk Section structure).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkSection {
    /// Non-air, non-cave-air, non-void-air block count.
    pub block_count: i16,
    /// Waterlogged blocks plus water/lava with any state, in this section.
    pub fluid_count: i16,
    /// 4096 entries.
    pub block_states: PalettedContainer,
    /// 64 entries (4×4×4 regions).
    pub biomes: PalettedContainer,
}

/// A server-reported heightmap.
///
/// Unpacking into per-column heights needs the dimension's world height
/// (from registry NBT this client stores but does not parse yet), so the
/// packed longs are retained as received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heightmap {
    /// `VarInt` enum; see wiki Chunk format § Heightmap structure.
    pub kind: i32,
    /// Packed data array, in wire order.
    pub data: Vec<u64>,
}

/// One block entity (wiki Packets § Chunk Data and Update Light).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockEntity {
    /// 0..=15, relative to the chunk.
    pub x: u8,
    /// 0..=15, relative to the chunk.
    pub z: u8,
    /// Absolute world height.
    pub y: i16,
    /// Block-entity type; interpreting it needs a registry this client
    /// does not have yet.
    pub kind: i32,
    /// Validated compound NBT, without the X/Y/Z fields, sharing packet storage.
    pub data: Bytes,
}

/// Per-section sky/block light (wiki Packets § Light Data).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightData {
    /// Bit per world section (+2); set means that section has sky light data below.
    pub sky_light_mask: Vec<u64>,
    /// As above, for block light.
    pub block_light_mask: Vec<u64>,
    /// Bit per world section (+2); set means all-zero sky light for that section.
    pub empty_sky_light_mask: Vec<u64>,
    /// As above, for block light.
    pub empty_block_light_mask: Vec<u64>,
    /// One 2048-byte array per bit set in `sky_light_mask`, in ascending order.
    pub sky_light: Vec<Bytes>,
    /// One 2048-byte array per bit set in `block_light_mask`, in ascending order.
    pub block_light: Vec<Bytes>,
}

/// A fully decoded `level_chunk_with_light` packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelChunk {
    /// Chunk coordinate (block coordinate divided by 16, rounded down).
    pub x: i32,
    /// Chunk coordinate (block coordinate divided by 16, rounded down).
    pub z: i32,
    /// Every heightmap the server sent; a missing kind is not an error (the
    /// wiki: absent heightmaps default to minimum height on the client).
    pub heightmaps: Vec<Heightmap>,
    /// Bottom-to-top; the array's own byte length ends the loop, so this
    /// client never needs to know the dimension's height to parse it.
    pub sections: Vec<ChunkSection>,
    /// It is legal for a server to send these later via Block Entity Data instead.
    pub block_entities: Vec<BlockEntity>,
    /// Sky/block light for this chunk and its vertical neighbors.
    pub light: LightData,
}

/// Decode one `level_chunk_with_light` packet.
pub fn decode(packet: &RawPacket) -> Result<LevelChunk, Error> {
    if packet.id != LEVEL_CHUNK_WITH_LIGHT_ID {
        return Err(CodecError::InvalidPacketId(packet.id).into());
    }
    let mut reader = Reader::new(&packet.payload);
    let x = reader.i32()?;
    let z = reader.i32()?;
    let heightmaps = decode_heightmaps(&mut reader)?;
    let sections = decode_sections(&mut reader)?;
    let block_entities = decode_block_entities(&mut reader, &packet.payload)?;
    let light = decode_light(&mut reader, &packet.payload)?;
    reader.finish()?;
    Ok(LevelChunk { x, z, heightmaps, sections, block_entities, light })
}

/// Decode `chunk_batch_finished`'s batch size.
pub fn decode_batch_finished(packet: &RawPacket) -> Result<i32, CodecError> {
    if packet.id != BATCH_FINISHED_ID {
        return Err(CodecError::InvalidPacketId(packet.id));
    }
    let mut reader = Reader::new(&packet.payload);
    let batch_size = reader.varint()?;
    reader.finish()?;
    Ok(batch_size)
}

/// Encode `chunk_batch_received`'s desired chunks-per-tick.
#[must_use]
pub fn batch_received(chunks_per_tick: f32) -> Vec<u8> {
    chunks_per_tick.to_be_bytes().to_vec()
}

fn decode_heightmaps(reader: &mut Reader<'_>) -> Result<Vec<Heightmap>, Error> {
    let count = reader.count(MAX_HEIGHTMAPS)?;
    let mut heightmaps = Vec::with_capacity(count);
    for _ in 0..count {
        let kind = reader.varint()?;
        let longs = reader.count(MAX_HEIGHTMAP_LONGS)?;
        let mut data = Vec::with_capacity(longs);
        for _ in 0..longs {
            data.push(reader.u64()?);
        }
        heightmaps.push(Heightmap { kind, data });
    }
    Ok(heightmaps)
}

fn decode_sections(reader: &mut Reader<'_>) -> Result<Vec<ChunkSection>, Error> {
    let byte_len = usize::try_from(reader.varint()?).map_err(|_| CodecError::InvalidLength)?;
    let mut sub = Reader::new(reader.take(byte_len)?);
    let mut sections = Vec::new();
    while !sub.remaining().is_empty() {
        if sections.len() == MAX_SECTIONS {
            return Err(CodecError::InvalidValue("too many chunk sections").into());
        }
        sections.push(decode_section(&mut sub)?);
    }
    Ok(sections)
}

fn decode_section(reader: &mut Reader<'_>) -> Result<ChunkSection, Error> {
    let block_count = reader.i16()?;
    let fluid_count = reader.i16()?;
    let block_states =
        decode_paletted_container(reader, 4096, MAX_INDIRECT_BLOCK_BITS, MAX_BLOCK_PALETTE)?;
    let biomes = decode_paletted_container(reader, 64, MAX_INDIRECT_BIOME_BITS, MAX_BIOME_PALETTE)?;
    Ok(ChunkSection { block_count, fluid_count, block_states, biomes })
}

fn decode_paletted_container(
    reader: &mut Reader<'_>,
    entries_len: usize,
    max_indirect_bits: u8,
    max_palette_len: usize,
) -> Result<PalettedContainer, Error> {
    let bits_per_entry = reader.u8()?;
    if bits_per_entry == 0 {
        let value = reader.varint()?;
        return Ok(PalettedContainer {
            bits_per_entry,
            palette: Palette::Single(value),
            indices: Vec::new(),
        });
    }
    if u32::from(bits_per_entry) > MAX_BITS_PER_ENTRY {
        return Err(CodecError::InvalidValue("paletted container bits per entry").into());
    }
    let local_palette = if bits_per_entry <= max_indirect_bits {
        let len = reader.count(max_palette_len)?;
        let mut values = Vec::with_capacity(len);
        for _ in 0..len {
            values.push(reader.varint()?);
        }
        Some(values)
    } else {
        None
    };
    let entries_per_long = 64 / u32::from(bits_per_entry);
    let num_longs = entries_len.div_ceil(entries_per_long as usize);
    let mut longs = Vec::with_capacity(num_longs);
    for _ in 0..num_longs {
        longs.push(reader.u64()?);
    }
    let indices = unpack(&longs, u32::from(bits_per_entry), entries_len);
    let palette = match local_palette {
        Some(values) => {
            if indices.iter().any(|&index| index as usize >= values.len()) {
                return Err(
                    CodecError::InvalidValue("paletted container index out of range").into()
                );
            }
            Palette::Indirect(values)
        }
        None => Palette::Direct,
    };
    Ok(PalettedContainer { bits_per_entry, palette, indices })
}

/// Unpack `count` `bits`-wide unsigned entries from big-endian longs; an
/// entry never spans two longs (wiki Chunk format § Data Array format).
///
/// `bits` is bounded by [`MAX_BITS_PER_ENTRY`] (32), so the masked value
/// always fits in 32 bits.
#[allow(clippy::cast_possible_truncation)]
fn unpack(longs: &[u64], bits: u32, count: usize) -> Vec<u32> {
    let entries_per_long = 64 / bits;
    let mask = (1_u64 << bits) - 1;
    let mut values = Vec::with_capacity(count);
    'longs: for &long in longs {
        for slot in 0..entries_per_long {
            if values.len() == count {
                break 'longs;
            }
            values.push(((long >> (slot * bits)) & mask) as u32);
        }
    }
    values
}

fn decode_block_entities(
    reader: &mut Reader<'_>,
    payload: &Bytes,
) -> Result<Vec<BlockEntity>, Error> {
    let count = reader.count(MAX_BLOCK_ENTITIES)?;
    let mut entities = Vec::with_capacity(count);
    for _ in 0..count {
        let packed_xz = reader.u8()?;
        let y = reader.i16()?;
        let kind = reader.varint()?;
        let (_, length) = nbt::decode_network(reader.remaining(), nbt::Limits::default())?;
        let data = payload.slice_ref(reader.take(length)?);
        entities.push(BlockEntity { x: packed_xz >> 4, z: packed_xz & 0x0f, y, kind, data });
    }
    Ok(entities)
}

fn decode_light(reader: &mut Reader<'_>, payload: &Bytes) -> Result<LightData, Error> {
    let sky_light_mask = decode_bitset(reader)?;
    let block_light_mask = decode_bitset(reader)?;
    let empty_sky_light_mask = decode_bitset(reader)?;
    let empty_block_light_mask = decode_bitset(reader)?;
    let sky_light = decode_light_arrays(reader, payload, popcount(&sky_light_mask))?;
    let block_light = decode_light_arrays(reader, payload, popcount(&block_light_mask))?;
    Ok(LightData {
        sky_light_mask,
        block_light_mask,
        empty_sky_light_mask,
        empty_block_light_mask,
        sky_light,
        block_light,
    })
}

fn decode_bitset(reader: &mut Reader<'_>) -> Result<Vec<u64>, Error> {
    let count = reader.count(MAX_MASK_LONGS)?;
    let mut longs = Vec::with_capacity(count);
    for _ in 0..count {
        longs.push(reader.u64()?);
    }
    Ok(longs)
}

fn popcount(mask: &[u64]) -> usize {
    mask.iter().map(|long| long.count_ones() as usize).sum()
}

fn decode_light_arrays(
    reader: &mut Reader<'_>,
    payload: &Bytes,
    expected: usize,
) -> Result<Vec<Bytes>, Error> {
    let count = reader.count(MAX_LIGHT_ARRAYS)?;
    if count != expected {
        return Err(CodecError::InvalidValue("light array count does not match its mask").into());
    }
    let mut arrays = Vec::with_capacity(count);
    for _ in 0..count {
        let length = usize::try_from(reader.varint()?).map_err(|_| CodecError::InvalidLength)?;
        if length != LIGHT_ARRAY_LENGTH {
            return Err(CodecError::InvalidValue("light array length").into());
        }
        arrays.push(payload.slice_ref(reader.take(length)?));
    }
    Ok(arrays)
}
