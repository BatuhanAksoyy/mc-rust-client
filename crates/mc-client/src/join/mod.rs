//! Offline Login → Configuration → Play → spawn confirmation. See `docs/JOIN.md`
//! (protocol 776).

mod registries;
pub use registries::{Registries, RegistryEntry};

use crate::transport::{Connection, TransportError};
use mc_protocol::{
    CodecError, chunk, configuration, encode_varint, framing::RawPacket, login, play,
};
use std::time::{Duration, Instant};
use tokio::time::timeout;

/// Client policy bound on chunks decoded before spawn; a real batch is
/// bounded by view distance, not this. Guards against many tiny chunks
/// amplifying a bounded byte budget into a large `Vec<LevelChunk>`.
const MAX_CHUNKS_PER_JOIN: usize = 512;

/// Terminal join failure; cancellation and errors close the connection.
#[derive(Debug, thiserror::Error)]
pub enum JoinError {
    /// TCP or framing failed.
    #[error(transparent)]
    Transport(#[from] TransportError),
    /// Login or Play Login packet fields were malformed.
    #[error("invalid join packet: {0}")]
    Codec(#[from] CodecError),
    /// Configuration fields or NBT were malformed/unsupported.
    #[error("invalid configuration packet: {0}")]
    Configuration(#[from] configuration::Error),
    /// Pre-spawn Play packet fields or NBT were malformed.
    #[error("invalid play packet: {0}")]
    Play(#[from] play::Error),
    /// A chunk or block-entity field or NBT was malformed.
    #[error("invalid chunk packet: {0}")]
    Chunk(#[from] chunk::Error),
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
    /// Chunk-loading area center, if the server sent one before spawn.
    /// Absent means the default area centered on the world origin applies.
    pub center_chunk: Option<(i32, i32)>,
    /// Confirmed initial spawn position and rotation.
    pub spawn: play::Teleport,
    /// Chunks received before spawn was confirmed, in arrival order. Pumpkin
    /// sends the initial view-area batch before the spawn teleport; further
    /// chunks (from movement or a changed loading area) are later work.
    pub chunks: Vec<chunk::LevelChunk>,
    connection: Connection,
    batch_started: Option<Instant>,
}

impl Joined {
    /// Receive a subsequent Play packet, retaining coalesced input from joining.
    /// Callers own timing/packet interpretation and must drop `self` on errors.
    pub async fn next_packet(&mut self) -> Result<RawPacket, TransportError> {
        self.connection.read().await
    }

    /// Drain post-spawn chunk batches until the requested square view area is
    /// loaded or `deadline` elapses. A deadline returns the chunks received so
    /// far; malformed packets and disconnects remain errors.
    pub async fn load_initial_chunks(
        &mut self,
        render_distance: u8,
        deadline: Duration,
    ) -> Result<(), JoinError> {
        if !(2..=8).contains(&render_distance) {
            return Err(JoinError::InvalidState("render distance outside 2..=8"));
        }
        let diameter = usize::from(render_distance) * 2 + 1;
        let target = diameter * diameter;
        let started = Instant::now();
        loop {
            if self.chunks.len() >= target && self.batch_started.is_none() {
                return Ok(());
            }
            let remaining = deadline.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Ok(());
            }
            let packet = match timeout(remaining, self.connection.read()).await {
                Ok(result) => result?,
                Err(_) => return Ok(()),
            };
            match packet.id {
                chunk::BATCH_START_ID => self.batch_started = Some(Instant::now()),
                chunk::LEVEL_CHUNK_WITH_LIGHT_ID => {
                    let decoded = chunk::decode(&packet)?;
                    if let Some(existing) = self
                        .chunks
                        .iter_mut()
                        .find(|loaded| loaded.x == decoded.x && loaded.z == decoded.z)
                    {
                        *existing = decoded;
                    } else if self.chunks.len() == MAX_CHUNKS_PER_JOIN {
                        return Err(JoinError::Limit);
                    } else {
                        self.chunks.push(decoded);
                    }
                }
                chunk::BATCH_FINISHED_ID => {
                    let Some(batch_started) = self.batch_started.take() else {
                        return Err(JoinError::InvalidState(
                            "chunk batch finished without a start",
                        ));
                    };
                    let batch_size = chunk::decode_batch_finished(&packet)?;
                    let reply =
                        chunk::batch_received(desired_chunks_per_tick(batch_started, batch_size));
                    self.connection.send(chunk::BATCH_RECEIVED_ID, &reply).await?;
                    if self.chunks.len() >= target {
                        return Ok(());
                    }
                }
                _ => self.handle_post_spawn(packet).await?,
            }
        }
    }

    async fn handle_post_spawn(&mut self, packet: RawPacket) -> Result<(), JoinError> {
        match play::decode(&packet)? {
            Some(play::Packet::Disconnect) => Err(JoinError::Disconnected("play")),
            Some(play::Packet::KeepAlive(id)) => {
                self.connection.send(play::SERVERBOUND_KEEP_ALIVE_ID, &id.to_be_bytes()).await?;
                Ok(())
            }
            Some(play::Packet::SetCenterChunk { x, z }) => {
                self.center_chunk = Some((x, z));
                Ok(())
            }
            Some(play::Packet::SynchronizePosition(teleport)) => {
                let mut id = Vec::new();
                encode_varint(teleport.id, &mut id);
                self.connection.send(play::ACCEPT_TELEPORTATION_ID, &id).await?;
                Ok(())
            }
            Some(play::Packet::GameEvent { .. }) | None => Ok(()),
        }
    }
}

/// Join an offline server with one deadline covering DNS through spawn
/// confirmation (Confirm Teleportation + Player Loaded sent).
///
/// Dropping the future (or the returned session) closes TCP. No auth fallback.
pub async fn connect(
    host: &str,
    port: u16,
    name: &str,
    deadline: Duration,
) -> Result<Joined, JoinError> {
    connect_with_render_distance(host, port, name, 4, deadline).await
}

/// Join while advertising the requested chunk view distance (2 through 8).
pub async fn connect_with_render_distance(
    host: &str,
    port: u16,
    name: &str,
    render_distance: u8,
    deadline: Duration,
) -> Result<Joined, JoinError> {
    let handshake = login::handshake(host, port)?;
    let start = login::start(name)?;
    timeout(deadline, exchange(host, port, &handshake, &start, render_distance))
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
    render_distance: u8,
) -> Result<Joined, JoinError> {
    let mut connection = Connection::connect(host, port).await?;
    let mut budget = Budget::default();
    connection.send(0, handshake).await?;
    connection.send(login::START_ID, start).await?;
    let profile = login_phase(&mut connection, &mut budget).await?;
    connection.send(login::ACK_ID, &[]).await?;
    connection
        .send(configuration::INFORMATION_ID, &configuration::information(render_distance)?)
        .await?;
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
    let (center_chunk, spawn, chunks, batch_started) =
        await_spawn(&mut connection, &mut budget).await?;
    Ok(Joined {
        profile,
        world,
        registries,
        metadata,
        center_chunk,
        spawn,
        chunks,
        connection,
        batch_started,
    })
}

/// Consume Play packets until the initial spawn teleport is confirmed,
/// retaining any chunk batch received first (Pumpkin sends the initial view
/// area before teleporting the player). Every Play packet ID this client
/// does not otherwise recognize is discarded unread: see the policy note on
/// `play::Packet`. Budget limits still bound this loop.
async fn await_spawn(
    connection: &mut Connection,
    budget: &mut Budget,
) -> Result<(Option<(i32, i32)>, play::Teleport, Vec<chunk::LevelChunk>, Option<Instant>), JoinError>
{
    let mut center_chunk = None;
    let mut chunks = Vec::new();
    let mut batch_started = None;
    loop {
        let packet = budget.read(connection).await?;
        match packet.id {
            chunk::BATCH_START_ID => batch_started = Some(Instant::now()),
            chunk::LEVEL_CHUNK_WITH_LIGHT_ID => {
                if chunks.len() == MAX_CHUNKS_PER_JOIN {
                    return Err(JoinError::Limit);
                }
                chunks.push(chunk::decode(&packet)?);
            }
            chunk::BATCH_FINISHED_ID => {
                let Some(started) = batch_started.take() else {
                    return Err(JoinError::InvalidState("chunk batch finished without a start"));
                };
                let batch_size = chunk::decode_batch_finished(&packet)?;
                let reply = chunk::batch_received(desired_chunks_per_tick(started, batch_size));
                connection.send(chunk::BATCH_RECEIVED_ID, &reply).await?;
            }
            _ => match play::decode(&packet)? {
                Some(play::Packet::Disconnect) => return Err(JoinError::Disconnected("play")),
                Some(play::Packet::KeepAlive(id)) => {
                    connection.send(play::SERVERBOUND_KEEP_ALIVE_ID, &id.to_be_bytes()).await?;
                }
                Some(play::Packet::SetCenterChunk { x, z }) => center_chunk = Some((x, z)),
                Some(play::Packet::SynchronizePosition(teleport)) => {
                    if teleport.flags != 0 {
                        // No prior position exists yet to apply relative deltas to.
                        return Err(JoinError::InvalidState("relative initial spawn teleport"));
                    }
                    let mut id = Vec::new();
                    encode_varint(teleport.id, &mut id);
                    connection.send(play::ACCEPT_TELEPORTATION_ID, &id).await?;
                    connection.send(play::PLAYER_LOADED_ID, &[]).await?;
                    return Ok((center_chunk, teleport, chunks, batch_started));
                }
                // Game Event, and every packet ID this client doesn't
                // recognize (entity data, recipes, ...), are later work.
                Some(play::Packet::GameEvent { .. }) | None => {}
            },
        }
    }
}

/// Estimate chunks-per-tick from one batch, per wiki Packets § Chunk Batch
/// Finished's `25 / millisPerChunk` formula. Vanilla smooths this over its
/// last 15 batches; a single-batch estimate is enough for this join step.
fn desired_chunks_per_tick(started: Instant, batch_size: i32) -> f32 {
    #[allow(clippy::cast_precision_loss)] // A chunk count never approaches f32's precision limit.
    let size = batch_size.clamp(1, i32::from(i16::MAX)) as f32;
    let millis_per_chunk = started.elapsed().as_secs_f32() * 1000.0 / size;
    25.0 / millis_per_chunk.max(0.01)
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
