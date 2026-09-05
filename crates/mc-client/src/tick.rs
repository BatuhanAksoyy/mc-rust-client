//! Deterministic 20 TPS scheduling, independent of wall clock, I/O, and rendering.
//!
//! Call `advance` once per rendered frame, execute the returned tick count, then
//! interpolate the previous/current simulation states by `alpha`. The caller
//! owns those states. See `docs/WORLD_PHYSICS_ASSETS.md` for overload policy.

use std::{num::NonZeroU32, time::Duration};

use crate::TICK_NANOS;

/// Simulation work and interpolation for a single rendered frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TickBatch {
    /// Whole fixed ticks to execute before rendering.
    pub ticks: u32,
    /// Fraction of a tick left over, always in [0, 1).
    pub alpha: f64,
    /// Whole-tick time discarded to bound catch-up work.
    pub dropped: Duration,
}

/// Fixed timestep accumulator with a caller-selected catch-up limit.
#[derive(Debug, Clone)]
pub struct TickScheduler {
    max_catch_up: NonZeroU32,
    remainder_nanos: u32,
}

impl TickScheduler {
    /// Create a scheduler with no accumulated time.
    #[must_use]
    pub const fn new(max_catch_up: NonZeroU32) -> Self {
        Self { max_catch_up, remainder_nanos: 0 }
    }

    /// Accumulate elapsed time and report bounded simulation work.
    ///
    /// Excess whole ticks are dropped, preserving the sub-tick remainder.
    /// Integer arithmetic avoids accumulated floating-point drift, even for
    /// `Duration::MAX`. Calling with zero duration does not advance simulation.
    #[must_use]
    pub fn advance(&mut self, elapsed: Duration) -> TickBatch {
        let total = elapsed.as_nanos() + u128::from(self.remainder_nanos);
        let tick_nanos = u128::from(TICK_NANOS);
        let due = total / tick_nanos;
        let ticks = u32::try_from(due).unwrap_or(u32::MAX).min(self.max_catch_up.get());
        self.remainder_nanos = u32::try_from(total % tick_nanos)
            .expect("a sub-tick remainder is less than 50 million nanoseconds");
        let dropped_nanos = (due - u128::from(ticks)) * tick_nanos;
        // If dropping time, at least one tick is executed, so the dropped
        // duration is no greater than elapsed and always fits Duration.
        let dropped = Duration::new(
            u64::try_from(dropped_nanos / 1_000_000_000).expect("dropped seconds fit Duration"),
            u32::try_from(dropped_nanos % 1_000_000_000).expect("sub-second nanos fit u32"),
        );
        TickBatch {
            ticks,
            alpha: Duration::from_nanos(u64::from(self.remainder_nanos))
                .div_duration_f64(Duration::from_nanos(TICK_NANOS)),
            dropped,
        }
    }
}
