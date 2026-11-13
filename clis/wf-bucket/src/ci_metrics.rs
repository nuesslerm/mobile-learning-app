//! The cache layers' structured events and gauges. An event is one logfmt line on stderr (stdout carries a verb's JSON); the
//! gauges go to the Better Stack source `vm-health-metrics` over the OTLP sink of `clis/telemetry`. Both are best effort: a
//! missing key, a refusing source or a slow one costs at most `BUDGET` (6 s) and never fails a run.
//!
//! A `Reading` has two labels, `scope` and `name`, so a metric's two labels are those two in the order its name documents
//! (`ci.cache.requests{kind,result}` is scope=kind, name=result); a metric with one label puts `ALL` in `name`.
use blob_cache::domain::Stats;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, SystemTime};
use telemetry::{Destination, MetricsSink, OtlpMetricsSink, Reading};
use wf_kit::logfmt;

const SOURCE: &str = "ci-cache";
const SERVICE: &str = "wf-ci";
// Vault resolution, the TLS handshake and the export each cost seconds on a cold runner; 2 s dropped the batch every time.
const BUDGET: Duration = Duration::from_secs(6);
const HOST: &str = "BETTERSTACK_METRICS_HOST";
const TOKEN: &str = "BETTERSTACK_METRICS_TOKEN";

/// The `name` of a metric that has one label only.
pub const ALL: &str = "all";

/// One structured event, `level` `info` or `warn`; the name is static and what varies is a field.
pub fn event(level: &str, name: &str, fields: &[(&str, String)]) {
    logfmt::mcp(level, SOURCE, name, fields);
}

/// The gauges of one step, sent together as one batch.
#[derive(Default)]
pub struct Gauges(Vec<Reading>);

impl Gauges {
    pub fn new() -> Gauges {
        Gauges::default()
    }

    pub fn set(&mut self, metric: &str, scope: &str, name: &str, value: f64) -> &mut Gauges {
        self.0.push(Reading { metric: metric.to_string(), scope: scope.to_string(), name: name.to_string(), value, at: SystemTime::now() });
        self
    }

    /// `ci.cache.requests{kind,result}` for what one run did against the store, and `ci.cache.degraded{kind,reason}` when it went cold.
    pub fn store(&mut self, kind: &str, stats: &Stats) -> &mut Gauges {
        let counts = [("hit", stats.hits), ("miss", stats.misses), ("put", stats.puts), ("skipped", stats.skipped), ("dropped-put", stats.dropped_puts), ("quota-exceeded", stats.quota_exceeded)];
        for (result, n) in counts {
            self.set("ci.cache.requests", kind, result, n as f64);
        }
        if let Some(seeded) = stats.seeded {
            self.set("ci.cache.seeded", kind, "checked", seeded.checked as f64);
            self.set("ci.cache.seeded", kind, "present", seeded.present as f64);
        }
        if let Some(why) = stats.degraded {
            self.set("ci.cache.degraded", kind, why.as_str(), 1.0);
        }
        self
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Sends the batch to the source the environment, then the vault, names; nothing is sent when there is none.
    pub fn send(&mut self) {
        if !self.is_empty() {
            send_via(std::mem::take(&mut self.0), resolve, BUDGET);
        }
    }

    /// `send` to a sink the caller built.
    pub fn send_to(&mut self, sink: Arc<dyn MetricsSink>, budget: Duration) {
        if !self.is_empty() {
            send_via(std::mem::take(&mut self.0), move || Some(sink), budget);
        }
    }
}

/// `WF_CI_METRICS_BASE` (a loopback or other scheme://authority, for a stand-in source) or the vault's Better Stack host and token;
/// `WF_CI_METRICS=off` turns the gauges off.
fn resolve() -> Option<Arc<dyn MetricsSink>> {
    if std::env::var("WF_CI_METRICS").is_ok_and(|v| v == "off") {
        return None;
    }
    let destination = match std::env::var("WF_CI_METRICS_BASE").ok().filter(|v| !v.is_empty()) {
        Some(base) => Destination::new(base, std::env::var(TOKEN).unwrap_or_default()),
        None => {
            let values = secrets::load_values(&[HOST, TOKEN]);
            let get = |key: &str| values.get(key).cloned().flatten();
            Destination::from_host(&get(HOST)?, &get(TOKEN)?)?
        }
    };
    Some(Arc::new(OtlpMetricsSink::new(destination, SERVICE)))
}

/// Resolution and the request both run on a thread of their own, so the run waits at most `budget` for either; a timeout names the
/// stage it was in, so a slow vault is told apart from a slow source.
fn send_via(readings: Vec<Reading>, sink: impl FnOnce() -> Option<Arc<dyn MetricsSink>> + Send + 'static, budget: Duration) {
    let (done, answer) = mpsc::channel();
    let count = readings.len();
    let resolved = Arc::new(AtomicBool::new(false));
    let flag = resolved.clone();
    std::thread::spawn(move || {
        let sent = sink().map(|sink| {
            flag.store(true, Ordering::SeqCst);
            sink.record(&readings)
        });
        let _ = done.send(sent);
    });
    match answer.recv_timeout(budget) {
        Ok(Some(Ok(()))) | Ok(None) => {}
        Ok(Some(Err(error))) => event("warn", "ci.metrics.refused", &[("readings", count.to_string()), ("error", error)]),
        Err(_) => {
            let stage = if resolved.load(Ordering::SeqCst) { "send" } else { "resolve" };
            event("warn", "ci.metrics.timeout", &[("readings", count.to_string()), ("budget_ms", budget.as_millis().to_string()), ("stage", stage.to_string())]);
        }
    }
}
