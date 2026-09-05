//! Real loopback TCP exchange with synthetic protocol 776 fixtures.
use std::{io, time::Duration};

use mc_client::status::{StatusError, query};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};

async fn receive_requests(stream: &mut TcpStream, port: u16) {
    // Literal fixture independent of the production frame/handshake codecs.
    let mut expected =
        vec![16, 0, 0x88, 6, 9, b'1', b'2', b'7', b'.', b'0', b'.', b'0', b'.', b'1'];
    expected.extend_from_slice(&port.to_be_bytes());
    expected.extend_from_slice(&[1, 1, 0]);
    let mut received = vec![0; expected.len()];
    stream.read_exact(&mut received).await.unwrap();
    assert_eq!(received, expected);
}

fn response(json: &[u8]) -> Vec<u8> {
    assert!(json.len() < 125);
    let mut response =
        vec![u8::try_from(json.len() + 2).unwrap(), 0, u8::try_from(json.len()).unwrap()];
    response.extend_from_slice(json);
    response
}

#[tokio::test]
async fn fragmented_status_and_ping_exchange() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = async {
        let (mut stream, _) = listener.accept().await.unwrap();
        receive_requests(&mut stream, port).await;
        let json = br#"{"version":{"name":"26.2","protocol":776},"description":{"text":"Synthetic"},"extra":true}"#;
        for byte in response(json) {
            stream.write_all(&[byte]).await.unwrap();
            tokio::task::yield_now().await;
        }
        let mut ping = [0; 10];
        stream.read_exact(&mut ping).await.unwrap();
        assert_eq!(&ping[..2], &[9, 1]);
        stream.write_all(&ping).await.unwrap();
    };
    let client = Box::pin(query("127.0.0.1", port, Duration::from_secs(3)));
    let ((), result) =
        timeout(Duration::from_secs(5), async { tokio::join!(server, client) }).await.unwrap();
    let result = result.unwrap();
    assert_eq!(result.json["version"]["protocol"], 776);
    assert_eq!(result.json["extra"], true);
    assert_eq!(result.json["description"]["text"], "Synthetic");
}

async fn with_reply(reply: Vec<u8>) -> Result<mc_client::status::StatusResult, StatusError> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = async {
        let (mut stream, _) = listener.accept().await.unwrap();
        receive_requests(&mut stream, port).await;
        stream.write_all(&reply).await.unwrap();
        stream.shutdown().await.unwrap();
    };
    let ((), result) = timeout(Duration::from_secs(5), async {
        tokio::join!(server, query("127.0.0.1", port, Duration::from_secs(3)))
    })
    .await
    .unwrap();
    result
}

#[tokio::test]
async fn malformed_responses_are_reported() {
    assert!(matches!(with_reply(response(b"{")).await, Err(StatusError::Json(_))));
    assert!(matches!(with_reply(response(b"[]")).await, Err(StatusError::InvalidStatus)));
    assert!(matches!(with_reply(vec![1, 3]).await, Err(StatusError::Codec(_))));
    assert!(matches!(with_reply(vec![0x80, 0x80, 0x80]).await, Err(StatusError::Codec(_))));
    assert!(
        matches!(with_reply(vec![5, 0]).await, Err(StatusError::Io(e)) if e.kind() == io::ErrorKind::UnexpectedEof)
    );
}

#[tokio::test]
async fn mismatched_pong_is_rejected() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = async {
        let (mut stream, _) = listener.accept().await.unwrap();
        receive_requests(&mut stream, port).await;
        stream.write_all(&response(b"{}")).await.unwrap();
        let mut ping = [0; 10];
        stream.read_exact(&mut ping).await.unwrap();
        ping[9] ^= 1;
        stream.write_all(&ping).await.unwrap();
    };
    let ((), result) = timeout(Duration::from_secs(5), async {
        tokio::join!(server, query("127.0.0.1", port, Duration::from_secs(3)))
    })
    .await
    .unwrap();
    assert!(matches!(result, Err(StatusError::PongMismatch)));
}

#[tokio::test]
async fn deadline_closes_a_stalled_connection() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = async {
        let (mut stream, _) = listener.accept().await.unwrap();
        receive_requests(&mut stream, port).await;
        let mut byte = [0];
        // Client cancellation must close TCP without a detached reader.
        assert_eq!(stream.read(&mut byte).await.unwrap(), 0);
    };
    let ((), result) = timeout(Duration::from_secs(5), async {
        tokio::join!(server, query("127.0.0.1", port, Duration::from_millis(250)))
    })
    .await
    .unwrap();
    assert!(matches!(result, Err(StatusError::Timeout)));
}
