use thiserror::Error;

/// Invalid or incomplete protocol input. Packet errors terminate the connection.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CodecError {
    /// A `VarInt` exceeded five bytes.
    #[error("VarInt exceeds five bytes")]
    VarIntTooLong,
    /// A `VarLong` exceeded ten bytes.
    #[error("VarLong exceeds ten bytes")]
    VarLongTooLong,
    /// Unused high bits were set in the last byte.
    #[error("variable integer exceeds its bit width")]
    IntegerOverflow,
    /// More bytes are needed to decode the value.
    #[error("unexpected end of input")]
    UnexpectedEof,
    /// A packet exceeds a wire or decompressed size limit.
    #[error("packet too large: {0} bytes")]
    PacketTooLarge(usize),
    /// A size was negative, zero where forbidden, or had an oversized prefix.
    #[error("invalid length")]
    InvalidLength,
    /// A string exceeded the specified UTF-16 or byte length limit.
    #[error("string exceeds its length limit")]
    StringTooLong,
    /// A string was not UTF-8.
    #[error("invalid UTF-8")]
    InvalidUtf8,
    /// Boolean fields may only contain zero or one.
    #[error("invalid Boolean byte: {0}")]
    InvalidBoolean(u8),
    /// A block coordinate does not fit the packed representation.
    #[error("position out of range")]
    PositionOutOfRange,
    /// Packet IDs must be nonnegative and correct for the current state.
    #[error("unexpected packet ID: {0}")]
    InvalidPacketId(i32),
    /// A packet had extra fields after its defined payload.
    #[error("trailing packet bytes")]
    TrailingData,
    /// A compressed/uncompressed packet violated the negotiated threshold.
    #[error("compression threshold violation")]
    CompressionThreshold,
    /// Invalid, incomplete, concatenated, or incorrectly sized zlib data.
    #[error("invalid compressed packet")]
    InvalidCompression,
}
