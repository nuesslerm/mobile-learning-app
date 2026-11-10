//! The world around the service: a Docker Engine on a Unix socket, `/proc` and the clock as doubles; the metrics
//! store, which a real database would hold, an in-memory double with the same averaging and cutting; the repository files real, in a temp dir. Requests enter through the router `main` serves.
use async_trait::async_trait;
use axum::body::Body;
use axum::extract::State;
use axum::http::{header, Method, Request, StatusCode, Uri};
use axum::Json;
use http_body_util::BodyExt;
use percent_encoding::percent_decode_str;
use reporting_core::application::{Clock, Heartbeat, HostReaders, MetricsStore, NoPublisher, PortError, RunnerObservation, RunnerSource, SamplePublisher};
use reporting_core::domain::prune::StoragePolicy;
use reporting_core::domain::sample::{Sample, SeriesPoint, SeriesQuery};
use reporting_core::domain::window::granularity_for;
use reporting_service::api::{App, Ports};
use reporting_service::infrastructure::{DockerSocket, PlanFiles};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::TempDir;
use tower::ServiceExt;

pub const NOW: i64 = 1_700_000_000_000;
pub const MINUTE: i64 = 60_000;
pub const HOUR: i64 = 3_600_000;
pub const DAY: i64 = 86_400_000;

pub struct Container {
    pub id: &'static str,
    pub names: &'static [&'static str],
    /// The engine's status line (`Up 3 minutes (unhealthy)`), empty for none.
    pub status: &'static str,
    /// The stats document, or the status the engine fails the stats call with.
    pub stats: Result<Value, u16>,
}

#[derive(Default)]
pub struct EngineState {
    pub containers: Vec<Container>,
    pub df: Value,
    /// What `GET /events` answers, one JSON object per line.
    pub events: String,
    pub requests: Vec<String>,
}

/// A Docker Engine answering the three calls the service makes, recording each.
#[derive(Clone, Default)]
pub struct Engine(pub Arc<Mutex<EngineState>>);

impl Engine {
    pub fn requests(&self) -> Vec<String> {
        self.0.lock().unwrap().requests.clone()
    }
}

async fn engine_answer(State(engine): State<Engine>, method: Method, uri: Uri) -> axum::response::Response {
    use axum::response::IntoResponse;
    let target = uri.path_and_query().map(|p| p.as_str().to_string()).unwrap_or_default();
    let mut state = engine.0.lock().unwrap();
    state.requests.push(format!("{method} {target}"));
    if target.starts_with("/events?") {
        return (StatusCode::OK, state.events.clone()).into_response();
    }
    if target == "/containers/json" {
        let list = state
            .containers
            .iter()
            .map(|c| {
                let mut entry = if c.names.is_empty() { json!({ "Id": c.id }) } else { json!({ "Id": c.id, "Names": c.names }) };
                if !c.status.is_empty() {
                    entry["Status"] = json!(c.status);
                }
                entry
            })
            .collect();
        return (StatusCode::OK, Json(Value::Array(list))).into_response();
    }
    if target == "/system/df?verbose=1" {
        return (StatusCode::OK, Json(state.df.clone())).into_response();
    }
    let stats_of = target.strip_prefix("/containers/").and_then(|rest| rest.strip_suffix("/stats?stream=false"));
    match stats_of.map(|id| percent_decode_str(id).decode_utf8_lossy().into_owned()).and_then(|id| state.containers.iter().find(|c| c.id == id)) {
        Some(Container { stats: Ok(stats), .. }) => (StatusCode::OK, Json(stats.clone())).into_response(),
        Some(Container { stats: Err(status), .. }) => (StatusCode::from_u16(*status).unwrap(), Json(json!({ "message": "boom" }))).into_response(),
        None => (StatusCode::NOT_FOUND, Json(json!({ "message": "not found" }))).into_response(),
    }
}

/// `/proc` and the root filesystem: `/proc/stat` advances 50 % busy per read; meminfo answers its readings in turn, the last repeating.
pub struct Proc {
    pub stat: bool,
    pub meminfo: Vec<&'static str>,
    pub root_kb: Option<(f64, f64)>,
    stat_reads: AtomicUsize,
    meminfo_reads: AtomicUsize,
}

impl Proc {
    pub fn new(stat: bool, meminfo: Vec<&'static str>, root_kb: Option<(f64, f64)>) -> Self {
        Self { stat, meminfo, root_kb, stat_reads: AtomicUsize::new(0), meminfo_reads: AtomicUsize::new(0) }
    }
}

#[async_trait]
impl HostReaders for Proc {
    async fn read_stat(&self) -> Result<String, PortError> {
        if !self.stat {
            return Err(PortError::new("no /proc/stat"));
        }
        let n = self.stat_reads.fetch_add(1, Ordering::SeqCst) as u64;
        Ok(format!("cpu  {} 0 {} {} {} 0 0 {}\n", 100 + 30 * n, 100 + 30 * n, 800 + 80 * n, 20 * n, 40 * n))
    }

    async fn read_meminfo(&self) -> Result<String, PortError> {
        let n = self.meminfo_reads.fetch_add(1, Ordering::SeqCst).min(self.meminfo.len().saturating_sub(1));
        self.meminfo.get(n).map(|text| text.to_string()).ok_or_else(|| PortError::new("no /proc/meminfo"))
    }

    async fn read_loadavg(&self) -> Result<String, PortError> {
        Ok("0.50 0.40 0.30 1/200 999\n".to_string())
    }

    async fn read_pressure(&self, resource: &str) -> Result<String, PortError> {
        match resource {
            "memory" => Ok("some avg10=0.00 avg60=0.00 avg300=0.00 total=1\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n".to_string()),
            "io" => Ok("some avg10=12.50 avg60=3.00 avg300=1.00 total=99\nfull avg10=4.00 avg60=0.00 avg300=0.00 total=9\n".to_string()),
            _ => Err(PortError::new("no pressure file")),
        }
    }

    async fn root_fs_kb(&self) -> Result<(f64, f64), PortError> {
        self.root_kb.ok_or_else(|| PortError::new("no statvfs"))
    }
}

/// Starts at `start` and moves `step` milliseconds on every reading.
pub struct TestClock {
    now: AtomicI64,
    step: i64,
}

impl TestClock {
    pub fn new(start: i64, step: i64) -> Self {
        Self { now: AtomicI64::new(start), step }
    }
}

impl Clock for TestClock {
    fn now_ms(&self) -> i64 {
        self.now.fetch_add(self.step, Ordering::SeqCst)
    }
}

pub struct Doubles {
    pub proc: Proc,
    pub clock: TestClock,
    pub commit: Option<&'static str>,
    /// Where the service looks for the Docker socket; the engine's by default.
    pub socket: Option<PathBuf>,
    pub policy: StoragePolicy,
    pub beats: Arc<Beats>,
    pub publisher: Arc<dyn SamplePublisher>,
    pub runner: Option<Arc<dyn RunnerSource>>,
    /// What one stored sample weighs in `used_bytes`.
    pub bytes_per_row: u64,
}

impl Default for Doubles {
    fn default() -> Self {
        Self { proc: Proc::new(false, vec![], None), clock: TestClock::new(NOW, 0), commit: None, socket: None, policy: StoragePolicy::FREE_TIER, beats: Arc::new(Beats::default()), publisher: Arc::new(NoPublisher), runner: None, bytes_per_row: 100 }
    }
}

/// GitHub's view of the runner as a script: each observation is answered once, the last one repeating; `None` is GitHub not answering.
pub struct ScriptedRunner {
    script: Vec<Option<RunnerObservation>>,
    asked: AtomicUsize,
}

impl ScriptedRunner {
    pub fn new(script: Vec<Option<RunnerObservation>>) -> Self {
        Self { script, asked: AtomicUsize::new(0) }
    }
}

#[async_trait]
impl RunnerSource for ScriptedRunner {
    fn name(&self) -> String {
        "vm-main".into()
    }

    async fn observe(&self) -> Result<RunnerObservation, PortError> {
        let n = self.asked.fetch_add(1, Ordering::SeqCst).min(self.script.len() - 1);
        self.script[n].ok_or_else(|| PortError::new("GitHub answered 502"))
    }
}

/// A heartbeat that counts the pings it is given.
#[derive(Default)]
pub struct Beats(pub AtomicUsize);

#[async_trait]
impl Heartbeat for Beats {
    async fn beat(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

type Key = (i64, String, String, String);

/// The metrics database as a double: the same averaging per window, newest-per-series and oldest-first cutting the Oracle store does in SQL.
#[derive(Default)]
pub struct MemoryStore {
    rows: Mutex<std::collections::BTreeMap<Key, f64>>,
    bytes_per_row: AtomicU64,
}

impl MemoryStore {
    pub fn insert(&self, ts: i64, scope: &str, name: &str, metric: &str, value: f64) {
        self.rows.lock().unwrap().insert((ts, scope.into(), name.into(), metric.into()), value);
    }
}

#[async_trait]
impl MetricsStore for MemoryStore {
    async fn insert_samples(&self, samples: Vec<Sample>) -> Result<(), PortError> {
        for s in samples {
            self.insert(s.ts, &s.scope, &s.name, &s.metric, s.value);
        }
        Ok(())
    }

    async fn series_for(&self, window_ms: i64, now: i64, query: &SeriesQuery) -> Result<Vec<SeriesPoint>, PortError> {
        let (granularity, bucket) = granularity_for(window_ms);
        let mut buckets: std::collections::BTreeMap<Key, (f64, f64)> = std::collections::BTreeMap::new();
        for ((ts, scope, name, metric), value) in self.rows.lock().unwrap().iter().filter(|((ts, row_scope, _, row_metric), _)| *ts >= (now - window_ms).max(query.since_ms.unwrap_or(i64::MIN)) && query.scope.as_deref().is_none_or(|s| s == row_scope) && (query.metrics.is_empty() || query.metrics.iter().any(|m| m == row_metric))) {
            let at = if bucket == 0 { *ts } else { ts.div_euclid(bucket) * bucket };
            let entry = buckets.entry((at, scope.clone(), name.clone(), metric.clone())).or_insert((0.0, 0.0));
            entry.0 += value;
            entry.1 += 1.0;
        }
        Ok(buckets.into_iter().map(|((ts, scope, name, metric), (sum, n))| SeriesPoint { sample: Sample { ts, scope, name, metric, value: sum / n }, granularity }).collect())
    }

    async fn latest(&self) -> Result<Vec<Sample>, PortError> {
        let mut newest: std::collections::BTreeMap<(String, String, String), (i64, f64)> = std::collections::BTreeMap::new();
        for ((ts, scope, name, metric), value) in self.rows.lock().unwrap().iter() {
            let entry = newest.entry((scope.clone(), name.clone(), metric.clone())).or_insert((*ts, *value));
            if *ts >= entry.0 {
                *entry = (*ts, *value);
            }
        }
        Ok(newest.into_iter().map(|((scope, name, metric), (ts, value))| Sample { ts, scope, name, metric, value }).collect())
    }

    async fn row_count(&self) -> Result<u64, PortError> {
        Ok(self.rows.lock().unwrap().len() as u64)
    }

    async fn used_bytes(&self) -> Result<u64, PortError> {
        Ok(self.rows.lock().unwrap().len() as u64 * self.bytes_per_row.load(Ordering::SeqCst))
    }

    async fn delete_oldest(&self, rows: u64) -> Result<u64, PortError> {
        let mut stored = self.rows.lock().unwrap();
        let Some(limit) = stored.keys().map(|k| k.0).nth(rows.saturating_sub(1) as usize) else { return Ok(0) };
        let before = stored.len();
        stored.retain(|(ts, ..), _| *ts > limit);
        Ok((before - stored.len()) as u64)
    }
}

/// A temp dir holding the database, the repository root and the engine's socket, which outlives the apps built over it.
pub struct World {
    dir: TempDir,
    pub engine: Engine,
    /// The database, which outlives the apps built over it like the real one.
    store: Arc<MemoryStore>,
}

impl World {
    pub async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("repo")).unwrap();
        let engine = Engine::default();
        let listener = tokio::net::UnixListener::bind(dir.path().join("docker.sock")).unwrap();
        let router = axum::Router::new().fallback(engine_answer).with_state(engine.clone());
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self { dir, engine, store: Arc::new(MemoryStore::default()) }
    }

    pub fn repo(&self) -> PathBuf {
        self.dir.path().join("repo")
    }

    pub fn app(&self, doubles: Doubles) -> App {
        let socket = doubles.socket.unwrap_or_else(|| self.dir.path().join("docker.sock"));
        self.store.bytes_per_row.store(doubles.bytes_per_row, Ordering::SeqCst);
        App::new(Ports {
            store: self.store.clone(),
            docker: Arc::new(DockerSocket::new(&socket).unwrap()),
            host: Arc::new(doubles.proc),
            plans: Arc::new(PlanFiles::new(self.repo())),
            clock: Arc::new(doubles.clock),
            commit: doubles.commit.map(String::from),
            policy: doubles.policy,
            heartbeat: doubles.beats,
            publisher: doubles.publisher,
            runner: doubles.runner,
        })
    }

    /// Writes rows the way an earlier run of the service would have left them: `(ts, scope, name, metric, value)`.
    pub fn seed(&self, samples: &[(i64, &str, &str, &str, f64)]) {
        for (ts, scope, name, metric, value) in samples {
            self.store.insert(*ts, scope, name, metric, *value);
        }
    }

    pub fn write(&self, relative: &str, contents: &str) {
        let path = self.repo().join(relative);
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new("."))).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    pub fn remove(&self, relative: &str) {
        std::fs::remove_file(self.repo().join(relative)).unwrap();
    }
}

#[derive(Debug)]
pub struct Reply {
    pub status: StatusCode,
    pub content_type: Option<String>,
    pub body: Value,
}

pub async fn call(app: &App, method: Method, path: &str) -> Reply {
    let response = app.router().oneshot(Request::builder().method(method).uri(path).body(Body::empty()).unwrap()).await.unwrap();
    let status = response.status();
    let content_type = response.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(String::from);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    Reply { status, content_type, body: serde_json::from_slice(&bytes).unwrap_or(Value::Null) }
}

pub async fn get(app: &App, path: &str) -> Value {
    call(app, Method::GET, path).await.body
}

/// Polls `path` until `done` holds or five seconds pass, and returns the last body either way.
pub async fn eventually(app: &App, path: &str, done: impl Fn(&Value) -> bool) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let body = get(app, path).await;
        if done(&body) || tokio::time::Instant::now() > deadline {
            return body;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
