//! The service as `main` builds it (`App::new` over a `Config` and `Ports`, then `routes::router`), with the in-process doubles as its
//! ports, driven through the router: an HTTP request goes in with `oneshot`, a WebSocket over an in-memory duplex pipe that the
//! router serves as `axum::serve` serves a socket. The environment the service reads is a map of the flow's own (`Env`).

use super::doubles::{CommandsPort, FakeCommands, FakeOpencode, FakeOutbound, OpencodePort, OutboundPort};
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use rca_cli_driver::application::ports::Spawner;
use rca_cli_driver::testing::ScriptedSpawner;
use rca_service::api::app::{AgentApi, App, Config};
use rca_service::api::routes::router;
use rca_ladder::file_store::FileLedgerStore;
use rca_ladder::store::CooldownStore;
use remote_chat_agent_core::application::ports::Ledgers;
use rca_group_accounts::testing::ScriptedModelApi;
use rca_group_schedules::testing::{CountingIds, FakeClock};
use rca_service::infrastructure::env::ProcessEnv;
use rca_service::infrastructure::provider_dirs::ProviderFiles;
use rca_service::infrastructure::provider_login::CliLogin;
use rca_service::Accounts;
use rca_service_core::application::environment::Environment;
use rca_service_core::testing::MapEnv;
use rca_service_core::application::ports::Ports;
use rca_service_core::application::settings::Settings;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use tower::ServiceExt;

pub const SECRET: &str = "flow-secret";
pub const ALLOWED_ORIGIN: &str = "https://allowed.example";

/// Flows over a map share it; a flow that reads the process environment holds it alone, so nothing else loads the machine under it.
static PROCESS: RwLock<()> = RwLock::new(());

enum Held {
    Free,
    Shared(#[allow(dead_code)] RwLockReadGuard<'static, ()>),
    Alone(#[allow(dead_code)] RwLockWriteGuard<'static, ()>),
}

/// The environment one flow's service reads: a map of its own with a fresh home, so flows run side by side. A flow whose real children or
/// executors read the process's own variables takes `Env::process` instead, which holds the process environment for its whole run.
pub struct Env {
    _held: Held,
    pub home: PathBuf,
    map: Option<Arc<MapEnv>>,
    /// The process's `HOME` before `Env::process` pointed it at this flow's, put back when the flow ends.
    saved_home: Option<std::ffi::OsString>,
    _dir: tempfile::TempDir,
}

impl Env {
    pub fn new(vars: &[(&str, &str)]) -> Self {
        Self::over_map(Held::Shared(PROCESS.read().unwrap_or_else(|e| e.into_inner())), vars)
    }

    /// A map environment that waits for no one, for a flow that only reads the tables of the groups the service registers.
    pub fn detached() -> Self {
        Self::over_map(Held::Free, &[])
    }

    fn over_map(held: Held, vars: &[(&str, &str)]) -> Self {
        let (home, dir) = fresh_home();
        let map = Arc::new(MapEnv::new(home.clone(), std::env::current_dir().ok(), std::env::temp_dir()));
        let env = Self { _held: held, home, map: Some(map), saved_home: None, _dir: dir };
        for (name, value) in vars {
            env.set(name, value);
        }
        env
    }

    /// The process environment, held by one flow: a fresh `HOME` and nothing else of the service's set but what the flow sets.
    pub fn process(vars: &[(&str, &str)]) -> Self {
        let held = PROCESS.write().unwrap_or_else(|e| e.into_inner());
        let (home, dir) = fresh_home();
        for name in Settings::names().iter().map(String::as_str).chain(["GH_TOKEN", "GITHUB_TOKEN"]) {
            std::env::remove_var(name);
        }
        let saved_home = std::env::var_os("HOME");
        std::env::set_var("HOME", &home);
        let env = Self { _held: Held::Alone(held), home, map: None, saved_home, _dir: dir };
        for (name, value) in vars {
            env.set(name, value);
        }
        env
    }

    /// What the service is given as its environment.
    pub fn source(&self) -> Arc<dyn Environment> {
        match &self.map {
            Some(map) => map.clone(),
            None => Arc::new(ProcessEnv),
        }
    }

    /// Where the providers' directories are in this flow: below the home, as the service's `Accounts::real` is given.
    pub fn providers_root(&self) -> PathBuf {
        self.home.join("providers")
    }

    pub fn set(&self, name: &str, value: &str) {
        match &self.map {
            Some(map) => map.set(name, value),
            None => std::env::set_var(name, value),
        }
    }

    pub fn unset(&self, name: &str) {
        match &self.map {
            Some(map) => map.unset(name),
            None => std::env::remove_var(name),
        }
    }

    /// A fresh directory under this flow's home.
    pub fn dir(&self, name: &str) -> PathBuf {
        let dir = self.home.join(name);
        std::fs::create_dir_all(&dir).expect("a directory under the home");
        dir
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        if self.map.is_none() {
            match &self.saved_home {
                Some(home) => std::env::set_var("HOME", home),
                None => std::env::remove_var("HOME"),
            }
        }
    }
}

fn fresh_home() -> (PathBuf, tempfile::TempDir) {
    let dir = tempfile::Builder::new().prefix("rca-flow-").tempdir().expect("a temporary home");
    (std::fs::canonicalize(dir.path()).expect("the home has a real path"), dir)
}

pub struct Options {
    pub secret: Option<&'static str>,
    pub state_dir: Option<PathBuf>,
    pub hard_timeout_ms: u64,
    pub heartbeat: Duration,
    pub default_cwd: Option<String>,
    pub requests_per_minute: u32,
    /// Why the shared cooldown ledger cannot be opened, when it cannot.
    pub ledger_error: Option<&'static str>,
}

impl Default for Options {
    fn default() -> Self {
        Self { secret: Some(SECRET), state_dir: None, hard_timeout_ms: 1_500, heartbeat: Duration::from_secs(15), default_cwd: None, requests_per_minute: rca_service::api::routes::REQUESTS_PER_MINUTE, ledger_error: None }
    }
}

pub struct Service {
    router: Router,
    app: Arc<App>,
    pub opencode: Arc<FakeOpencode>,
    pub commands: Arc<FakeCommands>,
    pub outbound: Arc<FakeOutbound>,
    pub spawner: Arc<ScriptedSpawner>,
    /// The model the `git.ai` group asks.
    pub models: Arc<ScriptedModelApi>,
    /// The time the schedules read: a Tuesday evening, still until a flow moves it.
    pub clock: Arc<FakeClock>,
    pub default_cwd: String,
}

/// The instant the schedules' flow clock starts at (2023-11-14 22:13:20 UTC).
pub const SCHEDULE_EPOCH_MS: u64 = 1_700_000_000_000;

/// The shared cooldown ledger as a file: `OPENCODE_POOL_LEDGER`, else `.agents/tmp/pool-cooldowns.json` under the run's directory.
struct FileLedgers {
    error: Option<&'static str>,
    env: Arc<dyn Environment>,
}

impl Ledgers for FileLedgers {
    fn open(&self, _runtime: &tokio::runtime::Handle, cwd: &str) -> Result<Box<dyn CooldownStore>, String> {
        if let Some(why) = self.error {
            return Err(why.to_string());
        }
        let path = self.env.var("OPENCODE_POOL_LEDGER").filter(|p| !p.is_empty()).unwrap_or_else(|| format!("{cwd}/.agents/tmp/pool-cooldowns.json"));
        Ok(Box::new(FileLedgerStore::new(path)))
    }
}

/// The agent API `main` serves (`rca_service::agent_api`) for `config`, its CLIs started by `spawner`, its per-device state and its device
/// bearers kept in the flow's home as the service keeps them.
pub fn agent_api_for(env: &Env, config: &Config, spawner: Arc<dyn Spawner>) -> AgentApi {
    agent_api_with(env, config, spawner, ScriptedModelApi::from_fixture(), Arc::new(FakeClock::at(SCHEDULE_EPOCH_MS)))
}

/// `agent_api_for` with the model the `git.ai` group asks: the providers' directories and logins are the real adapters under the flow's home.
pub fn agent_api_with(env: &Env, config: &Config, spawner: Arc<dyn Spawner>, models: Arc<ScriptedModelApi>, clock: Arc<FakeClock>) -> AgentApi {
    let root = env.home.join("providers").to_string_lossy().into_owned();
    let accounts = Accounts { dirs: Arc::new(ProviderFiles::at(&root)), login: Arc::new(CliLogin::at(&root)), models };
    let state = Arc::new(rca_service::infrastructure::agent_state::StateFile::at(env.home.join(".local/state/rca/agent-state.json")));
    let bearers = Arc::new(rca_service::infrastructure::device_bearers::BearerFile::at(env.home.join(".local/state/rca/device-bearers.json")));
    let schedules = rca_service::ScheduleParts { store: Arc::new(rca_service::infrastructure::schedules::ScheduleFile::at(env.home.join(".local/state/rca/schedules.json"))), clock, ids: Arc::new(CountingIds::default()) };
    let attachments = rca_service::AttachmentParts::real(env.home.join(".local/state/rca/attachments"));
    rca_service::agent_api(config, spawner, state, bearers, accounts, schedules, attachments)
}

/// The groups the service registers, for the flows that read the table and answer nothing.
pub fn agent_api(env: &Env) -> AgentApi {
    let home = env.home.to_string_lossy().into_owned();
    let config = Config { port: 0, allowed_origins: Vec::new(), api_secret: Some(SECRET.into()), commit: "flow-sha".into(), default_cwd: home.clone(), hard_timeout_ms: 1_500, state_dir: None, exec_root: home, heartbeat: Duration::from_secs(15), requests_per_minute: 120, edit_stats_flush: Duration::from_millis(50), state_seq_base: 0 };
    agent_api_for(env, &config, ScriptedSpawner::new())
}

/// The service over fresh doubles.
pub fn service(env: &Env, options: Options) -> Service {
    service_over(env, options, FakeOpencode::new(), FakeCommands::new(), FakeOutbound::new())
}

/// The service over the given doubles, as a restart of one would find them.
pub fn service_over(env: &Env, options: Options, opencode: Arc<FakeOpencode>, commands: Arc<FakeCommands>, outbound: Arc<FakeOutbound>) -> Service {
    let spawner = ScriptedSpawner::new();
    build(env, options, opencode, commands, outbound, spawner.clone(), spawner)
}

/// The service whose provider CLIs are started by `spawner` (a scripted program of the flow's own), the other ports doubled.
pub fn service_spawning(env: &Env, options: Options, spawner: Arc<dyn Spawner>) -> Service {
    build(env, options, FakeOpencode::new(), FakeCommands::new(), FakeOutbound::new(), spawner, ScriptedSpawner::new())
}

fn build(env: &Env, options: Options, opencode: Arc<FakeOpencode>, commands: Arc<FakeCommands>, outbound: Arc<FakeOutbound>, spawn: Arc<dyn Spawner>, spawner: Arc<ScriptedSpawner>) -> Service {
    let default_cwd = options.default_cwd.unwrap_or_else(|| env.home.to_string_lossy().into_owned());
    let config = Config {
        port: 0,
        allowed_origins: rca_service::api::cors::parse_allowed_origins(Some(&format!("{ALLOWED_ORIGIN}/, "))),
        api_secret: options.secret.map(str::to_string),
        commit: "flow-sha".into(),
        default_cwd: default_cwd.clone(),
        hard_timeout_ms: options.hard_timeout_ms,
        state_dir: options.state_dir.map(|d| d.to_string_lossy().into_owned()),
        exec_root: default_cwd.clone(),
        heartbeat: options.heartbeat,
        requests_per_minute: options.requests_per_minute,
        edit_stats_flush: Duration::from_millis(50),
        state_seq_base: 0,
    };
    let models = ScriptedModelApi::from_fixture();
    let clock = Arc::new(FakeClock::at(SCHEDULE_EPOCH_MS));
    let agent_api = agent_api_with(env, &config, spawn.clone(), models.clone(), clock.clone());
    let commands_port = Arc::new(CommandsPort(Arc::clone(&commands)));
    let outbound_port = Arc::new(OutboundPort(Arc::clone(&outbound)));
    let settings = env.source().settings();
    let client = rca_service::compose::opencode_client(Arc::new(OpencodePort(Arc::clone(&opencode))), env.source());
    let (orch, gateway) = rca_service::compose::orchestration(&config, &client, env.source().as_ref());
    let ports = Ports { documents: Arc::new(remote_chat_agent_host::Documents::new(commands_port.clone(), &settings)), speech: Arc::new(remote_chat_agent_speech::Speech::new(outbound_port.clone(), commands_port.clone(), env.source())), spawner: spawn, client, orch, gateway, commands: commands_port, outbound: outbound_port, ledgers: Arc::new(FileLedgers { error: options.ledger_error, env: env.source() }), files: Arc::new(remote_chat_agent_host::HostFiles), hosts: Arc::new(remote_chat_agent_host::Hosts), env: env.source() };
    let app = Arc::new(App::new(config, ports, agent_api));
    app.start();
    Service { router: router(Arc::clone(&app)), app, opencode, commands, outbound, spawner, models, clock, default_cwd }
}

pub struct Answer {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Answer {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|e| panic!("not JSON ({e}): {}", self.text()))
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }
}

/// A request on its way in.
pub struct Call {
    builder: axum::http::request::Builder,
    peer: SocketAddr,
}

pub fn call(method: Method, path: &str) -> Call {
    Call { builder: Request::builder().method(method).uri(path), peer: SocketAddr::from(([127, 0, 0, 1], 40_000)) }
}

pub fn get(path: &str) -> Call {
    call(Method::GET, path)
}

pub fn post(path: &str) -> Call {
    call(Method::POST, path)
}

pub fn delete(path: &str) -> Call {
    call(Method::DELETE, path)
}

impl Call {
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.builder = self.builder.header(name, value);
        self
    }

    /// The address the request arrives from, loopback unless a flow says otherwise.
    pub fn from(mut self, peer: [u8; 4]) -> Self {
        self.peer = SocketAddr::from((peer, 40_000));
        self
    }

    pub fn secret(self) -> Self {
        self.header("x-bot-secret", SECRET)
    }

    fn with(self, content_type: &str, body: Vec<u8>) -> (Request<Body>, SocketAddr) {
        (self.builder.header(header::CONTENT_TYPE, content_type).body(Body::from(body)).expect("a request"), self.peer)
    }

    pub fn json(self, body: Value) -> Sent {
        Sent(self.with("application/json", body.to_string().into_bytes()))
    }

    pub fn raw_json(self, body: &str) -> Sent {
        Sent(self.with("application/json", body.as_bytes().to_vec()))
    }

    pub fn form(self, form: Form) -> Sent {
        let (content_type, body) = form.encode();
        Sent(self.with(&content_type, body))
    }

    pub fn empty(self) -> Sent {
        Sent((self.builder.body(Body::empty()).expect("a request"), self.peer))
    }
}

pub struct Sent((Request<Body>, SocketAddr));

impl Service {
    /// A device bearer minted as an operator mints one, with the service secret: its token.
    pub async fn mint(&self, label: &str) -> String {
        let answer = self.send(post("/agent/v1/bearers").secret().json(json!({"label": label}))).await;
        assert_eq!(answer.status, StatusCode::CREATED, "{}", answer.text());
        answer.json()["token"].as_str().expect("the mint answers the token").to_string()
    }

    pub async fn send(&self, sent: impl Into<Sent>) -> Answer {
        let Sent((mut request, peer)) = sent.into();
        request.extensions_mut().insert(ConnectInfo(peer));
        let response = self.router.clone().oneshot(request).await.expect("the router answers");
        let (parts, body) = response.into_parts();
        let body = body.collect().await.expect("the body is read").to_bytes().to_vec();
        Answer { status: parts.status, headers: parts.headers, body }
    }
}

impl From<Call> for Sent {
    fn from(call: Call) -> Self {
        call.empty()
    }
}

/// One part of a form: its field name, a file's name and content type when it is a file, and its bytes.
type Part = (String, Option<(String, String)>, Vec<u8>);

/// A `multipart/form-data` body.
#[derive(Default)]
pub struct Form {
    parts: Vec<Part>,
}

impl Form {
    pub fn field(mut self, name: &str, value: &str) -> Self {
        self.parts.push((name.into(), None, value.as_bytes().to_vec()));
        self
    }

    pub fn file(mut self, name: &str, file_name: &str, mime: &str, bytes: &[u8]) -> Self {
        self.parts.push((name.into(), Some((file_name.into(), mime.into())), bytes.to_vec()));
        self
    }

    fn encode(self) -> (String, Vec<u8>) {
        let boundary = "flow-boundary-7MA4YWxkTrZu0gW";
        let mut body = Vec::new();
        for (name, file, bytes) in self.parts {
            body.extend(format!("--{boundary}\r\n").as_bytes());
            match file {
                Some((file_name, mime)) => body.extend(format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{file_name}\"\r\nContent-Type: {mime}\r\n\r\n").as_bytes()),
                None => body.extend(format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes()),
            }
            body.extend(bytes);
            body.extend(b"\r\n");
        }
        body.extend(format!("--{boundary}--\r\n").as_bytes());
        (format!("multipart/form-data; boundary={boundary}"), body)
    }
}

/// Waits until `done` holds, polling; fails the flow with `what` when it does not within five seconds.
pub async fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    for _ in 0..500 {
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("never happened: {what}");
}

pub fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/bench/fixtures").join(name)).expect("a fixture")
}

/// The router served over in-memory pipes, as `axum::serve` serves it over sockets, for the WebSocket.
pub struct Pipes(tokio::sync::mpsc::UnboundedReceiver<tokio::io::DuplexStream>);

impl axum::serve::Listener for Pipes {
    type Io = tokio::io::DuplexStream;
    type Addr = ();

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.0.recv().await {
            Some(io) => (io, ()),
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        Ok(())
    }
}

pub struct Socket {
    tx: futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<tokio::io::DuplexStream>, Message>,
    frames: tokio::sync::mpsc::UnboundedReceiver<Value>,
    pings: Arc<std::sync::atomic::AtomicUsize>,
    unread: Vec<Value>,
}

pub struct Served {
    dial: tokio::sync::mpsc::UnboundedSender<tokio::io::DuplexStream>,
}

impl Service {
    /// Serves the router on a real loopback port the way `main` does (peer addresses included) and returns the port.
    /// The schedules the service answers for: a flow ticks their clock itself.
    pub fn schedules(&self) -> Arc<rca_service_core::application::agent_groups::Schedules> {
        Arc::clone(&self.app.schedules)
    }

    pub async fn listen(&self) -> u16 {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("a free loopback port");
        let port = listener.local_addr().expect("a bound address").port();
        self.app.listening_on(port);
        let router = self.router.clone();
        tokio::spawn(async move { axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>()).await });
        port
    }

    /// Serves the router the way `main` does, over pipes instead of sockets.
    pub fn serve(&self) -> Served {
        let (dial, accepted) = tokio::sync::mpsc::unbounded_channel();
        let router = self.router.clone().layer(axum::Extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 40_001)))));
        tokio::spawn(async move { axum::serve(Pipes(accepted), router).await });
        Served { dial }
    }
}

impl Served {
    /// The upgrade's answer when it is refused (status and body), or the open socket.
    pub async fn connect(&self, query: &str) -> Result<Socket, (u16, String)> {
        let (client, server) = tokio::io::duplex(1 << 20);
        self.dial.send(server).expect("the server takes the pipe");
        let request = format!("ws://service/ws{query}");
        match tokio_tungstenite::client_async(request, client).await {
            Ok((stream, _)) => {
                let (tx, mut rx) = stream.split();
                let (frames_tx, frames) = tokio::sync::mpsc::unbounded_channel();
                let pings = Arc::new(std::sync::atomic::AtomicUsize::new(0));
                let counted = Arc::clone(&pings);
                tokio::spawn(async move {
                    while let Some(Ok(message)) = rx.next().await {
                        match message {
                            Message::Text(text) => {
                                let _ = frames_tx.send(serde_json::from_str(&text).expect("a JSON frame"));
                            }
                            Message::Ping(_) => {
                                counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            }
                            _ => {}
                        }
                    }
                });
                Ok(Socket { tx, frames, pings, unread: Vec::new() })
            }
            Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
                let body = response.body().as_ref().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
                Err((response.status().as_u16(), body))
            }
            Err(e) => panic!("the handshake failed: {e}"),
        }
    }
}

impl Socket {
    pub async fn send(&mut self, value: Value) {
        self.send_text(&value.to_string()).await;
    }

    pub async fn send_text(&mut self, text: &str) {
        self.tx.send(Message::Text(text.into())).await.expect("the socket takes the message");
    }

    pub async fn send_binary(&mut self, value: Value) {
        self.tx.send(Message::Binary(value.to_string().into_bytes().into())).await.expect("the socket takes the message");
    }

    pub async fn close(mut self) {
        let _ = self.tx.close().await;
    }

    pub fn pings(&self) -> usize {
        self.pings.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// The first unread frame that matches; frames before it stay unread.
    pub async fn next(&mut self, matching: impl Fn(&Value) -> bool) -> Value {
        if let Some(at) = self.unread.iter().position(&matching) {
            return self.unread.remove(at);
        }
        let wait = async {
            while let Some(frame) = self.frames.recv().await {
                if matching(&frame) {
                    return frame;
                }
                self.unread.push(frame);
            }
            panic!("the socket closed");
        };
        match tokio::time::timeout(Duration::from_secs(5), wait).await {
            Ok(frame) => frame,
            Err(_) => panic!("no matching frame; unread: {:?}", self.unread),
        }
    }

    pub async fn first(&mut self) -> Value {
        self.next(|_| true).await
    }

    /// What arrives within `ms` besides what is unread already.
    pub async fn quiet(&mut self, ms: u64) -> Vec<Value> {
        tokio::time::sleep(Duration::from_millis(ms)).await;
        let mut out = std::mem::take(&mut self.unread);
        while let Ok(frame) = self.frames.try_recv() {
            out.push(frame);
        }
        out
    }

    pub async fn request(&mut self, id: &str, method: &str, params: Value) -> Value {
        self.send(json!({ "kind": "request", "id": id, "method": method, "params": params })).await;
        let id = id.to_string();
        self.next(move |f| f["kind"] == "response" && f["id"] == id.as_str()).await
    }

    pub async fn dispatch(&mut self, id: &str, command: Value) -> Value {
        self.request(id, "orchestration.dispatchCommand", command).await
    }
}
