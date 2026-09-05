// SPDX-License-Identifier: MIT OR Apache-2.0
//! Headless client entry point. Start with `mc-client status --help`.

use std::{path::PathBuf, process::ExitCode, time::Duration};

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(about = "Minecraft protocol 776 headless client")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Probe offline login through Play Login, print context, then disconnect.
    Join {
        /// Offline server hostname or IP address.
        #[arg(default_value = "localhost")]
        host: String,
        /// Server TCP port.
        #[arg(long, default_value_t = 25565, value_parser = clap::value_parser!(u16).range(1..))]
        port: u16,
        /// Development username (1–16 ASCII letters, digits or underscores).
        #[arg(long, default_value = "RustPlayer")]
        name: String,
        /// Overall deadline through Play Login, in milliseconds.
        #[arg(long, default_value_t = 30_000, value_parser = clap::value_parser!(u64).range(1..=300_000))]
        timeout_ms: u64,
    },
    /// Start a managed local Pumpkin server (headless; gameplay client follows).
    Local {
        /// Path to the pinned Pumpkin executable.
        #[arg(long)]
        pumpkin: PathBuf,
        /// New or previously managed session directory outside any Git checkout.
        #[arg(long)]
        session: PathBuf,
        /// Local TCP port.
        #[arg(long, default_value_t = 25565, value_parser = clap::value_parser!(u16).range(1..))]
        port: u16,
        /// Maximum time to wait for server readiness.
        #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u64).range(1..=600))]
        startup_seconds: u64,
        /// Stop after verifying readiness instead of waiting for Ctrl-C.
        #[arg(long)]
        check: bool,
    },
    /// Join, take the first chunk received before spawn, and render it in a window.
    Render {
        /// Offline server hostname or IP address.
        #[arg(default_value = "localhost")]
        host: String,
        /// Server TCP port.
        #[arg(long, default_value_t = 25565, value_parser = clap::value_parser!(u16).range(1..))]
        port: u16,
        /// Development username (1–16 ASCII letters, digits or underscores).
        #[arg(long, default_value = "RustPlayer")]
        name: String,
        /// Overall join deadline, in milliseconds.
        #[arg(long, default_value_t = 30_000, value_parser = clap::value_parser!(u64).range(1..=300_000))]
        timeout_ms: u64,
    },
    /// Query server status and measure ping latency without logging in.
    Status {
        /// Hostname or IP address (IPv6 addresses do not need brackets).
        #[arg(default_value = "localhost")]
        host: String,
        /// TCP port, also sent in the handshake.
        #[arg(long, default_value_t = 25565, value_parser = clap::value_parser!(u16).range(1..))]
        port: u16,
        /// Overall timeout in milliseconds, including DNS and connect.
        #[arg(long, default_value_t = 5000, value_parser = clap::value_parser!(u64).range(1..=300_000))]
        timeout_ms: u64,
    },
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Join { host, port, name, timeout_ms } => {
            run_join(host, port, name, timeout_ms).await
        }
        Command::Local { pumpkin, session, port, startup_seconds, check } => {
            run_local(pumpkin, session, port, startup_seconds, check).await
        }
        Command::Render { host, port, name, timeout_ms } => {
            run_render(host, port, name, timeout_ms).await
        }
        Command::Status { host, port, timeout_ms } => run_status(host, port, timeout_ms).await,
    }
}

async fn run_join(host: String, port: u16, name: String, timeout_ms: u64) -> ExitCode {
    match mc_client::join::connect(&host, port, &name, Duration::from_millis(timeout_ms)).await {
        Ok(joined) => {
            println!(
                "Reached spawn: player={}, entity={}, dimension={}, registries={}, entries={}, \
                 position=({:.1}, {:.1}, {:.1})",
                joined.profile.name,
                joined.world.entity_id,
                joined.world.dimension_name,
                joined.registries.len(),
                joined.registries.entry_count(),
                joined.spawn.x,
                joined.spawn.y,
                joined.spawn.z
            );
            eprintln!(
                "Headless join probe complete; disconnecting. Rendering/gameplay are not implemented yet."
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run_local(
    pumpkin: PathBuf,
    session: PathBuf,
    port: u16,
    startup_seconds: u64,
    check: bool,
) -> ExitCode {
    // Install the handler before startup, so Ctrl-C also cancels startup
    // and drops any partially started child.
    let interrupted = tokio::signal::ctrl_c();
    tokio::pin!(interrupted);
    let startup =
        mc_client::local::start(&pumpkin, &session, port, Duration::from_secs(startup_seconds));
    let result = tokio::select! {
        result = startup => result,
        result = &mut interrupted => {
            eprintln!("Local startup interrupted: {result:?}");
            return ExitCode::FAILURE;
        }
    };
    let (mut server, status) = match result {
        Ok(started) => started,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    println!("{}", status.json);
    eprintln!("Pumpkin ready on 127.0.0.1:{}. Ctrl-C saves and stops.", server.port());
    if !check {
        loop {
            tokio::select! {
                result = &mut interrupted => {
                    if let Err(error) = result { eprintln!("Signal handler failed: {error}"); }
                    break;
                }
                () = tokio::time::sleep(Duration::from_millis(100)) => {
                    match server.try_wait() {
                        Ok(None) => {},
                        result => {
                            eprintln!("Pumpkin stopped unexpectedly: {result:?}");
                            return ExitCode::FAILURE;
                        }
                    }
                }
            }
        }
    }
    match server.shutdown(Duration::from_secs(30)).await {
        Ok(()) => {
            eprintln!("Pumpkin stopped cleanly.");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// Join, render the first received chunk, and block until the window closes.
///
/// `mc_render::run` is a blocking, synchronous call (`winit` requires the
/// platform's main thread on macOS); calling it here is safe only because
/// nothing else is scheduled on this single-threaded runtime by the time we
/// reach it — the join future has already resolved.
async fn run_render(host: String, port: u16, name: String, timeout_ms: u64) -> ExitCode {
    let joined =
        match mc_client::join::connect(&host, port, &name, Duration::from_millis(timeout_ms)).await
        {
            Ok(joined) => joined,
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        };
    let Some(level_chunk) = joined.chunks.first() else {
        eprintln!(
            "reached spawn but received no chunk beforehand; nothing to render (try again, or a larger view distance)"
        );
        return ExitCode::FAILURE;
    };
    let registry = mc_world::BlockRegistry::load_cached("26.2");
    let chunk = mc_world::Chunk::from_level(level_chunk);
    let mesh = mc_render::mesh::mesh_chunk(&chunk, &registry);
    eprintln!(
        "Rendering chunk ({}, {}): {} sections, {} vertices. Close the window to exit.",
        chunk.position.x,
        chunk.position.z,
        chunk.section_count(),
        mesh.vertices.len()
    );
    #[allow(clippy::cast_precision_loss)] // A chunk's height in blocks is tiny.
    let height = (chunk.section_count() * 16) as f32;
    let target = glam::Vec3::new(8.0, height * 0.5, 8.0);
    let radius = height.max(32.0) * 1.2;
    match mc_render::run(mesh, "mc-rust-client", target, radius) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run_status(host: String, port: u16, timeout_ms: u64) -> ExitCode {
    match mc_client::status::query(&host, port, Duration::from_millis(timeout_ms)).await {
        Ok(result) => {
            println!("{}", result.json);
            eprintln!("Ping: {:.2} ms", result.latency.as_secs_f64() * 1000.0);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
