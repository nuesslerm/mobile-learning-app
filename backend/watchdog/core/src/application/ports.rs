use crate::domain::snapshot::Snapshot;
use async_trait::async_trait;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct PortError(pub String);

/// The outcome of one `docker compose` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recreated {
    pub ok: bool,
    /// The last 500 characters of what the command wrote to stderr, trimmed (only when it failed).
    pub error: Option<String>,
}

#[async_trait]
pub trait Docker: Send + Sync {
    /// The ids of every container, running or not.
    async fn container_ids(&self) -> Result<Vec<String>, PortError>;
    async fn inspect(&self, ids: &[String]) -> Result<Vec<Snapshot>, PortError>;
    async fn compose(&self, args: &[String]) -> Recreated;
    /// The services a project's compose file declares: the lines `docker compose ... config --services` printed.
    async fn services(&self, args: &[String]) -> Result<Vec<String>, PortError>;
}

/// What the watchdog tells the world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Started,
    SweepFailed { error: String },
    Recreated { container: String, reason: &'static str, ok: bool, error: Option<String> },
}

#[async_trait]
pub trait Events: Send + Sync {
    async fn emit(&self, event: Event);
}

pub trait Clock: Send + Sync {
    fn now_ms(&self) -> i64;
}
