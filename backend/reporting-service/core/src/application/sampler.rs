//! The sweeps: container stats, host figures, container disk use, and the prune pass over what they stored.
use crate::application::ports::{Clock, DockerEngine, Heartbeat, HostReaders, MetricsStore, RunnerSource, SamplePublisher};
use crate::application::prune;
use crate::domain::{container, events::{notable, Notable}, host, prune::StoragePolicy, sample::{PruneResult, Sample}};
use std::sync::Arc;
use std::time::Duration;

/// `/proc/stat` is read twice and the delta taken; read back to back it is a few jiffies over a near-zero window, so the second read waits.
const PROC_READ_GAP: Duration = Duration::from_millis(120);

pub struct Sampler {
    pub store: Arc<dyn MetricsStore>,
    pub docker: Arc<dyn DockerEngine>,
    pub host: Arc<dyn HostReaders>,
    pub clock: Arc<dyn Clock>,
    pub policy: StoragePolicy,
    pub heartbeat: Arc<dyn Heartbeat>,
    pub publisher: Arc<dyn SamplePublisher>,
    /// The CI runner to watch; none when GitHub is not configured.
    pub runner: Option<Arc<dyn RunnerSource>>,
    /// Since when the runner has not been busy (ms); none while it is.
    pub runner_idle_since: std::sync::Mutex<Option<i64>>,
    /// Where the next look at Docker's events starts (ms); set when the service starts.
    pub events_from: std::sync::atomic::AtomicI64,
}

impl Sampler {
    pub async fn containers(&self, now: i64) -> Vec<Sample> {
        let asked = self.clock.now_ms();
        let listed = self.docker.list_containers().await;
        let docker_ms = (self.clock.now_ms() - asked) as f64;
        let containers = match listed {
            Ok(containers) => containers,
            Err(error) => {
                tracing::error!(error = %error, after_ms = docker_ms, "sample.containers.list_failed");
                return Vec::new();
            }
        };
        let outcomes = futures_util::future::join_all(containers.iter().map(|summary| async move {
            let name = summary.display_name();
            let health = summary.health();
            match self.docker.container_stats(&summary.id).await {
                Ok(stats) => (name, health, Some(stats)),
                Err(error) => {
                    tracing::error!(container = %name, error = %error, "sample.containers.stats_failed");
                    (name, health, None)
                }
            }
        }))
        .await;
        let mut samples = self.docker_events(now).await;
        // How long the Docker daemon took to answer: it climbs before the daemon stops answering at all.
        samples.push(Sample::new(now, "host", "docker", "list_ms", docker_ms));
        for (name, health, stats) in outcomes {
            if let Some(health) = health {
                samples.push(Sample::new(now, "container", &name, "health", health));
            }
            let Some(stats) = stats else {
                samples.push(Sample::new(now, "container", &name, "absent", 1.0));
                continue;
            };
            let (used, limit) = container::memory_usage(&stats);
            samples.push(Sample::new(now, "container", &name, "cpu", container::cpu_percent(&stats)));
            samples.push(Sample::new(now, "container", &name, "memory", used));
            samples.push(Sample::new(now, "container", &name, "memory_limit", limit));
            let (read, written) = container::disk_bytes(&stats);
            samples.push(Sample::new(now, "container", &name, "disk_read_bytes", read));
            samples.push(Sample::new(now, "container", &name, "disk_write_bytes", written));
        }
        samples
    }

    /// What killed a container since the last look: the engine's `oom` and failed `die` events, logged and stored at their own time.
    async fn docker_events(&self, now: i64) -> Vec<Sample> {
        use std::sync::atomic::Ordering;
        let from = self.events_from.load(Ordering::SeqCst);
        let events = match self.docker.events(from / 1000, now / 1000).await {
            Ok(events) => events,
            Err(error) => {
                tracing::error!(error = %error, "sample.docker.events_failed");
                return Vec::new();
            }
        };
        self.events_from.store(now, Ordering::SeqCst);
        let mut samples = Vec::new();
        for event in events.iter().filter(|e| e.time_ms >= from - 1000) {
            match notable(event) {
                Some(Notable::OutOfMemory) => {
                    tracing::warn!(container = %event.container, "docker.oom");
                    samples.push(Sample::new(event.time_ms, "container", &event.container, "oom", 1.0));
                }
                Some(Notable::Died(code)) => {
                    tracing::warn!(container = %event.container, exit_code = code, "docker.died");
                    samples.push(Sample::new(event.time_ms, "container", &event.container, "died", code as f64));
                }
                None => {}
            }
        }
        samples
    }

    pub async fn host(&self, now: i64) -> Vec<Sample> {
        let mut samples = Vec::new();
        let cpu = async {
            let first = self.host.read_stat().await?;
            tokio::time::sleep(PROC_READ_GAP).await;
            let second = self.host.read_stat().await?;
            Ok::<_, crate::application::PortError>((host::cpu_percent(&first, &second), host::cpu_steal_iowait(&first, &second), host::cpu_kernel(&first, &second)))
        };
        match cpu.await {
            Ok((value, (steal, iowait), kernel)) => {
                samples.push(Sample::new(now, "host", "host", "cpu", value));
                samples.push(Sample::new(now, "host", "host", "cpu_steal", steal));
                samples.push(Sample::new(now, "host", "host", "cpu_iowait", iowait));
                samples.push(Sample::new(now, "host", "host", "cpu_kernel", kernel));
            }
            Err(error) => tracing::error!(error = %error, "sample.host.stat_failed"),
        }
        match self.host.read_meminfo().await {
            Ok(contents) => {
                let (used, total) = host::memory_usage(&contents);
                samples.push(Sample::new(now, "host", "host", "memory_used", used));
                samples.push(Sample::new(now, "host", "host", "memory_total", total));
                samples.push(Sample::new(now, "host", "host", "swap_used", host::swap_used(&contents)));
            }
            Err(error) => tracing::error!(error = %error, "sample.host.meminfo_failed"),
        }
        match self.host.read_loadavg().await {
            Ok(contents) => {
                if let Some(load) = host::load1(&contents) {
                    samples.push(Sample::new(now, "host", "host", "load1", load));
                }
            }
            Err(error) => tracing::error!(error = %error, "sample.host.loadavg_failed"),
        }
        // A kernel without pressure stall information has no such files; the other figures still stand.
        for (resource, metric) in [("memory", "psi_memory"), ("io", "psi_io"), ("cpu", "psi_cpu")] {
            if let Some(value) = self.host.read_pressure(resource).await.ok().and_then(|text| host::pressure_some_avg10(&text)) {
                samples.push(Sample::new(now, "host", "host", metric, value));
            }
        }
        match self.host.root_fs_kb().await {
            Ok((total_kb, available_kb)) => {
                samples.push(Sample::new(now, "host", "host", "disk_used", (total_kb - available_kb) * 1024.0));
                samples.push(Sample::new(now, "host", "host", "disk_total", total_kb * 1024.0));
            }
            Err(error) => tracing::error!(error = %error, "sample.host.df_failed"),
        }
        // The history a stall leaves at the log sink, next to the stored series.
        let figure = |metric: &str| samples.iter().find(|s| s.metric == metric).map_or(-1.0, |s| s.value);
        tracing::info!(
            cpu = figure("cpu"),
            memory_used = figure("memory_used"),
            memory_total = figure("memory_total"),
            swap_used = figure("swap_used"),
            load1 = figure("load1"),
            psi_memory = figure("psi_memory"),
            psi_io = figure("psi_io"),
            psi_cpu = figure("psi_cpu"),
            "sample.host"
        );
        self.heartbeat.beat().await;
        samples
    }

    pub async fn storage(&self, now: i64) -> Vec<Sample> {
        let containers = match self.docker.system_df().await {
            Ok(containers) => containers,
            Err(error) => {
                tracing::error!(error = %error, "sample.storage.df_failed");
                return Vec::new();
            }
        };
        containers
            .iter()
            .filter_map(|container| {
                let name = container.display_name();
                (!name.is_empty()).then(|| {
                    [
                        Sample::new(now, "container", &name, "size_rw", container.size_rw.unwrap_or(0.0)),
                        Sample::new(now, "container", &name, "size_rootfs", container.size_root_fs.unwrap_or(0.0)),
                    ]
                })
            })
            .flatten()
            .collect()
    }

    /// The CI runner's liveness, its queue and how long it has sat idle. When GitHub cannot be asked only `github_reachable` is 0: the
    /// rest is unknown, not zero.
    pub async fn runner(&self, now: i64) -> Vec<Sample> {
        let Some(source) = &self.runner else { return Vec::new() };
        let name = source.name();
        let observed = match source.observe().await {
            Ok(observed) => observed,
            Err(error) => {
                tracing::error!(runner = %name, error = %error, "sample.runner.github_failed");
                return vec![Sample::new(now, "runner", &name, "github_reachable", 0.0)];
            }
        };
        let idle_s = {
            let mut since = self.runner_idle_since.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if observed.busy {
                *since = None;
                0.0
            } else {
                (now - *since.get_or_insert(now)) as f64 / 1000.0
            }
        };
        vec![
            Sample::new(now, "runner", &name, "listener_up", f64::from(u8::from(observed.listener_up))),
            Sample::new(now, "runner", &name, "github_reachable", 1.0),
            Sample::new(now, "runner", &name, "queued_jobs_waiting", observed.queued_jobs as f64),
            Sample::new(now, "runner", &name, "idle_seconds", idle_s),
        ]
    }

    /// Writes the samples to the store, then hands the same ones to the publisher whether or not the write succeeded.
    pub async fn store(&self, samples: Vec<Sample>) -> Result<(), crate::application::PortError> {
        if samples.is_empty() {
            return Ok(());
        }
        let stored = self.store.insert_samples(samples.clone()).await;
        self.publisher.publish(&samples).await;
        stored
    }

    /// Cuts the history back when the database nears its free-tier size (see [`prune::run`]).
    pub async fn prune(&self) -> Result<PruneResult, crate::application::PortError> {
        prune::run(self.store.as_ref(), &self.policy).await
    }

    pub fn now(&self) -> i64 {
        self.clock.now_ms()
    }
}
