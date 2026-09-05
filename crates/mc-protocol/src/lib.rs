// SPDX-License-Identifier: MIT OR Apache-2.0
//! Minecraft Java protocol 776 codecs.
//!
//! Spec: `docs/PROTOCOL-776.md`. Clean-room: no Mojang code.

use thiserror::Error;

/// Supported protocol version for 26.2.
pub const PROTOCOL_VERSION: i32 = 776;

/// Max bytes for a VarInt length/ID field.
pub const MAX_VARINT_BYTES: usize = 5;
/// Max packet size (2^21 - 1).
pub const MAX_PACKET_SIZE: usize = 2_097_151;

#[derive(Debug, Error)]
pub enum CodecError {
    #[error("varint too long (>{MAX_VARINT_BYTES} bytes)")]
    VarIntTooLong,
    #[error("unexpected end of input")]
    UnexpectedEof,
    #[error("packet too large: {0} bytes")]
    PacketTooLarge(usize),
}

/// Encode non-negative `i32` as unsigned VarInt (protocol IDs/lengths).
pub fn encode_varint(value: i32, out: &mut Vec<u8>) {
    debug_assert!(value >= 0, "protocol VarInt values are non-negative");
    let mut v = value as u32;
    loop {
        let mut temp = (v & 0x7F) as u8;
        v >>= 7;
        if v != 0 {
            temp |= 0x80;
        }
        out.push(temp);
        if v == 0 {
            break;
        }
    }
}

/// Decode unsigned VarInt, returning `(value, bytes_read)`.
pub fn decode_varint(input: &[u8]) -> Result<(i32, usize), CodecError> {
    let mut num: u32 = 0;
    for i in 0..MAX_VARINT_BYTES {
        let byte = *input.get(i).ok_or(CodecError::UnexpectedEof)?;
        num |= ((byte & 0x7F) as u32) << (7 * i);
        if byte & 0x80 == 0 {
            return Ok((num as i32, i + 1));
        }
    }
    Err(CodecError::VarIntTooLong)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_roundtrip_boundaries() {
        for v in [0, 1, 127, 128, 255, 2097151, 2147483647i32] {
            let mut buf = Vec::new();
            encode_varint(v, &mut buf);
            assert!(buf.len() <= 3 || v > 2097151, "length field must be <= 3 bytes for packets");
            let (d, n) = decode_varint(&buf).unwrap();
            assert_eq!((v, buf.len()), (d, n));
        }
    }

    #[test]
    fn varint_rejects_overlong() {
        let bad = [0x80u8; 6];
        assert!(matches!(
            decode_varint(&bad),
            Err(CodecError::VarIntTooLong) | Err(CodecError::UnexpectedEof)
        ));
    }
}
