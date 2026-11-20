//! The CI's structured events, gauges and error lines. An event is one logfmt line on stderr (stdout carries a verb's JSON); the
//! gauges go to every `MetricsSink` there is (the job's CI database, `LocalDbSink`, and the Better Stack source `vm-health-metrics` over the
//! OTLP sink of `clis/telemetry`), the error lines (`log`) to the OTLP logs ingest. All are best effort: a missing key, a refusing
//! source or a slow one costs at most `BUDGET` (6 s) and never fails a run. A gauge's name must be in `ci_metric_catalog`.
//!
//! A `Reading` has two labels, `scope` and `name`, so a metric's two labels are those two in the order its name documents
//! (`ci.cache.requests{kind,result}` is scope=kind, name=result); a metric with one label puts `ALL` in `name`.
use blob_cache::domain::Stats;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, SystemTime};
use crate::ci_metric_catalog;
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

/// The gauges of one step, sent together as one batch; a name the catalogue does not hold is refused and counted.
#[derive(Default)]
pub struct Gauges {
    readings: Vec<Reading>,
    rejected: Vec<String>,
}

/// The second `MetricsSink`: readings into `CI_METRICS` of the job's local database (the report job merges it), so a source that is down
/// or unconfigured still leaves the rows a query reads. Nothing is written outside a job with a local database.
pub struct LocalDbSink {
    pub run: String,
}

impl LocalDbSink {
    pub fn from_env() -> LocalDbSink {
        LocalDbSink { run: std::env::var("GITHUB_RUN_ID").unwrap_or_default() }
    }
}

impl MetricsSink for LocalDbSink {
    fn record(&self, readings: &[Reading]) -> Result<(), String> {
        let rows: Vec<crate::ci_db::MetricRow> = readings
            .iter()
            .map(|r| crate::ci_db::MetricRow {
                run: self.run.clone(),
                ts: r.at.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64),
                metric: r.metric.clone(),
                scope: r.scope.clone(),
                name: r.name.clone(),
                value: r.value,
            })
            .collect();
        crate::ci_db::record_metrics_local(&rows).map(|_| ())
    }
}

impl Gauges {
    pub fn new() -> Gauges {
        Gauges::default()
    }

    pub fn set(&mut self, metric: &str, scope: &str, name: &str, value: f64) -> &mut Gauges {
        if ci_metric_catalog::lookup(metric).is_none() {
            self.rejected.push(metric.to_string());
            return self;
        }
        self.readings.push(Reading { metric: metric.to_string(), scope: scope.to_string(), name: name.to_string(), value, at: SystemTime::now() });
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
        self.readings.is_empty()
    }

    /// The readings the catalogue accepted, and the names it refused.
    pub fn readings(&self) -> &[Reading] {
        &self.readings
    }

    pub fn rejected(&self) -> &[String] {
        &self.rejected
    }

    fn report_rejected(&mut self) {
        if !self.rejected.is_empty() {
            self.rejected.sort();
            self.rejected.dedup();
            event("warn", "ci.metrics.unknown", &[("names", self.rejected.join(","))]);
            self.rejected.clear();
        }
    }

    /// Gives the batch to the job's database, then to the source the environment, then the vault, names; the OTLP source is skipped when there is none.
    pub fn send(&mut self) {
        self.report_rejected();
        if !self.is_empty() {
            if let Err(why) = LocalDbSink::from_env().record(&self.readings) {
                event("warn", "ci.metrics.not_kept", &[("reason", why)]);
            }
            send_via(std::mem::take(&mut self.readings), resolve, BUDGET);
        }
    }

    /// `send` to a sink the caller built.
    pub fn send_to(&mut self, sink: Arc<dyn MetricsSink>, budget: Duration) {
        self.report_rejected();
        if !self.is_empty() {
            send_via(std::mem::take(&mut self.readings), move || Some(sink), budget);
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

/// `text` with the values of the environment's secret-named variables replaced, the scrub the log sink applies.
pub fn scrub(text: &str) -> String {
    telemetry::redact::Redactor::from_env(Vec::new()).scrub(text).into_owned()
}

/// One error line to the OTLP logs ingest: `WF_CI_LOGS_BASE` (a loopback or other scheme://authority, for a stand-in ingest), else the environment's, else the
/// vault's `BETTERSTACK_OTLP_HOST` and `BETTERSTACK_OTLP_TOKEN`. The sink scrubs the fields of the values of the environment variables whose names look like secrets (not of any other secret). `Err` says why nothing was sent.
pub fn log(level: &'static str, name: &str, fields: &[(&str, String)]) -> Result<(), String> {
    let name = name.to_string();
    let fields: Vec<(String, String)> = fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
    let (done, answer) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(send_log(level, &name, &fields));
    });
    answer.recv_timeout(BUDGET).unwrap_or_else(|_| Err(format!("logs ingest did not answer within {} ms", BUDGET.as_millis())))
}

fn send_log(level: &'static str, name: &str, fields: &[(String, String)]) -> Result<(), String> {
    let destination = match std::env::var("WF_CI_LOGS_BASE").ok().filter(|v| !v.is_empty()) {
        Some(base) => Destination::new(base, std::env::var("BETTERSTACK_OTLP_TOKEN").unwrap_or_default()),
        None => Destination::from_env()
            .or_else(|| {
                let values = secrets::load_values(&["BETTERSTACK_OTLP_HOST", "BETTERSTACK_OTLP_TOKEN"]);
                let get = |key: &str| values.get(key).cloned().flatten();
                Destination::from_host(&get("BETTERSTACK_OTLP_HOST")?, &get("BETTERSTACK_OTLP_TOKEN")?)
            })
            .ok_or("no OTLP destination: BETTERSTACK_OTLP_HOST and BETTERSTACK_OTLP_TOKEN are in neither the environment nor the vault")?,
    };
    let record = fields.iter().fold(telemetry::oneshot::Record::new(level, name), |r, (k, v)| r.attr(k.clone(), v.clone()));
    telemetry::oneshot::send(&destination, &telemetry::Source::new(SERVICE, ""), std::slice::from_ref(&record))
}
