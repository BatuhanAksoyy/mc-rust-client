//! Shared bounded TCP framing for status and joining; timing belongs to callers.

use bytes::BytesMut;
use mc_protocol::{
    CodecError, MAX_PACKET_SIZE,
    framing::{FrameCodec, RawPacket},
};
use std::io;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

/// Terminal TCP or framing failure. Discard the connection after an error.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// Connection, read or write failure.
    #[error("connection I/O failed: {0}")]
    Io(#[from] io::Error),
    /// Malformed framing or compression.
    #[error("invalid packet frame: {0}")]
    Codec(#[from] CodecError),
}

#[derive(Debug)]
pub struct Connection {
    stream: TcpStream,
    pub codec: FrameCodec,
    input: BytesMut,
}

impl Connection {
    pub async fn connect(host: &str, port: u16) -> Result<Self, TransportError> {
        let stream = TcpStream::connect((host, port)).await?;
        stream.set_nodelay(true)?;
        Ok(Self { stream, codec: FrameCodec::default(), input: BytesMut::new() })
    }

    pub async fn send(&mut self, id: i32, payload: &[u8]) -> Result<(), TransportError> {
        self.stream.write_all(&self.codec.encode(id, payload)?).await?;
        Ok(())
    }

    pub async fn read(&mut self) -> Result<RawPacket, TransportError> {
        let mut scratch = [0; 8192];
        loop {
            if let Some(packet) = self.codec.decode(&mut self.input)? {
                return Ok(packet);
            }
            let available = (MAX_PACKET_SIZE + 3).saturating_sub(self.input.len());
            if available == 0 {
                return Err(CodecError::PacketTooLarge(self.input.len()).into());
            }
            let read_limit = available.min(scratch.len());
            let count = self.stream.read(&mut scratch[..read_limit]).await?;
            if count == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "server closed connection",
                )
                .into());
            }
            self.input.extend_from_slice(&scratch[..count]);
        }
    }
}
