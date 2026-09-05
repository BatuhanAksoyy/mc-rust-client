//! Protocol 776 Configuration bodies; see `docs/JOIN.md`.
//! [Reference](https://minecraft.wiki/w/Java_Edition_protocol/Packets#Configuration).

use crate::{
    CodecError,
    framing::RawPacket,
    nbt::{self, NbtError, Tag},
    types::{Reader, encode_string},
};

/// 26.2 sb configuration 0x00 `client_information` (wiki Packets 776).
pub const INFORMATION_ID: i32 = 0;
/// 26.2 sb configuration 0x01 `cookie_response` (wiki Packets 776).
pub const COOKIE_RESPONSE_ID: i32 = 1;
/// 26.2 sb/cb configuration 0x03 `finish_configuration` (wiki Packets 776).
pub const FINISH_ID: i32 = 3;
/// 26.2 sb/cb configuration 0x04 `keep_alive` (wiki Packets 776).
pub const KEEP_ALIVE_ID: i32 = 4;
/// 26.2 sb configuration 0x05 pong (wiki Packets 776).
pub const PONG_ID: i32 = 5;
/// 26.2 sb configuration 0x07 `select_known_packs` (wiki Packets 776).
pub const KNOWN_PACKS_ID: i32 = 7;
/// Client policy limit for packet collections; not a wire constant.
pub const MAX_ENTRIES: usize = 65_536;

/// Configuration field or embedded NBT validation failure.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    /// Invalid packet field.
    #[error(transparent)]
    Codec(#[from] CodecError),
    /// Invalid or over-budget NBT.
    #[error(transparent)]
    Nbt(#[from] NbtError),
}

/// Registry entry; its index in the packet is its numeric protocol ID.
#[derive(Debug)]
pub struct Entry<'a> {
    /// Textual entry identifier.
    pub id: &'a str,
    /// Validated compound NBT bytes, or absent known-pack data.
    pub data: Option<&'a [u8]>,
}

/// Clientbound packets implemented for the initial headless configuration.
#[derive(Debug)]
pub enum Packet<'a> {
    /// `cookie_request` (0x00).
    Cookie(&'a str),
    /// `custom_payload` (0x01), structurally validated and intentionally ignored.
    Plugin,
    /// disconnect (0x02), validated NBT text component.
    Disconnect,
    /// `finish_configuration` (0x03).
    Finish,
    /// `keep_alive` (0x04).
    KeepAlive(i64),
    /// ping (0x05).
    Ping(i32),
    /// `reset_chat` (0x06); no chat state exists yet.
    ResetChat,
    /// `registry_data` (0x07).
    Registry {
        /// Registry's identifier.
        id: &'a str,
        /// Entries in numeric-ID order.
        entries: Vec<Entry<'a>>,
    },
    /// `update_enabled_features` (0x0c), structurally validated.
    Features,
    /// `update_tags` (0x0d), structurally validated.
    Tags,
    /// `select_known_packs` (0x0e), validated; client responds with zero packs.
    KnownPacks,
    /// `server_links` (0x10), structurally validated and intentionally ignored:
    /// this client never follows a server-supplied link.
    ServerLinks,
}

/// Fixed development settings, not a user-facing preferences API yet.
pub fn information() -> Result<Vec<u8>, CodecError> {
    let mut body = Vec::new();
    encode_string("en_US", 16, &mut body)?;
    body.extend_from_slice(&[4, 2, 1, 0x7f, 1, 0, 0, 0]);
    Ok(body)
}

/// Decode supported Configuration packets, rejecting unknown IDs and extra data.
pub fn decode(packet: &RawPacket) -> Result<Packet<'_>, Error> {
    let mut reader = Reader::new(&packet.payload);
    let value = match packet.id {
        0 => Packet::Cookie(reader.identifier()?),
        1 => {
            reader.identifier()?;
            if reader.remaining().len() > 1 << 20 {
                return Err(CodecError::PacketTooLarge(reader.remaining().len()).into());
            }
            reader.take(reader.remaining().len())?;
            Packet::Plugin
        }
        2 => {
            let (_, length) = nbt::decode_network(reader.remaining(), nbt::Limits::default())?;
            reader.take(length)?;
            Packet::Disconnect
        }
        3 => Packet::Finish,
        4 => Packet::KeepAlive(reader.i64()?),
        5 => Packet::Ping(reader.i32()?),
        6 => Packet::ResetChat,
        7 => {
            let id = reader.identifier()?;
            let count = reader.count(MAX_ENTRIES)?;
            let mut entries = Vec::new();
            for _ in 0..count {
                let id = reader.identifier()?;
                let data = if reader.boolean()? {
                    let (tag, length) =
                        nbt::decode_network(reader.remaining(), nbt::Limits::default())?;
                    if !matches!(tag, Tag::Compound(_)) {
                        return Err(CodecError::InvalidValue("registry NBT root").into());
                    }
                    Some(reader.take(length)?)
                } else {
                    None
                };
                entries.push(Entry { id, data });
            }
            Packet::Registry { id, entries }
        }
        12 => {
            for _ in 0..reader.count(MAX_ENTRIES)? {
                reader.identifier()?;
            }
            Packet::Features
        }
        13 => {
            tags(&mut reader)?;
            Packet::Tags
        }
        14 => {
            for _ in 0..reader.count(64)? {
                reader.string(32_767)?;
                reader.string(32_767)?;
                reader.string(32_767)?;
            }
            Packet::KnownPacks
        }
        16 => {
            for _ in 0..reader.count(64)? {
                if reader.boolean()? {
                    reader.varint()?; // Built-in label enum; the meaning is never acted on.
                } else {
                    let (_, length) =
                        nbt::decode_network(reader.remaining(), nbt::Limits::default())?;
                    reader.take(length)?;
                }
                reader.string(32_767)?; // URL; never dereferenced by this client.
            }
            Packet::ServerLinks
        }
        id => return Err(CodecError::InvalidPacketId(id).into()),
    };
    reader.finish()?;
    Ok(value)
}

fn tags(reader: &mut Reader<'_>) -> Result<(), CodecError> {
    let mut budget = MAX_ENTRIES;
    let registries = reader.count(budget)?;
    budget -= registries;
    for _ in 0..registries {
        reader.identifier()?;
        let count = reader.count(budget)?;
        budget -= count;
        for _ in 0..count {
            reader.identifier()?;
            let count = reader.count(budget)?;
            budget -= count;
            for _ in 0..count {
                reader.count(i32::MAX as usize)?;
            }
        }
    }
    Ok(())
}
