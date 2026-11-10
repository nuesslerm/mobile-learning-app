//! In-process doubles of the service ports `Opencode`, `Commands` and `Outbound`: opencode's HTTP API with its event stream, the
//! programs the service runs, and the HTTP services it posts to. Each records what it was asked and answers what the test scripts.

use async_trait::async_trait;
use crate::application::ports::{Answer, Commands, DataStream, ExecError, Opencode, Output, Outbound, Payload, Post, PostError, Reply};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

fn locked<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Clone, Debug)]
pub struct Recorded {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub body: Option<Value>,
}

impl Recorded {
    pub fn directory(&self) -> Option<&str> {
        self.query.get("directory").map(String::as_str)
    }
}

/// What a prompt sent to opencode carried: the session it names and the body.
pub struct Prompt {
    pub session: String,
    pub body: Value,
}

impl Prompt {
    pub fn text(&self) -> String {
        self.body["parts"][0]["text"].as_str().unwrap_or_default().to_string()
    }
}

/// What the "model" does when a prompt arrives: the events it emits, in order. None emitted is a model that never answers.
pub type Script = Arc<dyn Fn(&Prompt) -> Vec<Value> + Send + Sync>;

struct OpencodeState {
    requests: Vec<Recorded>,
    overrides: Vec<(String, String, u16, Value)>,
    sessions: usize,
    script: Script,
    subscribers: Vec<mpsc::UnboundedSender<Result<String, String>>>,
    pending: Vec<Value>,
    connects: usize,
    connects_at_create: HashMap<String, usize>,
    down: bool,
}

/// `opencode serve`: the session, prompt, abort, question, permission and inventory calls, and `/event`. A prompt's script runs once
/// an event subscription opened after its session was created, as a real server's events arrive after the subscription.
pub struct FakeOpencode {
    state: Mutex<OpencodeState>,
}

pub fn ev(kind: &str, properties: Value) -> Value {
    json!({ "type": kind, "properties": properties })
}

pub fn idle(session: &str) -> Value {
    ev("session.idle", json!({ "sessionID": session }))
}

/// A text part said in one snapshot.
pub fn says(session: &str, text: &str) -> Value {
    ev("message.part.updated", json!({ "sessionID": session, "part": { "id": format!("p-{session}"), "type": "text", "text": text, "messageID": format!("m-{session}"), "sessionID": session } }))
}

pub fn fails(session: &str, message: &str) -> Value {
    ev("session.error", json!({ "sessionID": session, "error": { "name": "ProviderError", "data": { "message": message } } }))
}

fn session_info(id: &str, directory: &str) -> Value {
    json!({ "id": id, "slug": id, "projectID": "p1", "directory": directory, "title": "fake", "version": "1", "time": { "created": 1, "updated": 1 } })
}

fn split(path: &str) -> (String, HashMap<String, String>) {
    let url = url::Url::parse(&format!("http://opencode{path}")).expect("a path opencode is asked for");
    (url.path().to_string(), url.query_pairs().into_owned().collect())
}

impl FakeOpencode {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(OpencodeState {
                requests: Vec::new(),
                overrides: Vec::new(),
                sessions: 0,
                script: Arc::new(|p: &Prompt| vec![idle(&p.session)]),
                subscribers: Vec::new(),
                pending: Vec::new(),
                connects: 0,
                connects_at_create: HashMap::new(),
                down: false,
            }),
        })
    }

    pub fn on_prompt(&self, script: Script) {
        locked(&self.state).script = script;
    }

    /// Answers `method path` (the path without its query) with `status` and `body` from now on.
    pub fn respond(&self, method: &str, path: &str, status: u16, body: Value) {
        locked(&self.state).overrides.insert(0, (method.into(), path.into(), status, body));
    }

    /// Nothing listening where opencode should be.
    pub fn go_down(&self) {
        locked(&self.state).down = true;
    }

    pub fn requests(&self) -> Vec<Recorded> {
        locked(&self.state).requests.clone()
    }

    pub fn forget_requests(&self) {
        locked(&self.state).requests.clear();
    }

    /// The requests to `method` whose path (without the query) is `path`, or ends with it when it starts with `*`.
    pub fn find(&self, method: &str, path: &str) -> Vec<Recorded> {
        self.requests().into_iter().filter(|r| r.method == method && matches(path, &r.path)).collect()
    }

    /// Every call but the probes and the event subscriptions.
    pub fn calls(&self) -> Vec<Recorded> {
        self.requests().into_iter().filter(|r| r.path != "/find/file" && r.path != "/event").collect()
    }

    pub fn subscribers(&self) -> usize {
        locked(&self.state).subscribers.iter().filter(|s| !s.is_closed()).count()
    }

    /// opencode closing every event stream it serves.
    pub fn end_streams(&self) {
        locked(&self.state).subscribers.clear();
    }

    pub fn emit(&self, event: Value) {
        let mut state = locked(&self.state);
        state.subscribers.retain(|s| !s.is_closed());
        if state.subscribers.is_empty() {
            state.pending.push(event);
            return;
        }
        for subscriber in &state.subscribers {
            let _ = subscriber.send(Ok(event.to_string()));
        }
    }

    fn prompted(self: &Arc<Self>, session: String, body: Value) {
        let (seen, script) = {
            let state = locked(&self.state);
            (state.connects_at_create.get(&session).copied().unwrap_or(state.connects), Arc::clone(&state.script))
        };
        let me = Arc::clone(self);
        tokio::spawn(async move {
            for _ in 0..400 {
                if locked(&me.state).connects > seen {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
            for event in script(&Prompt { session, body }) {
                me.emit(event);
            }
        });
    }
}

fn matches(pattern: &str, path: &str) -> bool {
    match pattern.strip_prefix('*') {
        Some(suffix) => path.ends_with(suffix),
        None => pattern == path,
    }
}

fn reply(status: u16, body: &Value) -> Reply {
    Reply { status, body: if body.is_null() { Vec::new() } else { body.to_string().into_bytes() } }
}

/// The handle tests hold: the double behind an `Arc` the app's port shares.
pub struct OpencodePort(pub Arc<FakeOpencode>);

#[async_trait]
impl Opencode for OpencodePort {
    async fn request(&self, method: &str, path: &str, body: Option<&Value>, _timeout: Duration) -> Result<Reply, String> {
        let fake = &self.0;
        let (route, query) = split(path);
        let mut state = locked(&fake.state);
        state.requests.push(Recorded { method: method.into(), path: route.clone(), query: query.clone(), body: body.cloned() });
        if state.down {
            return Err(format!("error sending request for url (http://127.0.0.1:1{path})"));
        }
        if let Some((_, _, status, body)) = state.overrides.iter().find(|(m, p, _, _)| m == method && matches(p, &route)) {
            return Ok(reply(*status, body));
        }
        let segments: Vec<&str> = route.trim_start_matches('/').split('/').collect();
        let answer = match (method, segments.as_slice()) {
            ("GET", ["find", "file"]) => reply(200, &json!([])),
            ("POST", ["session"]) => {
                state.sessions += 1;
                let id = format!("oc-ses-{}", state.sessions);
                let connects = state.connects;
                state.connects_at_create.insert(id.clone(), connects);
                reply(200, &session_info(&id, query.get("directory").map_or("/work", String::as_str)))
            }
            ("GET" | "PATCH", ["session", id]) => reply(200, &session_info(id, "/work")),
            ("POST", ["session", _, "abort"]) => reply(200, &json!(true)),
            ("POST", ["session", id, "prompt_async"]) => {
                let id = (*id).to_string();
                drop(state);
                fake.prompted(id, body.cloned().unwrap_or(Value::Null));
                return Ok(reply(204, &Value::Null));
            }
            ("POST", ["api", "session", _, "prompt"]) => reply(200, &json!({})),
            ("POST", ["question", _, "reply" | "reject"]) | ("POST", ["permission", _, "reply"]) => reply(200, &json!(true)),
            ("GET", ["mcp"]) => reply(200, &json!({})),
            ("GET", ["command"]) | ("GET", ["api", "model"]) => reply(200, &json!([])),
            _ => reply(404, &json!({ "error": format!("fake opencode: no route for {method} {route}") })),
        };
        Ok(answer)
    }

    async fn open_stream(&self, path: &str) -> Result<DataStream, String> {
        let fake = &self.0;
        let (route, query) = split(path);
        let mut state = locked(&fake.state);
        state.requests.push(Recorded { method: "GET".into(), path: route.clone(), query, body: None });
        if state.down {
            return Err(format!("error sending request for url (http://127.0.0.1:1{path})"));
        }
        if let Some((_, _, status, _)) = state.overrides.iter().find(|(m, p, _, _)| m == "GET" && matches(p, &route)) {
            return Err(format!("opencode {path} responded with {status}"));
        }
        let (tx, rx) = mpsc::unbounded_channel();
        let _ = tx.send(Ok(ev("server.connected", json!({})).to_string()));
        state.connects += 1;
        for event in std::mem::take(&mut state.pending) {
            let _ = tx.send(Ok(event.to_string()));
        }
        state.subscribers.push(tx);
        Ok(Box::pin(tokio_stream_from(rx)))
    }
}

fn tokio_stream_from(mut rx: mpsc::UnboundedReceiver<Result<String, String>>) -> impl futures_util::Stream<Item = Result<String, String>> + Send {
    futures_util::stream::poll_fn(move |cx| rx.poll_recv(cx))
}

#[derive(Clone, Debug)]
pub struct Run {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
}

impl Run {
    pub fn is(&self, program: &str, args: &[&str]) -> bool {
        self.program == program && self.args.iter().map(String::as_str).eq(args.iter().copied())
    }

    pub fn ok(&self, stdout: &str) -> Option<Result<Output, ExecError>> {
        Some(Ok(Output { stdout: stdout.into() }))
    }

    /// The program exiting with a failure, worded as the real runner words it.
    pub fn fails(&self, stdout: &str, stderr: &str) -> Option<Result<Output, ExecError>> {
        let args: Vec<&str> = self.args.iter().map(String::as_str).collect();
        Some(Err(ExecError::failed(&self.program, &args, stdout.into(), stderr.into())))
    }
}

type Responder = Box<dyn Fn(&Run) -> Option<Result<Output, ExecError>> + Send + Sync>;

/// The programs the service runs. A program no responder answers is one that is not installed.
pub struct FakeCommands {
    runs: Mutex<Vec<Run>>,
    responder: Mutex<Responder>,
}

impl FakeCommands {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { runs: Mutex::new(Vec::new()), responder: Mutex::new(Box::new(|_| None)) })
    }

    pub fn answer(&self, responder: impl Fn(&Run) -> Option<Result<Output, ExecError>> + Send + Sync + 'static) {
        *locked(&self.responder) = Box::new(responder);
    }

    pub fn runs(&self) -> Vec<Run> {
        locked(&self.runs).clone()
    }

    pub fn of(&self, program: &str) -> Vec<Run> {
        self.runs().into_iter().filter(|r| r.program.ends_with(program)).collect()
    }

    pub fn forget(&self) {
        locked(&self.runs).clear();
    }
}

pub struct CommandsPort(pub Arc<FakeCommands>);

#[async_trait]
impl Commands for CommandsPort {
    async fn run(&self, program: &str, args: &[&str], cwd: Option<&Path>, _timeout: Option<Duration>) -> Result<Output, ExecError> {
        let run = Run { program: program.into(), args: args.iter().map(|a| (*a).to_string()).collect(), cwd: cwd.map(Path::to_path_buf) };
        locked(&self.0.runs).push(run.clone());
        let answered = (locked(&self.0.responder))(&run);
        answered.unwrap_or_else(|| Err(ExecError::not_started(program, "not found")))
    }
}

#[derive(Clone, Debug)]
pub struct Posted {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub basic_auth: Option<(String, String)>,
    pub json: Option<Value>,
    /// Form parts as (name, file name, bytes).
    pub form: Vec<(String, Option<String>, Vec<u8>)>,
}

type PostResponder = Box<dyn Fn(&Posted) -> Result<(u16, Value), String> + Send + Sync>;

/// The HTTP services other than opencode: the speech-to-text engines, the edit-stats table, Telegram.
pub struct FakeOutbound {
    posts: Mutex<Vec<Posted>>,
    responder: Mutex<PostResponder>,
}

impl FakeOutbound {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { posts: Mutex::new(Vec::new()), responder: Mutex::new(Box::new(|_| Ok((201, json!({}))))) })
    }

    pub fn answer(&self, responder: impl Fn(&Posted) -> Result<(u16, Value), String> + Send + Sync + 'static) {
        *locked(&self.responder) = Box::new(responder);
    }

    pub fn posts(&self) -> Vec<Posted> {
        locked(&self.posts).clone()
    }

    pub fn to(&self, url_part: &str) -> Vec<Posted> {
        self.posts().into_iter().filter(|p| p.url.contains(url_part)).collect()
    }
}

pub struct OutboundPort(pub Arc<FakeOutbound>);

#[async_trait]
impl Outbound for OutboundPort {
    async fn post(&self, post: Post) -> Result<Answer, PostError> {
        let (json, form) = match post.payload {
            Payload::Json(value) => (Some(value), Vec::new()),
            Payload::Form(parts) => (None, parts.into_iter().map(|p| (p.name, p.file_name, p.bytes)).collect()),
        };
        let posted = Posted { url: post.url, headers: post.headers, basic_auth: post.basic_auth, json, form };
        let answered = (locked(&self.0.responder))(&posted);
        locked(&self.0.posts).push(posted);
        match answered {
            Ok((status, body)) => Ok(Answer { status, body: body.to_string().into_bytes() }),
            Err(message) => Err(PostError { message: message.clone(), without_url: message }),
        }
    }
}
