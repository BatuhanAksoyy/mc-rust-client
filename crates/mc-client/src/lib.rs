// SPDX-License-Identifier: MIT OR Apache-2.0
//! Game orchestration: fixed 20 TPS tick + interpolated render. See `docs/WORLD_PHYSICS_ASSETS.md`.

pub mod status;

/// Ticks per second — vanilla parity.
pub const TPS: u32 = 20;
/// Nanoseconds per tick.
pub const TICK_NANOS: u64 = 1_000_000_000 / TPS as u64;
