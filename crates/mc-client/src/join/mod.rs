//! Offline Login → Configuration → Play Login. See `docs/JOIN.md` (protocol 776).

mod registries;
pub use registries::{Registries, RegistryEntry};

use crate::transport::{Connection, TransportError};
use mc_protocol::{CodecError, configuration, framing::RawPacket, login, play};
use std::time::Duration;
use tokio::time::timeout;

/// Terminal join failure; cancellation and errors close the connection.
#[derive(Debug, thiserror::Error)]
pub enum JoinError {
    /// TCP or framing failed.
    #[error(transparent)]
    Transport(#[from] TransportError),
    /// Login or Play packet fields were malformed.
    #[error("invalid join packet: {0}")]
    Codec(#[from] CodecError),
    /// Configuration fields or NBT were malformed/unsupported.
    #[error("invalid configuration packet: {0}")]
    Configuration(#[from] configuration::Error),
    /// One overall deadline was exceeded.
    #[error("client join timed out")]
    Timeout,
    /// No authentication or encryption is implemented in this offline path.
    #[error("server requires encryption/authentication; this client join is offline-only")]
    AuthenticationRequired,
    /// Server explicitly terminated this phase. Untrusted text is not logged.
    #[error("server disconnected during {0}")]
    Disconnected(&'static str),
    /// Invalid phase sequence or unusable registry context.
    #[error("invalid join state: {0}")]
    InvalidState(&'static str),
    /// Cumulative packet, payload or registry limits were exceeded.
    #[error("client join resource limit exceeded")]
    Limit,
}

/// Live Play-state connection and context. No background gameplay loop is started.
#[derive(Debug)]
pub struct Joined {
    /// Identity returned by the server, not the placeholder Login Start UUID.
    pub profile: login::Profile,
    /// Decoded Play Login context.
    pub world: play::Login,
    /// Complete synchronized registry entries in server numeric order.
    pub registries: Registries,
    /// Structurally validated feature/tag update packets, in arrival order.
    pub metadata: Vec<RawPacket>,
    connection: Connection,
}

impl Joined {
    /// Receive a subsequent Play packet, retaining coalesced input from joining.
    /// Callers own timing/packet interpretation and must drop `self` on errors.
    pub async fn next_packet(&mut self) -> Result<RawPacket, TransportError> {
        self.connection.read().await
    }
}

/// Join an offline server with one deadline covering DNS through Play Login.
/// Dropping the future (or the returned session) closes TCP. No auth fallback.
pub async fn connect(
    host: &str,
    port: u16,
    name: &str,
    deadline: Duration,
) -> Result<Joined, JoinError> {
    let handshake = login::handshake(host, port)?;
    let start = login::start(name)?;
    timeout(deadline, exchange(host, port, &handshake, &start))
        .await
        .map_err(|_| JoinError::Timeout)?
}

#[derive(Default)]
struct Budget {
    packets: usize,
    bytes: usize,
}

impl Budget {
    async fn read(&mut self, connection: &mut Connection) -> Result<RawPacket, JoinError> {
        if self.packets == 16_384 {
            return Err(JoinError::Limit);
        }
        let packet = connection.read().await?;
        self.packets += 1;
        self.bytes += packet.payload.len();
        if self.bytes > 32 << 20 {
            return Err(JoinError::Limit);
        }
        Ok(packet)
    }
}

async fn exchange(
    host: &str,
    port: u16,
    handshake: &[u8],
    start: &[u8],
) -> Result<Joined, JoinError> {
    let mut connection = Connection::connect(host, port).await?;
    let mut budget = Budget::default();
    connection.send(0, handshake).await?;
    connection.send(login::START_ID, start).await?;
    let profile = login_phase(&mut connection, &mut budget).await?;
    connection.send(login::ACK_ID, &[]).await?;
    connection.send(configuration::INFORMATION_ID, &configuration::information()?).await?;
    let (registries, metadata) = configure(&mut connection, &mut budget).await?;
    connection.send(configuration::FINISH_ID, &[]).await?;
    let packet = budget.read(&mut connection).await?;
    let world = play::login(&packet)?;
    if registries
        .get("minecraft:dimension_type")
        .and_then(|entries| entries.get(world.dimension_type))
        .is_none()
    {
        return Err(JoinError::InvalidState("unknown Play dimension-type index"));
    }
    Ok(Joined { profile, world, registries, metadata, connection })
}

async fn login_phase(
    connection: &mut Connection,
    budget: &mut Budget,
) -> Result<login::Profile, JoinError> {
    let mut compression_seen = false;
    loop {
        let packet = budget.read(connection).await?;
        match login::decode(&packet)? {
            login::Packet::Disconnect(_) => return Err(JoinError::Disconnected("login")),
            login::Packet::EncryptionRequired => return Err(JoinError::AuthenticationRequired),
            login::Packet::Success(profile) => return Ok(profile),
            login::Packet::Compression(threshold) => {
                if compression_seen {
                    return Err(JoinError::InvalidState("repeated compression negotiation"));
                }
                compression_seen = true;
                connection.codec.set_compression(threshold);
            }
            login::Packet::PluginQuery(id) => {
                connection.send(login::PLUGIN_RESPONSE_ID, &login::reject_plugin(id)).await?;
            }
            login::Packet::Cookie(key) => {
                connection.send(login::COOKIE_RESPONSE_ID, &login::absent_cookie(key)?).await?;
            }
        }
    }
}

async fn configure(
    connection: &mut Connection,
    budget: &mut Budget,
) -> Result<(Registries, Vec<RawPacket>), JoinError> {
    let mut registries = Registries::default();
    let mut metadata = Vec::new();
    let mut packs_seen = false;
    loop {
        let packet = budget.read(connection).await?;
        match configuration::decode(&packet)? {
            configuration::Packet::Cookie(key) => {
                connection
                    .send(configuration::COOKIE_RESPONSE_ID, &login::absent_cookie(key)?)
                    .await?;
            }
            configuration::Packet::Plugin
            | configuration::Packet::ResetChat
            | configuration::Packet::ServerLinks => {}
            configuration::Packet::Disconnect => {
                return Err(JoinError::Disconnected("configuration"));
            }
            configuration::Packet::Finish => {
                if registries
                    .get("minecraft:dimension_type")
                    .is_none_or(<[RegistryEntry]>::is_empty)
                {
                    return Err(JoinError::InvalidState("missing dimension-type registry"));
                }
                return Ok((registries, metadata));
            }
            configuration::Packet::KeepAlive(id) => {
                connection.send(configuration::KEEP_ALIVE_ID, &id.to_be_bytes()).await?;
            }
            configuration::Packet::Ping(id) => {
                connection.send(configuration::PONG_ID, &id.to_be_bytes()).await?;
            }
            configuration::Packet::Registry { id, entries } => {
                registries.insert(id, entries, &packet.payload)?;
            }
            configuration::Packet::Features | configuration::Packet::Tags => metadata.push(packet),
            configuration::Packet::KnownPacks => {
                if packs_seen || !registries.is_empty() {
                    return Err(JoinError::InvalidState("out-of-order known packs"));
                }
                packs_seen = true;
                connection.send(configuration::KNOWN_PACKS_ID, &[0]).await?;
            }
        }
    }
}
