use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

use sha2::{Digest, Sha256};

use super::PumpkinError;

/// Pinned upstream release, targeting Minecraft 26.2.
pub const RELEASE: &str = "0.1.0-dev+26.2-26.45";

/// Executable identity from the pinned GitHub release asset metadata.
#[derive(Debug, Clone, Copy)]
pub struct ReleaseAsset {
    /// Upstream executable filename.
    pub name: &'static str,
    /// SHA-256 digest from the release metadata retrieved 2026-09-06.
    pub sha256: &'static str,
}

/// Select a release asset by Rust OS and architecture names.
#[must_use]
pub fn release_asset(os: &str, arch: &str) -> Option<ReleaseAsset> {
    let (name, sha256) = match (os, arch) {
        ("macos", "aarch64") => (
            "pumpkin-ARM64-macOS",
            "e9844f15be101c0012c8fae99385fc40f859e5cd1293788654bebb7069e93aad",
        ),
        ("linux", "x86_64") => (
            "pumpkin-X64-Linux",
            "565db1641b229d6a311d299f7a28c7e2d27b2f572480cfed36b930b0ae89ac11",
        ),
        ("linux", "aarch64") => (
            "pumpkin-ARM64-Linux",
            "76cd4c96d422df95bf02d05bfcf3725c2e7aeb53b7f02c1853f6dcd7edc013b5",
        ),
        ("windows", "x86_64") => (
            "pumpkin-X64-Windows.exe",
            "a4eac6de473bd3f379516fba9cfc39a81cccb58af59883e923947ae2cfa0b119",
        ),
        ("windows", "aarch64") => (
            "pumpkin-ARM64-Windows.exe",
            "4ee818cfbcf06a80a1b51f3921920c6e2a79ca4a8f35bdecbf7842855acb36b1",
        ),
        _ => return None,
    };
    Some(ReleaseAsset { name, sha256 })
}

pub(super) fn verify(binary: &Path) -> Result<(), PumpkinError> {
    let asset = release_asset(std::env::consts::OS, std::env::consts::ARCH)
        .ok_or(PumpkinError::UnsupportedPlatform)?;
    let mut file = File::open(binary)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "executable must be a file").into());
    }
    let mut hash = Sha256::new();
    let mut buffer = [0; 8192];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    let digest = hash.finalize();
    if !digest
        .iter()
        .enumerate()
        .all(|(i, byte)| u8::from_str_radix(&asset.sha256[i * 2..i * 2 + 2], 16) == Ok(*byte))
    {
        return Err(PumpkinError::HashMismatch);
    }
    Ok(())
}
