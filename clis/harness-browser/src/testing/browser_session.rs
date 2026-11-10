use super::http_post_json;
use wf_testkit::{node_script, repo_root, run_wf, RunOpts, RunResult};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// Where Playwright keeps its browsers: `PLAYWRIGHT_BROWSERS_PATH`, else the per-user cache a CI install fills, else the image's `/opt/pw-browsers`.
pub fn browsers_dir() -> String {
    if let Ok(dir) = std::env::var("PLAYWRIGHT_BROWSERS_PATH") {
        return dir;
    }
    let cache = std::env::var("HOME").map(|h| format!("{h}/.cache/ms-playwright")).unwrap_or_default();
    if Path::new(&cache).is_dir() { cache } else { "/opt/pw-browsers".into() }
}

/// A browser test needs a Chromium and the built device-frame harness: a machine without them fails the test and names what is missing,
/// so a green run always means the browser ran.
pub fn require_browser() {
    let browsers = browsers_dir();
    let chromium = std::fs::read_dir(&browsers).map(|d| d.flatten().any(|e| e.file_name().to_string_lossy().starts_with("chromium-"))).unwrap_or(false);
    assert!(chromium, "no Chromium under {browsers} (PLAYWRIGHT_BROWSERS_PATH): install it with `pnpm --filter @lib/browser-client exec playwright install chromium`");
    let dist = repo_root().join("tooling/browser-harness/dist/index.html");
    assert!(dist.exists(), "{} is not built: run `pnpm --filter @tooling/browser-harness run build`", dist.display());
}

static CHROMIUM_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

thread_local! {
    static HELD: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// One real Chromium at a time per test process: the runner has two cores, and parallel launches made page loads miss their 30 s timeout.
/// A test that starts two sessions holds the gate once: the first takes it, the rest are counted.
pub struct ChromiumGate {
    _lock: Option<std::sync::MutexGuard<'static, ()>>,
}

pub fn chromium_gate() -> ChromiumGate {
    let first = HELD.with(|h| h.get()) == 0;
    let lock = first.then(|| CHROMIUM_GATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
    HELD.with(|h| h.set(h.get() + 1));
    ChromiumGate { _lock: lock }
}

impl Drop for ChromiumGate {
    fn drop(&mut self) {
        HELD.with(|h| h.set(h.get() - 1));
    }
}

/// A launched-browser test setting: a page server in a separate process (the test thread blocks while `wf` runs), a throwaway git repo
/// and registry, and `wf browser <args>` run against the Rust daemon. The session is stopped when it drops.
pub struct BrowserSession {
    pub root: tempfile::TempDir,
    pub repo: PathBuf,
    pub reg_dir: PathBuf,
    pub base: String,
    /// The page server's origin, without the page path.
    pub origin: String,
    server: Child,
    _gate: ChromiumGate,
}

impl BrowserSession {
    /// `server_script` is Node source that prints its port once listening (`s.listen(0, '127.0.0.1', () => console.log(s.address().port))`).
    pub fn start(prefix: &str, server_script: &str) -> Self {
        let gate = chromium_gate();
        let root = tempfile::Builder::new().prefix(prefix).tempdir().unwrap();
        let (repo, reg_dir) = (root.path().join("repo"), root.path().join("reg"));
        std::fs::create_dir(&repo).unwrap();
        std::fs::create_dir(&reg_dir).unwrap();
        assert!(Command::new("git").args(["init", "-q", "-b", "main"]).current_dir(&repo).status().unwrap().success());
        // the daemon serves the device frame from the checkout it runs in; a target dir outside the repo gives it no other way
        std::fs::create_dir_all(repo.join("tooling/browser-harness")).unwrap();
        std::os::unix::fs::symlink(repo_root().join("tooling/browser-harness/dist"), repo.join("tooling/browser-harness/dist")).unwrap();
        let mut server = node_script(server_script).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let mut first = String::new();
        BufReader::new(server.stdout.take().unwrap()).read_line(&mut first).unwrap();
        let port: u16 = first.trim().parse().expect("the page server prints its port");
        Self { root, repo, reg_dir, base: format!("http://127.0.0.1:{port}/page.html"), origin: format!("http://127.0.0.1:{port}"), server, _gate: gate }
    }

    pub fn wf(&self, args: &[&str]) -> RunResult {
        self.wf_env(args, &[])
    }

    pub fn wf_env(&self, args: &[&str], env: &[(&str, &str)]) -> RunResult {
        let mut full = vec!["browser"];
        full.extend_from_slice(args);
        let mut opts = RunOpts::default().cwd(&self.repo).timeout(Duration::from_secs(90)).env("WF_REGISTRY_DIR", &self.reg_dir.display().to_string()).env("WF_BROWSER_DAEMON", "rust");
        for (key, value) in env {
            opts = opts.env(key, value);
        }
        run_wf(&full, &opts)
    }

    /// A verb's `result`: the one JSON line it prints (`MULTI_BROWSER_JSON=1` makes the frozen daemon verbs print one too).
    pub fn result(&self, args: &[&str]) -> Value {
        let r = self.wf_env(args, &[("MULTI_BROWSER_JSON", "1")]);
        assert_eq!(r.exit_code, 0, "{args:?}: {}{}", r.stdout, r.stderr);
        let last = r.stdout.trim().lines().last().unwrap_or_default().to_string();
        serde_json::from_str::<Value>(&last).unwrap_or_else(|e| panic!("result line {last:?}: {e}"))["result"].clone()
    }

    /// The daemon's URL, starting it (`wf browser start`) when none is registered.
    pub fn daemon(&self) -> String {
        daemon_url(&self.reg_dir).unwrap_or_else(|| self.result(&["start"])["url"].as_str().expect("start names the daemon's url").to_string())
    }

    /// One daemon RPC, the way @lib/browser-client and the drivers call it: its result, or the daemon's error.
    pub fn call(&self, method: &str, args: Value) -> Result<Value, String> {
        call_at(&self.daemon(), method, args)
    }

    pub fn rpc(&self, method: &str, args: Value) -> Value {
        self.call(method, args.clone()).unwrap_or_else(|e| panic!("{method} {args}: {e}"))
    }

    /// Session `name` in the device frame, opened with the defaults `wf browser start-device` sends and `extra` over them.
    pub fn try_open(&self, name: &str, extra: Value) -> Result<Value, String> {
        let mut args = json!({ "name": name, "devicePreset": "iphone-15-pro", "orientation": "portrait", "colorScheme": "system", "frame": "screen" });
        for (k, v) in extra.as_object().into_iter().flatten() {
            args[k] = v.clone();
        }
        self.call("open", args)
    }

    pub fn open(&self, name: &str, extra: Value) -> Value {
        self.try_open(name, extra).unwrap_or_else(|e| panic!("open {name}: {e}"))
    }

    /// The `eval` RPC of an expression whose value is a JSON text, parsed.
    pub fn eval_json(&self, name: &str, expression: &str) -> Value {
        let text = self.rpc("eval", json!({ "name": name, "expression": expression }))["result"].as_str().unwrap_or("null").to_string();
        let inner: String = serde_json::from_str(&text).unwrap_or_else(|e| panic!("eval value {text:?}: {e}"));
        serde_json::from_str(&inner).unwrap_or_else(|e| panic!("eval value {inner:?}: {e}"))
    }
}

/// The daemon a test repo's `wf browser` registered under `reg_dir`, if any.
pub fn daemon_url(reg_dir: &Path) -> Option<String> {
    let registry: Value = serde_json::from_str(&std::fs::read_to_string(reg_dir.join("registry.json")).ok()?).ok()?;
    registry["slots"].as_object()?.values().find_map(|slot| slot["browserMcp"]["url"].as_str().map(str::to_string))
}

/// `POST /call {method, args}` to the daemon at `url`: its result, or its error.
pub fn call_at(url: &str, method: &str, args: Value) -> Result<Value, String> {
    let body = json!({ "method": method, "args": args }).to_string();
    let reply: Value = serde_json::from_str(&http_post_json(url.trim_start_matches("http://"), "/call", &body)).map_err(|e| format!("not JSON: {e}"))?;
    match reply["ok"].as_bool() {
        Some(true) => Ok(reply["result"].clone()),
        _ => Err(reply["error"].as_str().unwrap_or("the daemon refused the call").to_string()),
    }
}

impl Drop for BrowserSession {
    fn drop(&mut self) {
        self.wf(&["stop"]);
        let _ = self.server.kill();
        let _ = self.server.wait();
    }
}
