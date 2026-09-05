//! Synthetic TCP fixtures for the offline join path (`docs/JOIN.md`, protocol 776).
//! No game data or Java runtime required; packet shapes come from the wiki
//! Packets reference, encoded by hand or via the crate's own field primitives.

use std::{io, time::Duration};

use bytes::BytesMut;
use mc_client::{TransportError, join};
use mc_protocol::{
    CodecError, PROTOCOL_VERSION, configuration, encode_varint,
    framing::{FrameCodec, RawPacket},
    login, play,
    types::{Reader, encode_string},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};

const LOGIN_SUCCESS_ID: i32 = 2;
const LOGIN_COMPRESSION_ID: i32 = 3;
const LOGIN_ENCRYPTION_ID: i32 = 1;
const LOGIN_DISCONNECT_ID: i32 = 0;
const CONFIG_COOKIE_REQUEST_ID: i32 = 0;
const CONFIG_PLUGIN_ID: i32 = 1;
const CONFIG_DISCONNECT_ID: i32 = 2;
const CONFIG_RESET_CHAT_ID: i32 = 6;
const CONFIG_REGISTRY_ID: i32 = 7;
const CONFIG_FEATURES_ID: i32 = 12;
const CONFIG_TAGS_ID: i32 = 13;
const CONFIG_KNOWN_PACKS_REQUEST_ID: i32 = 14;
const CONFIG_SERVER_LINKS_ID: i32 = 16;

// --- Field builders, independent of the client's own encoders where it matters. ---

fn varint(value: i32) -> Vec<u8> {
    let mut out = Vec::new();
    encode_varint(value, &mut out);
    out
}

fn string(value: &str) -> Vec<u8> {
    let mut out = Vec::new();
    encode_string(value, 32_767, &mut out).unwrap();
    out
}

fn login_success(uuid: [u8; 16], name: &str, session: [u8; 16]) -> Vec<u8> {
    let mut body = uuid.to_vec();
    body.extend(string(name));
    body.extend(varint(0)); // Zero profile properties.
    body.extend(session);
    body
}

fn registry_data(id: &str, entries: &[(&str, Option<&[u8]>)]) -> Vec<u8> {
    let mut body = string(id);
    body.extend(varint(i32::try_from(entries.len()).unwrap()));
    for (entry_id, data) in entries {
        body.extend(string(entry_id));
        match data {
            Some(nbt) => {
                body.push(1);
                body.extend_from_slice(nbt);
            }
            None => body.push(0),
        }
    }
    body
}

fn known_packs_request(packs: &[(&str, &str, &str)]) -> Vec<u8> {
    let mut body = varint(i32::try_from(packs.len()).unwrap());
    for (namespace, id, version) in packs {
        body.extend(string(namespace));
        body.extend(string(id));
        body.extend(string(version));
    }
    body
}

fn features(ids: &[&str]) -> Vec<u8> {
    let mut body = varint(i32::try_from(ids.len()).unwrap());
    for id in ids {
        body.extend(string(id));
    }
    body
}

/// One registry's worth of tags: `(registry, [(tag, [numeric entries])])`.
fn tags(registry: &str, tag: &str, entries: &[i32]) -> Vec<u8> {
    let mut body = varint(1); // One registry.
    body.extend(string(registry));
    body.extend(varint(1)); // One tag.
    body.extend(string(tag));
    body.extend(varint(i32::try_from(entries.len()).unwrap()));
    for entry in entries {
        body.extend(varint(*entry));
    }
    body
}

/// One built-in server link (see wiki Packets § Server Links for the label enum).
fn server_links_builtin(label: i32, url: &str) -> Vec<u8> {
    let mut body = varint(1);
    body.push(1); // is_built_in
    body.extend(varint(label));
    body.extend(string(url));
    body
}

#[allow(clippy::too_many_arguments)]
fn play_login(
    entity_id: i32,
    dimension_type_index: i32,
    dimension_name: &str,
    game_mode: u8,
    previous_game_mode: i8,
    online_mode: bool,
) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(entity_id.to_be_bytes());
    body.push(0); // Not hardcore.
    body.extend(varint(1)); // One known dimension name.
    body.extend(string("minecraft:overworld"));
    body.extend(varint(20)); // Max players.
    body.extend(varint(10)); // View distance.
    body.extend(varint(10)); // Simulation distance.
    body.push(0); // Reduced debug info.
    body.push(1); // Enable respawn screen.
    body.push(0); // Do limited crafting.
    body.extend(varint(dimension_type_index));
    body.extend(string(dimension_name));
    body.extend(0_i64.to_be_bytes()); // Hashed seed.
    body.push(game_mode);
    body.push(previous_game_mode.cast_unsigned());
    body.push(0); // Is debug.
    body.push(0); // Is flat.
    body.push(0); // No death location.
    body.extend(varint(0)); // Portal cooldown.
    body.extend(varint(63)); // Sea level.
    body.push(u8::from(online_mode));
    body.push(0); // Does not enforce secure chat.
    body
}

fn game_event(event: u8, value: f32) -> Vec<u8> {
    let mut body = vec![event];
    body.extend(value.to_be_bytes());
    body
}

fn set_center_chunk(x: i32, z: i32) -> Vec<u8> {
    let mut body = varint(x);
    body.extend(varint(z));
    body
}

#[allow(clippy::too_many_arguments)]
fn synchronize_position(
    id: i32,
    x: f64,
    y: f64,
    z: f64,
    yaw: f32,
    pitch: f32,
    flags: i32,
) -> Vec<u8> {
    let mut body = varint(id);
    body.extend(x.to_be_bytes());
    body.extend(y.to_be_bytes());
    body.extend(z.to_be_bytes());
    body.extend(0_f64.to_be_bytes()); // Velocity X.
    body.extend(0_f64.to_be_bytes()); // Velocity Y.
    body.extend(0_f64.to_be_bytes()); // Velocity Z.
    body.extend(yaw.to_be_bytes());
    body.extend(pitch.to_be_bytes());
    body.extend(flags.to_be_bytes());
    body
}

// --- A minimal synthetic peer sharing the crate's own frame codec. ---

struct Server {
    stream: TcpStream,
    codec: FrameCodec,
    input: BytesMut,
}

impl Server {
    async fn accept(listener: &TcpListener) -> Self {
        let (stream, _) = listener.accept().await.unwrap();
        Self { stream, codec: FrameCodec::default(), input: BytesMut::new() }
    }

    fn set_compression(&mut self, threshold: i32) {
        self.codec.set_compression(threshold);
    }

    fn encode(&self, id: i32, payload: &[u8]) -> Vec<u8> {
        self.codec.encode(id, payload).unwrap().to_vec()
    }

    async fn send_raw(&mut self, bytes: &[u8]) {
        self.stream.write_all(bytes).await.unwrap();
    }

    async fn send(&mut self, id: i32, payload: &[u8]) {
        let frame = self.encode(id, payload);
        self.send_raw(&frame).await;
    }

    /// Encode every packet first, then write them in one call: exercises
    /// coalesced frames arriving in a single TCP segment.
    async fn send_batch(&mut self, packets: &[(i32, Vec<u8>)]) {
        let mut batch = Vec::new();
        for (id, payload) in packets {
            batch.extend(self.encode(*id, payload));
        }
        self.send_raw(&batch).await;
    }

    async fn read(&mut self) -> RawPacket {
        loop {
            if let Some(packet) = self.codec.decode(&mut self.input).unwrap() {
                return packet;
            }
            let mut buf = [0; 8192];
            let count = self.stream.read(&mut buf).await.unwrap();
            assert!(count > 0, "client closed the connection unexpectedly");
            self.input.extend_from_slice(&buf[..count]);
        }
    }

    async fn expect_handshake_and_login_start(&mut self, port: u16, name: &str) {
        let handshake = self.read().await;
        assert_eq!(handshake.id, 0);
        let mut reader = Reader::new(&handshake.payload);
        assert_eq!(reader.varint().unwrap(), PROTOCOL_VERSION);
        assert_eq!(reader.string(255).unwrap(), "127.0.0.1");
        assert_eq!(reader.u16().unwrap(), port);
        assert_eq!(reader.varint().unwrap(), 2); // Login intent.
        reader.finish().unwrap();

        let start = self.read().await;
        assert_eq!(start.id, login::START_ID);
        let mut reader = Reader::new(&start.payload);
        assert_eq!(reader.string(16).unwrap(), name);
        assert_eq!(reader.array::<16>().unwrap(), [0; 16]); // Nil placeholder UUID.
        reader.finish().unwrap();
    }

    /// Send a representative set of pre-spawn Play packets (a recognized
    /// event, the chunk center, one unrecognized ID that must be skipped
    /// rather than failing, and a keep-alive), then the spawn teleport that
    /// ends the phase, verifying the client's Confirm Teleportation and
    /// Player Loaded replies.
    async fn drive_spawn_sequence(&mut self) {
        self.send_batch(&[
            (play::GAME_EVENT_ID, game_event(13, 0.0)),
            (play::SET_CENTER_CHUNK_ID, set_center_chunk(1, -3)),
            (99, vec![0xaa, 0xbb]),
            (play::KEEP_ALIVE_ID, 99_i64.to_be_bytes().to_vec()),
            (
                play::SYNCHRONIZE_POSITION_ID,
                synchronize_position(7, 23.5, 80.0, -32.5, 0.0, 0.0, 0),
            ),
        ])
        .await;

        let keep_alive_reply = self.read().await;
        assert_eq!(keep_alive_reply.id, play::SERVERBOUND_KEEP_ALIVE_ID);
        assert_eq!(keep_alive_reply.payload.as_ref(), 99_i64.to_be_bytes());

        let confirm = self.read().await;
        assert_eq!(confirm.id, play::ACCEPT_TELEPORTATION_ID);
        let mut reader = Reader::new(&confirm.payload);
        assert_eq!(reader.varint().unwrap(), 7);
        reader.finish().unwrap();

        let player_loaded = self.read().await;
        assert_eq!(player_loaded.id, play::PLAYER_LOADED_ID);
        assert!(player_loaded.payload.is_empty());
    }
}

async fn connect(port: u16, deadline: Duration) -> Result<join::Joined, join::JoinError> {
    join::connect("127.0.0.1", port, "RustProbe", deadline).await
}

#[tokio::test]
async fn full_join_reaches_play_with_compression_coalescing_and_full_registry_retention() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let uuid = [7; 16];
    let session = [9; 16];

    let server = async {
        let mut server = Server::accept(&listener).await;
        server.expect_handshake_and_login_start(port, "RustProbe").await;

        // Compression negotiation and the very next frame arrive coalesced;
        // the negotiation packet itself must not be compressed.
        let mut batch = server.encode(LOGIN_COMPRESSION_ID, &varint(1));
        server.set_compression(1);
        batch.extend(server.encode(LOGIN_SUCCESS_ID, &login_success(uuid, "RustProbe", session)));
        server.send_raw(&batch).await;

        let ack = server.read().await;
        assert_eq!(ack.id, login::ACK_ID);
        assert!(ack.payload.is_empty());

        let info = server.read().await;
        assert_eq!(info.id, configuration::INFORMATION_ID);
        assert_eq!(info.payload.as_ref(), configuration::information().unwrap());

        // One coalesced batch covering every configuration packet kind:
        // known packs, cookie, keep-alive, ping, plugin, reset-chat, two
        // registries, features, tags, server links, and finish.
        server
            .send_batch(&[
                (CONFIG_KNOWN_PACKS_REQUEST_ID, known_packs_request(&[("minecraft", "core", "1")])),
                (CONFIG_COOKIE_REQUEST_ID, string("test:cookie")),
                (configuration::KEEP_ALIVE_ID, 42_i64.to_be_bytes().to_vec()),
                (configuration::PONG_ID, 7_i32.to_be_bytes().to_vec()),
                (CONFIG_PLUGIN_ID, [string("test:channel"), b"hello".to_vec()].concat()),
                (CONFIG_RESET_CHAT_ID, Vec::new()),
                (
                    CONFIG_REGISTRY_ID,
                    registry_data(
                        "minecraft:dimension_type",
                        &[("minecraft:overworld", Some(&[10, 0]))],
                    ),
                ),
                (
                    CONFIG_REGISTRY_ID,
                    registry_data(
                        "minecraft:worldgen/biome",
                        &[
                            ("minecraft:plains", Some(&[10, 0])),
                            ("minecraft:desert", Some(&[10, 0])),
                        ],
                    ),
                ),
                (CONFIG_FEATURES_ID, features(&["minecraft:vanilla", "minecraft:bundle"])),
                (CONFIG_TAGS_ID, tags("minecraft:block", "minecraft:mineable/pickaxe", &[0, 1])),
                (CONFIG_SERVER_LINKS_ID, server_links_builtin(6, "https://example.test")),
                (configuration::FINISH_ID, Vec::new()),
            ])
            .await;

        let known_packs_reply = server.read().await;
        assert_eq!(known_packs_reply.id, configuration::KNOWN_PACKS_ID);
        assert_eq!(known_packs_reply.payload.as_ref(), [0]);

        let cookie_reply = server.read().await;
        assert_eq!(cookie_reply.id, configuration::COOKIE_RESPONSE_ID);
        assert_eq!(cookie_reply.payload.as_ref(), login::absent_cookie("test:cookie").unwrap());

        let keep_alive_reply = server.read().await;
        assert_eq!(keep_alive_reply.id, configuration::KEEP_ALIVE_ID);
        assert_eq!(keep_alive_reply.payload.as_ref(), 42_i64.to_be_bytes());

        let pong_reply = server.read().await;
        assert_eq!(pong_reply.id, configuration::PONG_ID);
        assert_eq!(pong_reply.payload.as_ref(), 7_i32.to_be_bytes());

        let finish_ack = server.read().await;
        assert_eq!(finish_ack.id, configuration::FINISH_ID);
        assert!(finish_ack.payload.is_empty());

        server.send(play::LOGIN_ID, &play_login(123, 0, "minecraft:overworld", 1, -1, true)).await;
        server.drive_spawn_sequence().await;
    };

    let client = Box::pin(connect(port, Duration::from_secs(5)));
    let ((), result) =
        Box::pin(timeout(Duration::from_secs(5), async { tokio::join!(server, client) }))
            .await
            .unwrap();
    let joined = result.unwrap();

    assert_eq!(joined.profile.uuid, uuid);
    assert_eq!(joined.profile.name, "RustProbe");
    assert_eq!(joined.profile.session_id, session);

    assert_eq!(joined.world.entity_id, 123);
    assert_eq!(joined.world.dimension_type, 0);
    assert_eq!(joined.world.dimension_name, "minecraft:overworld");
    assert_eq!(joined.world.game_mode, 1);

    assert_eq!(joined.registries.len(), 2);
    assert_eq!(joined.registries.entry_count(), 3);
    assert_eq!(joined.registries.get("minecraft:dimension_type").unwrap().len(), 1);
    let biomes = joined.registries.get("minecraft:worldgen/biome").unwrap();
    assert_eq!(biomes.len(), 2);
    assert_eq!(biomes[0].id, "minecraft:plains");
    assert_eq!(biomes[1].id, "minecraft:desert");

    assert_eq!(joined.metadata.len(), 2);
    assert_eq!(joined.metadata[0].id, CONFIG_FEATURES_ID);
    assert_eq!(joined.metadata[1].id, CONFIG_TAGS_ID);

    assert_eq!(joined.center_chunk, Some((1, -3)));
    assert_eq!(joined.spawn.id, 7);
    assert_eq!(joined.spawn.x.to_bits(), 23.5_f64.to_bits());
    assert_eq!(joined.spawn.y.to_bits(), 80.0_f64.to_bits());
    assert_eq!(joined.spawn.z.to_bits(), (-32.5_f64).to_bits());
    assert_eq!(joined.spawn.yaw.to_bits(), 0.0_f32.to_bits());
    assert_eq!(joined.spawn.pitch.to_bits(), 0.0_f32.to_bits());
    assert_eq!(joined.spawn.flags, 0);
}

#[tokio::test]
async fn encryption_request_is_rejected() {
    let result = Box::pin(login_result((LOGIN_ENCRYPTION_ID, Vec::new()))).await;
    assert!(matches!(result, Err(join::JoinError::AuthenticationRequired)));
}

#[tokio::test]
async fn login_disconnect_is_reported() {
    let result = Box::pin(login_result((LOGIN_DISCONNECT_ID, string(r#"{"text":"bye"}"#)))).await;
    assert!(matches!(result, Err(join::JoinError::Disconnected("login"))));
}

#[tokio::test]
async fn configuration_disconnect_is_reported() {
    let result = Box::pin(configuration_result((CONFIG_DISCONNECT_ID, vec![8, 0, 0]))).await; // Empty NBT string root.
    assert!(matches!(result, Err(join::JoinError::Disconnected("configuration"))));
}

#[tokio::test]
async fn missing_registry_data_is_rejected() {
    let body = registry_data("minecraft:dimension_type", &[("minecraft:overworld", None)]);
    let result = Box::pin(configuration_result((CONFIG_REGISTRY_ID, body))).await;
    assert!(matches!(
        result,
        Err(join::JoinError::InvalidState("registry data omitted without known packs"))
    ));
}

#[tokio::test]
async fn malformed_configuration_packets_are_rejected() {
    let unknown = Box::pin(configuration_result((99, Vec::new()))).await;
    assert!(matches!(
        unknown,
        Err(join::JoinError::Configuration(configuration::Error::Codec(
            CodecError::InvalidPacketId(99)
        )))
    ));

    // Reset Chat takes no fields; one trailing byte must fail exact consumption.
    let trailing = Box::pin(configuration_result((CONFIG_RESET_CHAT_ID, vec![0xff]))).await;
    assert!(matches!(
        trailing,
        Err(join::JoinError::Configuration(configuration::Error::Codec(CodecError::TrailingData)))
    ));
}

#[tokio::test]
async fn play_disconnect_during_spawn_is_reported() {
    let result = Box::pin(spawn_result((play::DISCONNECT_ID, vec![8, 0, 0]))).await; // Empty NBT string reason.
    assert!(matches!(result, Err(join::JoinError::Disconnected("play"))));
}

#[tokio::test]
async fn relative_initial_spawn_teleport_is_rejected() {
    let body = synchronize_position(1, 0.0, 0.0, 0.0, 0.0, 0.0, 1); // Relative X bit set.
    let result = Box::pin(spawn_result((play::SYNCHRONIZE_POSITION_ID, body))).await;
    assert!(matches!(
        result,
        Err(join::JoinError::InvalidState("relative initial spawn teleport"))
    ));
}

#[tokio::test]
async fn eof_during_login_is_reported() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = async {
        let mut server = Server::accept(&listener).await;
        server.expect_handshake_and_login_start(port, "RustProbe").await;
        // Drop without replying; the client must see a clean EOF, not hang.
    };
    let client = Box::pin(connect(port, Duration::from_secs(3)));
    let ((), result) =
        Box::pin(timeout(Duration::from_secs(5), async { tokio::join!(server, client) }))
            .await
            .unwrap();
    assert!(matches!(
        result,
        Err(join::JoinError::Transport(TransportError::Io(error)))
            if error.kind() == io::ErrorKind::UnexpectedEof
    ));
}

#[tokio::test]
async fn join_deadline_closes_a_stalled_connection() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = async {
        let mut server = Server::accept(&listener).await;
        server.expect_handshake_and_login_start(port, "RustProbe").await;
        // Never reply; client cancellation must close TCP without a detached reader.
        let mut byte = [0; 1];
        assert_eq!(server.stream.read(&mut byte).await.unwrap(), 0);
    };
    let client = Box::pin(connect(port, Duration::from_millis(250)));
    let ((), result) =
        Box::pin(timeout(Duration::from_secs(5), async { tokio::join!(server, client) }))
            .await
            .unwrap();
    assert!(matches!(result, Err(join::JoinError::Timeout)));
}

/// Drive the server through login success, ack and client info, then send
/// one more configuration-state packet and return the client's outcome.
async fn configuration_result(next: (i32, Vec<u8>)) -> Result<join::Joined, join::JoinError> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = async {
        let mut server = Server::accept(&listener).await;
        server.expect_handshake_and_login_start(port, "RustProbe").await;
        server.send(LOGIN_SUCCESS_ID, &login_success([1; 16], "RustProbe", [2; 16])).await;
        let _ack = server.read().await;
        let _info = server.read().await;
        server.send(next.0, &next.1).await;
    };
    let client = Box::pin(connect(port, Duration::from_secs(3)));
    let ((), result) =
        Box::pin(timeout(Duration::from_secs(5), async { tokio::join!(server, client) }))
            .await
            .unwrap();
    result
}

/// Drive the server through login success, ack, client info, a minimal
/// one-entry dimension-type registry, and Play Login, then send one more
/// pre-spawn Play packet and return the client's outcome.
async fn spawn_result(next: (i32, Vec<u8>)) -> Result<join::Joined, join::JoinError> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = async {
        let mut server = Server::accept(&listener).await;
        server.expect_handshake_and_login_start(port, "RustProbe").await;
        server.send(LOGIN_SUCCESS_ID, &login_success([3; 16], "RustProbe", [4; 16])).await;
        let _ack = server.read().await;
        let _info = server.read().await;
        server
            .send(
                CONFIG_REGISTRY_ID,
                &registry_data(
                    "minecraft:dimension_type",
                    &[("minecraft:overworld", Some(&[10, 0]))],
                ),
            )
            .await;
        server.send(configuration::FINISH_ID, &[]).await;
        let _finish_ack = server.read().await;
        server.send(play::LOGIN_ID, &play_login(1, 0, "minecraft:overworld", 1, -1, true)).await;
        server.send(next.0, &next.1).await;
    };
    let client = Box::pin(connect(port, Duration::from_secs(3)));
    let ((), result) =
        Box::pin(timeout(Duration::from_secs(5), async { tokio::join!(server, client) }))
            .await
            .unwrap();
    result
}

/// Drive the server through the handshake and login start, then send one
/// login-state packet and return the client's outcome.
async fn login_result(next: (i32, Vec<u8>)) -> Result<join::Joined, join::JoinError> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = async {
        let mut server = Server::accept(&listener).await;
        server.expect_handshake_and_login_start(port, "RustProbe").await;
        server.send(next.0, &next.1).await;
    };
    let client = Box::pin(connect(port, Duration::from_secs(3)));
    let ((), result) =
        Box::pin(timeout(Duration::from_secs(5), async { tokio::join!(server, client) }))
            .await
            .unwrap();
    result
}
