//! The composition root: `main` fills [`Ports`] with the real adapters, the flow tests with doubles, and both get the same
//! router and the same sampling jobs.
use super::{app, AppState};
use axum::Router;
pub use reporting_core::application::Ports;
use reporting_core::application::{Intervals, Reporting, Sampler, Scheduler};
use std::sync::Arc;
use std::time::Instant;

pub struct App {
    router: Router,
    sampler: Arc<Sampler>,
}

impl App {
    pub fn new(ports: Ports) -> Self {
        let sampler = Arc::new(Sampler { store: ports.store.clone(), docker: ports.docker, host: ports.host, events_from: std::sync::atomic::AtomicI64::new(ports.clock.now_ms()), clock: ports.clock.clone(), policy: ports.policy, heartbeat: ports.heartbeat, publisher: ports.publisher, runner: ports.runner, runner_idle_since: std::sync::Mutex::new(None) });
        let reporting = Arc::new(Reporting { store: ports.store, plans: ports.plans, clock: ports.clock });
        let commit = ports.commit.unwrap_or_else(|| "unknown".into());
        Self { router: app(AppState { reporting, started: Instant::now(), commit }), sampler }
    }

    /// The read API with its middleware.
    pub fn router(&self) -> Router {
        self.router.clone()
    }

    /// The periodic sweeps; they run until the returned scheduler is stopped.
    pub fn start_sampling(&self, intervals: Intervals) -> Scheduler {
        Scheduler::start(self.sampler.clone(), intervals)
    }
}
