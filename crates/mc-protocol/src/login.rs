//! Protocol 776 Login bodies; `docs/JOIN.md` and wiki Packets § Login.
//! [Reference](https://minecraft.wiki/w/Java_Edition_protocol/Packets#Login).

use crate::{
    CodecError, encode_varint,
    framing::RawPacket,
    types::{Reader, encode_string},
};

/// 26.2 sb login 0x00 hello (wiki Packets 776).
pub const START_ID: i32 = 0;
/// 26.2 sb login 0x02 `custom_query_answer` (wiki Packets 776).
pub const PLUGIN_RESPONSE_ID: i32 = 2;
/// 26.2 sb login 0x03 `login_acknowledged` (wiki Packets 776).
pub const ACK_ID: i32 = 3;
/// 26.2 sb login 0x04 `cookie_response` (wiki Packets 776).
pub const COOKIE_RESPONSE_ID: i32 = 4;

/// Server-assigned identity. Properties are validated but not used for assets yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// Player UUID, in network byte order.
    pub uuid: [u8; 16],
    /// Server-returned username.
    pub name: String,
    /// Session UUID introduced in 26.2.
    pub session_id: [u8; 16],
}

/// Login clientbound packets needed for offline joining.
#[derive(Debug, PartialEq, Eq)]
pub enum Packet<'a> {
    /// `login_disconnect`, ID 0x00; JSON text component.
    Disconnect(&'a str),
    /// hello, ID 0x01; requires unsupported encryption/authentication.
    EncryptionRequired,
    /// `login_finished`, ID 0x02; profile and session UUID.
    Success(Profile),
    /// `login_compression`, ID 0x03; applies to subsequent frames.
    Compression(i32),
    /// `custom_query`, ID 0x04; reply unsupported with the message ID.
    PluginQuery(i32),
    /// `cookie_request`, ID 0x05; reply with no stored value.
    Cookie(&'a str),
}

/// Encode the protocol-776 intention for Login (intent 2).
pub fn handshake(host: &str, port: u16) -> Result<Vec<u8>, CodecError> {
    crate::status::intention(host, port, 2)
}

/// Encode Login Start for an offline development player. The server assigns UUID.
pub fn start(name: &str) -> Result<Vec<u8>, CodecError> {
    if name.is_empty()
        || name.len() > 16
        || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(CodecError::InvalidValue("offline username"));
    }
    let mut body = Vec::new();
    encode_string(name, 16, &mut body)?;
    body.extend_from_slice(&[0; 16]);
    Ok(body)
}

/// Decode a login packet; supported packets require exact field consumption.
pub fn decode(packet: &RawPacket) -> Result<Packet<'_>, CodecError> {
    let mut reader = Reader::new(&packet.payload);
    let result = match packet.id {
        0 => Packet::Disconnect(reader.string(32_767)?),
        // Fail closed in the client; no cryptographic payload is interpreted.
        1 => return Ok(Packet::EncryptionRequired),
        2 => {
            let uuid = reader.array()?;
            let name = reader.string(16)?.to_owned();
            for _ in 0..reader.count(16)? {
                reader.string(64)?;
                reader.string(32_767)?;
                if reader.boolean()? {
                    reader.string(1024)?;
                }
            }
            Packet::Success(Profile { uuid, name, session_id: reader.array()? })
        }
        3 => Packet::Compression(reader.varint()?),
        4 => {
            let message_id = reader.varint()?;
            reader.identifier()?;
            if reader.remaining().len() > 1 << 20 {
                return Err(CodecError::PacketTooLarge(reader.remaining().len()));
            }
            reader.take(reader.remaining().len())?;
            Packet::PluginQuery(message_id)
        }
        5 => Packet::Cookie(reader.identifier()?),
        id => return Err(CodecError::InvalidPacketId(id)),
    };
    reader.finish()?;
    Ok(result)
}

/// Encode an unsupported login plugin response (no data follows false).
#[must_use]
pub fn reject_plugin(message_id: i32) -> Vec<u8> {
    let mut body = Vec::new();
    encode_varint(message_id, &mut body);
    body.push(0);
    body
}

/// Encode an absent cookie response, shared by Login and Configuration.
pub fn absent_cookie(key: &str) -> Result<Vec<u8>, CodecError> {
    let mut body = Vec::new();
    encode_string(key, 32_767, &mut body)?;
    body.push(0);
    Ok(body)
}
