//! Bounded, cancellable TCP status exchange for protocol 776.
//!
//! One deadline covers DNS through pong. No authentication, assets, or background
//! tasks. See `docs/FOUNDATION.md`; packet serialization lives in mc-protocol.

use std::{io, time::Duration};

use bytes::BytesMut;
use mc_protocol::{
    CodecError, MAX_PACKET_SIZE,
    framing::{FrameCodec, RawPacket},
    status,
};
use serde_json::Value;
use thiserror::Error;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{Instant, timeout},
};

/// Server status JSON and the measured ping/pong round-trip duration.
#[derive(Debug)]
pub struct StatusResult {
    /// Complete response object, preserving unknown fields and text components.
    pub json: Value,
    /// Ping/pong latency, excluding DNS, connect, and the status request.
    pub latency: Duration,
}

/// Failure of a status exchange. The connection is dropped on every error.
#[derive(Debug, Error)]
pub enum StatusError {
    /// DNS, connection, read, or write failed.
    #[error("status I/O failed: {0}")]
    Io(#[from] io::Error),
    /// Invalid packet data.
    #[error("invalid status packet: {0}")]
    Codec(#[from] CodecError),
    /// The status string was not valid JSON.
    #[error("invalid status JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// JSON must describe an object.
    #[error("status JSON must be an object")]
    InvalidStatus,
    /// The overall exchange exceeded its deadline.
    #[error("status query timed out")]
    Timeout,
    /// The server did not echo the sent ping value.
    #[error("pong did not match the ping payload")]
    PongMismatch,
}

/// Query a server with one overall deadline. Cancelling this future closes TCP.
///
/// `host` is sent unchanged in the handshake and resolved for TCP. SRV discovery
/// and login are not part of status querying.
pub async fn query(host: &str, port: u16, deadline: Duration) -> Result<StatusResult, StatusError> {
    let handshake = status::handshake(host, port)?;
    timeout(deadline, exchange(host, port, &handshake)).await.map_err(|_| StatusError::Timeout)?
}

async fn exchange(host: &str, port: u16, handshake: &[u8]) -> Result<StatusResult, StatusError> {
    let mut stream = TcpStream::connect((host, port)).await?;
    stream.set_nodelay(true)?;
    let codec = FrameCodec::default();
    let mut input = BytesMut::new();
    stream.write_all(&codec.encode(status::HANDSHAKE_ID, handshake)?).await?;
    stream.write_all(&codec.encode(status::REQUEST_ID, &[])?).await?;
    let response = read_packet(&mut stream, &codec, &mut input).await?;
    let json: Value = serde_json::from_str(status::response(&response)?)?;
    if !json.is_object() {
        return Err(StatusError::InvalidStatus);
    }

    // A single outstanding ping needs only a stable echo token, not wall time.
    let payload = 0x4d43_5255_5354_0308;
    let started = Instant::now();
    stream.write_all(&codec.encode(status::PING_ID, &status::ping(payload))?).await?;
    let pong = read_packet(&mut stream, &codec, &mut input).await?;
    if status::pong(&pong)? != payload {
        return Err(StatusError::PongMismatch);
    }
    Ok(StatusResult { json, latency: started.elapsed() })
}

async fn read_packet(
    stream: &mut TcpStream,
    codec: &FrameCodec,
    input: &mut BytesMut,
) -> Result<RawPacket, StatusError> {
    let mut scratch = [0; 8192];
    loop {
        if let Some(packet) = codec.decode(input)? {
            return Ok(packet);
        }
        // Bound bytes read even if a peer never finishes its packet.
        let available = (MAX_PACKET_SIZE + 3).saturating_sub(input.len());
        if available == 0 {
            return Err(CodecError::PacketTooLarge(input.len()).into());
        }
        let read_limit = available.min(scratch.len());
        let count = stream.read(&mut scratch[..read_limit]).await?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "server closed during status",
            )
            .into());
        }
        input.extend_from_slice(&scratch[..count]);
    }
}
