//! Client orchestration for a local Pumpkin process, protocol 776.
//!
//! See `docs/SINGLEPLAYER.md`. Readiness is a complete protocol status exchange,
//! never a log substring. Gameplay login is the next client milestone.

use std::{path::Path, time::Duration};

use mc_launcher::pumpkin::{Pumpkin, PumpkinError};
use mc_protocol::PROTOCOL_VERSION;
use thiserror::Error;
use tokio::time::{Instant, sleep};

use crate::status::{self, StatusResult};

/// Local server startup failure.
#[derive(Debug, Error)]
pub enum LocalError {
    /// Executable, filesystem, or child-process failure.
    #[error(transparent)]
    Pumpkin(#[from] PumpkinError),
    /// No matching server became ready before the deadline.
    #[error("Pumpkin startup timed out; inspect session/console.log")]
    StartupTimeout,
    /// The responding server is incompatible with this client.
    #[error("local server did not report protocol 776")]
    ProtocolMismatch,
    /// Another program already uses the selected port.
    #[error("local port is already in use; choose another --port")]
    PortInUse,
    /// The listener belongs to a different local server.
    #[error("local status response belongs to another server")]
    IdentityMismatch,
}

/// Start Pumpkin and wait for a complete status/pong exchange.
///
/// Failures drop and reap the child before releasing the world lock. The caller
/// must eventually call `Pumpkin::shutdown`; merely dropping it does not save.
pub async fn start(
    binary: &Path,
    session: &Path,
    port: u16,
    deadline: Duration,
) -> Result<(Pumpkin, StatusResult), LocalError> {
    // Fail before writing session state if a listener already owns this port.
    // Pumpkin remains the authoritative bind; it fails if another process wins
    // the unavoidable handoff race after this reservation is released.
    let reservation =
        std::net::TcpListener::bind(("127.0.0.1", port)).map_err(|_| LocalError::PortInUse)?;
    drop(reservation);
    let mut server = Pumpkin::launch(binary, session, port)?;
    let status = wait_ready(port, &server.identity(), deadline, || {
        if let Some(exit) = server.try_wait()? {
            return Err(PumpkinError::Exit(exit).into());
        }
        Ok(())
    })
    .await?;
    Ok((server, status))
}

async fn wait_ready(
    port: u16,
    identity: &str,
    deadline: Duration,
    mut check_child: impl FnMut() -> Result<(), LocalError>,
) -> Result<StatusResult, LocalError> {
    let started = Instant::now();
    loop {
        check_child()?;
        let remaining = deadline.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(LocalError::StartupTimeout);
        }
        if let Ok(status) =
            status::query("127.0.0.1", port, remaining.min(Duration::from_millis(500))).await
        {
            if status.json["version"]["protocol"].as_i64() != Some(i64::from(PROTOCOL_VERSION)) {
                return Err(LocalError::ProtocolMismatch);
            }
            let description = &status.json["description"];
            if description.as_str().or_else(|| description["text"].as_str()) != Some(identity) {
                return Err(LocalError::IdentityMismatch);
            }
            check_child()?;
            return Ok(status);
        }
        sleep(Duration::from_millis(50).min(deadline.saturating_sub(started.elapsed()))).await;
    }
}

#[cfg(test)]
mod tests;
