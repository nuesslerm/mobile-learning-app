use async_trait::async_trait;
use reporting_core::application::SamplePublisher;
use reporting_core::domain::sample::Sample;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};
use telemetry::{MetricsSink, Reading};

/// Sends each sweep's samples to a metrics sink as one batch. A sink that fails is counted and logged once per outage; nothing reaches the caller.
pub struct SinkPublisher {
    sink: Arc<dyn MetricsSink>,
    failures: AtomicU64,
    failing: AtomicBool,
}

impl SinkPublisher {
    pub fn new(sink: Arc<dyn MetricsSink>) -> Self {
        Self { sink, failures: AtomicU64::new(0), failing: AtomicBool::new(false) }
    }

    /// Batches the sink has refused so far.
    pub fn failures(&self) -> u64 {
        self.failures.load(Ordering::SeqCst)
    }
}

/// `container.health`, `host.cpu`, `host.docker.list_ms` (a host subject other than the host itself keeps its name), `ci.runner.idle_seconds`.
fn metric_name(sample: &Sample) -> String {
    match (sample.scope.as_str(), sample.name.as_str()) {
        ("runner", _) => format!("ci.runner.{}", sample.metric),
        ("host", "host") => format!("host.{}", sample.metric),
        ("host", name) => format!("host.{name}.{}", sample.metric),
        (scope, _) => format!("{scope}.{}", sample.metric),
    }
}

fn reading(sample: &Sample) -> Reading {
    Reading { metric: metric_name(sample), scope: sample.scope.clone(), name: sample.name.clone(), value: sample.value, at: UNIX_EPOCH + Duration::from_millis(sample.ts.max(0) as u64) }
}

#[async_trait]
impl SamplePublisher for SinkPublisher {
    async fn publish(&self, samples: &[Sample]) {
        let readings: Vec<Reading> = samples.iter().map(reading).collect();
        let sink = self.sink.clone();
        let sent = tokio::task::spawn_blocking(move || sink.record(&readings)).await.unwrap_or_else(|error| Err(error.to_string()));
        match sent {
            Ok(()) => {
                if self.failing.swap(false, Ordering::SeqCst) {
                    tracing::info!("metrics.publish.recovered");
                }
            }
            Err(error) => {
                self.failures.fetch_add(1, Ordering::SeqCst);
                if !self.failing.swap(true, Ordering::SeqCst) {
                    tracing::error!(error = %error, "metrics.publish.failed");
                }
            }
        }
    }
}
