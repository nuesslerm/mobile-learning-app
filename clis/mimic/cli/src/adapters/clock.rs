use mimic_core::ports::{Clock, Rng};
use mimic_core::rng::SplitMix64;

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        wf_kit::time::now_ms().max(0) as u64
    }
}

/// A fresh stream per process: the clock's nanoseconds and the pid, for a target that makes things up on every run.
pub fn entropy() -> impl Rng {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    SplitMix64::new(nanos ^ (u64::from(std::process::id()) << 32))
}
