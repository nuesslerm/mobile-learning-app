//! The helpers every parity group imports (`parity`, `parity_features`, `parity_capabilities`, and `ask` for its caller and retries): `opencode run` and `wf offload ask` side by side with a real model: the same prompt, options and model through both, in checkouts of
//! the same shape, and what each leaves (exit code, stdout, the JSON events, files, session) compared. It runs with the CLI e2e tests
//! (the `cargo-e2e` task of this crate, so Nx replays it while the crate and the option table are unchanged); it needs the `opencode`
//! binary on PATH (the cli-e2e job installs it), the key `OPENCODE_GO_API_KEY` (the environment's, else the vault's); the service it asks is the `rca-service` this run starts on loopback (`rca.rs`). The model is
//! pinned and the prompts are deterministic, but a model may still word an answer differently, so the checks are on the marker each
//! prompt asks for and on the structure of what is printed, not on whole answers.
use crate::rca::Model;
use serde_json::Value;
use std::process::Command;
use std::sync::OnceLock;

const MODEL: &str = "opencode-go/deepseek-v4.1-flash";
const READ_ONLY: &str = include_str!("../../../../.opencode/agents/read-only.md");

pub(crate) struct Out {
    pub(crate) code: i32,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

/// The key the opencode binary authenticates with (`OPENCODE_GO_API_KEY` as the environment has it, else the vault's), handed only to the
/// children it starts.
pub(crate) fn key() -> &'static str {
    static KEY: OnceLock<String> = OnceLock::new();
    KEY.get_or_init(|| {
        if let Some(key) = std::env::var("OPENCODE_GO_API_KEY").ok().filter(|k| !k.is_empty()) {
            return key;
        }
        let out = Command::new(std::env::var("WF_BIN").unwrap_or_else(|_| "wf".into())).args(["secret", "get", "OPENCODE_GO_API_KEY", "--backend", "bitwarden"]).output().expect("the wf binary starts");
        assert!(out.status.success() && !out.stdout.is_empty(), "OPENCODE_GO_API_KEY is neither in the environment nor in the vault for this machine; the e2e starts its own rca-service and opencode and needs only this model key: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    })
}

fn plain(bytes: &[u8]) -> String {
    let raw = String::from_utf8_lossy(bytes);
    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for n in chars.by_ref() {
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The caller name a test's asks go under: every session the test makes is the service's to hold for the test only, and `purge` drops them.
pub(crate) fn caller_of(dir: &std::path::Path) -> String {
    format!("e2e-{}", dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
}

/// Tells the service the test is done with the sessions it made: they are deleted from opencode (which gives their memory back) and from
/// the service's disk now, not when they expire. A service that cannot be reached is left to its expiry.
pub(crate) fn purge(caller: &str, model: Model) {
    let _ = crate::rca::point(Command::new(crate::support::wf_binary()).args(["offload", "sessions", "rm", "--caller", caller]).env("WF_USAGE", "off"), model).output();
}

/// A checkout and the home the opencode binary keeps its sessions in, one pair per side so neither sees the other's.
pub(crate) struct Side {
    pub(crate) dir: tempfile::TempDir,
    home: tempfile::TempDir,
}

impl Drop for Side {
    fn drop(&mut self) {
        purge(&caller_of(self.dir.path()), Model::Live);
    }
}

pub(crate) fn side(files: &[(&str, &str)]) -> Side {
    let dir = tempfile::Builder::new().prefix("wf-parity-").tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    for (path, content) in files.iter().chain(&[(".opencode/agents/read-only.md", READ_ONLY)]) {
        let target = dir.path().join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, content).unwrap();
    }
    Side { dir, home: tempfile::tempdir().unwrap() }
}

/// One npm cache for every opencode the tests start: each has a home of its own, and opencode installs the packages of its plugins and
/// tools there, a hundred megabytes a home.
fn npm_cache() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("wf-parity-npm-cache");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// At most this many model-driving processes at once, whatever the number of test threads: the provider answers 429 to a crowd, and a
/// run it refuses is a failure of the test, not of what it checks (`WF_PARITY_CONCURRENCY`, four by default).
struct Limiter {
    used: std::sync::Mutex<usize>,
    free: std::sync::Condvar,
    limit: usize,
}

fn limiter() -> &'static Limiter {
    static LIMITER: OnceLock<Limiter> = OnceLock::new();
    LIMITER.get_or_init(|| Limiter { used: std::sync::Mutex::new(0), free: std::sync::Condvar::new(), limit: std::env::var("WF_PARITY_CONCURRENCY").ok().and_then(|v| v.parse().ok()).unwrap_or(4) })
}

/// Holds one of the slots until it is dropped.
struct Slot;

fn slot() -> Slot {
    let limiter = limiter();
    let mut used = limiter.used.lock().unwrap();
    while *used >= limiter.limit {
        used = limiter.free.wait(used).unwrap();
    }
    *used += 1;
    Slot
}

impl Drop for Slot {
    fn drop(&mut self) {
        let limiter = limiter();
        *limiter.used.lock().unwrap() -= 1;
        limiter.free.notify_one();
    }
}

/// Runs the command to its end with `input` (or an empty stdin) and collects what it printed.
fn finish(mut command: Command, input: Option<&str>) -> Out {
    use std::io::Write;
    let _slot = slot();
    let mut child = command.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().expect("the command starts");
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.unwrap_or_default().as_bytes());
    }
    let out = child.wait_with_output().expect("the command ends");
    Out { code: out.status.code().unwrap_or(-1), stdout: plain(&out.stdout), stderr: plain(&out.stderr) }
}

impl Side {
    pub(crate) fn opencode(&self, args: &[&str]) -> Out {
        self.opencode_in(args, None)
    }

    pub(crate) fn ask(&self, args: &[&str]) -> Out {
        self.ask_in(args, None)
    }

    /// `opencode run` with `input` on its stdin (none: nothing to read).
    pub(crate) fn opencode_in(&self, args: &[&str], input: Option<&str>) -> Out {
        let mut command = Command::new("opencode");
        command.arg("run").args(args).args(["-m", MODEL]).current_dir(self.dir.path()).env("HOME", self.home.path()).env("PWD", self.dir.path()).env("OPENCODE_API_KEY", key()).env("npm_config_cache", npm_cache());
        finish(command, input)
    }

    /// `opencode run` with the arguments as they are (the model among them).
    pub(crate) fn opencode_without_model(&self, args: &[&str]) -> Out {
        let mut command = Command::new("opencode");
        command.arg("run").args(args).current_dir(self.dir.path()).env("HOME", self.home.path()).env("PWD", self.dir.path()).env("OPENCODE_API_KEY", key()).env("npm_config_cache", npm_cache());
        finish(command, None)
    }

    /// `wf offload ask` with the arguments as they are (the model among them).
    pub(crate) fn ask_without_model(&self, args: &[&str]) -> Out {
        let mut command = Command::new(crate::support::wf_binary());
        command.args(["offload", "ask"]).args(args).args(["--timeout-ms", "600000", "--caller", &caller_of(self.dir.path())]).current_dir(self.dir.path()).env("WF_USAGE", "off");
        crate::rca::point(&mut command, Model::Live);
        finish(command, None)
    }

    /// `wf offload ask` with `input` on its stdin.
    pub(crate) fn ask_in(&self, args: &[&str], input: Option<&str>) -> Out {
        let mut command = Command::new(crate::support::wf_binary());
        command.args(["offload", "ask"]).args(args).args(["-m", MODEL, "--timeout-ms", "600000", "--caller", &caller_of(self.dir.path())]).current_dir(self.dir.path()).env("WF_USAGE", "off");
        crate::rca::point(&mut command, Model::Live);
        finish(command, input)
    }

    /// Both CLIs, the same arguments, on this pair of sides.
    pub(crate) fn both(a: &Side, b: &Side, args: &[&str]) -> (Out, Out) {
        (a.opencode(args), b.ask(args))
    }

    /// Both CLIs on a prompt that reads files by tool: `opencode run` sees the checkout it runs in, while `ask` has no checkout on the
    /// service and sees only the files it is told to send (`--files`, which stages them without attaching them to the message).
    pub(crate) fn both_reading(a: &Side, b: &Side, args: &[&str], files: &[&str]) -> (Out, Out) {
        let listed = files.join(",");
        let mut with_files = args.to_vec();
        with_files.extend(["--files", &listed]);
        (a.opencode(args), b.ask(&with_files))
    }

    pub(crate) fn read(&self, file: &str) -> String {
        std::fs::read_to_string(self.dir.path().join(file)).unwrap_or_default()
    }
}

pub(crate) fn say(out: &Out, marker: &str) -> bool {
    out.stdout.to_uppercase().contains(&marker.to_uppercase())
}

/// Both ran, and both said `marker` on stdout.
pub(crate) fn both_say(marker: &str, opencode: &Out, ask: &Out) {
    for (who, out) in [("opencode run", opencode), ("wf offload ask", ask)] {
        assert_eq!(out.code, 0, "{who} exited {}:\n{}{}", out.code, out.stdout, out.stderr);
        assert!(say(out, marker), "{who} did not say {marker}:\n{}{}", out.stdout, out.stderr);
    }
}

/// The events `--format json` printed, one per line.
pub(crate) fn events(out: &Out) -> Vec<Value> {
    out.stdout.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).collect()
}

pub(crate) fn session_of(out: &Out) -> String {
    events(out).iter().find_map(|v| v["sessionID"].as_str().map(str::to_string)).unwrap_or_else(|| panic!("no sessionID in:\n{}{}", out.stdout, out.stderr))
}

/// The names of the tools the turn called, in order.
pub(crate) fn tools(out: &Out) -> Vec<String> {
    events(out).iter().filter(|e| e["type"] == "tool_use").filter_map(|e| e["part"]["tool"].as_str().map(str::to_string)).collect()
}

/// An event's shape: its kind, its own keys and its part's keys, so two runs compare without their ids, times and text.
pub(crate) fn shape(event: &Value) -> (String, Vec<String>, Vec<String>) {
    let keys = |v: &Value| {
        let mut k: Vec<String> = v.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
        k.sort();
        k
    };
    (event["type"].as_str().unwrap_or_default().to_string(), keys(event), keys(&event["part"]))
}

pub(crate) const PONG: &str = "Reply with exactly the word PONG and nothing else.";

/// A live-model test body, tried up to three times: the model's wording varies from run to run, so a miss is retried and only three in a
/// row fail the test. Every failed attempt is printed with its number.
pub(crate) fn retrying(body: impl Fn() + std::panic::RefUnwindSafe) {
    run_attempts(3, body);
}

pub(crate) fn run_attempts(attempts: usize, body: impl Fn() + std::panic::RefUnwindSafe) {
    let mut misses = Vec::new();
    for attempt in 1..=attempts {
        match std::panic::catch_unwind(&body) {
            Ok(()) => return,
            Err(why) => {
                let text = why.downcast_ref::<String>().cloned().or_else(|| why.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                eprintln!("attempt {attempt}/{attempts} failed: {text}");
                misses.push(format!("attempt {attempt}/{attempts}: {text}"));
            }
        }
    }
    panic!("{}", misses.join("\n"));
}
