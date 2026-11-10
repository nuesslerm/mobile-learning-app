//! The chain's processes and the phone that drives them. The service, the executor and the mapper are real processes; the executor and the mapper
//! reach the service through a `Tap`, so every agent API call the mapper makes and every tool the service asks the executor is seen.
use crate::support::*;
use exec_testkit::{Case, HostedClis, RcaExec, RcaService, ScriptedCli, Suite, Tap, TempRepo};
use std::collections::BTreeSet;
use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub static SUITE: Suite = Suite::new("t3-serve-integration", "t3");

/// The service's secret: the executor channel's, and the operator's who mints the mapper's bearer.
pub const SECRET: &str = "chain-service-secret";
const BIN: &str = env!("CARGO_BIN_EXE_t3-serve");
const METHODS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/plans/rca-parity/coverage/t3-methods.json");

/// How long a turn of the scripted CLI may take to settle.
const SETTLE: Duration = Duration::from_secs(40);

/// The coverage rows of the reference scan, so a trace can only name a row the table has.
fn known_rows() -> &'static BTreeSet<String> {
    static ROWS: OnceLock<BTreeSet<String>> = OnceLock::new();
    ROWS.get_or_init(|| {
        let list: Value = serde_json::from_str(&std::fs::read_to_string(METHODS).unwrap_or_else(|e| panic!("{METHODS}: {e}"))).unwrap();
        list["rows"].as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap().to_string()).collect()
    })
}

/// `ids`, each checked to be a row of `t3-methods.json`.
pub fn rows<'a>(ids: &[&'a str]) -> Vec<&'a str> {
    for id in ids {
        assert!(known_rows().contains(*id), "{id} is no row of docs/plans/rca-parity/coverage/t3-methods.json");
    }
    ids.to_vec()
}

/// A client of rca's agent API that does not go through the tap: what the test reads of rca itself (the ids it made, the state it holds) and the
/// requests a phone cannot make today (`queue.add`), kept out of every call's window.
pub struct Direct {
    client: agent_api_client::Client,
}

impl Direct {
    /// The method `name` of `group`, which the contract's own constants spell.
    pub fn call(&mut self, group: &'static str, name: &'static str, params: Value) -> Result<Value, agent_api_client::Error> {
        self.client.call(agent_api_proto::Method::new(agent_api_proto::GroupName::new(group), name), params)
    }

    /// Every event of `thread` in rca's log whose `type` is `kind` (none while rca has not made the thread).
    pub fn events(&mut self, thread: &str, kind: &str) -> Vec<Value> {
        let page = match self.call("agent.threads", "thread.history", json!({"thread": thread, "after_seq": 0, "limit": 1000})) {
            Ok(page) => page,
            Err(e) if e.code == "THREAD_NOT_FOUND" => return Vec::new(),
            Err(e) => panic!("rca answers the history: {e:?}"),
        };
        page["events"].as_array().cloned().unwrap_or_default().into_iter().filter(|e| e["type"] == kind).collect()
    }

    /// The first event of `kind` once rca has logged one.
    pub fn wait_event(&mut self, thread: &str, kind: &str) -> Value {
        let deadline = Instant::now() + SETTLE;
        loop {
            if let Some(event) = self.events(thread, kind).into_iter().next() {
                return event;
            }
            assert!(Instant::now() < deadline, "rca never logged {kind} on {thread}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

pub struct Chain {
    pub tap: Tap,
    pub cli: ScriptedCli,
    pub repo: TempRepo,
    pub service: RcaService,
    pub addr: SocketAddr,
    pub state: tempfile::TempDir,
    /// Where the executor puts the worktrees it makes, and the one directory outside its root it may list (`alpha`, `beta`, and a file).
    pub base: PathBuf,
    pub shared: PathBuf,
    /// The fake `gh` and `glab` first on the executor's `PATH`, for the chains that read and act on hosted source control.
    pub hosted: Option<HostedClis>,
    _dirs: tempfile::TempDir,
    server: Child,
    errlog: PathBuf,
    _exec: RcaExec,
}

/// A paired phone's two sockets: one for requests, one for the streams it keeps open (an open stream's chunks would interleave with answers).
pub struct Phone {
    pub calls: Rpc,
    pub watch: Rpc,
    /// The `Authorization` header value of the phone's paired session, for the HTTP routes.
    pub bearer: String,
}

impl Chain {
    /// The service with the scripted CLIs first on its `PATH`, the executor attached for workspace `w1` through the tap on a new repository, and
    /// `t3-serve serve` pointed at the tap with a bearer the operator minted. It returns once serve says it listens.
    pub fn start() -> Chain {
        Chain::start_with(None)
    }

    /// `start` with the executor's `PATH` led by the fake `gh` and `glab` (`Chain::hosted`).
    pub fn start_hosted() -> Chain {
        Chain::start_with(Some(HostedClis::install()))
    }

    /// The fake `gh` and `glab` of a chain made by `start_hosted`.
    pub fn clis(&self) -> &HostedClis {
        self.hosted.as_ref().expect("the chain was started with start_hosted")
    }

    fn start_with(hosted: Option<HostedClis>) -> Chain {
        let cli = ScriptedCli::install();
        let repo = TempRepo::new();
        let service = RcaService::start(SECRET, &cli.path());
        let tap = Tap::to(service.addr());
        let dirs = tempfile::Builder::new().prefix("t3-serve-dirs-").tempdir().unwrap();
        let root = std::fs::canonicalize(dirs.path()).unwrap();
        let (base, shared) = (root.join("worktrees"), root.join("shared"));
        for dir in [&base, &shared.join("alpha"), &shared.join("beta")] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(shared.join("notes.txt"), "not a directory").unwrap();
        let flags = ["--worktree-base", base.to_str().unwrap(), "--browse-allow", shared.to_str().unwrap()];
        let exec = match &hosted {
            Some(clis) => RcaExec::serve_on(&tap.url(), "w1", &repo, SECRET, &flags, &clis.path()),
            None => RcaExec::serve_with(&tap.url(), "w1", &repo, SECRET, &flags),
        };
        let bearer = service.mint("t3 chain");
        let state = tempfile::Builder::new().prefix("t3-serve-chain-").tempdir().unwrap();
        let errlog = state.path().join("serve.err");
        let (server, addr) = spawn_serve(state.path(), &errlog, &tap.url(), &bearer);
        Chain { tap, cli, repo, service, addr, state, base, shared, hosted, _dirs: dirs, server, errlog, _exec: exec }
    }

    /// `t3-serve serve` stopped and started again on the same state directory and the same service, as a restart of the server is: what it kept in its
    /// own store stays, what it held in memory is gone, and a phone pairs and connects again (the address is new).
    pub fn restart_serve(&mut self) {
        let _ = self.server.kill();
        let _ = self.server.wait();
        let bearer = self.service.mint("t3 chain restarted");
        let (server, addr) = spawn_serve(self.state.path(), &self.errlog, &self.tap.url(), &bearer);
        self.server = server;
        self.addr = addr;
    }

    /// A pairing code minted by a separate `t3-serve pair` on the server's state directory, as an operator hands one to a phone, with the scopes the
    /// phone is to get.
    pub fn pair_with(&self, scopes: &[String]) -> String {
        let mut command = Command::new(BIN);
        command.args(["t3", "pair", "--state-dir"]).arg(self.state.path());
        if !scopes.is_empty() {
            command.args(["--scope", &scopes.join(",")]);
        }
        let out = command.stdin(Stdio::null()).output().unwrap();
        assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        serde_json::from_slice::<Value>(&out.stdout).unwrap()["code"].as_str().unwrap().to_string()
    }

    pub fn direct(&self) -> Direct {
        let config = agent_api_client::Config::new(format!("ws://{}", self.service.addr()), self.service.mint("chain side door"));
        Direct { client: agent_api_client::Client::connect(config).expect("the side door opens") }
    }

    /// What the mapper and the service have logged so far: a failure message's evidence.
    pub fn evidence(&self) -> String {
        format!("t3-serve serve:\n{}\nrca-service:\n{}", std::fs::read_to_string(&self.errlog).unwrap_or_default(), self.service.logs())
    }
}

impl Drop for Chain {
    fn drop(&mut self) {
        let _ = self.server.kill();
        let _ = self.server.wait();
    }
}

/// `t3-serve serve` on `state` pointed at the service through the tap, listening once it says so.
fn spawn_serve(state: &std::path::Path, errlog: &std::path::Path, tap_url: &str, bearer: &str) -> (Child, SocketAddr) {
    let mut server = Command::new(BIN)
        .args(["t3", "serve", "--port", "0", "--state-dir"])
        .arg(state)
        .args(["--rca-url", tap_url, "--rca-workspace", "w1"])
        .env("T3_RCA_BEARER", bearer)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(std::fs::OpenOptions::new().create(true).append(true).open(errlog).unwrap())
        .spawn()
        .expect("t3-serve starts");
    let mut line = String::new();
    BufReader::new(server.stdout.as_mut().unwrap()).read_line(&mut line).unwrap();
    let listening: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("t3-serve serve printed {line:?} ({e}); its log:\n{}", std::fs::read_to_string(errlog).unwrap_or_default()));
    assert_eq!((listening["ok"].clone(), listening["event"].as_str()), (json!(true), Some("listening")), "{listening}");
    (server, SocketAddr::from(([127, 0, 0, 1], listening["port"].as_u64().unwrap() as u16)))
}

/// One HTTP request as a call of the trace: served when the status is `status`.
pub fn http_call(case: &mut Case, chain: &Chain, ids: &[&str], request: (&str, &str, &[(&str, &str)], &str), status: u16) -> Reply {
    let (method, path, headers, body) = request;
    case.call(&chain.tap, &rows(ids), || {
        let reply = http(chain.addr, method, path, headers, body);
        (reply.status == status, reply)
    })
}

impl Phone {
    /// Pairs and connects the way the phone does, each step a call of the trace: the environment descriptor, the code's exchange for a bearer,
    /// the session, a ticket for each of the two sockets, and the sockets themselves.
    pub fn connect(case: &mut Case, chain: &Chain) -> Phone {
        Phone::connect_with(case, chain, &[])
    }

    /// The same for a phone paired with `scopes` (the default set when none is given).
    pub fn connect_with(case: &mut Case, chain: &Chain, scopes: &[String]) -> Phone {
        let descriptor = http_call(case, chain, &["http:metadata.descriptor"], ("GET", "/.well-known/t3/environment", &[], ""), 200);
        assert!(descriptor.body["environmentId"].is_string(), "{:?}", descriptor.body);
        let code = chain.pair_with(scopes);
        let exchanged = case.call(&chain.tap, &rows(&["http:auth.token"]), || {
            let reply = exchange(chain.addr, &code);
            (reply.status == 200, reply)
        });
        let bearer = format!("Bearer {}", exchanged.body["access_token"].as_str().expect("the exchange answers a token"));
        let auth = [("Authorization", bearer.as_str())];
        let session = http_call(case, chain, &["http:auth.session"], ("GET", "/api/auth/session", &auth, ""), 200);
        assert_eq!(session.body["authenticated"], json!(true), "{:?}", session.body);
        let first = http_call(case, chain, &["http:auth.webSocketTicket"], ("POST", "/api/auth/websocket-ticket", &auth, ""), 200);
        let second = http_call(case, chain, &["http:auth.webSocketTicketFor"], ("POST", "/api/auth/websocket-ticket/rpc", &auth, ""), 200);
        let open = |ticket: &Reply| open_socket(chain.addr, &format!("orchestrationProtocol=2&wsTicket={}", ticket.body["ticket"].as_str().unwrap())).expect("the socket opens");
        Phone { calls: open(&first), watch: open(&second), bearer: bearer.clone() }
    }

    /// A GET of an orchestration route as one call of the trace: served when it answers 200.
    pub fn get(&self, case: &mut Case, chain: &Chain, ids: &[&str], path: &str) -> Reply {
        let headers = [("Authorization", self.bearer.as_str()), ("x-t3-orchestration-protocol", "2")];
        http_call(case, chain, ids, ("GET", path, &headers, ""), 200)
    }

    /// A request on the calls socket as one call of the trace: served when it exits with a success.
    pub fn call(&mut self, case: &mut Case, chain: &Chain, ids: &[&str], tag: &str, payload: Value) -> Value {
        case.call(&chain.tap, &rows(ids), || {
            let exit = self.calls.call(tag, payload);
            (exit["_tag"] == "Success", exit)
        })
    }

    /// A stream opened on the watch socket as one call of the trace: served when its first chunk holds values. The stream's id and those values.
    pub fn stream(&mut self, case: &mut Case, chain: &Chain, ids: &[&str], tag: &str, payload: Value) -> (Value, Vec<Value>) {
        case.call(&chain.tap, &rows(ids), || {
            let id = self.watch.send(tag, payload);
            let values = self.watch.chunk(&id);
            (!values.is_empty(), (id, values))
        })
    }

    /// Every frame the watch socket gets until it is quiet, chunks acknowledged.
    pub fn drain_watch(&mut self) -> Vec<Value> {
        let mut frames = Vec::new();
        while let Some(frame) = self.watch.try_recv(Duration::from_millis(500)) {
            if frame["_tag"] == "Chunk" {
                self.watch.ack(&frame["requestId"]);
            }
            frames.push(frame);
        }
        frames
    }

    /// The thread's projection once none of its runs is running.
    pub fn settled(&mut self, thread: &str) -> Value {
        let deadline = Instant::now() + SETTLE;
        loop {
            let projection = success(&self.calls.call("orchestration.getThreadProjection", json!({"threadId": thread}))).clone();
            let running = projection["runs"].as_array().is_some_and(|runs| runs.iter().any(|r| r["status"] == "running"));
            if !running || Instant::now() > deadline {
                return projection;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// A message to `thread` as one call of the trace, which ends when the run it started has: served when the command was accepted and the
    /// run completed, so the call's window holds the whole turn (the agent API calls, the executor's tools).
    pub fn message(&mut self, case: &mut Case, chain: &Chain, thread: &str, command_id: &str, text: &str) -> Value {
        let command = json!({"type": "message.dispatch", "commandId": command_id, "threadId": thread, "message": {"messageId": format!("m-{command_id}"), "text": text}});
        case.call(&chain.tap, &rows(&["rpc:orchestration.dispatchCommand", "command:message.dispatch"]), || {
            let exit = self.calls.call("orchestration.dispatchCommand", command);
            let projection = self.settled(thread);
            let completed = projection["runs"].as_array().and_then(|r| r.last()).is_some_and(|r| r["status"] == "completed");
            (exit["_tag"] == "Success" && completed, projection)
        })
    }

    /// A thread command as one call of the trace, with `extra` rows beside the dispatch row of its type: served when the command was accepted.
    pub fn command(&mut self, case: &mut Case, chain: &Chain, command: Value) -> Value {
        let kind = command["type"].as_str().expect("a command has a type").to_string();
        let row = format!("command:{kind}");
        case.call(&chain.tap, &rows(&["rpc:orchestration.dispatchCommand", &row]), || {
            let exit = self.calls.call("orchestration.dispatchCommand", command);
            (exit["_tag"] == "Success", exit)
        })
    }

    /// A command that starts a run, which ends when the thread has no running run: served when it was accepted and the thread settled with
    /// `expect` true of its projection, so the window holds the executor tools the run used.
    pub fn command_settled(&mut self, case: &mut Case, chain: &Chain, command: Value, thread: &str, expect: impl Fn(&Value) -> bool) -> Value {
        let kind = command["type"].as_str().expect("a command has a type").to_string();
        let row = format!("command:{kind}");
        case.call(&chain.tap, &rows(&["rpc:orchestration.dispatchCommand", &row]), || {
            let exit = self.calls.call("orchestration.dispatchCommand", command);
            let projection = self.settled(thread);
            (exit["_tag"] == "Success" && expect(&projection), projection)
        })
    }

    /// The thread's newest run id, once it has one.
    pub fn run_of(&mut self, thread: &str) -> String {
        let deadline = Instant::now() + SETTLE;
        loop {
            let projection = success(&self.calls.call("orchestration.getThreadProjection", json!({"threadId": thread}))).clone();
            if let Some(id) = projection["runs"].as_array().and_then(|r| r.last()).and_then(|r| r["id"].as_str()) {
                return id.to_string();
            }
            assert!(Instant::now() < deadline, "{thread} never got a run");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// A project on the repository the executor serves, and a thread on it that runs Claude Code (the scripted `claude`).
    pub fn seed(&mut self, case: &mut Case, chain: &Chain) {
        let root = chain.repo.path().display().to_string();
        let project = json!({"type": "project.create", "commandId": "c-p1", "projectId": "p1", "title": "Chain", "workspaceRoot": root});
        success(&self.call(case, chain, &["rpc:projects.mutate", "mutation:project.create"], "projects.mutate", project));
        let thread = json!({"type": "thread.create", "commandId": "c-t1", "threadId": "t1", "projectId": "p1", "title": "Notes", "modelSelection": {"instanceId": "claudeAgent", "model": "opus"}});
        success(&self.call(case, chain, &["rpc:orchestration.dispatchCommand", "command:thread.create"], "orchestration.dispatchCommand", thread));
    }
}
