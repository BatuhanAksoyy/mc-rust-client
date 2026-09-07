use std::{
    fs,
    io::{self, BufRead},
    process::Command,
    time::Duration,
};

use super::{Pumpkin, PumpkinError, release, release_asset, session};

#[test]
fn supported_release_assets_and_corrupt_binary() {
    for (os, arch) in [("macos", "aarch64"), ("linux", "x86_64"), ("windows", "x86_64")] {
        let asset = release_asset(os, arch).unwrap();
        assert_eq!(asset.sha256.len(), 64);
        assert!(asset.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
    }
    assert!(release_asset("macos", "x86_64").is_none());
    let binary = tempfile::NamedTempFile::new().unwrap();
    assert!(matches!(release::verify(binary.path()), Err(PumpkinError::HashMismatch)));
}

#[test]
fn session_ownership_lock_and_restart_preserve_world_data() {
    let root = tempfile::tempdir().unwrap();
    assert!(session::prepare(root.path(), 25565).is_err());
    let path = root.path().join("session");
    let (directory, lock) = session::prepare(&path, 25565).unwrap();
    fs::create_dir(directory.join("world")).unwrap();
    fs::write(directory.join("world/synthetic-save"), b"preserve").unwrap();
    assert!(matches!(session::prepare(&path, 25566), Err(PumpkinError::SessionBusy)));
    drop(lock);
    let (_, _lock) = session::prepare(&path, 25566).unwrap();
    assert_eq!(fs::read(directory.join("world/synthetic-save")).unwrap(), b"preserve");
    let config: toml::Value =
        toml::from_str(&fs::read_to_string(directory.join("pumpkin.toml")).unwrap()).unwrap();
    assert_eq!(config["networking"]["java"]["address"].as_str(), Some("127.0.0.1:25566"));
    assert_eq!(config["networking"]["java"]["online_mode"].as_bool(), Some(false));
    assert_eq!(config["networking"]["java"]["encryption"].as_bool(), Some(false));
    for service in ["bedrock", "query", "rcon", "proxy", "lan_broadcast"] {
        assert_eq!(config["networking"][service]["enabled"].as_bool(), Some(false));
    }
    assert_eq!(config["plugins"]["enabled"].as_bool(), Some(false));
    assert_eq!(config["commands"]["use_tty"].as_bool(), Some(false));
}

#[test]
fn rejects_checkout_and_modified_ownership_marker() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session");
    let (_, lock) = session::prepare(&path, 25565).unwrap();
    drop(lock);
    fs::write(path.join(".mc-client-pumpkin"), b"another version").unwrap();
    assert!(session::prepare(&path, 25565).is_err());
    fs::create_dir(root.path().join(".git")).unwrap();
    assert!(session::prepare(&root.path().join("new"), 25565).is_err());
    assert!(!root.path().join("new").exists());
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_session_and_config() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session");
    let (_, lock) = session::prepare(&path, 25565).unwrap();
    drop(lock);
    let alias = root.path().join("alias");
    symlink(&path, &alias).unwrap();
    assert!(session::prepare(&alias, 25565).is_err());
    fs::remove_file(path.join("pumpkin.toml")).unwrap();
    let target = root.path().join("untouched");
    fs::write(&target, b"keep").unwrap();
    symlink(&target, path.join("pumpkin.toml")).unwrap();
    assert!(session::prepare(&path, 25565).is_err());
    assert_eq!(fs::read(target).unwrap(), b"keep");
}

fn fake(root: &std::path::Path, mode: &str) -> Pumpkin {
    let (directory, lock) = session::prepare(&root.join("session"), 25565).unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "pumpkin::tests::child", "--nocapture"])
        .env("MC_TEST_PUMPKIN_MODE", mode);
    Pumpkin::spawn(command, &directory, lock, 25565).unwrap()
}

// The same Rust test executable is a portable subprocess fixture. No shell or
// upstream executable is needed in CI, and the environment is child-local.
#[test]
fn child() {
    let Ok(mode) = std::env::var("MC_TEST_PUMPKIN_MODE") else { return };
    fs::write("ready", b"ready").unwrap();
    if mode == "exit" {
        return;
    }
    for line in io::stdin().lock().lines() {
        if line.unwrap() == "stop" && mode != "stall" {
            fs::write("saved", b"saved").unwrap();
            return;
        }
    }
}

async fn ready(root: &std::path::Path) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !root.join("session/ready").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn graceful_stop_saves_and_releases_lock() {
    let root = tempfile::tempdir().unwrap();
    let child = fake(root.path(), "save");
    ready(root.path()).await;
    child.shutdown(Duration::from_secs(5)).await.unwrap();
    assert!(root.path().join("session/saved").exists());
    assert!(session::prepare(&root.path().join("session"), 25565).is_ok());
}

#[tokio::test]
async fn forced_stop_is_an_error_and_releases_lock() {
    let root = tempfile::tempdir().unwrap();
    let child = fake(root.path(), "stall");
    ready(root.path()).await;
    assert!(matches!(
        child.shutdown(Duration::from_millis(50)).await,
        Err(PumpkinError::ShutdownTimeout)
    ));
    assert!(!root.path().join("session/saved").exists());
    assert!(session::prepare(&root.path().join("session"), 25565).is_ok());
}

#[tokio::test]
async fn drop_reaps_child_before_releasing_lock() {
    let root = tempfile::tempdir().unwrap();
    let child = fake(root.path(), "stall");
    ready(root.path()).await;
    drop(child);
    assert!(!root.path().join("session/saved").exists());
    assert!(session::prepare(&root.path().join("session"), 25565).is_ok());
}

#[tokio::test]
async fn unexpected_exit_is_not_reported_as_a_save() {
    let root = tempfile::tempdir().unwrap();
    let mut child = fake(root.path(), "exit");
    tokio::time::timeout(Duration::from_secs(5), async {
        while child.try_wait().unwrap().is_none() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(matches!(child.shutdown(Duration::from_secs(1)).await, Err(PumpkinError::Exit(_))));
}

#[test]
fn checkout_data_session_can_restart_but_nested_checkout_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join(".git"), "gitdir: worktree").unwrap();
    let data = root.path().join(".data");
    fs::create_dir(&data).unwrap();
    let path = data.join("server");
    let (_, lock) = session::prepare(&path, 25565).unwrap();
    drop(lock);
    assert!(session::prepare(&path, 25565).is_ok());
    fs::create_dir(data.join(".git")).unwrap();
    assert!(session::prepare(&data.join("other"), 25565).is_err());
}
