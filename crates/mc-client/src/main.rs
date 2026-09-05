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
        Command::Local { pumpkin, session, port, startup_seconds, check } => {
            // Install the handler before startup, so Ctrl-C also cancels startup
            // and drops any partially started child.
            let interrupted = tokio::signal::ctrl_c();
            tokio::pin!(interrupted);
            let startup = mc_client::local::start(
                &pumpkin,
                &session,
                port,
                Duration::from_secs(startup_seconds),
            );
            let result = tokio::select! {
                result = startup => result,
                result = &mut interrupted => {
                    eprintln!("Local startup interrupted: {result:?}");
                    return ExitCode::FAILURE;
                }
            };
            match result {
                Ok((mut server, status)) => {
                    println!("{}", status.json);
                    eprintln!(
                        "Pumpkin ready on 127.0.0.1:{}. Ctrl-C saves and stops.",
                        server.port()
                    );
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
                Err(error) => {
                    eprintln!("{error}");
                    ExitCode::FAILURE
                }
            }
        }
        Command::Status { host, port, timeout_ms } => {
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
    }
}
