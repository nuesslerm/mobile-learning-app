//! Doubles of the ports: a clock that moves when told and counting ids.
use crate::ports::{Clock, Ids};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
pub struct FakeClock(AtomicU64);

impl FakeClock {
    pub fn at(ms: u64) -> FakeClock {
        FakeClock(AtomicU64::new(ms))
    }

    pub fn advance(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for FakeClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Default)]
pub struct CountingIds(AtomicU64);

impl Ids for CountingIds {
    fn id(&self, prefix: &str) -> String {
        format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}
