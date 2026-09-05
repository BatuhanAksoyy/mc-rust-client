// SPDX-License-Identifier: MIT OR Apache-2.0
//! Microsoft/Xbox/Minecraft auth + ownership gate. See `docs/AUTH.md`.

use zeroize::Zeroize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ownership {
    FullGame,
    GamePassProfileOnly,
    None,
}

/// Secret string that is never logged.
#[derive(Clone)]
pub struct RedactedString(String);

impl RedactedString {
    pub fn new(s: String) -> Self {
        Self(s)
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Drop for RedactedString {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl std::fmt::Debug for RedactedString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RedactedString(REDACTED)")
    }
}
