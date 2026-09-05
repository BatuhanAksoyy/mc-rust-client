//! Deterministic scheduling tests: no sleeps, renderer, or network.
use std::{num::NonZeroU32, time::Duration};

use mc_client::{TICK_NANOS, tick::TickScheduler};

const fn scheduler(max: u32) -> TickScheduler {
    TickScheduler::new(NonZeroU32::new(max).unwrap())
}

#[test]
fn exact_tick_boundary_and_fractional_interpolation() {
    let mut scheduler = scheduler(4);
    let first = scheduler.advance(Duration::from_millis(25));
    assert_eq!(first.ticks, 0);
    assert!((first.alpha - 0.5).abs() < f64::EPSILON);
    let second = scheduler.advance(Duration::from_millis(25));
    assert_eq!(second.ticks, 1);
    assert!(second.alpha.abs() < f64::EPSILON);
    assert_eq!(second.dropped, Duration::ZERO);
    assert_eq!(scheduler.advance(Duration::ZERO).ticks, 0);
}

#[test]
fn catch_up_drops_only_whole_ticks_and_preserves_remainder() {
    let mut scheduler = scheduler(3);
    let batch = scheduler.advance(Duration::from_millis(525));
    assert_eq!(batch.ticks, 3);
    assert_eq!(batch.dropped, Duration::from_millis(350));
    assert!((batch.alpha - 0.5).abs() < f64::EPSILON);
    assert_eq!(scheduler.advance(Duration::from_millis(25)).ticks, 1);
}

#[test]
fn elapsed_partitioning_has_no_drift() {
    let mut scheduler = scheduler(20);
    let mut ticks = 0;
    for _ in 0..10_000 {
        ticks += scheduler.advance(Duration::from_millis(1)).ticks;
    }
    assert_eq!(ticks, 200);
    let mut scheduler = self::scheduler(20);
    let mut ticks = 0;
    for _ in 0..60 {
        ticks += scheduler.advance(Duration::from_nanos(16_666_667)).ticks;
    }
    assert_eq!(ticks, 20);
    assert_eq!(scheduler.advance(Duration::from_nanos(TICK_NANOS - 20)).ticks, 1);
}

#[test]
fn extreme_elapsed_time_remains_bounded_and_conserves_time() {
    let mut scheduler = scheduler(1);
    let _ = scheduler.advance(Duration::from_nanos(TICK_NANOS - 1));
    let batch = scheduler.advance(Duration::MAX);
    assert_eq!(batch.ticks, 1);
    assert!((0.0..1.0).contains(&batch.alpha));
    let total = Duration::MAX.as_nanos() + u128::from(TICK_NANOS - 1);
    let remainder = total % u128::from(TICK_NANOS);
    assert_eq!(batch.dropped.as_nanos() + u128::from(TICK_NANOS) + remainder, total);
}
