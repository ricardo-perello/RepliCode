// runtime/src/runtime/clock.rs
use std::sync::atomic::{AtomicU64, Ordering};

pub struct GlobalClock;

static CLOCK: AtomicU64 = AtomicU64::new(0);

impl GlobalClock {
    /// Returns the current simulation time (in nanoseconds, for example).
    pub fn now() -> u64 {
        CLOCK.load(Ordering::SeqCst)
    }

    /// Increments the clock by `delta` units.
    pub fn increment(delta: u64) {
        CLOCK.fetch_add(delta, Ordering::SeqCst);
    }
}
