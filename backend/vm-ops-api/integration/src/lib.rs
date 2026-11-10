//! The vm-ops-api integration harness. A case starts the real service (`VM_OPS_API_CMD`, the `vm-ops-api-rs` binary) as a child
//! process in a temp world of its own, and talks to it only over its base URL, its stdout and the doubles. The safety rails keep a case off the real VM and away from the CI runner's credentials: neither the child
//! nor the HTTP client sees this process's environment, the base URL is loopback or a Unix socket in the case's dir, and
//! `docker` and `wf` on the child's PATH resolve to the doubles in the case's `bin/` before the first request.
use base64::Engine as _;
use ed25519_dalek::pkcs8::spki::der::pem::LineEnding;
use ed25519_dalek::pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey};
use ed25519_dalek::{Signer, SigningKey};
use hmac::{Hmac, Mac};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub mod coverage;
mod engine;
mod github_raw;
pub use engine::{frame, node_tar, unstamped, Archive, Engine, Exec, Logs, TarEntry};
pub use github_raw::GithubRaw;

/// `VM_OPS_API_AUTH_TOKEN` of every case: the HMAC seed of the rotating token and the legacy raw token.
pub const SEED: &str = "seed-for-tests";
/// `VM_OPS_API_CI_AUTH_TOKEN` of a case that opens the CI port: the seed of the rotating token that port takes.
pub const CI_SEED: &str = "ci-seed-for-tests";
pub const GIT_COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
pub const IMAGE_TAG: &str = "sha-1234567";
pub const HOST_LABEL: &str = "main";
/// The fixed test key pair: no randomness, so a run is reproducible.
const SIGNING_KEY: [u8; 32] = [7; 32];

/// Every variable a case may hand the child. Anything else panics, so no runner credential can ride along.
pub const CHILD_ENV: &[&str] = &[
    "PATH", "PORT", "CI_PORT", "VM_OPS_API_AUTH_TOKEN", "VM_OPS_API_CI_AUTH_TOKEN", "VM_GATEWAY_REQUEST_SIGNING_PUBLIC_KEY", "REQUIRE_SIGNED_AUTH", "HOST_LABEL", "VM_DIR",
    "DEBUG_MODE", "IMAGE_NAMESPACE", "VM_OPS_API_IMAGE_TAG", "GIT_COMMIT", "VM_PROVISION_GITHUB_TOKEN",
    "OPENCODE_LOG_STATE_DIR", "POOL_COOLDOWN_STATE_FILE", "DOCKER_SOCKET",
    "PROC_ROOT", "FAKE_TOOL_RULES", "FAKE_TOOL_LOG", "BETTERSTACK_OTLP_TOKEN", "BETTERSTACK_OTLP_HOST",
];
/// The host paths the child reads and writes. Each must be set and inside the case's dir: unset or empty, the service
/// falls back to the host's own (`/var/run/docker.sock`, `/proc`, `/home/ubuntu/vm-ops-api`, `/state/...`).
const WORLD_PATHS: [&str; 6] = ["PROC_ROOT", "VM_DIR", "POOL_COOLDOWN_STATE_FILE", "OPENCODE_LOG_STATE_DIR", "FAKE_TOOL_RULES", "FAKE_TOOL_LOG"];
const DOUBLED_TOOLS: [&str; 2] = ["docker", "wf"];
const SUITE: &str = "backend/vm-ops-api/integration";
/// Where every provision URL of a sibling script points, which a helper case points at its [`GithubRaw`] instead.
pub const GITHUB_RAW: &str = "https://raw.githubusercontent.com/";
/// The one variable a helper command reads: the sibling's `-e GITHUB_RAW_TOKEN`.
const HELPER_ENV: [&str; 1] = ["GITHUB_RAW_TOKEN"];
/// The variables a sibling script is run with, from its own `-e` list; the doubles record them at each call.
const SCRIPT_ENV: [&str; 5] = ["GITHUB_RAW_TOKEN", "IMAGE_NAMESPACE", "BETTERSTACK_OTLP_HOST", "BETTERSTACK_OTLP_TOKEN", "IMAGE_TAG"];
/// A helper line's word that `sh` takes as it is: no quote, `$`, backtick, `~`, `=`, glob or operator.
const BARE_WORD: &str = r"[A-Za-z0-9_./:@%+,-]+";

/// The fake host's `/proc/meminfo`: 12,000,000 kB total, 6,000,000 kB available, 2,097,148 kB swap, and a line with no
/// `kB` that the parser skips.
pub const MEMINFO: &str = "MemTotal:       12000000 kB\nMemFree:         1000000 kB\nMemAvailable:    6000000 kB\n\
    Cached:          4000000 kB\nSwapTotal:       2097148 kB\nSwapFree:        2097148 kB\nHugePages_Total:       0\n";
pub const LOADAVG: &str = "0.52 0.58 0.59 1/389 12345\n";

/// The program and arguments of `VM_OPS_API_CMD`, split on whitespace; the program must be an absolute path, because the
/// child's PATH holds only the doubles and the system dirs.
fn service_cmd() -> (String, Vec<String>) {
    let raw = std::env::var("VM_OPS_API_CMD").unwrap_or_default();
    let mut words = raw.split_whitespace().map(str::to_string);
    let program = words.next().unwrap_or_default();
    assert!(Path::new(&program).is_absolute(), "VM_OPS_API_CMD must start with an absolute path, got {raw:?}");
    (program, words.collect())
}

fn inside(path: &str, case_dir: &Path) -> bool {
    let path = Path::new(path);
    path.starts_with(case_dir) && !path.components().any(|c| c == Component::ParentDir)
}

/// Safety rail: a case only ever talks to loopback TCP or a Unix socket inside its own dir, never to the VM.
pub fn assert_local(target: &str, case_dir: &Path) {
    let ok = match target.strip_prefix("unix:") {
        Some(socket) => inside(socket, case_dir),
        None => target
            .strip_prefix("http://127.0.0.1:")
            .and_then(|rest| rest.split('/').next())
            .is_some_and(|port| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit())),
    };
    assert!(ok, "safety rail: {target} is neither http://127.0.0.1:<port> nor a Unix socket in {}", case_dir.display());
}

fn is_executable(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// One case's temp dir (`bin/` with the doubles, `proc/`, `vm/`, `state/`) and the environment its service gets.
pub struct World {
    /// Declared first, so the fake Engine stops before its socket's dir is removed.
    engine: Option<Engine>,
    dir: tempfile::TempDir,
    env: BTreeMap<String, String>,
    /// The canonical path of the double every `docker` and `wf` on the child's PATH must resolve to.
    fake_tool: PathBuf,
    /// The doubles' rules, written to `FAKE_TOOL_RULES` as they are added.
    tool_rules: Vec<Value>,
    /// The CI port the service is asked to open, if the case asked.
    ci_port: Option<u16>,
}

impl World {
    /// The world of plan section 3's by-hand start, with `fake_tool` (this crate's `fake-tool` binary) linked as `docker`
    /// and `wf`.
    pub fn new(fake_tool: &str) -> World {
        assert!(
            Path::new(fake_tool).file_name().is_some_and(|n| n == "fake-tool"),
            "safety rail: {fake_tool} is not this crate's fake-tool, so it cannot stand in for docker and wf"
        );
        let fake_tool = std::fs::canonicalize(fake_tool).unwrap_or_else(|e| panic!("the fake-tool binary {fake_tool}: {e}"));
        let dir = tempfile::Builder::new().prefix("vm-ops-api-it-").tempdir().expect("a temp dir");
        let at = |rel: &str| dir.path().join(rel).display().to_string();
        for sub in ["bin", "proc", "vm", "state"] {
            std::fs::create_dir_all(dir.path().join(sub)).expect("the world's dirs");
        }
        for tool in DOUBLED_TOOLS {
            std::os::unix::fs::symlink(&fake_tool, dir.path().join("bin").join(tool)).expect("a double on PATH");
        }
        let host = [("meminfo", MEMINFO), ("loadavg", LOADAVG)];
        for (file, text) in host {
            let path = dir.path().join("proc").join(file);
            std::fs::create_dir_all(path.parent().expect("a dir in proc/")).expect("the fake /proc");
            std::fs::write(path, text).expect("the fake /proc");
        }
        let env = [
            ("PATH", format!("{}:/usr/bin:/bin", at("bin"))),
            ("VM_OPS_API_AUTH_TOKEN", SEED.into()),
            ("VM_GATEWAY_REQUEST_SIGNING_PUBLIC_KEY", public_key_pem().replace('\n', "\\n")),
            ("HOST_LABEL", HOST_LABEL.into()),
            ("VM_DIR", at("vm")),
            ("IMAGE_NAMESPACE", "ns".into()),
            ("VM_OPS_API_IMAGE_TAG", IMAGE_TAG.into()),
            ("GIT_COMMIT", GIT_COMMIT.into()),
            ("VM_PROVISION_GITHUB_TOKEN", "tok".into()),
            ("POOL_COOLDOWN_STATE_FILE", at("state/pool-cooldowns.json")),
            ("OPENCODE_LOG_STATE_DIR", at("state/opencode-log")),
            ("DOCKER_SOCKET", at("docker.sock")),
            ("PROC_ROOT", at("proc")),
            ("FAKE_TOOL_RULES", at("rules.json")),
            ("FAKE_TOOL_LOG", at("calls.jsonl")),
        ];
        let env = env.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        World { engine: None, env, dir, fake_tool, tool_rules: Vec::new(), ci_port: None }
    }

    pub fn dir(&self) -> &Path {
        self.dir.path()
    }

    /// Sets one of the child's variables; a name outside the allowlist panics.
    pub fn set(&mut self, key: &str, value: &str) -> &mut World {
        assert!(CHILD_ENV.contains(&key), "safety rail: {key} is not a variable the harness hands the service");
        self.env.insert(key.to_string(), value.to_string());
        self
    }

    /// Opens the CI port on a free loopback port, with its own seed ([`CI_SEED`]).
    pub fn with_ci_port(&mut self) -> &mut World {
        let port = free_port();
        self.ci_port = Some(port);
        self.set("CI_PORT", &port.to_string()).set("VM_OPS_API_CI_AUTH_TOKEN", CI_SEED)
    }

    pub fn unset(&mut self, key: &str) -> &mut World {
        self.env.remove(key);
        self
    }

    /// Serves the fake Engine on this world's `DOCKER_SOCKET`, answering the container list with `list_status` and
    /// `list_body` (a JSON array of containers when 200); `fake_engine` scripts the rest.
    pub fn engine(&mut self, list_status: u16, list_body: &str) -> &mut World {
        assert!(self.engine.is_none(), "this world already has its fake Engine");
        let socket = self.env.get("DOCKER_SOCKET").cloned().unwrap_or_default();
        assert_local(&format!("unix:{socket}"), self.dir.path());
        self.engine.insert(Engine::serve(Path::new(&socket))).list(list_status, list_body);
        self
    }

    /// The fake Engine, to script its execs, logs, stats and failures, before or after the service starts.
    pub fn fake_engine(&self) -> &Engine {
        self.engine.as_ref().expect("this world has no fake Engine: call engine() first")
    }

    /// Makes each call of the `tool` double (`docker` or `wf`) whose argv starts with `prefix` wait `delay` before it
    /// answers as it would without the rule. The first rule that matches wins.
    pub fn tool_rule(&mut self, tool: &str, prefix: &[&str], delay: Duration) -> &mut World {
        self.rule(ToolRule::new(tool, prefix).delay(delay))
    }

    /// Adds a rule for the doubles, after the ones already added: the first rule that matches a call wins.
    pub fn rule(&mut self, rule: ToolRule) -> &mut World {
        self.tool_rules.push(rule.0);
        let rules = self.dir.path().join("rules.json");
        std::fs::write(rules, Value::Array(self.tool_rules.clone()).to_string()).expect("the doubles' rules");
        self
    }

    /// Writes (or with `None` removes) a file of the fake `/proc` the service reads as `PROC_ROOT`.
    pub fn proc_file(&mut self, rel: &str, text: Option<&str>) -> &mut World {
        let path = self.dir.path().join("proc").join(rel);
        assert!(inside(&path.display().to_string(), &self.dir.path().join("proc")), "{rel} is not a file of the fake /proc");
        match text {
            Some(text) => std::fs::write(&path, text).expect("a file of the fake /proc"),
            None => std::fs::remove_file(&path).expect("a file of the fake /proc"),
        }
        self
    }

    /// Every request the fake Engine received, in order; none when the world has no Engine.
    pub fn engine_requests(&self) -> Vec<Value> {
        self.engine.as_ref().map(Engine::requests).unwrap_or_default()
    }

    /// Every call the `docker` and `wf` doubles recorded, in order. A line that is no record panics, naming it, so a torn
    /// record fails the case instead of quietly dropping a sibling.
    pub fn calls(&self) -> Vec<Value> {
        self.read_calls().unwrap_or_else(|torn| panic!("the doubles' log holds a line that is no record: {torn}"))
    }

    /// The records of the doubles' log, or the first line that is no record. Each record is one write ending in a newline,
    /// so text after the last newline is a record still being written, left for a later read.
    fn read_calls(&self) -> Result<Vec<Value>, String> {
        let text = std::fs::read_to_string(self.dir.path().join("calls.jsonl")).unwrap_or_default();
        let complete = text.rsplit_once('\n').map_or("", |(done, _)| done);
        complete.lines().map(|l| serde_json::from_str(l).map_err(|e| format!("{l:?}: {e}"))).collect()
    }

    /// The argv of every call the `docker` double recorded, in order: the siblings the service ran.
    pub fn docker_calls(&self) -> Vec<Vec<String>> {
        let argv = |c: &Value| c["argv"].as_array().map(|a| a.iter().map(|s| s.as_str().unwrap_or("").to_string()).collect());
        self.calls().iter().filter(|c| c["tool"] == "docker").filter_map(argv).collect()
    }

    /// Safety rail: `docker` and `wf` on the child's PATH must be the doubles in this world's `bin/`, and each must still be
    /// the fake-tool, not a real CLI linked or written there.
    fn assert_tools_doubled(&self) {
        self.assert_tools_doubled_on(self.env.get("PATH").map(String::as_str).unwrap_or(""));
    }

    fn assert_tools_doubled_on(&self, path: &str) {
        self.assert_doubled(path, &self.dir.path().join("bin"), &DOUBLED_TOOLS);
    }

    /// Safety rail: each of `tools` on `path` is the fake-tool linked in `bin`, not another executable ahead of it.
    fn assert_doubled(&self, path: &str, bin: &Path, tools: &[&str]) {
        for tool in tools {
            let found = path.split(':').filter(|d| !d.is_empty()).map(|d| Path::new(d).join(tool)).find(|c| is_executable(c));
            assert!(
                found.as_deref().and_then(Path::parent) == Some(bin),
                "safety rail: `{tool}` on the child's PATH resolves to {found:?}, not to the double in {}",
                bin.display()
            );
            let target = found.as_deref().and_then(|f| std::fs::canonicalize(f).ok());
            assert!(
                target.as_deref() == Some(self.fake_tool.as_path()),
                "safety rail: `{tool}` in {} is {target:?}, not the fake-tool {}",
                bin.display(),
                self.fake_tool.display()
            );
        }
    }

    /// Safety rail: the Docker socket and every host path the child uses are set and inside this world's dir.
    fn assert_paths_in_world(&self) {
        let dir = self.dir.path();
        let sock = self.env.get("DOCKER_SOCKET").filter(|s| !s.is_empty());
        let sock = sock.unwrap_or_else(|| panic!("safety rail: DOCKER_SOCKET is unset or empty, so the service would use /var/run/docker.sock"));
        assert_local(&format!("unix:{sock}"), dir);
        for key in WORLD_PATHS {
            let value = self.env.get(key).map(String::as_str).unwrap_or("");
            assert!(inside(value, dir), "safety rail: {key}={value:?} is not inside the case's dir {}", dir.display());
        }
    }

    fn spawn(&self, program: &str, args: &[String], port: u16) -> Child {
        for key in self.env.keys() {
            assert!(CHILD_ENV.contains(&key.as_str()), "safety rail: {key} is not a variable the harness hands the service");
        }
        self.assert_tools_doubled();
        self.assert_paths_in_world();
        let mut command = Command::new(program);
        command.args(args).env_clear().envs(&self.env).current_dir(self.dir.path());
        command.env("PORT", port.to_string());
        spawn_in_own_group(command, program)
    }

    /// Starts the service and runs it until it exits by itself, or kills it after `within`.
    pub fn run_to_exit(&self, within: Duration) -> Exit {
        let (program, args) = service_cmd();
        self.run_program_to_exit(&program, &args, within)
    }

    /// Runs another program exactly as the service is started (same rails, same environment); the rails cases use it to
    /// see what the child is handed.
    pub fn run_program_to_exit(&self, program: &str, args: &[String], within: Duration) -> Exit {
        run_until_exit(self.spawn(program, args, free_port()), within)
    }

    /// Runs one line of a sibling script under `sh -c` as the sibling runs it, in this world's dir: the doubles first on
    /// PATH, then the service's own program dir, then the system dirs, and nothing of this process's environment but `env`,
    /// which may name only `GITHUB_RAW_TOKEN`. The line must be words `sh` takes as they are, bare or wholly single-quoted
    /// and one space apart, every URL among them loopback and every other path inside the world, with no `..`, so a helper
    /// never fetches from GitHub or writes to the host.
    pub fn run_helper(&self, line: &str, env: &[(&str, &str)], within: Duration) -> Exit {
        for (key, _) in env {
            assert!(HELPER_ENV.contains(key), "safety rail: {key} is not a variable a helper command reads");
        }
        for word in helper_words(line) {
            // a path can hide behind `=`, a `,` or a space inside quotes, or be glued to an option
            for part in word.split(|c: char| c.is_whitespace() || c == '=' || c == ',').filter(|p| !p.is_empty()) {
                if part.contains("://") {
                    assert_local(part, self.dir());
                    continue;
                }
                let climbs = Path::new(part).components().any(|c| c == Component::ParentDir);
                let glued = part.starts_with('-') && part.contains('/');
                let ok = !climbs && !glued && (!part.starts_with('/') || inside(part, self.dir()));
                assert!(ok, "safety rail: the helper line names {part}, outside {}", self.dir().display());
            }
        }
        let (program, _) = service_cmd();
        let program_dir = Path::new(&program).parent().expect("the service's program has a dir");
        let path = format!("{}:{}:/usr/bin:/bin", self.dir().join("bin").display(), program_dir.display());
        self.assert_tools_doubled_on(&path);
        let mut command = Command::new("/bin/sh");
        command.args(["-c", line]).env_clear().env("PATH", path).envs(env.iter().copied()).current_dir(self.dir());
        run_until_exit(spawn_in_own_group(command, "/bin/sh"), within)
    }

    /// Runs the script of a sibling the `docker` double recorded (`docker_calls()`) under `sh -c`, in the sibling's `-w`
    /// dir and with its `-e` variables, as the sibling runs it, but in this world: the doubles first on PATH answering by
    /// `rules` (their own file and log, apart from the service's), then the service's program dir, then the system dirs;
    /// provision URLs point at `raw`; the sibling's `/home/ubuntu/` is `<world>/home/ubuntu/`. Every URL of the script must be loopback and every absolute path inside the world (but `/` for `df`
    /// and `/dev/null`), so a script never fetches from GitHub or writes to the host. `see` names files each recorded call
    /// reads at that moment (`seen`), with the call's `cwd` and the sibling's OTLP and tag variables (`env`).
    pub fn run_script(&self, sibling: &[String], raw: &GithubRaw, rules: &[ToolRule], see: &[PathBuf], within: Duration) -> ScriptRun {
        let script = sibling.last().expect("a sibling has a script");
        assert!(sibling.len() > 3 && sibling[sibling.len() - 3..sibling.len() - 1] == ["sh", "-c"], "not a sibling `sh -c` call: {sibling:?}");
        let mut env = Vec::new();
        for pair in sibling.windows(2).filter(|w| w[0] == "-e").map(|w| w[1].as_str()) {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            assert!(SCRIPT_ENV.contains(&name), "safety rail: {name} is not a variable a sibling script run may hand on");
            env.push((name.to_string(), value.to_string()));
        }
        let (world, (program, _)) = (self.dir().display().to_string(), service_cmd());
        let home = format!("{world}/home/ubuntu/");
        let localise = |text: &str| text.replace(GITHUB_RAW, &format!("{}/", raw.base())).replace("/home/ubuntu/", &home);
        let script = localise(script);
        assert_script_local(&script, self.dir());
        let cwd = sibling.windows(2).find(|w| w[0] == "-w").map(|w| localise(&w[1])).expect("a sibling has a working dir");
        assert!(inside(&cwd, self.dir()), "safety rail: the sibling's working dir {cwd} is outside {world}");
        std::fs::create_dir_all(&cwd).expect("the sibling's working dir");
        let program_dir = Path::new(&program).parent().expect("the service's program has a dir");
        let path = format!("{}:{}:/usr/bin:/bin", self.dir().join("bin").display(), program_dir.display());
        self.assert_tools_doubled_on(&path);
        let (rules_file, log) = (self.dir().join("script-rules.json"), self.dir().join("script-calls.jsonl"));
        std::fs::write(&rules_file, Value::Array(rules.iter().map(|r| r.0.clone()).collect()).to_string()).expect("the script's rules");
        let _ = std::fs::remove_file(&log);
        let seen: Vec<String> = see.iter().map(|p| p.display().to_string()).collect();
        let mut command = Command::new("/bin/sh");
        command.args(["-c", &script]).env_clear().envs(env).current_dir(&cwd);
        command.env("PATH", path).env("FAKE_TOOL_RULES", &rules_file).env("FAKE_TOOL_LOG", &log);
        command.env("FAKE_TOOL_SEE", seen.join(":")).env("FAKE_TOOL_ENV", SCRIPT_ENV.join(","));
        let exit = run_until_exit(spawn_in_own_group(command, "/bin/sh"), within);
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        let calls: Vec<Value> = text.lines().map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{l:?}: {e}"))).collect();
        stash("scriptCalls", json!(calls));
        ScriptRun { exit, calls }
    }

    /// Starts the service on a free loopback port and waits until this child says it listens there. The port is free
    /// when picked but not reserved, so a child that loses it to another process is started once more on a fresh one.
    pub fn start(self) -> Service {
        let (program, args) = service_cmd();
        let mut attempt = 0;
        let (child, port, stdout, stderr) = loop {
            attempt += 1;
            let port = free_port();
            assert_local(&format!("http://127.0.0.1:{port}"), self.dir());
            let mut child = self.spawn(&program, &args, port);
            let (stdout, stderr) = (drain(child.stdout.take()), drain(child.stderr.take()));
            match wait_listening(&mut child, &stdout, &stderr, port) {
                Ok(()) => break (child, port, stdout, stderr),
                Err(status) => {
                    let (out, err) = (stdout.finish(), stderr.finish());
                    stash("stdout", json!(out));
                    stash("stderr", json!(err));
                    if attempt == 1 && (err.contains("EADDRINUSE") || err.contains("Address already in use")) {
                        continue;
                    }
                    panic!("the service exited ({status}) before it listened on {port}\nstderr: {err}\nstdout: {out}");
                }
            }
        };
        // reqwest reads only the proxy variables from the environment; no_proxy() drops those too
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()
            .expect("an HTTP client");
        let ci_port = self.ci_port;
        let service = Service { child, base: format!("http://127.0.0.1:{port}"), port, ci_port, stdout, stderr, client, world: self };
        if let Some(ci) = ci_port {
            let said = service.wait_event("vm-ops-api.ci_listening", Duration::from_secs(30));
            assert_eq!(said["port"], json!(ci), "the CI listener's port");
        }
        service
    }
}

/// A rule for the calls of the `docker` or `wf` double whose argv starts with `prefix` and, given [`ToolRule::rest`],
/// whose further arguments joined with one space match a regex: how long such a call waits, and, given
/// [`ToolRule::answer`], what it prints and exits instead of the double's default answer.
#[derive(Debug, Clone)]
pub struct ToolRule(Value);

impl ToolRule {
    pub fn new(tool: &str, prefix: &[&str]) -> ToolRule {
        assert!(DOUBLED_TOOLS.contains(&tool), "{tool} is not a doubled tool");
        ToolRule(json!({ "tool": tool, "prefix": prefix }))
    }

    pub fn docker(prefix: &[&str]) -> ToolRule {
        ToolRule::new("docker", prefix)
    }

    pub fn rest(mut self, regex: &str) -> ToolRule {
        regex::Regex::new(regex).unwrap_or_else(|e| panic!("the tool rule's rest regex {regex:?}: {e}"));
        self.0["rest"] = json!(regex);
        self
    }

    pub fn delay(mut self, delay: Duration) -> ToolRule {
        self.0["delayMs"] = json!(delay.as_millis() as u64);
        self
    }

    /// The call prints `stdout` and `stderr`, each in one write, and exits `exit_code`.
    pub fn answer(mut self, stdout: &str, stderr: &str, exit_code: i32) -> ToolRule {
        self.0["answer"] = json!({ "stdout": stdout, "stderr": stderr, "exitCode": exit_code });
        self
    }

    /// The call reads its standard input to the end and records it as `stdin`, such as the token of a `docker login`.
    pub fn capture_stdin(mut self) -> ToolRule {
        self.0["stdin"] = json!(true);
        self
    }

    /// The rule applies only to the `n`th call (1-based) it would otherwise match, such as the second `compose up`.
    pub fn nth(mut self, n: u64) -> ToolRule {
        self.0["nth"] = json!(n);
        self
    }

    /// Before it answers, the call writes `text` to `file`, relative to the directory it runs in. Needs an answer().
    pub fn writes(mut self, file: &str, text: &str) -> ToolRule {
        let answer = self.0.get_mut("answer").filter(|a| a.is_object()).expect("writes() needs an answer()");
        answer["writes"][file] = json!(text);
        self
    }

    /// Before it answers, the call removes `file`, relative to the directory it runs in. Needs an answer().
    pub fn removes(mut self, file: &str) -> ToolRule {
        let answer = self.0.get_mut("answer").filter(|a| a.is_object()).expect("removes() needs an answer()");
        answer["removes"] = json!([file]);
        self
    }

    /// The answer's stdout and stderr each go out in two writes, cut at these byte offsets, a pause between them, so the
    /// service reads two chunks; a cut inside a character splits it.
    pub fn cut(mut self, stdout_at: usize, stderr_at: usize) -> ToolRule {
        let answer = self.0.get_mut("answer").filter(|a| a.is_object()).expect("cut() needs an answer()");
        answer["stdoutCut"] = json!(stdout_at);
        answer["stderrCut"] = json!(stderr_at);
        self
    }
}

/// Waits until the child prints `vm-ops-api.listening` for `port` and the port accepts a connection, so a case never
/// talks to another process that holds the port. `Err` is how the child exited before that; past 30 s it is killed.
fn wait_listening(child: &mut Child, stdout: &Drained, stderr: &Drained, port: u16) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let said_listening = || {
        stdout.text().lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).any(|e| {
            e["message"] == "vm-ops-api.listening" && e["port"] == json!(port)
        })
    };
    loop {
        if let Some(status) = child.try_wait().expect("the child's status") {
            return Err(status.to_string());
        }
        if said_listening() && std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
            return Ok(());
        }
        if Instant::now() > deadline {
            kill_group(child);
            panic!("the service did not listen on {port} within 30 s\nstderr: {}\nstdout: {}", stderr.text(), stdout.text());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Spawns `command` with its output piped, leading a process group of its own and killed when this process dies.
fn spawn_in_own_group(mut command: Command, program: &str) -> Child {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        // Its own group lets kill_group reach the children it forks; a Ctrl-C of the run then misses that group, so
        // the child dies with the test's thread instead
        command.process_group(0);
        // SAFETY: prctl(2) is async-signal-safe and reads no memory of the parent
        unsafe {
            command.pre_exec(|| match libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) {
                0 => Ok(()),
                _ => Err(std::io::Error::last_os_error()),
            })
        };
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("cannot start {program}: {e}"))
}

/// Waits for `child` to exit by itself, or kills its group after `within`, and keeps its output for the report.
fn run_until_exit(mut child: Child, within: Duration) -> Exit {
    let (stdout, stderr) = (drain(child.stdout.take()), drain(child.stderr.take()));
    let deadline = Instant::now() + within;
    let code = loop {
        if let Some(status) = child.try_wait().expect("the child's status") {
            break status.code();
        }
        if Instant::now() > deadline {
            kill_group(&mut child);
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let exit = Exit { code, stdout: stdout.finish(), stderr: stderr.finish() };
    stash("stdout", json!(exit.stdout));
    stash("stderr", json!(exit.stderr));
    exit
}

/// The words of a helper line, unquoted. Safety rail: a line with anything `sh` would expand, split or redirect panics.
fn helper_words(line: &str) -> Vec<String> {
    let word = format!("(?:{BARE_WORD}|'[^']*')");
    let whole = regex::Regex::new(&format!("^{word}(?: {word})*$")).expect("a regex");
    assert!(whole.is_match(line), "safety rail: the helper line {line:?} holds a word sh would expand");
    let each = regex::Regex::new(&format!("({BARE_WORD})|'([^']*)'")).expect("a regex");
    each.captures_iter(line).filter_map(|c| c.get(1).or(c.get(2))).map(|m| m.as_str().to_string()).collect()
}

/// Safety rail: every URL of a sibling script is loopback and every absolute path in it inside the world, but `/` (the
/// `df` of the disk check) and `/dev/null`, so running it can neither fetch from GitHub nor write to the host.
fn assert_script_local(script: &str, world: &Path) {
    let url = regex::Regex::new(r#"https?://[^\s'"]+"#).expect("a regex");
    for found in url.find_iter(script) {
        assert_local(found.as_str().trim_end_matches('/'), world);
    }
    let path = regex::Regex::new(r#"(?:^|[\s='"(;&|<>{}])(/[^\s'"();&|<>{}]*)"#).expect("a regex");
    for found in path.captures_iter(&url.replace_all(script, "")) {
        let ok = matches!(&found[1], "/" | "/dev/null") || inside(&found[1], world);
        assert!(ok, "safety rail: the script names {}, outside {}", &found[1], world.display());
    }
}

/// Kills the child and every process of its group (on Linux it leads its own), such as the `wf log emit` children it
/// fires and forgets, then reaps it. Called only before the child is reaped, so no other group can have its id.
fn kill_group(child: &mut Child) {
    let group = libc::pid_t::try_from(child.id()).expect("a pid fits pid_t");
    // SAFETY: kill(2) takes no pointers; a negative pid names the process group the child leads, if it leads one
    unsafe { libc::kill(-group, libc::SIGKILL) };
    let _ = child.kill();
    let _ = child.wait();
}

impl Drop for World {
    fn drop(&mut self) {
        stash("calls", self.read_calls().map_or_else(|torn| json!({ "torn": torn }), |calls| json!(calls)));
        if self.engine.is_some() {
            stash("engine", json!(self.engine_requests()));
        }
        self.engine.take();
        // A double's write landing between remove_dir_all's listing and its rmdir fails the removal, which TempDir ignores
        for _ in 0..20 {
            match std::fs::remove_dir_all(self.dir.path()) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => std::thread::sleep(Duration::from_millis(25)),
                _ => break,
            }
        }
    }
}

/// How a service that ran to its end ended: `code` is None when it was killed at the deadline.
pub struct Exit {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// A sibling script run ([`World::run_script`]): how it ended and each call its doubles recorded, in order.
pub struct ScriptRun {
    pub exit: Exit,
    pub calls: Vec<Value>,
}

impl ScriptRun {
    /// The records of the `tool` double's calls whose argv starts with `prefix`.
    pub fn calls_of(&self, tool: &str, prefix: &[&str]) -> Vec<&Value> {
        let starts = |c: &&Value| {
            let argv: Vec<&str> = c["argv"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            c["tool"] == tool && argv.starts_with(prefix)
        };
        self.calls.iter().filter(starts).collect()
    }
}

/// A running service, killed on drop with its whole process group, before its world's dir is removed.
pub struct Service {
    child: Child,
    base: String,
    port: u16,
    ci_port: Option<u16>,
    stdout: Drained,
    stderr: Drained,
    client: reqwest::blocking::Client,
    world: World,
}

#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// How a raw request's connection ended when no whole reply came back: the bytes it got and each side's error.
#[derive(Debug)]
pub struct NoReply {
    pub request: String,
    pub bytes: usize,
    pub write: Option<std::io::ErrorKind>,
    pub read: Option<std::io::ErrorKind>,
}

impl std::fmt::Display for NoReply {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let Self { request, bytes, write, read } = self;
        write!(f, "no whole reply to {request}: {bytes} bytes back, write error {write:?}, read error {read:?}")
    }
}

impl Reply {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or_else(|e| panic!("not JSON ({e}): {}", self.body))
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

impl Service {
    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    /// What the service printed so far.
    pub fn stdout(&self) -> String {
        self.stdout.text()
    }

    /// A request with today's rotating token and a signature over it, as wf sends every call.
    pub fn signed(&self, method: &str, path_with_query: &str, body: Option<&str>) -> Reply {
        let headers = sign_request(SEED, &private_key_pem(), method, path_with_query, body, now_ms()).expect("the test key signs");
        self.request(method, path_with_query, &headers, body)
    }

    pub fn request(&self, method: &str, path_with_query: &str, headers: &[(String, String)], body: Option<&str>) -> Reply {
        assert!(path_with_query.starts_with('/'), "a path starts with /: {path_with_query}");
        assert_local(&self.base, self.world.dir());
        let method = reqwest::Method::from_bytes(method.as_bytes()).expect("an HTTP method");
        let mut req = self.client.request(method, format!("{}{path_with_query}", self.base));
        for (k, v) in headers {
            req = req.header(k, v);
        }
        if let Some(body) = body {
            req = req.body(body.to_string());
        }
        let resp = req.send().unwrap_or_else(|e| panic!("{path_with_query}: {e}\nstdout: {}", self.stdout()));
        let status = resp.status().as_u16();
        let headers = resp.headers().iter().map(|(k, v)| (k.to_string(), String::from_utf8_lossy(v.as_bytes()).to_string())).collect();
        let body = String::from_utf8_lossy(&resp.bytes().expect("the reply body")).to_string();
        Reply { status, headers, body }
    }

    /// One request written byte for byte on a fresh loopback connection, for what reqwest normalises or refuses: dot
    /// segments, a leading `//`, padded header text, a wrong-case scheme, an oversized body. Header names and values go
    /// out as given; `body` adds its `Content-Length`. `Err` says how the connection ended when no whole reply came back.
    pub fn raw(&self, method: &str, target: &str, headers: &[(String, String)], body: Option<&[u8]>) -> Result<Reply, NoReply> {
        self.raw_at(self.port, method, target, headers, body)
    }

    fn raw_at(&self, port: u16, method: &str, target: &str, headers: &[(String, String)], body: Option<&[u8]>) -> Result<Reply, NoReply> {
        assert_local(&format!("http://127.0.0.1:{port}"), self.world.dir());
        let mut head = format!("{method} {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n");
        for (k, v) in headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        if let Some(body) = body {
            head.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        head.push_str("\r\n");
        let mut wire = head.into_bytes();
        wire.extend_from_slice(body.unwrap_or_default());
        self.wire_at(port, method, target, &wire)
    }

    fn wire_at(&self, port: u16, method: &str, target: &str, wire: &[u8]) -> Result<Reply, NoReply> {
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port))
            .unwrap_or_else(|e| panic!("{method} {target}: cannot connect to the service: {e}\nstdout: {}", self.stdout()));
        stream.set_read_timeout(Some(Duration::from_secs(30))).expect("a read timeout on a TCP stream");
        let wrote = stream.write_all(wire);
        let mut got = Vec::new();
        let read = stream.read_to_end(&mut got);
        parse_reply(method, &got).ok_or_else(|| NoReply {
            request: format!("{method} {target}"),
            bytes: got.len(),
            write: wrote.err().map(|e| e.kind()),
            read: read.err().map(|e| e.kind()),
        })
    }

    /// The CI port of a service started [`World::with_ci_port`].
    pub fn ci_port(&self) -> u16 {
        self.ci_port.expect("this case did not open the CI port: call with_ci_port() before start()")
    }

    /// A request on the CI port, signed with `seed` the way wf signs every call.
    pub fn signed_on_ci_with(&self, seed: &str, method: &str, target: &str, body: Option<&str>) -> Reply {
        let headers = sign_request(seed, &private_key_pem(), method, target, body, now_ms()).expect("the test key signs");
        self.raw_at(self.ci_port(), method, target, &headers, body.map(str::as_bytes)).unwrap_or_else(|e| panic!("{method} {target} on the CI port: {e}"))
    }

    /// A request on the CI port, signed with the CI seed.
    pub fn ci_signed(&self, method: &str, target: &str, body: Option<&str>) -> Reply {
        self.signed_on_ci_with(CI_SEED, method, target, body)
    }

    /// A request on the admin port signed with `seed`, written byte for byte.
    pub fn signed_with(&self, seed: &str, method: &str, target: &str, body: Option<&str>) -> Reply {
        let headers = sign_request(seed, &private_key_pem(), method, target, body, now_ms()).expect("the test key signs");
        self.raw(method, target, &headers, body.map(str::as_bytes)).unwrap_or_else(|e| panic!("{method} {target}: {e}"))
    }

    /// The JSON lines the service printed so far: its events, `{level, message, ...attributes}`.
    pub fn events(&self) -> Vec<Value> {
        self.stdout().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
    }

    /// The first event with this `message`, waited for up to `within`, since the reader thread may lag the reply.
    pub fn wait_event(&self, message: &str, within: Duration) -> Value {
        let deadline = Instant::now() + within;
        loop {
            if let Some(e) = self.events().into_iter().find(|e| e["message"] == message) {
                return e;
            }
            assert!(Instant::now() < deadline, "no event {message}\nstdout: {}", self.stdout());
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The first stdout line that is exactly `line`, waited for up to `within`, since the reader thread may lag the reply.
    pub fn wait_line(&self, line: &str, within: Duration) {
        let deadline = Instant::now() + within;
        while !self.stdout().lines().any(|l| l == line) {
            assert!(Instant::now() < deadline, "no stdout line {line}\nstdout: {}", self.stdout());
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        kill_group(&mut self.child);
        stash("stdout", json!(self.stdout.text()));
        stash("stderr", json!(self.stderr.text()));
    }
}

/// An HTTP/1.1 reply read to the connection's close: `None` until the head is whole and the body is all there, by its
/// chunks or its `content-length` (a reply to HEAD has none).
fn parse_reply(method: &str, wire: &[u8]) -> Option<Reply> {
    let end = wire.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&wire[..end]).ok()?;
    let mut lines = head.split("\r\n");
    let status: u16 = lines.next()?.split(' ').nth(1)?.parse().ok()?;
    let headers: Vec<(String, String)> =
        lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    let field = |name: &str| headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    let rest = &wire[end + 4..];
    let body = if method == "HEAD" || status == 204 || status == 304 {
        Vec::new()
    } else if field("transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        dechunk(rest)?
    } else if let Some(n) = field("content-length") {
        rest.get(..n.parse::<usize>().ok()?)?.to_vec()
    } else {
        rest.to_vec()
    };
    Some(Reply { status, headers, body: String::from_utf8_lossy(&body).to_string() })
}

fn dechunk(mut rest: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let eol = rest.windows(2).position(|w| w == b"\r\n")?;
        let size = usize::from_str_radix(std::str::from_utf8(&rest[..eol]).ok()?.split(';').next()?.trim(), 16).ok()?;
        rest = &rest[eol + 2..];
        if size == 0 {
            return Some(out);
        }
        out.extend_from_slice(rest.get(..size)?);
        rest = rest.get(size + 2..)?;
    }
}

/// A child's output stream, read on its own thread as it arrives.
struct Drained {
    buf: Arc<Mutex<Vec<u8>>>,
    reader: Option<JoinHandle<()>>,
}

impl Drained {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.buf.lock().unwrap()).to_string()
    }

    /// Everything, once the stream has closed.
    fn finish(mut self) -> String {
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
        self.text()
    }
}

fn drain(stream: Option<impl Read + Send + 'static>) -> Drained {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let sink = buf.clone();
    let reader = stream.map(|mut s| {
        std::thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            while let Ok(n) = s.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                sink.lock().unwrap().extend_from_slice(&chunk[..n]);
            }
        })
    });
    Drained { buf, reader }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).expect("a free loopback port").port()
}

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or_default()
}

/// Year, month and day of a day count since 1970-01-01 (Howard Hinnant's civil_from_days).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

/// `YYYY-MM-DD` of the UTC day `ms` falls on.
pub fn utc_date(ms: i64) -> String {
    let (y, m, d) = civil(ms.div_euclid(86_400_000));
    format!("{y:04}-{m:02}-{d:02}")
}

/// `ms` as JavaScript's `toISOString()` prints it.
pub fn iso(ms: i64) -> String {
    let t = ms.rem_euclid(86_400_000);
    format!("{}T{:02}:{:02}:{:02}.{:03}Z", utc_date(ms), t / 3_600_000, t / 60_000 % 60, t / 1000 % 60, t % 1000)
}

pub fn rotating_token(seed: &str, date: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(seed.as_bytes()).expect("hmac accepts any key length");
    mac.update(date.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// The three headers of one authenticated request. A GET hashes the empty body, as the verifier does.
/// A copy of wf's `sign_request` (clis/wf-vmops/src/vmops.rs), so the suite signs exactly as wf does.
pub fn sign_request(
    seed: &str,
    private_key_pem: &str,
    method: &str,
    path_with_query: &str,
    body: Option<&str>,
    now: i64,
) -> Result<Vec<(String, String)>, String> {
    let timestamp = now.to_string();
    let body_hash = hex::encode(Sha256::digest(body.unwrap_or("").as_bytes()));
    let canonical = format!("{method}\n{path_with_query}\n{timestamp}\n{body_hash}");
    let key = SigningKey::from_pkcs8_pem(private_key_pem).map_err(|e| format!("cannot read the signing key: {e}"))?;
    let signature = base64::engine::general_purpose::STANDARD.encode(key.sign(canonical.as_bytes()).to_bytes());
    Ok(vec![
        ("Authorization".into(), format!("Bearer {}", rotating_token(seed, &utc_date(now)))),
        ("X-Signature".into(), signature),
        ("X-Signature-Timestamp".into(), timestamp),
    ])
}

/// The string the service verifies: `METHOD\n<pathname><search>\n<timestamp header text>\n<sha256 hex of the body>`.
/// `signed_path` is what the service derives from the request target, which a case states itself (dot segments
/// resolved, a bare `?` dropped, a leading `//` taken as a host), so a case can also sign what the service will not.
pub fn canonical(method: &str, signed_path: &str, timestamp: &str, body: &[u8]) -> String {
    format!("{method}\n{signed_path}\n{timestamp}\n{}", hex::encode(Sha256::digest(body)))
}

/// The test key's Ed25519 signature over `canonical`, in standard base64 as wf sends it.
pub fn signature(canonical: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(SigningKey::from_bytes(&SIGNING_KEY).sign(canonical.as_bytes()).to_bytes())
}

/// The rotating token of the UTC day `days_ago` days before today. Near midnight it first waits for the new day, so the
/// service checks the token on the day it was made for; a case that pins the skew bound reads its clock after this.
pub fn token(days_ago: i64) -> String {
    settle_midnight();
    rotating_token(SEED, &utc_date(now_ms() - days_ago * 86_400_000))
}

fn settle_midnight() {
    let left = 86_400_000 - now_ms().rem_euclid(86_400_000);
    if left < 3_000 {
        std::thread::sleep(Duration::from_millis(left as u64 + 100));
    }
}

/// The three headers of a request signed over `signed_path`, `timestamp` (the header text) and `body`, with today's
/// token: what wf sends, built from parts a case can vary.
pub fn auth(method: &str, signed_path: &str, timestamp: &str, body: &[u8]) -> Vec<(String, String)> {
    vec![
        ("Authorization".into(), format!("Bearer {}", token(0))),
        ("X-Signature".into(), signature(&canonical(method, signed_path, timestamp, body))),
        ("X-Signature-Timestamp".into(), timestamp.into()),
    ]
}

/// The test key pair's private half, as PKCS#8 PEM (what wf reads from `VM_GATEWAY_REQUEST_SIGNING_PRIVATE_KEY`).
pub fn private_key_pem() -> String {
    SigningKey::from_bytes(&SIGNING_KEY).to_pkcs8_pem(LineEnding::LF).expect("the test key encodes").to_string()
}

/// Its public half, as SPKI PEM (what the service reads from `VM_GATEWAY_REQUEST_SIGNING_PUBLIC_KEY`).
pub fn public_key_pem() -> String {
    SigningKey::from_bytes(&SIGNING_KEY).verifying_key().to_public_key_pem(LineEnding::LF).expect("the test key encodes")
}

thread_local! {
    /// What the current case's service and doubles left behind, for the report of a failed case.
    static EVIDENCE: RefCell<Map<String, Value>> = RefCell::new(Map::new());
}

fn stash(key: &str, value: Value) {
    EVIDENCE.with(|e| e.borrow_mut().insert(key.to_string(), value));
}

struct Run {
    started_ms: i64,
    cases: Vec<Value>,
}

static RUN: Mutex<Option<Run>> = Mutex::new(None);

fn git_head() -> Option<String> {
    static HEAD: OnceLock<Option<String>> = OnceLock::new();
    HEAD.get_or_init(|| {
        let out = Command::new("git").args(["rev-parse", "HEAD"]).current_dir(env!("CARGO_MANIFEST_DIR")).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    })
    .clone()
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "panicked".into())
}

/// Asserts that `f` panics with a message containing `expected`: how the rails cases check that a rail stops a case.
pub fn expect_panic(expected: &str, f: impl FnOnce()) {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    let message = outcome.err().map(|p| panic_text(p.as_ref()));
    assert!(message.as_deref().is_some_and(|m| m.contains(expected)), "expected a panic with {expected:?}, got {message:?}");
}

/// How many tests this run runs: the binary lists itself with the same arguments (libtest's `--list`), so a run that
/// stops early, or a `#[test]` outside `case!`, never leaves a report that says pass.
fn expected_cases() -> Option<usize> {
    static EXPECTED: OnceLock<Option<usize>> = OnceLock::new();
    *EXPECTED.get_or_init(|| {
        let args: Vec<String> = std::env::args().skip(1).filter(|a| a != "--list").collect();
        let out = Command::new(std::env::current_exe().ok()?).args(args).arg("--list").env_clear().output().ok()?;
        let n = String::from_utf8_lossy(&out.stdout).lines().filter(|l| l.ends_with(": test")).count();
        (out.status.success() && n > 0).then_some(n)
    })
}

/// Every test of this binary, whatever filter this run has (libtest's `--list`), as `<module>::<fn>`.
pub fn binary_tests() -> Vec<String> {
    let exe = std::env::current_exe().expect("this test binary's path");
    let out = Command::new(exe).arg("--list").env_clear().output().expect("the test binary lists its tests");
    assert!(out.status.success(), "the test binary cannot list its tests: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.strip_suffix(": test")).map(str::to_string).collect()
}

/// Runs one case and records it in the run's report, with the route ids it checks (plan section 2); a failed case keeps
/// the calls the doubles recorded and the service's output.
pub fn run_case(name: &str, routes: &[&str], body: impl FnOnce()) {
    let started = now_ms();
    EVIDENCE.with(|e| e.borrow_mut().clear());
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        for r in routes {
            assert!(coverage::ROUTE_IDS.contains(r), "{name} names {r:?}, which is no route id of plan section 2");
        }
        body()
    }));
    let mut case = json!({ "name": name, "verdict": if outcome.is_ok() { "pass" } else { "fail" }, "ms": now_ms() - started });
    if !routes.is_empty() {
        case["routes"] = json!(routes);
    }
    if let Err(p) = &outcome {
        case["message"] = json!(panic_text(p.as_ref()));
        for (k, v) in EVIDENCE.with(|e| std::mem::take(&mut *e.borrow_mut())) {
            case[k] = v;
        }
    }
    record(case, started);
    if let Err(p) = outcome {
        std::panic::resume_unwind(p);
    }
}

fn record(case: Value, started: i64) {
    let mut run = RUN.lock().unwrap_or_else(|e| e.into_inner());
    let run = run.get_or_insert_with(|| Run { started_ms: started, cases: Vec::new() });
    run.started_ms = run.started_ms.min(started);
    run.cases.push(case);
    if let Some(out) = std::env::var_os("WF_TEST_OUTPUT_DIR").filter(|d| !d.is_empty()) {
        let dir = PathBuf::from(out).join("vm-ops-api-integration");
        if let Err(e) = write_report(&dir, run) {
            eprintln!("vm-ops-api-integration: cannot write {}/report.json: {e}", dir.display());
        }
    }
}

/// The run as a harness run record `wf test publish` takes (`report.json`, schema 1), rewritten after every case. Until
/// the last listed case is in, it says `error`, so a run killed or aborted partway is never published as a pass.
fn write_report(dir: &Path, run: &Run) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let finished = now_ms();
    let failed: Vec<&Value> = run.cases.iter().filter(|c| c["verdict"] == "fail").collect();
    let expected = expected_cases();
    let unfinished = expected.filter(|&n| run.cases.len() < n);
    let (verdict, message) = match (failed.first(), unfinished) {
        (Some(c), _) => ("fail", Some(format!("{}: {}", c["name"].as_str().unwrap_or(""), c["message"].as_str().unwrap_or("")))),
        (None, Some(n)) => ("error", Some(format!("the run stopped after {} of {n} cases", run.cases.len()))),
        (None, None) => ("pass", None),
    };
    let ok = verdict == "pass";
    let report = json!({
        "schema": 1,
        "verdict": verdict,
        "ok": ok,
        "exitCode": if ok { 0 } else { 1 },
        "message": message,
        "expectedCases": expected,
        "script": SUITE,
        "platform": "integration",
        "sha": git_head(),
        "startedMs": run.started_ms,
        "finishedMs": finished,
        "started": iso(run.started_ms),
        "finished": iso(finished),
        "durationS": ((finished - run.started_ms).max(0) as f64 / 100.0).round() / 10.0,
        "recordDir": dir.display().to_string(),
        "cases": run.cases,
        "warnings": if expected.is_none() { json!(["the binary could not list its tests, so a run that stopped early reads as pass"]) } else { json!([]) },
    });
    let tmp = dir.join("report.json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&report)? + "\n")?;
    std::fs::rename(tmp, dir.join("report.json"))
}

/// Asserts that `actual` equals the golden `goldens/<name>`.
pub fn assert_golden(name: &str, actual: &str) {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("goldens").join(name);
    let golden = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("the golden {}: {e}", file.display()));
    assert!(actual == golden, "{name} differs from its golden\n--- golden\n{golden}\n--- actual\n{actual}");
}

/// The image every sibling runs (plan section 4, deliberate difference 7): `ns/vm-ops-api-rs:<tag>`, where `tag` is
/// `VM_OPS_API_IMAGE_TAG`, or `latest` when that is `unknown`.
pub fn sibling_image(tag: &str) -> String {
    format!("ns/vm-ops-api-rs:{tag}")
}

/// How each helper command of a sibling script reads, `vm-ops-api-rs <subcommand>` (deliberate difference 3), beside its
/// placeholder in the goldens.
fn helper_commands() -> [(String, &'static str); 3] {
    let names = [("provision", "provision"), ("appendEnvLocal", "append-env-local"), ("verifyEnv", "verify-env")];
    names.map(|(placeholder, subcommand)| (format!("vm-ops-api-rs {subcommand}"), placeholder))
}

/// How a sibling script calls the helper `name` (`provision`, `appendEnvLocal` or `verifyEnv`).
pub fn helper_command(name: &str) -> String {
    let found = helper_commands().into_iter().find(|(_, n)| *n == name);
    found.map(|(command, _)| command).unwrap_or_else(|| panic!("{name} is no helper command"))
}

/// The sibling argv the `docker` double recorded, normalised as plan section 3 ("Comparing recorded calls") says, as the
/// pretty JSON a golden holds: the case's dir becomes `<world>`, a history line's `ts` becomes `<ts>`, and, exactly per
/// image (never a wildcard), the element equal to [`sibling_image`] of `tag` becomes `<image>` and each helper command
/// `<helper:name>`. So an argv that names another image or helper differs from the golden.
pub fn siblings_json(world: &World, tag: &str) -> String {
    let (dir, image) = (world.dir().display().to_string(), sibling_image(tag));
    let ts = regex::Regex::new(r#""ts":"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z""#).expect("a regex");
    let helpers = helper_commands();
    let normalised: Vec<Vec<String>> = world
        .docker_calls()
        .into_iter()
        .map(|argv| {
            let each = argv.into_iter().map(|arg| {
                if arg == image {
                    return "<image>".to_string();
                }
                let arg = ts.replace_all(&arg.replace(&dir, "<world>"), r#""ts":"<ts>""#).to_string();
                helpers.iter().fold(arg, |a, (command, name)| a.replace(command.as_str(), &format!("<helper:{name}>")))
            });
            each.collect()
        })
        .collect();
    serde_json::to_string_pretty(&normalised).expect("JSON") + "\n"
}

/// One integration case: a `#[test]` whose outcome, timing and (on failure) evidence go into the run's report. Every test
/// of the binary is one, the rails cases included, or the report never reaches pass. `case!(name, ["GET /health"], {…})`
/// names the route ids whose contract it checks; the coverage case reads that list from the source.
#[macro_export]
macro_rules! case {
    ($name:ident, [$($route:literal),* $(,)?], $body:block) => {
        #[test]
        fn $name() {
            $crate::run_case(concat!(module_path!(), "::", stringify!($name)), &[$($route),*], || $body)
        }
    };
    ($name:ident, $body:block) => {
        $crate::case!($name, [], $body);
    };
}
