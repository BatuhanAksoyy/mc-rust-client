//! Opt-in real runtime smoke test. No binary or world fixtures are vendored.
use std::{net::TcpListener, path::PathBuf, time::Duration};

#[tokio::test]
#[ignore = "requires PUMPKIN_BIN pointing to the pinned external executable"]
async fn pinned_pumpkin_starts_stops_and_reopens_world() {
    let binary = PathBuf::from(std::env::var_os("PUMPKIN_BIN").expect("set PUMPKIN_BIN"));
    let root = tempfile::tempdir().unwrap();
    let session = root.path().join("session");
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    for _ in 0..2 {
        let (server, status) =
            mc_client::local::start(&binary, &session, port, Duration::from_mins(2)).await.unwrap();
        assert_eq!(status.json["version"]["protocol"], 776);
        server.shutdown(Duration::from_secs(30)).await.unwrap();
        assert!(session.join("world/level.dat").metadata().unwrap().len() > 0);
    }
}
