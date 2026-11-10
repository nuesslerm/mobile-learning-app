use crate::application::ports::{Clock, Docker, Event, Events};
use crate::domain::decide::{compose_args, decide, exited, Declared, History};
use crate::domain::snapshot::Snapshot;
use std::sync::Arc;
use std::time::Duration;

pub struct Watchdog {
    pub docker: Arc<dyn Docker>,
    pub events: Arc<dyn Events>,
    pub clock: Arc<dyn Clock>,
}

impl Watchdog {
    /// One sweep; returns the next history. Failures are reported as events and never raised; a failed sweep keeps the history.
    pub async fn tick(&self, history: History) -> History {
        let snapshots = match self.snapshots().await {
            Ok(snapshots) => snapshots,
            Err(error) => {
                self.events.emit(Event::SweepFailed { error }).await;
                return history;
            }
        };
        let now = self.clock.now_ms();
        let declared = self.declared(&snapshots, now).await;
        let (actions, next) = decide(&snapshots, &history, &declared, now);
        for action in actions {
            let outcome = self.docker.compose(&action.recreate_args()).await;
            self.events.emit(Event::Recreated { container: action.container, reason: action.reason.as_str(), ok: outcome.ok, error: outcome.error }).await;
        }
        next
    }

    /// What the compose file of each project with a long-stopped control-plane container declares. A project whose file cannot be
    /// read declares nothing, so its stopped containers are left alone, and the sweep says so.
    async fn declared(&self, snapshots: &[Snapshot], now: i64) -> Declared {
        let mut declared = Declared::new();
        for stopped in exited(snapshots, now) {
            let Some(project) = stopped.project.as_deref().filter(|project| !declared.contains_key(*project)) else { continue };
            let args = compose_args(project, stopped.working_dir.as_deref(), &stopped.config_files, &["config", "--services"]);
            let services = match self.docker.services(&args).await {
                Ok(services) => services,
                Err(error) => {
                    self.events.emit(Event::SweepFailed { error: error.to_string() }).await;
                    Vec::new()
                }
            };
            declared.insert(project.to_string(), services.into_iter().collect());
        }
        declared
    }

    async fn snapshots(&self) -> Result<Vec<Snapshot>, String> {
        let ids = self.docker.container_ids().await.map_err(|e| e.to_string())?;
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        self.docker.inspect(&ids).await.map_err(|e| e.to_string())
    }

    /// Says it started, then sweeps now and every `interval` after each sweep ends, until the future is dropped.
    pub async fn run(&self, interval: Duration) {
        self.events.emit(Event::Started).await;
        let mut history = History::new();
        loop {
            history = self.tick(history).await;
            tokio::time::sleep(interval).await;
        }
    }
}
