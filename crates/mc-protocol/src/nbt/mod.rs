//! Bounded Java NBT decoding for protocol 776; see `docs/NBT.md`.
//! [Wire reference](https://wikivg.booky.dev/NBT).

mod decode;
mod string;

pub use decode::{decode_named, decode_network};
pub use string::NbtString;

/// Safety ceiling for the recursive decoder, independent of caller limits.
pub const MAX_DEPTH: usize = 64;

/// Per-root resource limits. These are client policy, not protocol constants.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Maximum encoded bytes consumed, excluding subsequent packet fields.
    pub max_bytes: usize,
    /// Maximum tags, UTF-16 units and numeric-array elements combined.
    pub max_elements: usize,
    /// Maximum child depth (root is zero); cannot exceed [`MAX_DEPTH`].
    pub max_depth: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { max_bytes: 1 << 21, max_elements: 65_536, max_depth: MAX_DEPTH }
    }
}

/// A malformed value or exhausted decoding budget.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum NbtError {
    /// An underlying field was incomplete or had an invalid length.
    #[error(transparent)]
    Codec(#[from] crate::CodecError),
    /// Only IDs 0 through 12 are defined.
    #[error("unknown NBT tag ID {0}")]
    InvalidTag(u8),
    /// End has no payload and cannot be a nonempty list's element type.
    #[error("nonempty NBT list has End element type")]
    EndList,
    /// The encoded value exceeds the byte budget.
    #[error("NBT byte limit exceeded")]
    ByteLimit,
    /// The decoded value exceeds the cumulative allocation-unit budget.
    #[error("NBT element limit exceeded")]
    ElementLimit,
    /// Nesting exceeds caller policy or the hard recursion ceiling.
    #[error("NBT depth limit exceeded")]
    DepthLimit,
    /// A string contains an invalid modified UTF-8 sequence.
    #[error("invalid NBT modified UTF-8")]
    InvalidString,
}

/// A named tag, also used for compound entries. Order and duplicates are retained.
#[derive(Debug, Clone, PartialEq)]
pub struct NamedTag<'a> {
    /// Name as Java UTF-16 units (not necessarily valid Unicode scalars).
    pub name: NbtString,
    /// Decoded payload.
    pub tag: Tag<'a>,
}

/// An NBT value. Byte arrays borrow their original wire representation.
#[derive(Debug, Clone, PartialEq)]
pub enum Tag<'a> {
    /// Absent root value; compound terminators are not stored.
    End,
    /// Signed 8-bit integer.
    Byte(i8),
    /// Signed 16-bit integer.
    Short(i16),
    /// Signed 32-bit integer.
    Int(i32),
    /// Signed 64-bit integer.
    Long(i64),
    /// IEEE 754 binary32, including infinities and NaNs.
    Float(f32),
    /// IEEE 754 binary64, including infinities and NaNs.
    Double(f64),
    /// Raw two's-complement byte values, without copying.
    ByteArray(&'a [u8]),
    /// Java modified UTF-8 decoded to UTF-16 units.
    String(NbtString),
    /// Homogeneous unnamed values, retaining the element ID for empty lists.
    List {
        /// Wire type ID, validated to be in 0..=12.
        element_id: u8,
        /// Elements in wire order.
        elements: Vec<Self>,
    },
    /// Named entries in wire order.
    Compound(Vec<NamedTag<'a>>),
    /// Signed 32-bit integers.
    IntArray(Vec<i32>),
    /// Signed 64-bit integers.
    LongArray(Vec<i64>),
}
