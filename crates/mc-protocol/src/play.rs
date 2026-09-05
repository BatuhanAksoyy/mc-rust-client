//! Initial Play Login decoder for protocol 776; not a gameplay packet loop yet.
//! [Reference](https://minecraft.wiki/w/Java_Edition_protocol/Packets#Login_(play)).

use crate::{CodecError, framing::RawPacket, types::Reader};

/// 26.2 cb play 0x31 login (wiki Packets 776; cache CSV).
pub const LOGIN_ID: i32 = 49;

/// Spawn context needed to select the first world's registry entry.
#[derive(Debug, PartialEq, Eq)]
pub struct Login {
    /// Server-assigned player entity ID.
    pub entity_id: i32,
    /// Numeric ID in the synchronized dimension-type registry.
    pub dimension_type: usize,
    /// Identifier of the active dimension.
    pub dimension_name: String,
    /// Current game mode: survival, creative, adventure or spectator (0..=3).
    pub game_mode: u8,
}

/// Validate all Play Login fields and retain the initial spawn context.
pub fn login(packet: &RawPacket) -> Result<Login, CodecError> {
    if packet.id != LOGIN_ID {
        return Err(CodecError::InvalidPacketId(packet.id));
    }
    let mut reader = Reader::new(&packet.payload);
    let entity_id = reader.i32()?;
    reader.boolean()?;
    for _ in 0..reader.count(1024)? {
        reader.identifier()?;
    }
    reader.count(i32::MAX as usize)?;
    reader.count(i32::MAX as usize)?;
    reader.count(i32::MAX as usize)?;
    reader.boolean()?;
    reader.boolean()?;
    reader.boolean()?;
    let dimension_type = reader.count(i32::MAX as usize)?;
    let dimension_name = reader.identifier()?.to_owned();
    reader.i64()?;
    let game_mode = reader.u8()?;
    let previous = reader.i8()?;
    if game_mode > 3 || !(-1..=3).contains(&previous) {
        return Err(CodecError::InvalidValue("game mode"));
    }
    reader.boolean()?;
    reader.boolean()?;
    if reader.boolean()? {
        reader.identifier()?;
        reader.u64()?;
    }
    reader.count(i32::MAX as usize)?;
    reader.varint()?;
    reader.boolean()?; // Online mode, added in 26.2.
    reader.boolean()?; // Enforces secure chat.
    reader.finish()?;
    Ok(Login { entity_id, dimension_type, dimension_name, game_mode })
}
