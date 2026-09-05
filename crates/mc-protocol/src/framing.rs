//! Incremental protocol 776 framing and zlib; wiki Packets § Packet format.
//! [Wire reference](https://minecraft.wiki/w/Java_Edition_protocol/Packets#Packet_format).
//!
//! The caller bounds its receive buffer and owns I/O. Incomplete input and errors
//! consume nothing. Errors are terminal. Uncompressed payloads share input storage.

use std::io::Write;

use bytes::{Bytes, BytesMut};
use flate2::{Compression, Decompress, FlushDecompress, Status, write::ZlibEncoder};

use crate::{CodecError, MAX_PACKET_SIZE, MAX_UNCOMPRESSED_SIZE, decode_varint, encode_varint};

/// A decoded packet with its frame and ID prefixes removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPacket {
    /// Nonnegative packet ID, interpreted in the current connection state.
    pub id: i32,
    /// Packet fields, sharing storage when compression is disabled.
    pub payload: Bytes,
}

/// Framing state. Set compression only after consuming its negotiation packet.
#[derive(Debug, Clone, Default)]
pub struct FrameCodec {
    threshold: Option<usize>,
}

impl FrameCodec {
    /// Use the negotiated threshold; any negative value disables compression.
    pub fn set_compression(&mut self, threshold: i32) {
        self.threshold = usize::try_from(threshold).ok();
    }

    /// Decode one frame, or return `None` without changing incomplete input.
    pub fn decode(&self, input: &mut BytesMut) -> Result<Option<RawPacket>, CodecError> {
        let Some((length, prefix)) = frame_length(input)? else { return Ok(None) };
        let total = prefix + length;
        if input.len() < total {
            return Ok(None);
        }
        let body = &input[prefix..total];
        let mut offset = 0;
        let inflated = if let Some(threshold) = self.threshold {
            let (declared, count) = decode_varint(body)?;
            offset = count;
            let declared = usize::try_from(declared).map_err(|_| CodecError::InvalidLength)?;
            if declared == 0 {
                if body.len() - offset >= threshold {
                    return Err(CodecError::CompressionThreshold);
                }
                None
            } else {
                if declared > MAX_UNCOMPRESSED_SIZE {
                    return Err(CodecError::PacketTooLarge(declared));
                }
                if declared < threshold {
                    return Err(CodecError::CompressionThreshold);
                }
                Some(inflate(&body[offset..], declared)?)
            }
        } else {
            None
        };

        let packet = inflated.as_deref().unwrap_or_else(|| &body[offset..]);
        let (id, id_length) = decode_varint(packet)?;
        if id < 0 {
            return Err(CodecError::InvalidPacketId(id));
        }
        let frame = input.split_to(total).freeze();
        let payload = inflated.map_or_else(
            || frame.slice(prefix + offset + id_length..),
            |bytes| Bytes::from(bytes).slice(id_length..),
        );
        Ok(Some(RawPacket { id, payload }))
    }

    /// Encode one packet, checking sizes before allocating packet storage.
    pub fn encode(&self, id: i32, payload: &[u8]) -> Result<Bytes, CodecError> {
        if id < 0 {
            return Err(CodecError::InvalidPacketId(id));
        }
        let mut id_bytes = Vec::with_capacity(5);
        encode_varint(id, &mut id_bytes);
        let length = payload
            .len()
            .checked_add(id_bytes.len())
            .ok_or(CodecError::PacketTooLarge(usize::MAX))?;
        let limit = if self.threshold.is_some() { MAX_UNCOMPRESSED_SIZE } else { MAX_PACKET_SIZE };
        if length > limit {
            return Err(CodecError::PacketTooLarge(length));
        }

        let mut body = Vec::new();
        if let Some(threshold) = self.threshold {
            if length >= threshold {
                encode_varint(
                    i32::try_from(length).map_err(|_| CodecError::InvalidLength)?,
                    &mut body,
                );
                let mut zlib = ZlibEncoder::new(body, Compression::default());
                zlib.write_all(&id_bytes).map_err(|_| CodecError::InvalidCompression)?;
                zlib.write_all(payload).map_err(|_| CodecError::InvalidCompression)?;
                body = zlib.finish().map_err(|_| CodecError::InvalidCompression)?;
            } else {
                body.push(0);
                body.extend_from_slice(&id_bytes);
                body.extend_from_slice(payload);
            }
        } else {
            body.extend_from_slice(&id_bytes);
            body.extend_from_slice(payload);
        }
        if body.len() > MAX_PACKET_SIZE {
            return Err(CodecError::PacketTooLarge(body.len()));
        }
        let mut frame = Vec::with_capacity(body.len() + 3);
        encode_varint(
            i32::try_from(body.len()).map_err(|_| CodecError::InvalidLength)?,
            &mut frame,
        );
        frame.extend_from_slice(&body);
        Ok(Bytes::from(frame))
    }
}

fn frame_length(input: &[u8]) -> Result<Option<(usize, usize)>, CodecError> {
    let mut length = 0;
    for i in 0..3 {
        let Some(&byte) = input.get(i) else { return Ok(None) };
        length |= usize::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return if length == 0 {
                Err(CodecError::InvalidLength)
            } else {
                Ok(Some((length, i + 1)))
            };
        }
    }
    Err(CodecError::InvalidLength)
}

fn inflate(input: &[u8], declared: usize) -> Result<Vec<u8>, CodecError> {
    // One extra byte detects under-declared output and allows checksum completion
    // when the packet fills the declared output buffer exactly.
    let mut output = vec![0; declared + 1];
    let mut zlib = Decompress::new(true);
    let status = zlib
        .decompress(input, &mut output, FlushDecompress::Finish)
        .map_err(|_| CodecError::InvalidCompression)?;
    if status != Status::StreamEnd
        || zlib.total_out() != declared as u64
        || zlib.total_in() != input.len() as u64
    {
        return Err(CodecError::InvalidCompression);
    }
    output.truncate(declared);
    Ok(output)
}
