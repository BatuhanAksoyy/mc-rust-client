//! Managed external Pumpkin runtime. See `docs/SINGLEPLAYER.md` (protocol 776).
//!
//! No Pumpkin implementation or assets are linked into this crate.

mod release;
mod session;
#[cfg(test)]
mod tests;

use std::{
    fs::File,
    io::{self, Write},
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    time::Duration,
};

use thiserror::Error;
use tokio::time::{Instant, sleep};

pub use release::{RELEASE, ReleaseAsset, release_asset};

/// Failure to prepare, launch, or stop the managed server.
#[derive(Debug, Error)]
pub enum PumpkinError {
    /// Filesystem or process operation failed.
    #[error("Pumpkin I/O error: {0}")]
    Io(#[from] io::Error),
    /// No executable is pinned for this platform.
    #[error("no pinned Pumpkin executable for this platform")]
    UnsupportedPlatform,
    /// The executable did not match the pinned release.
    #[error("Pumpkin executable SHA-256 does not match the pinned release")]
    HashMismatch,
    /// Managed sessions cannot adopt existing directories or live in repositories.
    #[error("unsafe or unmanaged session directory: {0}")]
    InvalidSession(&'static str),
    /// A second managed process already owns this session.
    #[error("this Pumpkin session is already in use")]
    SessionBusy,
    /// The child exited unsuccessfully.
    #[error("Pumpkin exited with {0}")]
    Exit(ExitStatus),
    /// Saving did not finish before the shutdown deadline.
    #[error("Pumpkin shutdown timed out; process was terminated, save is not confirmed")]
    ShutdownTimeout,
}

/// Owns one server process and its exclusive session lock.
///
/// Call `shutdown` to save. Drop kills and reaps as emergency cleanup; it does
/// not save. The lock is retained until the child is reaped.
pub struct Pumpkin {
    child: Child,
    _session_lock: File,
    port: u16,
}

impl Pumpkin {
    /// Verify the pinned binary and launch in a new or previously managed session.
    ///
    /// This performs blocking filesystem/hash work; GUI callers should run startup
    /// off their render thread. The session parent must already exist.
    pub fn launch(binary: &Path, directory: &Path, port: u16) -> Result<Self, PumpkinError> {
        if port == 0 {
            return Err(PumpkinError::InvalidSession("port must be nonzero"));
        }
        let binary = binary.canonicalize()?;
        release::verify(&binary)?;
        let (directory, lock) = session::prepare(directory, port)?;
        Self::spawn(Command::new(binary), &directory, lock, port)
    }

    fn spawn(
        mut command: Command,
        directory: &Path,
        lock: File,
        port: u16,
    ) -> Result<Self, PumpkinError> {
        // Ctrl-C belongs to the client. It requests an orderly console stop;
        // the server must not simultaneously receive the terminal interrupt.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            command.creation_flags(CREATE_NEW_PROCESS_GROUP);
        }
        let log = session::managed_file(directory, "console.log")?;
        log.set_len(0)?;
        let child = command
            .current_dir(directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .spawn()?;
        Ok(Self { child, _session_lock: lock, port })
    }

    /// Local TCP port used for the Java Edition protocol.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// Expected status description identifying this managed launch.
    ///
    /// This disambiguates accidental port collisions; it is not authentication.
    #[must_use]
    pub fn identity(&self) -> String {
        session::identity(self.port)
    }

    /// Check whether the server has exited, reaping it if necessary.
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>, PumpkinError> {
        Ok(self.child.try_wait()?)
    }

    /// Ask Pumpkin to save and stop, then wait for exit within the deadline.
    ///
    /// An expired deadline forces termination and returns an error. Cancelling
    /// this future drops the owner, also triggering emergency termination.
    pub async fn shutdown(mut self, deadline: Duration) -> Result<(), PumpkinError> {
        if let Some(status) = self.child.try_wait()? {
            return Err(PumpkinError::Exit(status));
        }
        let mut stdin =
            self.child.stdin.take().ok_or_else(|| {
                io::Error::new(io::ErrorKind::BrokenPipe, "missing Pumpkin console")
            })?;
        stdin.write_all(b"stop\n")?;
        stdin.flush()?;
        // Keep stdin open until the command has completed; an early EOF must not
        // race the server's asynchronous console command handler.
        let started = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait()? {
                return if status.success() { Ok(()) } else { Err(PumpkinError::Exit(status)) };
            }
            if started.elapsed() >= deadline {
                return Err(PumpkinError::ShutdownTimeout);
            }
            sleep(Duration::from_millis(25)).await;
        }
    }
}

impl Drop for Pumpkin {
    fn drop(&mut self) {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
