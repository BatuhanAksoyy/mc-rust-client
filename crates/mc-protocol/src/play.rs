//! Play Login and pre-spawn packets for protocol 776; not a gameplay packet
//! loop yet. [Reference](https://minecraft.wiki/w/Java_Edition_protocol/Packets#Play).

use crate::{
    CodecError,
    framing::RawPacket,
    nbt::{self, NbtError},
    types::Reader,
};

/// 26.2 cb play 0x31 login (wiki Packets 776; cache CSV).
pub const LOGIN_ID: i32 = 49;
/// 26.2 cb play 0x20 disconnect (wiki Packets 776).
pub const DISCONNECT_ID: i32 = 32;
/// 26.2 cb play 0x26 `game_event` (wiki Packets 776).
pub const GAME_EVENT_ID: i32 = 38;
/// 26.2 cb play 0x2c `keep_alive` (wiki Packets 776).
pub const KEEP_ALIVE_ID: i32 = 44;
/// 26.2 cb play 0x48 `player_position`, Synchronize Player Position (wiki Packets 776).
pub const SYNCHRONIZE_POSITION_ID: i32 = 72;
/// 26.2 cb play 0x5e `set_chunk_cache_center` (wiki Packets 776).
pub const SET_CENTER_CHUNK_ID: i32 = 94;
/// 26.2 sb play 0x00 `accept_teleportation` (wiki Packets 776).
pub const ACCEPT_TELEPORTATION_ID: i32 = 0;
/// 26.2 sb play 0x1c `keep_alive` (wiki Packets 776).
pub const SERVERBOUND_KEEP_ALIVE_ID: i32 = 28;
/// 26.2 sb play 0x2c `player_loaded` (wiki Packets 776).
pub const PLAYER_LOADED_ID: i32 = 44;

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

/// Synchronize Player Position fields needed to confirm the initial spawn.
///
/// Velocity is not retained: it has no meaning before any client-side physics
/// exist, and vanilla always sends absolute values for this first teleport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Teleport {
    /// Echoed back in Confirm Teleportation.
    pub id: i32,
    /// Absolute or relative per `flags`; the join path requires absolute.
    pub x: f64,
    /// Absolute or relative per `flags`; the join path requires absolute.
    pub y: f64,
    /// Absolute or relative per `flags`; the join path requires absolute.
    pub z: f64,
    /// Absolute or relative per `flags`; the join path requires absolute.
    pub yaw: f32,
    /// Absolute or relative per `flags`; the join path requires absolute.
    pub pitch: f32,
    /// Teleport Flags bitmask (wiki Packets § Teleport Flags); zero means
    /// every field above is absolute. Relative deltas need a prior position
    /// this client does not have yet, so callers reject a nonzero value here.
    pub flags: i32,
}

/// Play field or embedded NBT validation failure.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    /// Invalid packet field.
    #[error(transparent)]
    Codec(#[from] CodecError),
    /// Invalid or over-budget NBT.
    #[error(transparent)]
    Nbt(#[from] NbtError),
}

/// Play packets this client recognizes before chunk streaming begins.
///
/// Every other Play packet ID (chunk/light data, entity spawns, recipes, ...)
/// is deliberately left undecoded: unlike Login/Configuration, Play has far
/// more packet variety than this pre-spawn step needs to understand, and
/// receiving world/entity data is separate, later work.
#[derive(Debug)]
pub enum Packet {
    /// disconnect (0x20); NBT text component reason, not logged.
    Disconnect,
    /// `game_event` (0x26); structurally validated, otherwise unused here.
    GameEvent {
        /// See wiki Packets § Game Event for the event/value meaning.
        event: u8,
        /// Meaning depends on `event`.
        value: f32,
    },
    /// `keep_alive` (0x2c); echoed back unchanged.
    KeepAlive(i64),
    /// `set_chunk_cache_center` (0x5e); the client's chunk-loading area moved.
    SetCenterChunk {
        /// Chunk X coordinate of the loading area center.
        x: i32,
        /// Chunk Z coordinate of the loading area center.
        z: i32,
    },
    /// `player_position` (0x48); confirm with `ACCEPT_TELEPORTATION_ID`.
    SynchronizePosition(Teleport),
}

/// Decode one recognized pre-spawn Play packet, or `None` for any other
/// packet ID (see the `Packet` policy note above: the caller discards it
/// unread rather than treating an unknown ID as an error).
pub fn decode(packet: &RawPacket) -> Result<Option<Packet>, Error> {
    let mut reader = Reader::new(&packet.payload);
    let value = match packet.id {
        DISCONNECT_ID => {
            let (_, length) = nbt::decode_network(reader.remaining(), nbt::Limits::default())?;
            reader.take(length)?;
            Packet::Disconnect
        }
        GAME_EVENT_ID => Packet::GameEvent { event: reader.u8()?, value: reader.f32()? },
        KEEP_ALIVE_ID => Packet::KeepAlive(reader.i64()?),
        SYNCHRONIZE_POSITION_ID => {
            let id = reader.varint()?;
            let x = reader.f64()?;
            let y = reader.f64()?;
            let z = reader.f64()?;
            reader.f64()?; // Velocity X; meaningless before this client has physics.
            reader.f64()?; // Velocity Y.
            reader.f64()?; // Velocity Z.
            let yaw = reader.f32()?;
            let pitch = reader.f32()?;
            let flags = reader.i32()?;
            Packet::SynchronizePosition(Teleport { id, x, y, z, yaw, pitch, flags })
        }
        SET_CENTER_CHUNK_ID => Packet::SetCenterChunk { x: reader.varint()?, z: reader.varint()? },
        _ => return Ok(None),
    };
    reader.finish()?;
    Ok(Some(value))
}
