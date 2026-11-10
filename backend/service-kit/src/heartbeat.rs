//! One `health.heartbeat` event at boot and then every interval (10 minutes; 15 seconds while `DEBUG_MODE=true`), so a quiet
//! service still shows up at the log sink.

use std::time::{Duration, Instant};

const BYTES_PER_MB: usize = 1024 * 1024;

fn interval() -> Duration {
    if std::env::var("DEBUG_MODE").as_deref() == Ok("true") {
        Duration::from_secs(15)
    } else {
        Duration::from_secs(10 * 60)
    }
}

/// Must run inside the tokio runtime. `source` names the emitter, e.g. `leetcode-backend.heartbeat`.
pub fn spawn(source: &'static str) -> tokio::task::JoinHandle<()> {
    let started = Instant::now();
    tokio::spawn(async move {
        let mut ticks = tokio::time::interval(interval());
        loop {
            ticks.tick().await;
            let rss_mb = memory_stats::memory_stats().map_or(0, |usage| usage.physical_mem / BYTES_PER_MB);
            tracing::info!(source, uptimeSec = started.elapsed().as_secs(), rssMb = rss_mb as u64, ok = true, "health.heartbeat");
        }
    })
}
