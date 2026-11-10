//! Five periodic jobs (the runner one only with a runner to watch). A job whose previous run is still going skips its tick (and says so) instead of piling up.
use crate::application::sampler::Sampler;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::{interval_at, Instant, MissedTickBehavior};

/// How late a tick may fire before it is reported.
const LATE_TICK: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy)]
pub struct Intervals {
    pub containers: Duration,
    pub host: Duration,
    pub runner: Duration,
    pub storage: Duration,
    pub prune: Duration,
}

impl Default for Intervals {
    fn default() -> Self {
        Self {
            containers: Duration::from_secs(15),
            host: Duration::from_secs(15),
            runner: Duration::from_secs(15),
            storage: Duration::from_secs(15 * 60),
            prune: Duration::from_secs(6 * 3600),
        }
    }
}

pub struct Scheduler {
    handles: Vec<JoinHandle<()>>,
}

fn job<F, Fut>(label: &'static str, every: Duration, run: F) -> JoinHandle<()>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<(), crate::application::PortError>> + Send + 'static,
{
    let run = Arc::new(run);
    let in_flight = Arc::new(AtomicBool::new(false));
    tokio::spawn(async move {
        // The first run is one interval after start, like a timer, not at start.
        let mut ticks = interval_at(Instant::now() + every, every);
        ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            let due = ticks.tick().await;
            // A tick that fires long after it was due means the whole host (or this process) was stalled.
            let late = Instant::now().saturating_duration_since(due);
            if late >= LATE_TICK {
                tracing::warn!(job = label, late_ms = late.as_millis() as u64, "scheduler.tick_late");
            }
            if in_flight.swap(true, Ordering::AcqRel) {
                tracing::warn!(job = label, "scheduler.tick_skipped");
                continue;
            }
            let (run, in_flight) = (run.clone(), in_flight.clone());
            tokio::spawn(async move {
                if let Err(error) = run().await {
                    tracing::error!(job = label, error = %error, "scheduler.job_failed");
                }
                in_flight.store(false, Ordering::Release);
            });
        }
    })
}

impl Scheduler {
    pub fn start(sampler: Arc<Sampler>, intervals: Intervals) -> Self {
        let containers = sampler.clone();
        let host = sampler.clone();
        let storage = sampler.clone();
        let runner = sampler.clone();
        let prune = sampler;
        let mut handles = vec![
            job("containers", intervals.containers, move || {
                let sampler = containers.clone();
                async move {
                    let samples = sampler.containers(sampler.now()).await;
                    sampler.store(samples).await
                }
            }),
            job("host", intervals.host, move || {
                let sampler = host.clone();
                async move {
                    let samples = sampler.host(sampler.now()).await;
                    sampler.store(samples).await
                }
            }),
            job("storage", intervals.storage, move || {
                let sampler = storage.clone();
                async move {
                    let samples = sampler.storage(sampler.now()).await;
                    sampler.store(samples).await
                }
            }),
            job("prune", intervals.prune, move || {
                let sampler = prune.clone();
                async move { sampler.prune().await.map(|_| ()) }
            }),
        ];
        if runner.runner.is_some() {
            handles.push(job("runner", intervals.runner, move || {
                let sampler = runner.clone();
                async move {
                    let samples = sampler.runner(sampler.now()).await;
                    sampler.store(samples).await
                }
            }));
        }
        Self { handles }
    }

    pub fn stop(self) {
        for handle in self.handles {
            handle.abort();
        }
    }
}
