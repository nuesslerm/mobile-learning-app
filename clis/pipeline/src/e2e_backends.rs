//! The backends the e2e tests run against, started on the runner so no test reaches a deployed service: `wf test backends up` builds
//! (or takes a prebuilt) `rca-service` and `leetcode-backend-rs`, starts each detached on a loopback port, waits for its `/health`, and prints
//! one JSON with their urls, pids and the environment a test points at them with; `down` stops what the state file names.
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use wf_kit::args::{parse_spec, Positional, Spec};

const SPEC: Spec = Spec { values: &["only", "dir", "wait", "rca-port", "leetcode-port", "stt-port", "opencode-port"], flags: &["no-build"], positional: Positional::Exactly(1), ..Spec::NONE };
const STATE: &str = "backends.json";
const DEFAULT_WAIT_S: u64 = 60;
const STOP_WAIT: Duration = Duration::from_secs(5);

/// How a backend's program is found and started.
#[derive(PartialEq)]
enum Kind {
    /// A crate `cargo build` makes.
    Cargo,
    /// `opencode serve` of the PATH (or OPENCODE_BIN), for rca-service's model calls; asked at `/app`.
    Opencode,
    /// The faster-whisper sidecar (`backend/remote-chat-agent/stt/server.py`, run by STT_PYTHON or python3), for rca-service's transcription.
    Stt,
    /// A desktop a phone pairs with: the integration tests' chain (`clis/t3-serve/examples/chain.rs`), t3-serve over an rca service and executor of
    /// its own with a scripted Claude, so a chat answers with no model. It picks its port and says it, with the command that mints a pairing link.
    Desktop,
}

/// A backend: where its crate is, which binary it makes, the variable that names a prebuilt one, and how it is asked if it is up.
struct Def {
    name: &'static str,
    kind: Kind,
    manifest: &'static str,
    package: &'static str,
    bin_env: &'static str,
    port_flag: &'static str,
    health: &'static str,
    /// Started by a bare `up` (the others only when `--only` names them).
    default: bool,
}

// the order is the start order: rca-service reads the urls of the two before it
const DEFS: [Def; 5] = [
    Def { name: "stt", kind: Kind::Stt, manifest: "", package: "stt", bin_env: "STT_SERVER", port_flag: "stt-port", health: "/health", default: false },
    Def { name: "opencode-server", kind: Kind::Opencode, manifest: "", package: "opencode", bin_env: "OPENCODE_BIN", port_flag: "opencode-port", health: "/app", default: false },
    Def { name: "rca-service", kind: Kind::Cargo, manifest: "backend/remote-chat-agent/Cargo.toml", package: "rca-service", bin_env: "RCA_SERVICE_BIN", port_flag: "rca-port", health: "/health", default: true },
    Def { name: "leetcode-backend", kind: Kind::Cargo, manifest: "backend/leetcode-backend/Cargo.toml", package: "leetcode-backend-rs", bin_env: "LEETCODE_BACKEND_BIN", port_flag: "leetcode-port", health: "/health", default: true },
    Def { name: "agent-desktop", kind: Kind::Desktop, manifest: "clis/Cargo.toml", package: "t3-serve", bin_env: "AGENT_DESKTOP_BIN", port_flag: "", health: "/.well-known/t3/environment", default: false },
];

/// What the scripted Claude of `agent-desktop` answers every message with (`e2e/rca-native.lua` waits for it).
const DESKTOP_REPLY: &str = "Hello from the scripted agent";
/// The desktop's first start builds the programs of its chain that no variable names.
const DESKTOP_BUILD_WAIT: Duration = Duration::from_secs(900);

fn fail(error: impl Into<String>, next: &str) -> i32 {
    println!("{}", json!({ "ok": false, "error": error.into(), "next": next }));
    1
}

/// `wf test backends up|down`: the words after `backends`.
pub fn run(args: &[String]) -> i32 {
    let Some(p) = parse_spec(args, &SPEC) else { return 2 };
    let dir = p.val("dir").map(PathBuf::from).unwrap_or_else(|| wf_kit::root::repo_root().join(".agents/tmp/e2e-backends"));
    match p.positional[0].as_str() {
        "up" => up(&p, &dir),
        "down" => down(&dir),
        other => fail(format!("unknown action '{other}'"), "wf test backends up|down"),
    }
}

fn up(p: &wf_kit::args::Parsed, dir: &Path) -> i32 {
    let wanted: Vec<&str> = p.val("only").map(|v| v.split(',').map(str::trim).filter(|s| !s.is_empty()).collect()).unwrap_or_else(|| DEFS.iter().filter(|d| d.default).map(|d| d.name).collect());
    if let Some(bad) = wanted.iter().find(|w| !DEFS.iter().any(|d| d.name == **w)) {
        return fail(format!("unknown backend '{bad}'"), "wf test backends up [--only stt,opencode-server,rca-service,leetcode-backend,agent-desktop]");
    }
    if let Some(state) = read_state(dir) {
        let alive: Vec<String> = recorded(&state).into_iter().filter(is_ours).map(|r| format!("{} (pid {})", r.name, r.pid)).collect();
        if !alive.is_empty() {
            return fail(format!("backends are already up: {}", alive.join(", ")), "wf test backends down");
        }
    }
    let wait = Duration::from_secs(p.val("wait").and_then(|w| w.parse().ok()).unwrap_or(DEFAULT_WAIT_S));
    if let Err(e) = std::fs::create_dir_all(dir) {
        return fail(format!("{}: {e}", dir.display()), "pass --dir <writable dir>");
    }
    let mut started: Vec<Value> = Vec::new();
    for def in DEFS.iter().filter(|d| wanted.contains(&d.name)) {
        match start_one(def, p, dir, wait, &started) {
            Ok(entry) => started.push(entry),
            Err(error) => {
                stop_all(&started.iter().filter_map(as_recorded).collect::<Vec<_>>());
                return fail(error, "wf test backends up --no-build (with RCA_SERVICE_BIN / LEETCODE_BACKEND_BIN naming a built binary) or check the log named above");
            }
        }
    }
    let out = json!({ "ok": true, "dir": dir.display().to_string(), "backends": started });
    if let Err(e) = write_state(dir, &out) {
        stop_all(&started.iter().filter_map(as_recorded).collect::<Vec<_>>());
        return fail(format!("{}: {e}", dir.join(STATE).display()), "pass --dir <writable dir>");
    }
    println!("{out}");
    0
}

fn start_one(def: &Def, p: &wf_kit::args::Parsed, dir: &Path, wait: Duration, peers: &[Value]) -> Result<Value, String> {
    let bin = binary(def, p.flag("no-build"))?;
    if def.kind == Kind::Desktop {
        return start_desktop(def, &bin, dir, wait.max(DESKTOP_BUILD_WAIT), peers);
    }
    let port = match p.val(def.port_flag) {
        Some(v) => v.parse::<u16>().map_err(|_| format!("--{} wants a port number, got '{v}'", def.port_flag))?,
        None => free_port()?,
    };
    let home = dir.join(format!("{}-home", def.name));
    std::fs::create_dir_all(&home).map_err(|e| format!("{}: {e}", home.display()))?;
    let (server_env, consumer_env) = environment(def, port, &home, peers)?;
    let log = dir.join(format!("{}.log", def.name));
    let log_file = std::fs::File::create(&log).map_err(|e| format!("{}: {e}", log.display()))?;
    let mut command = match def.kind {
        Kind::Cargo => Command::new(&bin),
        Kind::Opencode => {
            let mut c = Command::new(&bin);
            c.args(["serve", "--hostname", "127.0.0.1", "--port", &port.to_string()]);
            c
        }
        Kind::Stt => {
            let mut c = Command::new(std::env::var_os("STT_PYTHON").filter(|v| !v.is_empty()).unwrap_or_else(|| "python3".into()));
            c.arg(&bin);
            c
        }
        Kind::Desktop => unreachable!("start_desktop starts it"),
    };
    command.env_clear().env("PATH", std::env::var_os("PATH").unwrap_or_default()).env("HOME", &home).current_dir(&home).envs(server_env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    // its own process group, so `down` stops it with whatever it started and it outlives this command
    command.process_group(0).stdin(Stdio::null()).stdout(log_file.try_clone().map_err(|e| e.to_string())?).stderr(log_file);
    let mut child = command.spawn().map_err(|e| format!("cannot start {}: {e}", bin.display()))?;
    let pid = child.id();
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = Instant::now() + wait;
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("{} ended with {status} before /health answered; log {}:\n{}", def.name, log.display(), tail(&log)));
        }
        if health(addr, def.health) {
            break;
        }
        if Instant::now() >= deadline {
            kill_group(pid, true);
            let _ = child.wait();
            return Err(format!("{} did not answer /health within {}s; log {}:\n{}", def.name, wait.as_secs(), log.display(), tail(&log)));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let url = format!("http://127.0.0.1:{port}");
    Ok(json!({
        "name": def.name, "url": url, "host": "127.0.0.1", "port": port, "pid": pid, "health": format!("{url}{}", def.health),
        "bin": bin.display().to_string(), "log": log.display().to_string(),
        "env": consumer_env.into_iter().map(|(k, v)| (k, Value::String(v))).collect::<serde_json::Map<String, Value>>(),
    }))
}

/// A program of the chain that sits beside this `wf` (CI installs `t3-serve` and `rca-exec` there), unless its variable already names one.
fn beside_wf(var: &str, program: &str) -> Option<(String, String)> {
    if std::env::var_os(var).is_some_and(|v| !v.is_empty()) {
        return None;
    }
    let found = std::env::current_exe().ok()?.parent()?.join(program);
    found.is_file().then(|| (var.to_string(), found.display().to_string()))
}

/// Starts the desktop and waits for the line it says when it listens. It keeps the caller's environment: the chain builds what no variable names,
/// and takes the rca service this run already built.
fn start_desktop(def: &Def, bin: &Path, dir: &Path, wait: Duration, peers: &[Value]) -> Result<Value, String> {
    let log = dir.join(format!("{}.log", def.name));
    let said = dir.join(format!("{}.json", def.name));
    let open = |path: &Path| std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()));
    let mut command = Command::new(bin);
    command.args(["--claude-says", DESKTOP_REPLY, "--stay"]);
    // the server is the one built with the chain (a runner may hold `wf` alone); the executor is taken from beside `wf` where it is
    if std::env::var_os("T3_SERVE_BIN").is_none_or(|v| v.is_empty()) {
        if let Some(server) = bin.parent().and_then(Path::parent).map(|debug| debug.join("t3-serve")).filter(|p| p.is_file()) {
            command.env("T3_SERVE_BIN", server);
        }
    }
    command.envs(beside_wf("RCA_EXEC_BIN", "rca-exec"));
    if let Some(service) = peers.iter().find(|p| p["name"] == "rca-service").and_then(|p| p["bin"].as_str()) {
        command.env("RCA_SERVICE_BIN", service);
    }
    command.process_group(0).stdin(Stdio::null()).stdout(open(&said)?).stderr(open(&log)?);
    let mut child = command.spawn().map_err(|e| format!("cannot start {}: {e}", bin.display()))?;
    let pid = child.id();
    let deadline = Instant::now() + wait;
    let listening = loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("{} ended with {status} before it listened; log {}:\n{}", def.name, log.display(), tail(&log)));
        }
        let line = std::fs::read_to_string(&said).unwrap_or_default();
        if let Some(value) = line.lines().next().and_then(|l| serde_json::from_str::<Value>(l).ok()) {
            break value;
        }
        if Instant::now() >= deadline {
            kill_group(pid, true);
            let _ = child.wait();
            return Err(format!("{} did not listen within {}s; log {}:\n{}", def.name, wait.as_secs(), log.display(), tail(&log)));
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let addr: SocketAddr = listening["addr"].as_str().and_then(|a| a.parse().ok()).ok_or_else(|| format!("{} said no address: {listening}", def.name))?;
    if !health(addr, def.health) {
        kill_group(pid, true);
        let _ = child.wait();
        return Err(format!("{} listens on {addr} but {} does not answer; log {}:\n{}", def.name, def.health, log.display(), tail(&log)));
    }
    let url = format!("http://{addr}");
    Ok(json!({
        "name": def.name, "url": url, "host": addr.ip().to_string(), "port": addr.port(), "pid": pid, "health": format!("{url}{}", def.health),
        "bin": bin.display().to_string(), "log": log.display().to_string(), "env": {},
        // the command that mints a pairing link on this desktop; `--base-url <url>` makes the link one a phone on this machine opens
        "mint": listening["mint"], "reply": DESKTOP_REPLY,
    }))
}

/// The server's own environment and the one a test (or an app export) points at it with. Secrets are made for this run alone.
type EnvPairs = Vec<(String, String)>;

fn environment(def: &Def, port: u16, home: &Path, peers: &[Value]) -> Result<(EnvPairs, EnvPairs), String> {
    let s = |k: &str, v: String| (k.to_string(), v);
    let url = format!("http://127.0.0.1:{port}");
    let peer_url = |name: &str| peers.iter().find(|p| p["name"] == name).and_then(|p| p["url"].as_str().map(str::to_string));
    Ok(match def.name {
        "stt" => (vec![s("PORT", port.to_string()), s("STT_MODEL", std::env::var("STT_MODEL").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| "tiny.en".into()))], vec![s("STT_URL", format!("{url}/transcribe"))]),
        "opencode-server" => {
            // the key reaches the child through its environment only: not argv, not a file, not the JSON printed below
            let key = std::env::var("OPENCODE_GO_API_KEY").ok().filter(|v| !v.is_empty()).ok_or("opencode-server needs OPENCODE_GO_API_KEY in the environment (wf secret get OPENCODE_GO_API_KEY --backend bitwarden)")?;
            let config = wf_kit::root::repo_root().join("backend/remote-chat-agent/opencode-server/server-config.json");
            let server = vec![
                s("OPENCODE_API_KEY", key),
                s("OPENCODE_CONFIG", config.display().to_string()),
                s("npm_config_cache", home.join("npm-cache").display().to_string()),
            ];
            (server, vec![s("OPENCODE_SERVER_URL", url)])
        }
        "rca-service" => {
            let secret = random_hex();
            let mut server = vec![
                s("PORT", port.to_string()),
                s("CHAT_AGENT_API_SECRET", secret.clone()),
                s("RCA_EXEC_ROOT", home.display().to_string()),
                s("RCA_PROVIDERS_ROOT", home.join("providers").display().to_string()),
                s("RCA_ASK_ROOT", home.join("asks").display().to_string()),
                s("RCA_LEDGER_FILE", home.join("ledger.json").display().to_string()),
            ];
            // the opencode server and the speech sidecar: the ones started just before (--only), else the caller's environment
            if let Some(u) = peer_url("opencode-server") {
                server.push(s("OPENCODE_SERVER_URL", u));
            } else {
                for key in ["OPENCODE_SERVER_URL", "OPENCODE_SERVER_PASSWORD"] {
                    if let Some(v) = std::env::var(key).ok().filter(|v| !v.is_empty()) {
                        server.push(s(key, v));
                    }
                }
            }
            if let Some(u) = peer_url("stt") {
                server.push(s("STT_URL", format!("{u}/transcribe")));
            }
            if peer_url("opencode-server").is_some() {
                server.push(s("OPENCODE_CWD", home.display().to_string()));
                // the planner's own default model signs in with a Kimi key this job does not hold (every plan answered "Invalid Authentication"): the model it
                // plans with is one the opencode-go key serves, unless the caller names another
                let model = std::env::var("OPENCODE_TEXT_MODEL").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| "opencode-go/kimi-k2.7-code".into());
                server.push(s("OPENCODE_TEXT_MODEL", model));
            }
            let consumer = vec![
                s("RCA_HOST", "127.0.0.1".into()),
                s("RCA_PORT", port.to_string()),
                s("CHAT_AGENT_API_SECRET", secret),
                s("WF_TAILNET_SOCKS", String::new()),
                s("SECRETS_BACKEND", "env".into()),
                s("EXPO_PUBLIC_AGENT_BASE_URL", url),
            ];
            (server, consumer)
        }
        _ => {
            // a fixed bearer when the caller names one: an EAS Update baked it in before this backend started
            let bearer = std::env::var("E2E_LEETCODE_BEARER").ok().filter(|v| !v.is_empty()).unwrap_or_else(random_hex);
            let server = vec![
                s("PORT", port.to_string()),
                s("GIT_COMMIT", std::env::var("GITHUB_SHA").unwrap_or_else(|_| "e2e".into())),
                s("LEETCODE_BACKEND_SESSION_FILE", home.join("session.json").display().to_string()),
                // nothing listens there: the capture tool is not part of the e2e
                s("LEETCODE_CAPTURE_TOOL_BASE_URL", "http://127.0.0.1:9".into()),
                s("LEETCODE_CAPTURE_TRIGGER_SECRET", random_hex()),
                s("LEETCODE_BACKEND_ADMIN_SECRET", random_hex()),
                s("VM_GATEWAY_AUTH_SEED", bearer.clone()),
            ];
            let consumer = vec![s("EXPO_PUBLIC_LEETCODE_BACKEND_BASE_URL", url), s("EXPO_PUBLIC_LEETCODE_BACKEND_TOKEN", bearer)];
            (server, consumer)
        }
    })
}

/// The program of a backend: the prebuilt one its variable names, or the one `cargo build` makes (a build directory of its own, which
/// `exec_testkit::RcaService` shares for `rca-service`, so the same build serves both).
fn binary(def: &Def, no_build: bool) -> Result<PathBuf, String> {
    match def.kind {
        Kind::Opencode => return on_path(def, "opencode"),
        Kind::Stt => {
            let path = std::env::var_os(def.bin_env).filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| wf_kit::root::repo_root().join("backend/remote-chat-agent/stt/server.py"));
            return if path.is_file() { Ok(path) } else { Err(format!("{} is no file", path.display())) };
        }
        Kind::Cargo | Kind::Desktop => {}
    }
    if let Some(named) = std::env::var_os(def.bin_env).filter(|v| !v.is_empty()) {
        let path = PathBuf::from(named);
        return if path.is_file() { Ok(path) } else { Err(format!("{}={} is no file", def.bin_env, path.display())) };
    }
    let repo = wf_kit::root::repo_root();
    let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| repo.join("target")).join(format!("{}-build", def.name));
    let desktop = def.kind == Kind::Desktop;
    let built = if desktop { target.join("debug/examples/chain") } else { target.join("debug").join(def.package) };
    if no_build {
        return if built.is_file() { Ok(built) } else { Err(format!("--no-build: {} is not built (name a binary in {})", built.display(), def.bin_env)) };
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .args(["build", "--locked", "--quiet", "-p", def.package])
        .args(if desktop { vec!["--example", "chain", "--bin", def.package] } else { vec!["--bin", def.package] })
        .arg("--manifest-path")
        .arg(repo.join(def.manifest))
        .arg("--target-dir")
        .arg(&target)
        .stdout(Stdio::null())
        .status()
        .map_err(|e| format!("cargo does not start: {e}"))?;
    if status.success() {
        Ok(built)
    } else {
        Err(format!("building {} failed ({status})", def.package))
    }
}

/// The program a variable names, else `program` found on the PATH (as a full path, which is also what the process's command line shows).
fn on_path(def: &Def, program: &str) -> Result<PathBuf, String> {
    if let Some(named) = std::env::var_os(def.bin_env).filter(|v| !v.is_empty()) {
        let path = PathBuf::from(named);
        return if path.is_file() { Ok(path) } else { Err(format!("{}={} is no file", def.bin_env, path.display())) };
    }
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).map(|d| d.join(program)).find(|p| p.is_file()).ok_or_else(|| format!("no `{program}` on the PATH (or name one in {})", def.bin_env))
}

fn free_port() -> Result<u16, String> {
    TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).map(|a| a.port()).map_err(|e| format!("no free loopback port: {e}"))
}

fn random_hex() -> String {
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes)).expect("/dev/urandom");
    hex::encode(bytes)
}

/// `GET <path>` answered 200.
fn health(addr: SocketAddr, path: &str) -> bool {
    let _span = wf_kit::performance::external("probe.http");
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(2)) else { return false };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    if stream.write_all(format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n").as_bytes()).is_err() {
        return false;
    }
    let mut head = String::new();
    let _ = stream.take(64).read_to_string(&mut head);
    head.split(' ').nth(1) == Some("200")
}

fn tail(log: &Path) -> String {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(20)..].join("\n")
}

/// What the state file records of one backend.
struct Recorded {
    name: String,
    pid: u32,
    bin: String,
    log: Option<PathBuf>,
    secrets: Vec<String>,
}

fn as_recorded(entry: &Value) -> Option<Recorded> {
    Some(Recorded { name: entry["name"].as_str()?.to_string(), pid: u32::try_from(entry["pid"].as_u64()?).ok()?, bin: entry["bin"].as_str()?.to_string(), log: entry["log"].as_str().map(PathBuf::from), secrets: secrets_of(entry) })
}

/// The values a log must never show: the recorded env of the backend and the named bearer.
fn secrets_of(entry: &Value) -> Vec<String> {
    let env = entry["env"].as_object().into_iter().flat_map(|m| m.values()).filter_map(|v| v.as_str().map(str::to_string));
    env.chain(std::env::var("E2E_LEETCODE_BEARER").ok()).filter(|v| v.len() >= 8).collect()
}

fn recorded(state: &Value) -> Vec<Recorded> {
    state["backends"].as_array().map(|a| a.iter().filter_map(as_recorded).collect()).unwrap_or_default()
}

fn read_state(dir: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(dir.join(STATE)).ok()?).ok()
}

fn write_state(dir: &Path, state: &Value) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    // it holds this run's secrets
    let mut file = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(dir.join(STATE))?;
    file.write_all(format!("{state}\n").as_bytes())
}

/// Alive, and still the program that was started (a recycled pid is not ours to stop).
fn is_ours(r: &Recorded) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{}/stat", r.pid)) else { return false };
    let state = stat.rsplit_once(')').and_then(|(_, rest)| rest.trim().chars().next());
    if state == Some('Z') {
        return false;
    }
    std::fs::read(format!("/proc/{}/cmdline", r.pid)).is_ok_and(|c| String::from_utf8_lossy(&c).contains(&r.bin))
}

/// `is_ours`, or a process that is exiting: its command line is already empty but it is not yet a zombie, and a stop that returns then reports a pid that is still listed.
fn still_there(r: &Recorded) -> bool {
    if is_ours(r) {
        return true;
    }
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{}/stat", r.pid)) else { return false };
    let state = stat.rsplit_once(')').and_then(|(_, rest)| rest.trim().chars().next());
    state != Some('Z') && std::fs::read(format!("/proc/{}/cmdline", r.pid)).is_ok_and(|c| c.is_empty())
}

fn kill_group(pid: u32, hard: bool) {
    use nix::sys::signal::{killpg, Signal};
    let _ = killpg(nix::unistd::Pid::from_raw(pid as i32), if hard { Signal::SIGKILL } else { Signal::SIGTERM });
}

/// Stops each one still ours: SIGTERM to its group, SIGKILL when it is still there after the grace. Says how each ended.
fn stop_all(all: &[Recorded]) -> Vec<Value> {
    all.iter()
        .map(|r| {
            let how = if !is_ours(r) {
                "gone"
            } else {
                kill_group(r.pid, false);
                let deadline = Instant::now() + STOP_WAIT;
                while still_there(r) && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(50));
                }
                if still_there(r) {
                    kill_group(r.pid, true);
                    "kill"
                } else {
                    "term"
                }
            };
            json!({ "name": r.name, "pid": r.pid, "how": how })
        })
        .collect()
}

const LOG_TAIL_LINES: usize = 80;

/// The last lines of each backend's log in a `::group::` block, secrets masked, before the backends stop and the run's files go.
fn print_log_tails(all: &[Recorded]) {
    for r in all {
        let Some(log) = &r.log else { continue };
        let text = String::from_utf8_lossy(&std::fs::read(log).unwrap_or_default()).into_owned();
        let lines: Vec<&str> = text.lines().collect();
        println!("::group::{} log (last {LOG_TAIL_LINES} lines)", r.name);
        for line in &lines[lines.len().saturating_sub(LOG_TAIL_LINES)..] {
            println!("{}", r.secrets.iter().fold(crate::ci_comment::redact(line, ""), |l, secret| l.replace(secret.as_str(), "[REDACTED]")));
        }
        println!("::endgroup::");
    }
}

fn down(dir: &Path) -> i32 {
    let state = read_state(dir).unwrap_or(Value::Null);
    let all = recorded(&state);
    print_log_tails(&all);
    let stopped = stop_all(&all);
    let _ = std::fs::remove_file(dir.join(STATE));
    println!("{}", json!({ "ok": true, "dir": dir.display().to_string(), "stopped": stopped }));
    0
}
