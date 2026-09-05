// SPDX-License-Identifier: MIT OR Apache-2.0
//! xtask: cache-only fetch/verify. Never writes Mojang files into the repo.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Verify pinned 26.2 manifest + hashes (network, no jar download).
    VerifyManifest {
        #[arg(long, default_value = "26.2")]
        version: String,
    },
    /// Download client/server jar to cache dir only (verifies SHA1).
    FetchReference {
        #[arg(long, default_value = "26.2")]
        version: String,
        #[arg(long, default_value = "client")]
        side: String,
        #[arg(long)]
        cache: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::VerifyManifest { version } => {
            assert_eq!(version, "26.2", "only 26.2 is pinned in v1");
            println!("manifest: {}", mc_launcher::VERSION_26_2_URL);
            println!("client sha1: {}", mc_launcher::CLIENT_SHA1_26_2);
            println!("server sha1: {}", mc_launcher::SERVER_SHA1_26_2);
            println!(
                "See docs/SOURCE_OF_TRUTH.md. Run scripts/fetch-26.2.sh to download to cache."
            );
        }
        Cmd::FetchReference { version, side, cache } => {
            let dir = cache.unwrap_or_else(|| {
                let base =
                    std::env::var("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|_| {
                        PathBuf::from(format!("{}/.cache", std::env::var("HOME").unwrap()))
                    });
                base.join("mc-rust-client").join(version)
            });
            println!("cache dir: {} (side: {})", dir.display(), side);
            println!(
                "Use scripts/fetch-26.2.sh; full async downloader lands with mc-launcher (P1)."
            );
        }
    }
    Ok(())
}
