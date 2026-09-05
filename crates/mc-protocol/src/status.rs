//! Handshake/Status for 26.2 (776), wiki Java Edition protocol/Packets.
//! [Wire reference](https://minecraft.wiki/w/Java_Edition_protocol/Packets#Status).
//!
//! IDs checked against the cache's 26.2/packets-776.csv. JSON interpretation and
//! network timing belong to mc-client. These functions operate on packet bodies.

use crate::types::{Reader, encode_string};
use crate::{CodecError, PROTOCOL_VERSION, encode_varint, framing::RawPacket};

/// 26.2 sb handshaking 0x00 intention (wiki Packets 776).
pub const HANDSHAKE_ID: i32 = 0;
/// 26.2 sb status 0x00 `status_request` (wiki Packets 776).
pub const REQUEST_ID: i32 = 0;
/// 26.2 cb status 0x00 `status_response` (wiki Packets 776).
pub const RESPONSE_ID: i32 = 0;
/// 26.2 sb status 0x01 `ping_request` (wiki Packets 776).
pub const PING_ID: i32 = 1;
/// 26.2 cb status 0x01 `pong_response` (wiki Packets 776).
pub const PONG_ID: i32 = 1;
/// Maximum UTF-16 code units in the handshake address.
pub const MAX_HOST_UNITS: usize = 255;
/// Maximum UTF-16 code units in status JSON.
pub const MAX_JSON_UNITS: usize = 32_767;

/// Encode an intention packet for the status state (intent = 1).
pub fn handshake(host: &str, port: u16) -> Result<Vec<u8>, CodecError> {
    intention(host, port, 1)
}

pub(crate) fn intention(host: &str, port: u16, intent: i32) -> Result<Vec<u8>, CodecError> {
    let mut body = Vec::new();
    encode_varint(PROTOCOL_VERSION, &mut body);
    encode_string(host, MAX_HOST_UNITS, &mut body)?;
    body.extend_from_slice(&port.to_be_bytes());
    encode_varint(intent, &mut body);
    Ok(body)
}

/// Decode a status response's JSON string, requiring exact packet consumption.
pub fn response(packet: &RawPacket) -> Result<&str, CodecError> {
    require_id(packet, RESPONSE_ID)?;
    let mut reader = Reader::new(&packet.payload);
    let json = reader.string(MAX_JSON_UNITS)?;
    reader.finish()?;
    Ok(json)
}

/// Encode a ping payload; the server must echo all eight bytes.
#[must_use]
pub const fn ping(payload: i64) -> [u8; 8] {
    payload.to_be_bytes()
}

/// Decode a pong, requiring the correct packet ID and exact payload width.
pub fn pong(packet: &RawPacket) -> Result<i64, CodecError> {
    require_id(packet, PONG_ID)?;
    let mut reader = Reader::new(&packet.payload);
    let value = reader.i64()?;
    reader.finish()?;
    Ok(value)
}

const fn require_id(packet: &RawPacket, expected: i32) -> Result<(), CodecError> {
    if packet.id == expected { Ok(()) } else { Err(CodecError::InvalidPacketId(packet.id)) }
}
