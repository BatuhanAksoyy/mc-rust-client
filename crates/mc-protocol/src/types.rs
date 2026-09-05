//! Bounded protocol 776 fields; see wiki Packets § Data types and `FOUNDATION.md`.
//! [Wire reference](https://minecraft.wiki/w/Java_Edition_protocol/Packets#Data_types).

use crate::{CodecError, decode_varint, decode_varlong, encode_varint};

/// Borrowing packet cursor. Discard it on error; successful reads advance it.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    remaining: &'a [u8],
}

macro_rules! scalar {
    ($name:ident, $ty:ty, $doc:literal) => {
        #[doc = $doc]
        pub fn $name(&mut self) -> Result<$ty, CodecError> {
            Ok(<$ty>::from_be_bytes(self.array()?))
        }
    };
}

impl<'a> Reader<'a> {
    /// Borrow a packet body without allocating.
    #[must_use]
    pub const fn new(input: &'a [u8]) -> Self {
        Self { remaining: input }
    }

    /// Unconsumed bytes.
    #[must_use]
    pub const fn remaining(&self) -> &'a [u8] {
        self.remaining
    }

    /// Require exact packet consumption.
    pub const fn finish(self) -> Result<(), CodecError> {
        if self.remaining.is_empty() { Ok(()) } else { Err(CodecError::TrailingData) }
    }

    /// Borrow exactly `length` bytes, checking bounds before advancing.
    pub fn take(&mut self, length: usize) -> Result<&'a [u8], CodecError> {
        let (value, rest) =
            self.remaining.split_at_checked(length).ok_or(CodecError::UnexpectedEof)?;
        self.remaining = rest;
        Ok(value)
    }

    /// Read fixed-size bytes (also the representation of a network UUID).
    pub fn array<const N: usize>(&mut self) -> Result<[u8; N], CodecError> {
        self.take(N)?.try_into().map_err(|_| CodecError::UnexpectedEof)
    }

    scalar!(u8, u8, "Read an unsigned byte.");
    scalar!(i8, i8, "Read a signed byte.");
    scalar!(i16, i16, "Read a big-endian signed short.");
    scalar!(u16, u16, "Read a big-endian unsigned short.");
    scalar!(i32, i32, "Read a big-endian signed integer.");
    scalar!(i64, i64, "Read a big-endian signed long.");
    scalar!(u64, u64, "Read big-endian packed bits.");
    scalar!(f32, f32, "Read an IEEE 754 big-endian float.");
    scalar!(f64, f64, "Read an IEEE 754 big-endian double.");

    /// Read a Boolean encoded as exactly zero or one.
    pub fn boolean(&mut self) -> Result<bool, CodecError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(CodecError::InvalidBoolean(other)),
        }
    }

    /// Read a signed `VarInt`.
    pub fn varint(&mut self) -> Result<i32, CodecError> {
        let (value, length) = decode_varint(self.remaining)?;
        self.take(length)?;
        Ok(value)
    }

    /// Read a signed `VarLong`.
    pub fn varlong(&mut self) -> Result<i64, CodecError> {
        let (value, length) = decode_varlong(self.remaining)?;
        self.take(length)?;
        Ok(value)
    }

    /// Read a UTF-8 string limited by UTF-16 code units, without allocating.
    pub fn string(&mut self, max_units: usize) -> Result<&'a str, CodecError> {
        let length = usize::try_from(self.varint()?).map_err(|_| CodecError::InvalidLength)?;
        if length > max_units.saturating_mul(3) {
            return Err(CodecError::StringTooLong);
        }
        let value = std::str::from_utf8(self.take(length)?).map_err(|_| CodecError::InvalidUtf8)?;
        if value.encode_utf16().count() > max_units {
            return Err(CodecError::StringTooLong);
        }
        Ok(value)
    }
}

/// Append a UTF-8 string after validating its UTF-16 length. Errors do not write.
pub fn encode_string(value: &str, max_units: usize, out: &mut Vec<u8>) -> Result<(), CodecError> {
    let length = i32::try_from(value.len()).map_err(|_| CodecError::StringTooLong)?;
    if value.len() > max_units.saturating_mul(3) || value.encode_utf16().count() > max_units {
        return Err(CodecError::StringTooLong);
    }
    encode_varint(length, out);
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

/// Block position packed as x:26, z:26, y:12 signed bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// East/west coordinate, in [-33554432, 33554431].
    pub x: i32,
    /// Height, in [-2048, 2047].
    pub y: i32,
    /// North/south coordinate, in [-33554432, 33554431].
    pub z: i32,
}

impl Position {
    /// Pack coordinates, rejecting out-of-range values rather than wrapping.
    #[allow(clippy::cast_sign_loss)] // Mask the signed two's-complement bits.
    pub fn packed(self) -> Result<u64, CodecError> {
        if !(-(1 << 25)..(1 << 25)).contains(&self.x)
            || !(-(1 << 25)..(1 << 25)).contains(&self.z)
            || !(-2048..2048).contains(&self.y)
        {
            return Err(CodecError::PositionOutOfRange);
        }
        Ok(((self.x as u64 & 0x03ff_ffff) << 38)
            | ((self.z as u64 & 0x03ff_ffff) << 12)
            | (self.y as u64 & 0xfff))
    }

    /// Unpack coordinates using sign extension.
    #[must_use]
    #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    pub const fn from_packed(value: u64) -> Self {
        Self {
            x: (value as i64 >> 38) as i32,
            y: ((value << 52) as i64 >> 52) as i32,
            z: ((value << 26) as i64 >> 38) as i32,
        }
    }
}
