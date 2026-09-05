use std::time::Duration;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

use super::{LocalError, wait_ready};

async fn response(
    protocol: i32,
    identity: &str,
) -> Result<crate::status::StatusResult, LocalError> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = async {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut requests = [0; 19];
        stream.read_exact(&mut requests).await.unwrap();
        let json = format!(
            r#"{{"version":{{"protocol":{protocol}}},"description":{{"text":"{identity}"}}}}"#
        );
        let length = u8::try_from(json.len()).unwrap();
        assert!(length < 125);
        stream.write_all(&[length + 2, 0, length]).await.unwrap();
        stream.write_all(json.as_bytes()).await.unwrap();
        let mut ping = [0; 10];
        stream.read_exact(&mut ping).await.unwrap();
        stream.write_all(&ping).await.unwrap();
    };
    let ((), result) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(server, wait_ready(port, "expected", Duration::from_secs(3), || Ok(())))
    })
    .await
    .unwrap();
    result
}

#[tokio::test]
async fn readiness_checks_protocol_and_managed_identity() {
    assert!(response(776, "expected").await.is_ok());
    assert!(matches!(response(775, "expected").await, Err(LocalError::ProtocolMismatch)));
    assert!(matches!(response(776, "another").await, Err(LocalError::IdentityMismatch)));
}

#[tokio::test]
async fn readiness_has_a_deadline_and_observes_child_failure() {
    assert!(matches!(
        wait_ready(1, "expected", Duration::ZERO, || Ok(())).await,
        Err(LocalError::StartupTimeout)
    ));
    let failed = || Err(mc_launcher::pumpkin::PumpkinError::InvalidSession("child failed").into());
    assert!(matches!(
        wait_ready(1, "expected", Duration::from_secs(10), failed).await,
        Err(LocalError::Pumpkin(_))
    ));
}

#[tokio::test]
async fn occupied_port_fails_before_touching_paths() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let result = super::start(
        std::path::Path::new("nonexistent-binary"),
        std::path::Path::new("nonexistent-session"),
        port,
        Duration::from_secs(1),
    )
    .await;
    assert!(matches!(result, Err(LocalError::PortInUse)));
}
