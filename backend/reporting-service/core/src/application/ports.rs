//! What the use cases need from the outside: storage, the Docker Engine, `/proc` and the repository files.
use crate::domain::container::{ContainerStats, ContainerSummary, DiskContainer};
use crate::domain::events::DockerEvent;
use crate::domain::plans::{DispatchRun, PlanManifest, Pricing};
use crate::domain::sample::{Sample, SeriesPoint, SeriesQuery};
use async_trait::async_trait;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct PortError(pub String);

impl PortError {
    pub fn new(message: impl std::fmt::Display) -> Self {
        Self(message.to_string())
    }
}

#[async_trait]
pub trait MetricsStore: Send + Sync {
    async fn insert_samples(&self, samples: Vec<Sample>) -> Result<(), PortError>;
    /// The points at or after `now - window_ms`, ordered by time: raw for the shortest window, averages per minute or hour for the longer ones (`window::granularity_for`).
    /// With a query, only the points of its scope and metrics, and none before its `since_ms`.
    async fn series_for(&self, window_ms: i64, now: i64, query: &SeriesQuery) -> Result<Vec<SeriesPoint>, PortError>;
    /// The newest sample of every series.
    async fn latest(&self) -> Result<Vec<Sample>, PortError>;
    /// Rows stored.
    async fn row_count(&self) -> Result<u64, PortError>;
    /// Bytes the stored metrics (and whatever shares their database) occupy.
    async fn used_bytes(&self) -> Result<u64, PortError>;
    /// Deletes about the `rows` oldest samples (all those at the timestamp of the last one) and gives their space back; returns how many went.
    async fn delete_oldest(&self, rows: u64) -> Result<u64, PortError>;
}

#[async_trait]
pub trait DockerEngine: Send + Sync {
    async fn list_containers(&self) -> Result<Vec<ContainerSummary>, PortError>;
    async fn container_stats(&self, id: &str) -> Result<ContainerStats, PortError>;
    /// Per-container disk use (`/system/df?verbose=1`).
    async fn system_df(&self) -> Result<Vec<DiskContainer>, PortError>;
    /// The container `oom` and `die` events between two instants (seconds).
    async fn events(&self, since_s: i64, until_s: i64) -> Result<Vec<DockerEvent>, PortError>;
}

/// A dead man's switch outside the VM: told after every host sweep that the host is still being sampled; the one that listens alerts when
/// the telling stops, which is how a stalled or dead VM is noticed.
#[async_trait]
pub trait Heartbeat: Send + Sync {
    async fn beat(&self);
}

/// No heartbeat configured.
pub struct NoHeartbeat;

#[async_trait]
impl Heartbeat for NoHeartbeat {
    async fn beat(&self) {}
}

/// The live copy of what a sweep stored, for a sink that alerts. Infallible by design: an adapter that cannot deliver counts and logs
/// that itself, because a sweep never fails or waits on it.
#[async_trait]
pub trait SamplePublisher: Send + Sync {
    async fn publish(&self, samples: &[Sample]);
}

/// No sink configured.
pub struct NoPublisher;

#[async_trait]
impl SamplePublisher for NoPublisher {
    async fn publish(&self, _samples: &[Sample]) {}
}

/// What GitHub says about the CI runner and its queue at one instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunnerObservation {
    /// The runner is connected to GitHub (its listener is alive).
    pub listener_up: bool,
    pub busy: bool,
    pub queued_jobs: u64,
}

/// The CI runner as GitHub sees it. An `Err` means GitHub could not be asked.
#[async_trait]
pub trait RunnerSource: Send + Sync {
    /// The runner's name, the `name` of its samples.
    fn name(&self) -> String;
    async fn observe(&self) -> Result<RunnerObservation, PortError>;
}

#[async_trait]
pub trait HostReaders: Send + Sync {
    async fn read_stat(&self) -> Result<String, PortError>;
    async fn read_meminfo(&self) -> Result<String, PortError>;
    async fn read_loadavg(&self) -> Result<String, PortError>;
    /// `/proc/pressure/<resource>` (`memory`, `io`, `cpu`); an error on a kernel without pressure stall information.
    async fn read_pressure(&self, resource: &str) -> Result<String, PortError>;
    /// Total and available kilobytes of the root filesystem.
    async fn root_fs_kb(&self) -> Result<(f64, f64), PortError>;
}

#[async_trait]
pub trait PlanSource: Send + Sync {
    /// Valid plan manifests, sorted by slug.
    async fn plans(&self) -> Vec<PlanManifest>;
    async fn runs(&self) -> Vec<DispatchRun>;
    async fn pricing(&self) -> Pricing;
}

pub trait Clock: Send + Sync {
    fn now_ms(&self) -> i64;
}

/// Everything the service reaches outside its own code.
pub struct Ports {
    pub store: std::sync::Arc<dyn MetricsStore>,
    pub docker: std::sync::Arc<dyn DockerEngine>,
    pub host: std::sync::Arc<dyn HostReaders>,
    pub plans: std::sync::Arc<dyn PlanSource>,
    pub clock: std::sync::Arc<dyn Clock>,
    /// How full the metrics may fill the database before the oldest rows are cut.
    pub policy: crate::domain::prune::StoragePolicy,
    /// Told after every host sweep; `NoHeartbeat` when none is configured.
    pub heartbeat: std::sync::Arc<dyn Heartbeat>,
    /// Gets every sweep's samples after they are stored; `NoPublisher` when no metrics sink is configured.
    pub publisher: std::sync::Arc<dyn SamplePublisher>,
    /// The CI runner as GitHub sees it; none when GitHub is not configured.
    pub runner: Option<std::sync::Arc<dyn RunnerSource>>,
    /// The commit the image was built from; `/health` says `unknown` without one.
    pub commit: Option<String>,
}
