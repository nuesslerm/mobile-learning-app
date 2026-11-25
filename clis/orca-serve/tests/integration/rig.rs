//! The chain's processes and the phone that drives them. The service, the executor and `orca-serve` are real processes; the executor and
//! `orca-serve` reach the service through a `Tap`, so every agent API call the mapper makes is seen.
use exec_testkit::{Case, RcaExec, RcaService, ScriptedCli, Suite, Tap, TempRepo};
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
/// The `orca-serve` binary: the one `ORCA_SERVE_BIN` names (the chain launcher's `--orca-serve`), else the one cargo built beside the test.
fn bin() -> String {
    std::env::var("ORCA_SERVE_BIN").unwrap_or_else(|_| match option_env!("CARGO_BIN_EXE_orca-serve") {
        Some(path) => path.to_string(),
        None => panic!("no orca-serve beside this program: pass --orca-serve"),
    })
}
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
    _exec: RcaExec,
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
    _base: tempfile::TempDir,
    /// The executor's own home: its user-level skills are read from here.
    pub home: tempfile::TempDir,
    /// The bundle directory `start_recorded` made, kept on disk while the chain runs.
    _bundle: Option<tempfile::TempDir>,
}

impl Chain {
    /// The service and the executor with the scripted CLIs first on their `PATH`, the executor attached for workspace `w1` through the tap on a new repository, and
    /// `orca-serve serve` pointed at the tap with a bearer the operator minted (and the web bundle `web` when given). It returns once serve says
    /// it listens.
    pub fn start(web: Option<&Path>) -> Chain {
        Chain::start_with_env(web, &[])
    }

    /// `start` with the bundle the real host held, as far as the recorded manifest call shows it: that call reads no asset, and no recorded call asks for a window.
    /// The one provision of it, for the replay tests and the chain launcher alike.
    pub fn start_recorded() -> Chain {
        let recorded = Path::new(env!("CARGO_MANIFEST_DIR")).join("cassettes/orca-v1.4.259-recorded/interactions.jsonl");
        let held = std::fs::read_to_string(&recorded).unwrap_or_else(|e| panic!("{}: {e}", recorded.display()));
        let line = held.lines().find(|l| l.contains(r#""row":"rpc:mobileWeb.bundle.manifest""#)).expect("the recorded set holds the manifest call");
        let manifest = serde_json::from_str::<Value>(line).unwrap()["steps"][1]["payload"]["result"]["manifest"].to_string();
        let bundle = tempfile::tempdir().expect("a directory for the bundle");
        std::fs::write(bundle.path().join("manifest.json"), manifest).expect("the manifest is written");
        let mut chain = Chain::start(Some(bundle.path()));
        chain._bundle = Some(bundle);
        chain
    }

    /// `start`, with `env` also in the service's environment.
    pub fn start_with_env(web: Option<&Path>, env: &[(&str, &str)]) -> Chain {
        Chain::start_full(web, env, false)
    }

    /// `start` for a test that crashes the service: the service keeps its threads under `state` and the executor reconnects when the service is back.
    pub fn start_restartable(state: &Path) -> Chain {
        Chain::start_full(None, &[("RCA_STATE_DIR", state.to_str().expect("a utf-8 state path"))], true)
    }

    /// Kills the service and starts it again over its own home and port, as a crash and a restart do; the turn it was running is gone with it.
    pub fn restart_service(&mut self) {
        self.service.restart();
    }

    fn start_full(web: Option<&Path>, env: &[(&str, &str)], reconnect: bool) -> Chain {
        let cli = ScriptedCli::install();
        let repo = TempRepo::new();
        let service = RcaService::start_with_env(SECRET, &cli.path(), env);
        let tap = Tap::to(service.addr());
        let base_dir = tempfile::Builder::new().prefix("orca-chain-worktrees-").tempdir().unwrap();
        let base = base_dir.path().to_path_buf();
        let home = tempfile::Builder::new().prefix("orca-chain-home-").tempdir().unwrap();
        // What names `RCA_EXEC_` in `env` is the executor's own (the hosts its browser may reach), and where it finds Chromium; the rest the service's.
        let exec_env: Vec<(String, String)> = env.iter().filter(|(k, _)| k.starts_with("RCA_EXEC_") || *k == "PLAYWRIGHT_BROWSERS_PATH").map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let flags: Vec<&str> = ["--worktree-base", base.to_str().unwrap()].into_iter().chain(reconnect.then_some("--reconnect")).collect();
        let exec = RcaExec::serve_with_env(&tap.url(), "w1", &repo, SECRET, &flags, &hermetic_path(&cli), Some(home.path()), &exec_env);
        let bearer = service.mint("orca chain");
        let state = tempfile::Builder::new().prefix("orca-chain-").tempdir().unwrap();
        let errlog = state.path().join("serve.err");
        let (server, addr) = spawn_serve(state.path(), &errlog, &tap.url(), &bearer, web);
        Chain { tap, service, addr, state, errlog, server, cli, repo, _base: base_dir, home, _exec: exec, _bundle: None }
    }

    /// A pairing link minted by a separate `orca-serve pair` on the server's state directory, as an operator hands one to a phone.
    pub fn pair(&self) -> PairingOffer {
        PairingOffer::parse(&self.pair_url()).expect("a link the phone accepts")
    }

    /// The link itself, for a client that takes it from its environment (the scripted phone of the mimic driver).
    pub fn pair_url(&self) -> String {
        let out = Command::new(bin()).args(["pair", "--endpoint", &format!("ws://{}", self.addr), "--state-dir"]).arg(self.state.path()).stdin(Stdio::null()).output().unwrap();
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
        exec_testkit::stop_fixture(&mut self.server, "-USR1");
    }
}

fn spawn_serve(state: &Path, errlog: &Path, tap_url: &str, bearer: &str, web: Option<&Path>) -> (Child, SocketAddr) {
    let mut command = Command::new(bin());
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
    /// Streams the phone has seen end: binary frames they sent before their end can still be queued behind it.
    ended: Vec<u32>,
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
        Phone { client, ended: Vec::new() }
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

    /// The closing frame of a stream, after which the phone drops what the stream sent before it that is still queued.
    pub fn end_of(&mut self, stream: &TerminalStream) -> Value {
        let end = self.client.next_stream_frame(&stream.request);
        self.ended.push(stream.number);
        end
    }
}

pub struct Direct {
    client: agent_api_client::Client,
}

/// A terminal stream the phone opened with the binary stream and output credit: the request that carries its text frames, the number its binary
/// frames carry, the token that acknowledges and resumes it, and the JSON answer it opened with.
pub struct TerminalStream {
    pub request: String,
    pub number: u32,
    pub token: String,
    pub opened: Value,
}

/// What the phone read of a stream's snapshot or output: the text, and the UTF-16 units of the Output frames, which is what it acknowledges.
#[derive(Default)]
pub struct Heard {
    pub text: String,
    pub units: u64,
}

pub fn phone_client_params(handle: &str, resume: Option<(&str, u64)>) -> Value {
    let mut params = json!({"terminal": handle, "client": {"id": "chain-phone", "type": "mobile"}, "capabilities": {"terminalBinaryStream": 1, "outputAck": 1}});
    if let Some((token, units)) = resume {
        params["resume"] = json!({"streamToken": token, "units": units});
    }
    params
}

impl Phone {
    /// `terminal.subscribe` as the phone makes it, as one call of the trace: served when the host opens a stream with `subscribed`.
    pub fn subscribe_terminal(&mut self, case: &mut Case, chain: &Chain, handle: &str, resume: Option<(&str, u64)>) -> TerminalStream {
        let (request, opened) = self.subscribe(case, chain, &["rpc:terminal.subscribe"], "terminal.subscribe", phone_client_params(handle, resume));
        let result = &opened["result"];
        assert_eq!(result["type"], json!("subscribed"), "{opened}");
        TerminalStream { request, number: result["streamId"].as_u64().expect("a stream number") as u32, token: result["streamToken"].as_str().expect("a stream token").to_string(), opened }
    }

    /// The next binary frame of the stream, decoded with the mapper's own codec.
    fn terminal_frame(&mut self, stream: &TerminalStream) -> (u8, u64, Vec<u8>) {
        loop {
            let bytes = self.client.next_binary().expect("the socket stays open");
            let frame = orca_map_terminal::binary::decode(&bytes).expect("a terminal frame");
            if self.ended.contains(&frame.stream) {
                continue;
            }
            assert_eq!(frame.stream, stream.number, "a frame of another stream");
            return (frame.opcode, frame.seq, frame.payload.to_vec());
        }
    }

    /// The snapshot the stream opens with, from its start frame to its end frame.
    pub fn snapshot(&mut self, stream: &TerminalStream) -> String {
        use orca_map_terminal::binary::op;
        let (opcode, _, _) = self.terminal_frame(stream);
        assert_eq!(opcode, op::SNAPSHOT_START);
        let mut text = Vec::new();
        loop {
            let (opcode, _, payload) = self.terminal_frame(stream);
            match opcode {
                op::SNAPSHOT_CHUNK => text.extend(payload),
                op::SNAPSHOT_END => return String::from_utf8_lossy(&text).into_owned(),
                other => panic!("opcode {other} inside a snapshot"),
            }
        }
    }

    /// Output frames until their text holds `needle`.
    pub fn output_until(&mut self, stream: &TerminalStream, needle: &str) -> Heard {
        use orca_map_terminal::binary::op;
        let mut heard = Heard::default();
        while !heard.text.contains(needle) {
            let (opcode, _, payload) = self.terminal_frame(stream);
            if opcode == op::OUTPUT {
                let text = String::from_utf8(payload).expect("Output frames carry text");
                heard.units += text.encode_utf16().count() as u64;
                heard.text.push_str(&text);
            }
        }
        heard
    }

    /// The Resized announcement of the stream, past any output before it.
    pub fn resized(&mut self, stream: &TerminalStream) -> Value {
        loop {
            let (opcode, _, payload) = self.terminal_frame(stream);
            if opcode == orca_map_terminal::binary::op::RESIZED {
                return serde_json::from_slice(&payload).expect("a JSON body");
            }
        }
    }

    /// An Input, Resize or Unsubscribe frame to the stream.
    pub fn send_terminal_frame(&mut self, stream: &TerminalStream, opcode: u8, payload: &[u8]) {
        self.client.send_sealed_binary(&orca_map_terminal::binary::encode(opcode, stream.number, 0, payload));
    }
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

/// The scripted CLIs first, then the host's PATH without any directory that holds a real agent CLI, so a CLI taken away is missing.
fn hermetic_path(cli: &exec_testkit::ScriptedCli) -> std::ffi::OsString {
    let agents = ["claude", "codex", "opencode"];
    let mut dirs = vec![cli.dir().to_path_buf()];
    dirs.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).filter(|d| !agents.iter().any(|a| d.join(a).exists())));
    std::env::join_paths(dirs).expect("a PATH")
}
