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
    /// Show pinned manifest + hashes (placeholder; does not fetch or verify).
    VerifyManifest {
        #[arg(long, default_value = "26.2", value_parser = ["26.2"])]
        version: String,
    },
    /// Show cache download instructions (placeholder; does not download).
    FetchReference {
        #[arg(long, default_value = "26.2", value_parser = ["26.2"])]
        version: String,
        #[arg(long, default_value = "client", value_parser = ["client", "server"])]
        side: String,
        #[arg(long)]
        cache: Option<PathBuf>,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::VerifyManifest { version } => {
            println!("Pinned reference for {version} (no verification performed):");
            println!("manifest: {}", mc_launcher::VERSION_26_2_URL);
            println!("client sha1: {}", mc_launcher::CLIENT_SHA1_26_2);
            println!("server sha1: {}", mc_launcher::SERVER_SHA1_26_2);
            println!(
                "See docs/SOURCE_OF_TRUTH.md. Run scripts/fetch-26.2.sh to download to cache."
            );
        }
        Cmd::FetchReference { version, side, cache } => {
            let dir = cache
                .or_else(|| {
                    std::env::var_os("XDG_CACHE_HOME")
                        .or_else(|| std::env::var_os("LOCALAPPDATA"))
                        .map(PathBuf::from)
                        .or_else(|| {
                            std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".cache"))
                        })
                        .map(|base| base.join("mc-rust-client").join(version))
                })
                .ok_or("No cache directory available; provide --cache")?;
            println!("cache dir: {} (side: {})", dir.display(), side);
            println!(
                "Use scripts/fetch-26.2.sh; full async downloader lands with mc-launcher (P1)."
            );
        }
    }
    Ok(())
}
