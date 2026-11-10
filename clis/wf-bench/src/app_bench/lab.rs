//! The lab: a web export served at its base URL by a small static server, driven in headless Chromium at a phone
//! preset through the Rust browser daemon (the same device-frame session `wf browser start-device` launches).
use wf_harness::staticd::{Listener, Reply};
use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

pub struct LabParams {
    pub device: String,
    pub port: u16,
    pub base_url: String,
    pub settle_ms: u64,
}

impl LabParams {
    pub fn from(params: &Value) -> Result<LabParams, String> {
        Ok(LabParams {
            device: params["device"].as_str().ok_or("scenario params lack a device")?.to_string(),
            port: params["port"].as_u64().ok_or("scenario params lack a port")? as u16,
            base_url: params["baseUrl"].as_str().ok_or("scenario params lack a baseUrl")?.to_string(),
            settle_ms: params["settleMs"].as_u64().unwrap_or(0),
        })
    }
}

/// An Expo export writes a dynamic route to its literal template path (`leetcode/[id]/description.html`), so a
/// request for `/leetcode/two-sum/description` matches nothing on disk. Walk the path and, where a segment has no
/// directory of its own, take the single `[param]` sibling; with two, the choice is the router's, not ours.
fn resolve_dynamic(root: &Path, relative: &str) -> Option<PathBuf> {
    let segments: Vec<&str> = relative.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    let mut current = root.to_path_buf();
    for (i, segment) in segments.iter().enumerate() {
        let literal = current.join(segment);
        let last = i == segments.len() - 1;
        if literal.exists() {
            current = literal;
            continue;
        }
        let html = PathBuf::from(format!("{}.html", literal.display()));
        if last && html.exists() {
            return Some(html);
        }
        let entries = fs::read_dir(&current).ok()?;
        let mut params: Vec<String> = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('[') && name.contains(']') {
                let param = name.strip_suffix(".html").unwrap_or(&name).to_string();
                if !params.contains(&param) {
                    params.push(param);
                }
            }
        }
        if params.len() != 1 {
            return None;
        }
        let param = &params[0];
        if last {
            let as_file = current.join(format!("{param}.html"));
            return as_file.exists().then_some(as_file);
        }
        let as_dir = current.join(param);
        if !as_dir.exists() {
            return None;
        }
        current = as_dir;
    }
    let index = current.join("index.html");
    if index.exists() {
        return Some(index);
    }
    let html = PathBuf::from(format!("{}.html", current.display()));
    html.exists().then_some(html)
}

fn resolve_file(root: &Path, url_path: &str) -> Option<PathBuf> {
    let decoded = cli_http::decode_uri_component(url_path);
    let mut clean = PathBuf::new();
    for part in decoded.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                clean.pop();
            }
            p => clean.push(p),
        }
    }
    let relative = clean.to_string_lossy().into_owned();
    let base = root.join(&clean);
    for candidate in [base.clone(), PathBuf::from(format!("{}.html", base.display())), base.join("index.html")] {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    resolve_dynamic(root, &relative)
}

fn answer(target: &str, root: &Path) -> Reply {
    let path = target.split(['?', '#']).next().unwrap_or("/");
    match resolve_file(root, path).and_then(|f| fs::read(&f).ok().map(|b| (f, b))) {
        Some((file, body)) => Reply::file(&file, body),
        None => Reply::text(404, "not found"),
    }
}

pub struct Lab {
    params: LabParams,
    origin: String,
    daemon_url: String,
    daemon: Child,
    listener: Listener,
    counter: u32,
    registry: PathBuf,
}

pub struct Page {
    name: String,
    daemon_url: String,
    base: String,
    origin: String,
    settle: Duration,
}

/// A frame that mounts or unmounts under a call (the editor's WebView) invalidates the daemon's execution context for a
/// moment, so that one error is retried.
fn call(url: &str, method: &str, args: Value) -> Result<Value, String> {
    let mut attempt = 0;
    loop {
        match call_once(url, method, &args) {
            Err(e) if e.contains("Cannot find context") && attempt < 8 => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(400));
            }
            other => return other,
        }
    }
}

fn call_once(url: &str, method: &str, args: &Value) -> Result<Value, String> {
    let body = json!({ "method": method, "args": args }).to_string();
    let (_, text) = wf_harness::browser::http("POST", &format!("{url}/call"), Some(&body), Some(Duration::from_secs(300)))?;
    let reply: Value = serde_json::from_str(&text).map_err(|e| format!("daemon sent no JSON ({e}): {text}"))?;
    if reply["ok"].as_bool() == Some(true) {
        Ok(reply["result"].clone())
    } else {
        Err(reply["error"].as_str().unwrap_or("daemon call failed").to_string())
    }
}

/// The device frame the daemon renders the app in is a built page; without it every page stays blank and the suites read
/// nothing (a startup run once passed with zero executed bytes). Build it when it is missing, or say how.
fn ensure_harness_ui() -> Result<(), String> {
    let root = wf_kit::root::repo_root();
    let index = root.join("tooling/browser-harness/dist/index.html");
    if index.exists() {
        return Ok(());
    }
    let built = Command::new("pnpm")
        .args(["--config.verify-deps-before-run=false", "--filter", "@tooling/browser-harness", "run", "build"])
        .current_dir(&root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if built && index.exists() {
        Ok(())
    } else {
        Err(format!("the device-frame page is not built ({}); run `pnpm --filter @tooling/browser-harness run build`", index.display()))
    }
}

impl Lab {
    /// Serves `export_dir` at `params.base_url` and starts the daemon. A missing export is an error.
    pub fn open(params: LabParams, export_dir: &Path, tmp: &Path) -> Result<Lab, String> {
        if !export_dir.join("index.html").exists() {
            return Err(format!("no web export at {}; run `wf bench build` first, it leaves one there", export_dir.display()));
        }
        ensure_harness_ui()?;
        let serve_root = tmp.join(format!("serve-{}", params.port));
        fs::create_dir_all(&serve_root).map_err(|e| e.to_string())?;
        let link = serve_root.join(params.base_url.trim_start_matches('/'));
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(export_dir, &link).map_err(|e| e.to_string())?;
        let listener = Listener::bind("127.0.0.1", params.port, move |target| answer(target, &serve_root)).map_err(|e| format!("static server: {e}"))?;
        let registry = tmp.join(format!("daemon-{}", std::process::id()));
        fs::create_dir_all(&registry).map_err(|e| e.to_string())?;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut daemon = Command::new(exe)
            .args(["browser-daemon", "--port", "0", "--headless", "--screenshot-dir"])
            .arg(registry.join("shots"))
            .env("WF_BROWSER_SLOT", format!("bench/{}", std::process::id()))
            .env("WF_REGISTRY_DIR", &registry)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .map_err(|e| format!("cannot start the browser daemon: {e}"))?;
        let mut first = String::new();
        let stdout = daemon.stdout.take().ok_or("no daemon stdout")?;
        BufReader::new(stdout).read_line(&mut first).map_err(|e| e.to_string())?;
        let daemon_url = first.trim().strip_prefix("daemon ready ").ok_or_else(|| format!("the browser daemon did not start: {first:?}"))?.to_string();
        Ok(Lab { origin: format!("http://127.0.0.1:{}", params.port), params, daemon_url, daemon, listener, counter: 0, registry })
    }

    pub fn environment(&self) -> Vec<(String, String)> {
        vec![
            ("device".into(), self.params.device.clone()),
            ("settleMs".into(), self.params.settle_ms.to_string()),
            ("scope".into(), "lab-only (Chromium V8, headless)".into()),
        ]
    }

    pub fn open_page(&mut self) -> Result<Page, String> {
        self.counter += 1;
        let name = format!("bench-{}-{}", self.params.port, self.counter);
        call(&self.daemon_url, "open", json!({ "name": name, "devicePreset": self.params.device, "headless": true }))?;
        Ok(Page { name, daemon_url: self.daemon_url.clone(), base: self.params.base_url.clone(), origin: self.origin.clone(), settle: Duration::from_millis(self.params.settle_ms) })
    }

    pub fn dispose(mut self) {
        self.listener.stop();
        // The daemon was started as a process group leader; this signals only that group.
        let _ = nix::sys::signal::killpg(nix::unistd::Pid::from_raw(self.daemon.id() as i32), nix::sys::signal::Signal::SIGTERM);
        let _ = self.daemon.wait();
        let _ = fs::remove_dir_all(&self.registry);
    }
}

pub struct MemorySample {
    pub heap_bytes: f64,
    pub dom_nodes: f64,
    pub listeners: f64,
}

pub struct Script {
    pub url: String,
    pub length: usize,
    pub ranges: Vec<(usize, usize)>,
}

impl Page {
    pub fn settle(&self) {
        std::thread::sleep(self.settle);
    }

    pub fn load(&self, path: &str) -> Result<(), String> {
        let url = if path.starts_with("data:") { path.to_string() } else { format!("{}{}{path}", self.origin, self.base) };
        call(&self.daemon_url, "navigate", json!({ "name": self.name, "url": url }))?;
        self.settle();
        Ok(())
    }

    /// Moves to a route inside the running app, as a tap on a link would.
    pub fn go_to(&self, path: &str) -> Result<(), String> {
        let target = serde_json::to_string(&format!("{}{path}", self.base)).map_err(|e| e.to_string())?;
        // Once a screen mounts a WebView (the code editor) the daemon may evaluate inside its srcdoc frame, so climb to the app.
        self.evaluate(&format!("(() => {{ let w = window; while (w.location.href === 'about:srcdoc' && w.parent !== w) w = w.parent; w.history.pushState({{}}, '', {target}); w.dispatchEvent(new w.PopStateEvent('popstate')); }})()"))?;
        self.settle();
        Ok(())
    }

    /// The value of an expression in the app frame, as JSON.
    pub fn evaluate(&self, expression: &str) -> Result<Value, String> {
        let reply = call(&self.daemon_url, "eval", json!({ "name": self.name, "expression": expression }))?;
        let text = reply["result"].as_str().unwrap_or("null");
        serde_json::from_str(text).map_err(|e| format!("eval result is not JSON ({e}): {text}"))
    }

    pub fn tap(&self, selector: &str) -> Result<(), String> {
        call(&self.daemon_url, "tap", json!({ "name": self.name, "ref": selector, "outer": true, "indicator": false })).map(|_| ())
    }

    pub fn memory(&self, label: &str) -> Result<MemorySample, String> {
        let sample = call(&self.daemon_url, "memory", json!({ "name": self.name, "label": label, "gc": true }))?;
        let targets = sample["targets"].as_array().ok_or("no memory target sampled")?;
        let target = targets.iter().find(|t| t["target"] == "app-frame").or_else(|| targets.first()).ok_or("no memory target sampled")?;
        Ok(MemorySample { heap_bytes: target["jsHeapUsedBytes"].as_f64().unwrap_or(0.0), dom_nodes: target["domNodes"].as_f64().unwrap_or(0.0), listeners: target["listeners"].as_f64().unwrap_or(0.0) })
    }

    pub fn coverage_start(&self) -> Result<(), String> {
        call(&self.daemon_url, "coverage", json!({ "name": self.name, "action": "start" })).map(|_| ())
    }

    pub fn coverage_stop(&self) -> Result<Vec<Script>, String> {
        let report = call(&self.daemon_url, "coverage", json!({ "name": self.name, "action": "stop" }))?;
        Ok(report["scripts"]
            .as_array()
            .map(|all| {
                all.iter()
                    .map(|s| Script {
                        url: s["url"].as_str().unwrap_or("").to_string(),
                        length: s["length"].as_u64().unwrap_or(0) as usize,
                        ranges: s["ranges"].as_array().map(|r| r.iter().map(|p| (p[0].as_u64().unwrap_or(0) as usize, p[1].as_u64().unwrap_or(0) as usize)).collect()).unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn close(&self) {
        let _ = call(&self.daemon_url, "close", json!({ "name": self.name }));
    }
}
