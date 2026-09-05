// SPDX-License-Identifier: MIT OR Apache-2.0
//! Pure Minecraft Java protocol 776 serialization. No sockets or game logic.
//!
//! Contract: `docs/FOUNDATION.md` and `docs/PROTOCOL-776.md`.
//! Wire reference: [Java Edition packets](https://minecraft.wiki/w/Java_Edition_protocol/Packets).

mod error;
pub mod framing;
pub mod status;
pub mod types;
mod varint;

pub use error::CodecError;
pub use varint::{decode_varint, decode_varlong, encode_varint, encode_varlong};

/// Supported protocol version for 26.2.
pub const PROTOCOL_VERSION: i32 = 776;
/// Maximum bytes in a signed `VarInt`.
pub const MAX_VARINT_BYTES: usize = 5;
/// Maximum bytes in a signed `VarLong`.
pub const MAX_VARLONG_BYTES: usize = 10;
/// Maximum on-wire packet body size (excluding the three-byte length prefix).
pub const MAX_PACKET_SIZE: usize = (1 << 21) - 1;
/// Maximum inflated packet ID + payload size.
pub const MAX_UNCOMPRESSED_SIZE: usize = 1 << 23;
