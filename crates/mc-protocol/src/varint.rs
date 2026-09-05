//! Two's-complement variable integers, protocol 776, wiki Packets § Data types.
//! [Wire reference](https://minecraft.wiki/w/Java_Edition_protocol/Packets#Data_types).
// Casts deliberately preserve wire bits; masks and width checks bound truncation.
#![allow(clippy::cast_sign_loss, clippy::cast_possible_wrap, clippy::cast_possible_truncation)]

use crate::{CodecError, MAX_VARINT_BYTES, MAX_VARLONG_BYTES};

/// Append a signed `VarInt` (negative values take five bytes).
pub fn encode_varint(value: i32, out: &mut Vec<u8>) {
    encode(u64::from(value as u32), out);
}

/// Append a signed `VarLong` (negative values take ten bytes).
pub fn encode_varlong(value: i64, out: &mut Vec<u8>) {
    encode(value as u64, out);
}

fn encode(mut value: u64, out: &mut Vec<u8>) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        out.push(if value == 0 { byte } else { byte | 0x80 });
        if value == 0 {
            return;
        }
    }
}

/// Decode a signed `VarInt`, returning the value and number of bytes consumed.
pub fn decode_varint(input: &[u8]) -> Result<(i32, usize), CodecError> {
    let (value, length) = decode(input, MAX_VARINT_BYTES, 0x0f)?;
    Ok((value as i32, length))
}

/// Decode a signed `VarLong`, returning the value and number of bytes consumed.
pub fn decode_varlong(input: &[u8]) -> Result<(i64, usize), CodecError> {
    let (value, length) = decode(input, MAX_VARLONG_BYTES, 0x01)?;
    Ok((value as i64, length))
}

fn decode(input: &[u8], max: usize, last_mask: u8) -> Result<(u64, usize), CodecError> {
    let mut value = 0;
    for i in 0..max {
        let byte = *input.get(i).ok_or(CodecError::UnexpectedEof)?;
        if i == max - 1 {
            if byte & 0x80 != 0 {
                return Err(if max == MAX_VARINT_BYTES {
                    CodecError::VarIntTooLong
                } else {
                    CodecError::VarLongTooLong
                });
            }
            if byte & !last_mask != 0 {
                return Err(CodecError::IntegerOverflow);
            }
        }
        value |= u64::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return Ok((value, i + 1));
        }
    }
    unreachable!("the last byte either terminates or returns an error")
}
