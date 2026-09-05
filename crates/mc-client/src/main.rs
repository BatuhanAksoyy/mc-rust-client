// SPDX-License-Identifier: MIT OR Apache-2.0
//! Headless client entry point. Start with `mc-client status --help`.

use std::{process::ExitCode, time::Duration};

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(about = "Minecraft protocol 776 headless client")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
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
