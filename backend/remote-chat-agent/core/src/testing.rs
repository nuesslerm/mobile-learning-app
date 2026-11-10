//! The doubles of the core's ports for tests: a clock that stands still, ids that count, and opencode, the programs and the outbound HTTP services.

mod doubles;
mod files;
pub use doubles::*;
pub use files::MemoryFiles;

use crate::domain::ids::{Clock, IdSource};
use std::sync::atomic::{AtomicU64, Ordering};

/// A clock stopped at one instant.
pub struct FixedClock(pub u64);

impl Clock for FixedClock {
    fn now_ms(&self) -> u64 {
        self.0
    }
}

/// Ids that count from 1: `00000000-0000-4000-8000-000000000001` for a UUID, the number in hex for the rest.
#[derive(Default)]
pub struct CountingIds(AtomicU64);

impl CountingIds {
    fn next(&self) -> u64 {
        self.0.fetch_add(1, Ordering::SeqCst) + 1
    }
}

impl IdSource for CountingIds {
    fn uuid(&self) -> String {
        format!("00000000-0000-4000-8000-{:012x}", self.next())
    }

    fn hex(&self, n: usize) -> String {
        format!("{:0width$x}", self.next(), width = 2 * n.min(16))
    }
}
