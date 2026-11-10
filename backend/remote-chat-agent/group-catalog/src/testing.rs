//! A clock the test sets.
use crate::ports::Clock;
use std::sync::atomic::{AtomicU64, Ordering};

pub struct SetClock(pub AtomicU64);

impl SetClock {
    pub fn at(ms: u64) -> SetClock {
        SetClock(AtomicU64::new(ms))
    }

    pub fn advance(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for SetClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}
