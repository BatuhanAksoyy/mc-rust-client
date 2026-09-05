// SPDX-License-Identifier: MIT OR Apache-2.0
//! Microsoft/Xbox/Minecraft auth + ownership gate. See `docs/AUTH.md`.

use zeroize::Zeroize;

/// Ownership outcome used by the future authentication flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ownership {
    /// A permanent game entitlement.
    FullGame,
    /// A profile exists through a subscription.
    GamePassProfileOnly,
    /// No entitlement was found.
    None,
}

/// Secret string that is never logged.
#[derive(Clone)]
pub struct RedactedString(String);

impl RedactedString {
    /// Wrap a secret and erase its owned buffer on drop.
    #[must_use]
    pub const fn new(s: String) -> Self {
        Self(s)
    }
    /// Explicitly borrow the secret; callers must never log this value.
    #[must_use]
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
