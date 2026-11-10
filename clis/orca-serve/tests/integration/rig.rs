//! The chain's processes and the phone that drives them. The service, the executor and `orca-serve` are real processes; the executor and
//! `orca-serve` reach the service through a `Tap`, so every agent API call the mapper makes is seen.
use exec_testkit::{Case, HostedClis, RcaExec, RcaService, ScriptedCli, Suite, Tap, TempRepo};
use orca_crypto::pairing::PairingOffer;
use orca_serve::testing::PhoneClient;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;

pub static SUITE: Suite = Suite::new("orca-serve-integration", "orca");

/// The service's secret: the executor channel's, and the operator's who mints the mapper's bearer.
pub const SECRET: &str = "chain-service-secret";
const BIN: &str = env!("CARGO_BIN_EXE_orca-serve");
const METHODS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/plans/rca-parity/coverage/orca-methods.json");

fn known_rows() -> &'static BTreeSet<String> {
    static ROWS: OnceLock<BTreeSet<String>> = OnceLock::new();
    ROWS.get_or_init(|| {
        let list: Value = serde_json::from_str(&std::fs::read_to_string(METHODS).unwrap_or_else(|e| panic!("{METHODS}: {e}"))).unwrap();
        list["rows"].as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap().to_string()).collect()
    })
}

/// `ids`, each checked to be a row of `orca-methods.json`.
pub fn rows<'a>(ids: &[&'a str]) -> Vec<&'a str> {
    for id in ids {
        assert!(known_rows().contains(*id), "{id} is no row of docs/plans/rca-parity/coverage/orca-methods.json");
    }
    ids.to_vec()
}

pub struct Chain {
    pub tap: Tap,
    pub service: RcaService,
    pub addr: SocketAddr,
    pub state: tempfile::TempDir,
    errlog: PathBuf,
    server: Child,
    /// The scripted CLIs the service starts: what each turn does is set here.
    pub cli: ScriptedCli,
    /// The executor's repository: what the phone lists and reads is on this disk.
    pub repo: TempRepo,
    /// Where the executor puts the worktrees it makes.
    pub base: PathBuf,
    _base: tempfile::TempDir,
    /// The executor's own home: its user-level skills are read from here.
    pub home: tempfile::TempDir,
    _exec: RcaExec,
    /// The fake `gh` and `glab` first on the executor's `PATH`, for the chains that act on hosted source control.
    hosted: Option<HostedClis>,
}

impl Chain {
    /// The service and the executor with the scripted CLIs first on their `PATH`, the executor attached for workspace `w1` through the tap on a new repository, and
    /// `orca-serve serve` pointed at the tap with a bearer the operator minted (and the web bundle `web` when given). It returns once serve says
    /// it listens.
    pub fn start(web: Option<&Path>) -> Chain {
        Chain::start_with_env(web, &[])
    }

    /// `start`, with `env` also in the service's environment.
    pub fn start_with_env(web: Option<&Path>, env: &[(&str, &str)]) -> Chain {
        Chain::start_full(web, env, None)
    }

    /// `start` with the executor's `PATH` led by the fake `gh` and `glab` (`Chain::clis`).
    pub fn start_hosted() -> Chain {
        Chain::start_full(None, &[], Some(HostedClis::install()))
    }

    /// The fake `gh` and `glab` the executor runs: what they print is set here, and what they were started with is read back.
    pub fn clis(&self) -> &HostedClis {
        self.hosted.as_ref().expect("the chain was started with start_hosted")
    }

    fn start_full(web: Option<&Path>, env: &[(&str, &str)], hosted: Option<HostedClis>) -> Chain {
        let cli = ScriptedCli::install();
        let repo = TempRepo::new();
        let service = RcaService::start_with_env(SECRET, &cli.path(), env);
        let tap = Tap::to(service.addr());
        let base_dir = tempfile::Builder::new().prefix("orca-chain-worktrees-").tempdir().unwrap();
        let base = base_dir.path().to_path_buf();
        let home = tempfile::Builder::new().prefix("orca-chain-home-").tempdir().unwrap();
        let exec = RcaExec::serve_with_home(&tap.url(), "w1", &repo, SECRET, &["--worktree-base", base.to_str().unwrap()], &hermetic_path(&cli, hosted.as_ref()), Some(home.path()));
        let bearer = service.mint("orca chain");
        let state = tempfile::Builder::new().prefix("orca-chain-").tempdir().unwrap();
        let errlog = state.path().join("serve.err");
        let (server, addr) = spawn_serve(state.path(), &errlog, &tap.url(), &bearer, web);
        Chain { tap, service, addr, state, errlog, server, cli, repo, base, _base: base_dir, home, _exec: exec, hosted }
    }

    /// A pairing link minted by a separate `orca-serve pair` on the server's state directory, as an operator hands one to a phone.
    pub fn pair(&self) -> PairingOffer {
        PairingOffer::parse(&self.pair_url()).expect("a link the phone accepts")
    }

    /// The link itself, for a client that takes it from its environment (the scripted phone of the mimic driver).
    pub fn pair_url(&self) -> String {
        let out = Command::new(BIN).args(["pair", "--endpoint", &format!("ws://{}", self.addr), "--state-dir"]).arg(self.state.path()).stdin(Stdio::null()).output().unwrap();
        assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        let printed: Value = serde_json::from_slice(&out.stdout).unwrap();
        printed["url"].as_str().expect("pair prints the link").to_string()
    }

    /// A client of rca's agent API that does not go through the tap: what the test reads of rca itself and what only another device does
    /// (a workspace made, a value stored), kept out of every call's window.
    pub fn direct(&self) -> Direct {
        let config = agent_api_client::Config::new(format!("ws://{}", self.service.addr()), self.service.mint("orca direct"));
        Direct { client: agent_api_client::Client::connect(config).expect("the side door opens") }
    }

    /// What the mapper and the service have logged so far: a failure message's evidence.
    pub fn evidence(&self) -> String {
        format!("orca-serve:\n{}\nrca-service:\n{}", std::fs::read_to_string(&self.errlog).unwrap_or_default(), self.service.logs())
    }
}

impl Drop for Chain {
    fn drop(&mut self) {
        let _ = self.server.kill();
        let _ = self.server.wait();
    }
}

fn spawn_serve(state: &Path, errlog: &Path, tap_url: &str, bearer: &str, web: Option<&Path>) -> (Child, SocketAddr) {
    let mut command = Command::new(BIN);
    command.args(["serve", "--port", "0", "--state-dir"]).arg(state).args(["--rca-url", tap_url, "--label", "chain host"]).env("ORCA_RCA_BEARER", bearer);
    if let Some(dir) = web {
        command.arg("--mobile-web-dir").arg(dir);
    }
    let mut server = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(std::fs::OpenOptions::new().create(true).append(true).open(errlog).unwrap()).spawn().expect("orca-serve starts");
    let mut line = String::new();
    BufReader::new(server.stdout.as_mut().unwrap()).read_line(&mut line).unwrap();
    let listening: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("orca-serve printed {line:?} ({e}); its log:\n{}", std::fs::read_to_string(errlog).unwrap_or_default()));
    assert_eq!((listening["ok"].clone(), listening["event"].as_str()), (json!(true), Some("listening")), "{listening}");
    (server, SocketAddr::from(([127, 0, 0, 1], listening["port"].as_u64().unwrap() as u16)))
}

/// A paired phone past the v2 handshake. Its calls are windows on the chain's tap.
pub struct Phone {
    client: PhoneClient,
}

impl Phone {
    /// Pairs the way an operator and a phone do and opens the socket, the handshake and the authentication each a call of the trace.
    pub fn connect(case: &mut Case, chain: &Chain) -> Phone {
        let offer = chain.pair();
        let host_public = offer.host_public().expect("the offer names the host key");
        let mut client = PhoneClient::connect(chain.addr, host_public);
        case.call(&chain.tap, &rows(&["transport:e2ee.handshake"]), || {
            let ready = client.hello_v2();
            let authenticated = ready.is_ok().then(|| client.authenticate(&offer.device_token));
            (authenticated.as_ref().is_some_and(|a| a.as_ref().is_ok_and(|a| a["type"] == "e2ee_authenticated")), authenticated)
        });
        Phone { client }
    }

    /// A request as one call of the trace: served when the host answered `ok`.
    pub fn call(&mut self, case: &mut Case, chain: &Chain, ids: &[&str], method: &str, params: Value) -> Value {
        case.call(&chain.tap, &rows(ids), || {
            let reply = self.client.call(method, params);
            (reply["ok"] == true, reply)
        })
    }

    /// A request as one call of the trace that stays open until `settle` says what it started is done (a turn that ran, a file that was
    /// written), so the executor's tool calls and rca's agent calls of that work are in the call's window.
    pub fn call_settled(&mut self, case: &mut Case, chain: &Chain, ids: &[&str], method: &str, params: Value, settle: impl FnOnce(&Value)) -> Value {
        case.call(&chain.tap, &rows(ids), || {
            let reply = self.client.call(method, params);
            let served = reply["ok"] == true;
            if served {
                settle(&reply);
            }
            (served, reply)
        })
    }

    /// A request the test expects to be refused, which the trace records as not served.
    pub fn refused(&mut self, case: &mut Case, chain: &Chain, ids: &[&str], method: &str, params: Value) -> Value {
        case.call(&chain.tap, &rows(ids), || {
            let reply = self.client.call(method, params);
            (false, reply)
        })
    }

    /// A stream the phone opens, as one call of the trace: served when the first frame is the snapshot or the reset the host opens with.
    /// It returns the request id the stream's later frames carry, and that first frame.
    pub fn subscribe(&mut self, case: &mut Case, chain: &Chain, ids: &[&str], method: &str, params: Value) -> (String, Value) {
        let id = case.call(&chain.tap, &rows(ids), || {
            let id = self.client.send_request(method, params);
            let first = self.client.next_stream_frame(&id);
            (first["streaming"] == true && first["result"]["type"].is_string(), (id, first))
        });
        id
    }

    /// The next frame of a stream opened by `subscribe`.
    pub fn next_frame(&mut self, id: &str) -> Value {
        self.client.next_stream_frame(id)
    }

    pub fn raw(&mut self) -> &mut PhoneClient {
        &mut self.client
    }
}

pub struct Direct {
    client: agent_api_client::Client,
}

/// How long the chain waits for rca to log what a scripted turn does.
const SETTLE: std::time::Duration = std::time::Duration::from_secs(30);

impl Direct {
    /// The side door's own client, for a stream the test reads events of.
    pub fn client(&mut self) -> &mut agent_api_client::Client {
        &mut self.client
    }

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

    /// The first event of `kind` once rca has logged `count` of them.
    pub fn wait_events(&mut self, thread: &str, kind: &str, count: usize) -> Vec<Value> {
        let deadline = std::time::Instant::now() + SETTLE;
        loop {
            let found = self.events(thread, kind);
            if found.len() >= count {
                return found;
            }
            assert!(std::time::Instant::now() < deadline, "rca never logged {count} {kind} on {thread}");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

/// What the last call of the trace sent to the agent API, in order, each with whether it was answered.
pub fn last_window(case: &Case) -> Vec<(String, bool)> {
    let call = case.calls().last().expect("a call was made");
    call["agent"].as_array().unwrap().iter().map(|h| (h["method"].as_str().unwrap().to_string(), h["ok"] == true)).collect()
}

/// That the agent method `method` was answered (`ok`) or refused (not `ok`) in the last call's window.
pub fn answered(case: &Case, method: &str, ok: bool) {
    let window = last_window(case);
    assert!(window.iter().any(|(m, o)| m == method && *o == ok), "{method} was not {} in the call's window: {window:?}", if ok { "answered" } else { "refused" });
}

/// That the executor ran `action` successfully in the last call's window.
pub fn ran(case: &Case, action: &str) -> bool {
    case.calls().last().unwrap()["executor"].as_array().unwrap().iter().any(|h| h["tool"] == action && h["ok"] == true)
}

/// The scripted CLIs first, then the host's PATH without any directory that holds a real agent CLI, so a CLI taken away is missing.
fn hermetic_path(cli: &exec_testkit::ScriptedCli, hosted: Option<&HostedClis>) -> std::ffi::OsString {
    let agents = ["claude", "codex", "opencode"];
    let mut dirs = vec![cli.dir().to_path_buf()];
    dirs.extend(hosted.map(|h| h.dir().to_path_buf()));
    dirs.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).filter(|d| !agents.iter().any(|a| d.join(a).exists())));
    std::env::join_paths(dirs).expect("a PATH")
}
