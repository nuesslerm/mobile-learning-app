//! The world around the watchdog: the `docker` binary and the clock as doubles, virtual time advancing one interval per sweep,
//! and the OTLP ingest its events are exported to as a local server. The watchdog itself is what `main` builds and runs.
use async_trait::async_trait;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode, Uri};
use axum::routing::post;
use axum::Router;
use serde_json::{json, Value};
use service_kit::telemetry::{self, Destination, Source};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;
use tracing::instrument::WithSubscriber;
use watchdog_core::application::Clock;
use watchdog_service::infrastructure::wiring::{self, Ports};
use watchdog_service::infrastructure::{DockerCommand, Output};

#[derive(Clone)]
pub struct Container {
    pub name: &'static str,
    pub health: Option<&'static str>,
    pub restarts: i64,
    pub project: Option<&'static str>,
    pub service: Option<&'static str>,
    pub working_dir: Option<&'static str>,
    pub config_files: &'static [&'static str],
    /// The engine id; the name when `None`.
    pub id: Option<&'static str>,
    pub running: bool,
    /// When a stopped container stopped, as the engine writes it; the engine's zero time when `None`.
    pub finished_at: Option<&'static str>,
    /// When a container that never started was created; it reads `created` instead of `exited` when stopped.
    pub created_at: Option<&'static str>,
    /// The id of the container whose network this one shares.
    pub network_of: Option<&'static str>,
}

pub fn app(name: &'static str, health: Option<&'static str>, restarts: i64) -> Container {
    Container { name, health, restarts, project: Some("proj"), service: Some("app"), working_dir: None, config_files: &[], id: None, running: true, finished_at: None, created_at: None, network_of: None }
}

/// The `docker inspect` JSON of one container, the way the engine writes it.
fn inspected(c: &Container) -> Value {
    let mut labels = serde_json::Map::new();
    let mut label = |key: &str, value: Option<String>| {
        if let Some(value) = value {
            labels.insert(key.into(), value.into());
        }
    };
    label("com.docker.compose.project", c.project.map(String::from));
    label("com.docker.compose.service", c.service.map(String::from));
    label("com.docker.compose.project.working_dir", c.working_dir.map(String::from));
    label("com.docker.compose.project.config_files", (!c.config_files.is_empty()).then(|| c.config_files.join(",")));
    let mut state = c.health.map_or(json!({}), |status| json!({ "Health": { "Status": status } }));
    state["Running"] = c.running.into();
    state["Status"] = (if c.running { "running" } else if c.created_at.is_some() { "created" } else { "exited" }).into();
    state["FinishedAt"] = c.finished_at.unwrap_or("0001-01-01T00:00:00Z").into();
    let network = c.network_of.map_or_else(|| "bridge".to_string(), |id| format!("container:{id}"));
    json!({
        "Id": c.id.unwrap_or(c.name),
        "Name": format!("/{}", c.name),
        "Created": c.created_at.unwrap_or("2023-11-14T22:00:00Z"),
        "RestartCount": c.restarts,
        "State": state,
        "HostConfig": { "NetworkMode": network },
        "Config": { "Labels": labels },
    })
}

pub struct Failure {
    pub code: i32,
    pub stderr: &'static str,
}

/// The `docker` binary: each `ps` starts a sweep over the next list of containers (the last repeats), `inspect` describes that
/// list, and every call is recorded with the virtual second of its sweep. After the last sweep it asks the run to end.
pub struct Docker {
    pub sweeps: Vec<Vec<Container>>,
    pub ps: Option<Failure>,
    pub inspect: Option<Failure>,
    pub compose: Option<Failure>,
    /// The services each project's compose file declares, as `docker compose config --services` prints them.
    pub declares: Vec<(&'static str, &'static [&'static str])>,
    /// `config --services` failing, as it does when the compose file is gone.
    pub config: Option<Failure>,
    sweep: Mutex<usize>,
    last_sweep: Mutex<usize>,
    every: Mutex<Duration>,
    finished: Notify,
    calls: Mutex<Vec<(u64, Vec<String>)>>,
}

impl Docker {
    pub fn new(sweeps: Vec<Vec<Container>>) -> Self {
        Self {
            sweeps,
            ps: None,
            inspect: None,
            compose: None,
            declares: Vec::new(),
            config: None,
            sweep: Mutex::new(0),
            last_sweep: Mutex::new(0),
            every: Mutex::new(Duration::ZERO),
            finished: Notify::new(),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn current(&self) -> &[Container] {
        let sweep = *self.sweep.lock().unwrap();
        self.sweeps.get(sweep.saturating_sub(1).min(self.sweeps.len().saturating_sub(1))).map(Vec::as_slice).unwrap_or_default()
    }

    /// Virtual time: the sweeps so far, one interval apart, the first at zero.
    fn elapsed(&self) -> Duration {
        *self.every.lock().unwrap() * self.sweep.lock().unwrap().saturating_sub(1) as u32
    }
}

fn answer(failure: &Option<Failure>, stdout: String) -> Output {
    match failure {
        Some(Failure { code, stderr }) => Output { code: *code, stdout: String::new(), stderr: stderr.to_string() },
        None => Output { code: 0, stdout, stderr: String::new() },
    }
}

#[async_trait]
impl DockerCommand for Docker {
    async fn run(&self, args: &[&str]) -> Output {
        if args.first() == Some(&"ps") {
            *self.sweep.lock().unwrap() += 1;
        }
        if *self.sweep.lock().unwrap() > *self.last_sweep.lock().unwrap() {
            self.finished.notify_one();
            return answer(&None, String::new());
        }
        self.calls.lock().unwrap().push((self.elapsed().as_secs(), args.iter().map(|a| a.to_string()).collect()));
        match args.first().copied() {
            Some("ps") => answer(&self.ps, self.current().iter().enumerate().map(|(i, _)| format!("{}\n", json!({ "ID": format!("id{i}") }))).collect()),
            Some("inspect") => answer(&self.inspect, Value::Array(self.current().iter().map(inspected).collect()).to_string()),
            Some("compose") if args.contains(&"config") => {
                let project = args.get(2).copied().unwrap_or_default();
                let services = self.declares.iter().find(|(declared, _)| *declared == project).map(|(_, services)| *services).unwrap_or_default();
                answer(&self.config, services.iter().map(|service| format!("{service}\n")).collect())
            }
            Some("compose") => answer(&self.compose, String::new()),
            _ => answer(&None, String::new()),
        }
    }
}

/// Wall time as the sweeps move it.
struct VirtualClock(Arc<Docker>);

impl Clock for VirtualClock {
    fn now_ms(&self) -> i64 {
        1_700_000_000_000 + self.0.elapsed().as_millis() as i64
    }
}

#[derive(Clone, Default)]
struct Ingest(Arc<Mutex<Vec<(String, String, Value)>>>);

async fn record(State(ingest): State<Ingest>, uri: Uri, headers: HeaderMap, body: String) -> StatusCode {
    let auth = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
    ingest.0.lock().unwrap().push((uri.path().to_string(), auth, serde_json::from_str(&body).unwrap_or(Value::Null)));
    StatusCode::OK
}

/// One exported log record: its message, OTLP severity number and attributes as text.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub message: String,
    pub severity: i64,
    pub attributes: BTreeMap<String, String>,
}

impl Record {
    pub fn get(&self, key: &str) -> &str {
        self.attributes.get(key).map(String::as_str).unwrap_or_default()
    }
}

pub struct Observed {
    pub calls: Vec<(u64, Vec<String>)>,
    pub records: Vec<Record>,
    /// Path, authorization and `service.name` of every export request.
    pub exports: Vec<(String, String, String)>,
}

impl Observed {
    pub fn compose_calls(&self) -> Vec<(u64, Vec<String>)> {
        self.calls.iter().filter(|(_, argv)| argv[0] == "compose").cloned().collect()
    }

    /// The compose calls that started containers, not the ones that read a file.
    pub fn up_calls(&self) -> Vec<(u64, Vec<String>)> {
        self.compose_calls().into_iter().filter(|(_, argv)| argv.contains(&"up".to_string())).collect()
    }

    pub fn named(&self, message: &str) -> Vec<&Record> {
        self.records.iter().filter(|r| r.message == message).collect()
    }
}

fn text(value: &Value) -> String {
    value.as_object().and_then(|typed| typed.values().next()).map(|v| v.as_str().map_or_else(|| v.to_string(), String::from)).unwrap_or_default()
}

fn records(body: &Value) -> Vec<Record> {
    let logs = body["resourceLogs"].as_array().into_iter().flatten().flat_map(|r| r["scopeLogs"].as_array().into_iter().flatten()).flat_map(|s| s["logRecords"].as_array().into_iter().flatten());
    logs.map(|log| Record {
        message: text(&log["body"]),
        severity: log["severityNumber"].as_i64().unwrap_or_default(),
        attributes: log["attributes"].as_array().into_iter().flatten().map(|a| (a["key"].as_str().unwrap_or_default().to_string(), text(&a["value"]))).collect(),
    })
    .collect()
}

/// Runs the watchdog `main` builds for `sweeps` sweeps `every` apart in virtual time (a millisecond apart in real time), with
/// `docker` doubled and its events exported to `ingest` (a local one when `None`).
pub async fn run(docker: Docker, host_label: &str, every: Duration, sweeps: usize, ingest: Option<&str>) -> Observed {
    let recorded = Ingest::default();
    let base = match ingest {
        Some(base) => base.to_string(),
        None => {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let app = Router::new().route("/v1/logs", post(record)).route("/v1/traces", post(record)).with_state(recorded.clone());
            tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            base
        }
    };
    let (dispatch, telemetry) = telemetry::build(Source::new("watchdog", "0.0.0"), Vec::new(), Some(Destination::new(base, "tok")));
    *docker.every.lock().unwrap() = every;
    *docker.last_sweep.lock().unwrap() = sweeps;
    let docker = Arc::new(docker);
    let watchdog = wiring::watchdog(Ports { docker: docker.clone(), clock: Arc::new(VirtualClock(docker.clone())), host_label: host_label.into() });

    tokio::select! {
        () = watchdog.run(Duration::from_millis(1)).with_subscriber(dispatch) => {}
        () = docker.finished.notified() => {}
    }
    tokio::task::spawn_blocking(move || telemetry.shutdown()).await.unwrap();

    let received = recorded.0.lock().unwrap().clone();
    let calls = docker.calls.lock().unwrap().clone();
    let service = |body: &Value| body["resourceLogs"][0]["resource"]["attributes"].as_array().into_iter().flatten().find(|a| a["key"] == "service.name").map(|a| text(&a["value"])).unwrap_or_default();
    Observed {
        calls,
        records: received.iter().filter(|(path, _, _)| path == "/v1/logs").flat_map(|(_, _, body)| records(body)).collect(),
        exports: received.iter().filter(|(path, _, _)| path == "/v1/logs").map(|(path, auth, body)| (path.clone(), auth.clone(), service(body))).collect(),
    }
}
