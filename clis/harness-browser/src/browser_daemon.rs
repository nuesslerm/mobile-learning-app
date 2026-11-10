//! `wf browser-daemon [--port N] [--screenshot-dir <dir>]`: the browser daemon, in Rust. It
//! speaks the protocol the Playwright daemon speaks (`GET /health`, `GET /status`, `POST /stop`, `POST /call`, always
//! HTTP 200 with `{ok,result}` or `{ok:false,error}`) and registers itself in the slot registry the same way. It drives
//! a browser someone else launched (`open --cdp-endpoint`) or one it launches itself, over raw CDP.
use wf_kit::args::{parse_spec, Positional, Spec};
use base64::Engine as _;
use crate::browser_launch::Session;
use cdp::attached::Attached;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

/// A pen lifts between strokes.
const PEN_LIFT_MS: u64 = 300;

enum Job {
    Status(mpsc::Sender<Value>),
    Stop(mpsc::Sender<()>),
    Call(Value, mpsc::Sender<Value>),
}

fn serve_request(mut request: tiny_http::Request, jobs: &mpsc::Sender<Job>) {
    let mut body = Vec::new();
    if request.as_reader().read_to_end(&mut body).is_err() {
        return;
    }
    let method = request.method().as_str().to_string();
    let path = request.url().split('?').next().unwrap_or("").to_string();
    match (method.as_str(), path.as_str()) {
        ("GET", "/health") => respond(request, 200, &json!({ "ok": true })),
        ("GET", "/status") => {
            let (tx, rx) = mpsc::channel();
            let _ = jobs.send(Job::Status(tx));
            respond(request, 200, &rx.recv().unwrap_or(Value::Null));
        }
        ("POST", "/stop") => {
            // Answered once the browsers are closed and the registry entry is gone, so a caller can clean up after it.
            let (tx, rx) = mpsc::channel();
            let _ = jobs.send(Job::Stop(tx));
            let _ = rx.recv();
            respond(request, 200, &json!({ "ok": true }));
        }
        (_, p) if p == "/_mcp/device-frame" || p.starts_with("/_mcp/device-frame/") => serve_device_frame(request, p),
        ("POST", "/call") => {
            let (tx, rx) = mpsc::channel();
            let call: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            let _ = jobs.send(Job::Call(call, tx));
            respond(request, 200, &rx.recv().unwrap_or_else(|_| json!({ "ok": false, "error": "daemon stopped" })));
        }
        _ => respond(request, 404, &json!({ "ok": false, "error": "Not found" })),
    }
}

struct Browser {
    name: String,
    /// Set for attached sessions; launched ones keep their page in `session`.
    page: Option<Attached>,
    session: Option<Session>,
    summary: Map<String, Value>,
    /// Set while `open --video` is recording this browser's page.
    recording: Option<cdp::record::Recording>,
    /// `<video>.inputs.jsonl`: what the session did and when (ms into the recording), beside the video.
    input_log: Option<PathBuf>,
    /// Created by the first `debug-set`: source-mapped logpoints and their hits.
    debugger: Option<std::sync::Arc<harness_core::browser_debug::Debugger>>,
    /// The id of the last input action (tap, type, press, ...): input-log lines carry it, debug hits record it.
    actions: Arc<AtomicU64>,
}

/// Commands that act on the page like a user; each gets the next action id.
const INPUT_METHODS: [&str; 8] = ["tap", "press", "type", "drag", "stroke", "trace", "swipe", "upload"];

impl Browser {
    fn page(&mut self) -> &mut Attached {
        match (&mut self.page, &mut self.session) {
            (Some(p), _) => p,
            (None, Some(s)) => &mut s.page,
            (None, None) => unreachable!("a browser has a page or a session"),
        }
    }
}

struct Daemon {
    url: String,
    screenshot_dir: PathBuf,
    shots: u32,
    browsers: Vec<Browser>,
    active: Option<String>,
}

type Res<T> = Result<T, String>;

fn text<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

impl Daemon {
    fn find(&mut self, args: &Value) -> Res<&mut Browser> {
        let known: Vec<String> = self.browsers.iter().map(|b| b.name.clone()).collect();
        let name = match text(args, "name") {
            Some(n) => n.to_string(),
            None => return Err("name is required".into()),
        };
        self.browsers.iter_mut().find(|b| b.name == name).ok_or_else(|| format!("No browser named \"{name}\" (known: {})", known.join(", ")))
    }

    /// A browser for a command that reads or runs script in the app: a launched session first points at its app frame.
    fn find_app(&mut self, args: &Value, outer: bool) -> Res<&mut Browser> {
        let b = self.find(args)?;
        if let Some(session) = b.session.as_mut() {
            session.use_app_target(outer)?;
        }
        Ok(b)
    }

    /// The session's debugger, attached on first use.
    fn debugger(&mut self, args: &Value) -> Res<Arc<harness_core::browser_debug::Debugger>> {
        let b = self.find(args)?;
        if let Some(d) = &b.debugger {
            return Ok(Arc::clone(d));
        }
        let actions = Arc::clone(&b.actions);
        let page = b.page();
        let dbg = harness_core::browser_debug::Debugger::attach(page.cdp_handle().clone(), page.session_id(), actions)?;
        b.debugger = Some(Arc::clone(&dbg));
        Ok(dbg)
    }

    fn summaries(&self) -> Vec<Value> {
        self.browsers
            .iter()
            .map(|b| {
                let mut s = b.summary.clone();
                s.insert("active".into(), json!(self.active.as_deref() == Some(b.name.as_str())));
                s.insert("targetUrl".into(), b.summary.get("targetUrl").cloned().unwrap_or(Value::Null));
                // Key order of the Playwright daemon's summary.
                let order = ["name", "engine", "renderMode", "devicePreset", "orientation", "keyboardMode", "colorScheme", "video", "active", "targetUrl", "systemFont", "owner"];
                Value::Object(order.iter().filter_map(|k| s.get(*k).map(|v| ((*k).to_string(), v.clone()))).collect())
            })
            .collect()
    }

    fn open(&mut self, args: &Value) -> Res<Value> {
        let name = text(args, "name").ok_or("name is required")?.to_string();
        if self.browsers.iter().any(|b| b.name == name) {
            return Err(format!("Browser \"{name}\" already exists"));
        }
        if text(args, "cdpEndpoint").is_none() {
            let mut session = crate::browser_launch::open(args)?;
            let wants_video = args.get("video").and_then(Value::as_bool) == Some(true);
            let (mut recording, mut input_log) = (None, None);
            if wants_video {
                let stamp = rca_core::time::iso(wf_kit::time::now_ms()).replace([':', '.'], "-");
                let path = text(args, "videoPath").map_or_else(|| self.screenshot_dir.join(format!("session-{name}-{stamp}.mp4")), PathBuf::from);
                let (vw, vh) = session.viewport;
                let started = session.page.start_recording(path.clone(), vw, vh)?;
                let log = PathBuf::from(harness_core::video_check::input_log_path_for(&path.to_string_lossy()));
                started.log_inputs_to(log.clone())?;
                (recording, input_log) = (Some(started), Some(log));
            }
            let mut summary: Map<String, Value> = [("name", json!(name)), ("engine", json!("chromium"))].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
            for (k, v) in session.device.summary_fields() {
                summary.insert(k.to_string(), v);
            }
            for (k, v) in [("video", json!(wants_video)), ("targetUrl", Value::Null), ("systemFont", Value::Null)] {
                summary.insert(k.to_string(), v);
            }
            if let Some(owner) = args.get("owner").and_then(Value::as_u64) {
                summary.insert("owner".into(), json!(owner));
            }
            self.browsers.push(Browser { name: name.clone(), page: None, session: Some(session), summary, recording, input_log, debugger: None, actions: Arc::default() });
            self.active = Some(name);
            return Ok(self.summaries().pop().unwrap_or(Value::Null));
        }
        let endpoint = text(args, "cdpEndpoint").unwrap_or_default();
        let page = Attached::attach(endpoint)?;
        let pick = |key: &str, default: &str| json!(text(args, key).unwrap_or(default));
        let summary: Map<String, Value> = [
            ("name", json!(name)),
            ("engine", pick("engine", "chromium")),
            ("renderMode", pick("renderMode", "plain")),
            ("devicePreset", pick("devicePreset", "iphone-15-pro")),
            ("orientation", pick("orientation", "portrait")),
            ("keyboardMode", pick("keyboardMode", "none")),
            ("colorScheme", pick("colorScheme", "system")),
            ("video", json!(false)),
            ("targetUrl", Value::Null),
            ("systemFont", Value::Null),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let mut summary = summary;
        if let Some(owner) = args.get("owner").and_then(Value::as_u64) {
            summary.insert("owner".into(), json!(owner));
        }
        self.browsers.push(Browser { name: name.clone(), page: Some(page), session: None, summary, recording: None, input_log: None, debugger: None, actions: Arc::default() });
        self.active = Some(name);
        Ok(self.summaries().pop().unwrap_or(Value::Null))
    }

    /// Closes `run-<pid>` sessions whose `wf run` is gone (killed, timed out): their browser would otherwise live on.
    fn reap_dead_runs(&mut self) {
        let dead: Vec<String> = self
            .browsers
            .iter()
            .filter_map(|b| {
                let pid: i32 = b.summary.get("owner").and_then(Value::as_i64).map(|p| p as i32).or_else(|| b.name.strip_prefix("run-")?.parse().ok())?;
                (nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_err()).then(|| b.name.clone())
            })
            .collect();
        for name in dead {
            let _ = self.close(&json!({ "name": name }));
        }
    }

    fn close(&mut self, args: &Value) -> Res<Value> {
        let name = match text(args, "name") {
            Some(n) => n.to_string(),
            None => self.active.clone().ok_or("No active browser")?,
        };
        let at = self.browsers.iter().position(|b| b.name == name).ok_or_else(|| format!("No browser named \"{name}\" (known: {})", self.browsers.iter().map(|b| b.name.as_str()).collect::<Vec<_>>().join(", ")))?;
        let mut b = self.browsers.remove(at);
        let mut video = Value::Null;
        if let Some(recording) = b.recording.take() {
            let path = recording.path.clone();
            let page = match (b.page.as_mut(), b.session.as_mut()) {
                (Some(p), _) => Some(p),
                (None, Some(s)) => Some(&mut s.page),
                _ => None,
            };
            if let Some(page) = page {
                let bytes = page.stop_recording(recording)?;
                video = json!({ "path": path.to_string_lossy(), "format": "mp4", "bytes": bytes });
            }
        }
        match (b.page, b.session) {
            (Some(p), _) => p.detach(),
            (None, Some(s)) => s.close(),
            (None, None) => {}
        }
        if self.active.as_deref() == Some(name.as_str()) {
            self.active = self.browsers.first().map(|b| b.name.clone());
        }
        let mut closed = json!({ "closed": name });
        if !video.is_null() {
            closed["video"] = video;
            if let Some(log) = &b.input_log {
                closed["inputLog"] = json!(log.to_string_lossy());
            }
        }
        Ok(closed)
    }

    fn navigate(&mut self, args: &Value) -> Res<Value> {
        let mut url = text(args, "url").ok_or("Provide --url")?.to_string();
        let mut forwarded_from = None;
        if let Some((host, port)) = crate::tailnet_url::parse_target(&url) {
            // Chromium's SOCKS5 proxy setting breaks WebSockets, so the page rides a loopback forward instead.
            let (local_port, _) = crate::tailnet_forward::ensure(&host, port, None)?;
            let rewritten = if crate::tailnet_url::needs_sni_preserving_hosts_mapping(&url) {
                crate::tailnet_url::ensure_hosts_file_mapping(&host);
                crate::tailnet_url::with_loopback_port_only(&url, local_port)
            } else {
                crate::tailnet_url::with_loopback_authority(&url, local_port)
            };
            forwarded_from = Some(std::mem::replace(&mut url, rewritten));
        }
        let daemon_url = self.url.clone();
        let b = self.find(args)?;
        match &mut b.session {
            Some(session) => session.navigate(&daemon_url, &url)?,
            None => b.page().navigate(&url)?,
        }
        b.summary.insert("targetUrl".into(), json!(url));
        Ok(match forwarded_from {
            Some(from) => json!({ "navigated": url, "tailnetForwardedFrom": from }),
            None => json!({ "navigated": url }),
        })
    }

    fn screenshot(&mut self, args: &Value) -> Res<Value> {
        let full = args.get("fullPage").and_then(Value::as_bool).unwrap_or(false);
        self.shots += 1;
        let index = self.shots;
        let dir = self.screenshot_dir.clone();
        let png = self.find(args)?.page().screenshot_png(full)?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let stamp = rca_core::time::iso(wf_kit::time::now_ms()).replace([':', '.'], "-");
        let path = dir.join(format!("app-{index:04}-{stamp}.png"));
        std::fs::write(&path, &png).map_err(|e| e.to_string())?;
        Ok(json!({ "path": path.to_string_lossy(), "bytes": png.len() }))
    }

    fn memory(&mut self, args: &Value) -> Res<Value> {
        let label = text(args, "label").unwrap_or("sample").to_string();
        let gc = args.get("gc").and_then(Value::as_bool) == Some(true);
        let snapshot = text(args, "snapshot").map(str::to_string);
        let page = self.find(args)?.page();
        let metrics = page.memory_metrics(gc)?;
        let pick = |name: &str| metrics.iter().find(|m| m["name"] == name).and_then(|m| m["value"].as_f64()).map_or(0, |v| v.round() as i64);
        let mut sample = json!({
            "label": label,
            "at": wf_kit::time::now_ms(),
            "gc": gc,
            "scope": "lab-only",
            "targets": [{
                "target": "page",
                "jsHeapUsedBytes": pick("JSHeapUsedSize"),
                "jsHeapTotalBytes": pick("JSHeapTotalSize"),
                "domNodes": pick("Nodes"),
                "listeners": pick("JSEventListeners"),
                "layoutObjects": pick("LayoutObjects"),
                "documents": pick("Documents"),
                "frames": pick("Frames"),
            }],
        });
        if let Some(path) = snapshot {
            let bytes = page.heap_snapshot(Path::new(&path))?;
            sample["snapshot"] = json!({ "path": path, "bytes": bytes });
        }
        Ok(sample)
    }

    fn coverage(&mut self, args: &Value) -> Res<Value> {
        let page = self.find(args)?.page();
        if text(args, "action") == Some("start") {
            page.coverage_start()?;
            return Ok(json!({ "started": true }));
        }
        let scripts = page.coverage_stop()?;
        let out = text(args, "out").map(str::to_string);
        if let Some(path) = &out {
            if let Some(dir) = Path::new(path).parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(path, json!({ "scripts": scripts }).to_string()).map_err(|e| e.to_string())?;
        }
        let mut report = json!({ "scripts": scripts });
        if let Some(path) = out {
            report["outPath"] = json!(path);
        }
        Ok(report)
    }

    fn dispatch(&mut self, method: &str, args: &Value) -> Res<Value> {
        // what the page reported since the last command (a focus that raised the keyboard) is seen by this one
        if let Ok(Some(session)) = self.find(args).map(|b| b.session.as_mut()) {
            session.tick();
        }
        // Taken before the command runs, so a breakpoint hit while it runs already records it.
        let action = if INPUT_METHODS.contains(&method) { self.find(args).ok().map(|b| b.actions.fetch_add(1, Ordering::SeqCst) + 1) } else { None };
        if let (Some(n), Ok(b)) = (action, self.find(args)) {
            b.page().set_step(n);
        }
        let result = self.dispatch_inner(method, args)?;
        if let Some(action) = action {
            self.log_input(method, args, &result, action);
        }
        Ok(result)
    }

    /// Appends what a command did to the recording's input log, with its action id. Its `t` is the first frame captured after
    /// its marker was drawn (else after it), so the video at `t` shows it (a tap's dot) however slow captures are; the
    /// recording fills it in once that frame lands, so the next command never waits for a capture.
    fn log_input(&mut self, method: &str, args: &Value, result: &Value, action: u64) {
        let done = std::time::Instant::now();
        let Some(name) = text(args, "name") else { return };
        let Some(b) = self.browsers.iter_mut().find(|b| b.name == name) else { return };
        if b.recording.is_none() {
            return;
        }
        let since = b.page().marked_at().unwrap_or(done);
        let Some(recording) = &b.recording else { return };
        let t = Value::Null;
        let mut entry = match method {
            "tap" => json!({ "t": t, "kind": "tap", "x": result["x"], "y": result["y"], "ref": args.get("ref").unwrap_or(&args["selector"]) }),
            "press" => json!({ "t": t, "kind": "press", "key": args["key"] }),
            "type" if result["mode"] == "caret" => json!({ "t": t, "kind": "type", "chars": result["keys"] }),
            "drag" => json!({ "t": t, "kind": "drag", "ref": args["ref"] }),
            "stroke" | "trace" => json!({ "t": t, "kind": method, "ref": args["ref"], "samples": result["samples"], "coalesced": result["coalesced"], "maxMoveMs": result["maxMoveMs"] }),
            "swipe" => json!({ "t": t, "kind": "swipe", "x": result["from"]["x"], "y": result["from"]["y"] }),
            "upload" => json!({ "t": t, "kind": "upload", "files": result["delivered"].as_array().map_or(0, Vec::len) }),
            _ => return,
        };
        entry["action"] = json!(action);
        recording.log_input(since, entry);
    }

    fn dispatch_inner(&mut self, method: &str, args: &Value) -> Res<Value> {
        // `--indicator <colour>` tints the on-page markers, `--no-indicator` (sent as `false`) turns them off.
        if matches!(method, "tap" | "drag" | "stroke" | "trace" | "swipe" | "type") {
            let off = args.get("indicator") == Some(&Value::Bool(false));
            let color = args.get("indicator").and_then(Value::as_str).map(str::to_string);
            if let Ok(b) = self.find(args) {
                b.page().set_indicator(color, off);
            }
        }
        match method {
            "open" => self.open(args),
            "close" => self.close(args),
            "list" => Ok(json!(self.summaries())),
            "navigate" => self.navigate(args),
            "eval" => {
                let expression = text(args, "expression").ok_or("expression is required")?.to_string();
                let outer = args.get("outer").and_then(Value::as_bool) == Some(true);
                let value = self.find_app(args, outer)?.page().eval(&expression)?;
                Ok(json!({ "result": value.to_string() }))
            }
            "cookies" => {
                let urls: Option<Vec<String>> = args.get("urls").and_then(Value::as_array).map(|a| a.iter().filter_map(|u| u.as_str().map(str::to_string)).collect());
                Ok(json!({ "cookies": self.find(args)?.page().cookies(urls.as_deref())? }))
            }
            "screenshot" => self.screenshot(args),
            "memory" => self.memory(args),
            "coverage" => self.coverage(args),
            "snapshot" => {
                if args.get("outer").and_then(Value::as_bool) == Some(true) {
                    return Err("snapshot --outer is a device-frame option; attached sessions have no frame".into());
                }
                Ok(json!({ "yaml": self.find_app(args, false)?.page().snapshot()? }))
            }
            "tap" if text(args, "ref").is_none() && args.get("x").is_some() => {
                let (x, y) = (args["x"].as_f64().ok_or("tap: x must be a number")?, args["y"].as_f64().ok_or("tap: y must be a number")?);
                let force_mouse = args.get("mouse").and_then(Value::as_bool) == Some(true);
                let b = self.find(args)?;
                let touch = b.session.as_ref().is_some_and(|s| s.touch()) && !force_mouse;
                let (ox, oy, scale) = screen_origin(b);
                let (px, py, hold) = b.page().tap_point(ox + x * scale, oy + y * scale, touch)?;
                Ok(json!({ "tapped": format!("{x},{y}"), "x": px, "y": py, "screen": { "x": x, "y": y }, "holdMs": hold, "pointer": if touch { "touch" } else { "mouse" } }))
            }
            "tap" => {
                let reference = text(args, "ref").ok_or("ref is required")?.to_string();
                let outer = args.get("outer").and_then(Value::as_bool) == Some(true);
                let force_mouse = args.get("mouse").and_then(Value::as_bool) == Some(true);
                let b = self.find_app(args, outer)?;
                if b.session.as_ref().is_some_and(|s| s.touch()) && !force_mouse {
                    let (x, y, hold) = b.page().tap_touch(&reference)?;
                    return Ok(json!({ "tapped": reference, "x": x, "y": y, "holdMs": hold, "pointer": "touch" }));
                }
                let (x, y) = b.page().tap(&reference)?;
                Ok(json!({ "tapped": reference, "x": x, "y": y, "holdMs": 0, "pointer": "mouse" }))
            }
            "type" => {
                if args.get("atCaret").and_then(Value::as_bool) == Some(true) {
                    let reference = text(args, "ref").ok_or("ref is required")?.to_string();
                    let typed = args.get("text").and_then(Value::as_str).ok_or("text is required")?.to_string();
                    let human = args.get("human").and_then(Value::as_bool) == Some(true);
                    // the run's speed (WF_TEST_SPEED): a thumb's pause at that many times a user's pace
                    let speed = args.get("speed").and_then(Value::as_f64).filter(|s| *s >= 1.0).unwrap_or(1.0);
                    let outer = args.get("outer").and_then(Value::as_bool) == Some(true);
                    let b = self.find_app(args, outer)?;
                    let platform = b.session.as_ref().map_or_else(|| "ios".to_string(), |s| s.platform());
                    let page = b.page();
                    page.emulate_focus()?;
                    if !page.ref_has_focus(&reference)? {
                        return Err(format!("type --at-caret: focus is not in \"{reference}\" -- tap where the caret should go first"));
                    }
                    let mut starts: Vec<std::time::Instant> = Vec::new();
                    let mut previous = '\0';
                    for ch in typed.chars() {
                        if !starts.is_empty() {
                            let gap = if human { cdp::attached::human_key_gap_ms(previous) / speed } else { 35.0 };
                            std::thread::sleep(Duration::from_secs_f64(gap / 1000.0));
                        }
                        starts.push(std::time::Instant::now());
                        page.key_tint(ch);
                        page.type_char(ch, &platform)?;
                        previous = ch;
                    }
                    let mut gaps: Vec<f64> = starts.windows(2).map(|w| (w[1] - w[0]).as_secs_f64() * 1000.0).collect();
                    gaps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    let median = match gaps.len() {
                        0 => 0.0,
                        n if n % 2 == 1 => gaps[n / 2],
                        n => (gaps[n / 2 - 1] + gaps[n / 2]) / 2.0,
                    };
                    return Ok(json!({ "typed": reference, "perKey": false, "mode": "caret", "keys": starts.len(), "interKeyP50Ms": median.round() as i64 }));
                }
                let reference = text(args, "ref").ok_or("ref is required")?.to_string();
                let typed = args.get("text").and_then(Value::as_str).ok_or("text is required")?.to_string();
                let submit = args.get("submit").and_then(Value::as_bool).unwrap_or(false);
                let value_after = self.find_app(args, args.get("outer").and_then(Value::as_bool) == Some(true))?.page().type_text(&reference, &typed, submit)?;
                Ok(json!({ "typed": reference, "perKey": false, "mode": "field", "valueAfter": value_after }))
            }
            "press" => {
                let key = text(args, "key").ok_or("key is required")?.to_string();
                let repeat = args.get("repeat").and_then(Value::as_u64).unwrap_or(1);
                let b = self.find_app(args, false)?;
                if !b.page().editable_has_focus()? {
                    return Err("press: nothing editable has focus -- tap a field first".into());
                }
                for _ in 0..repeat {
                    b.page().press_key(&key)?;
                }
                Ok(json!({ "key": key, "repeat": repeat }))
            }
            "drag" => {
                let reference = text(args, "ref").ok_or("ref is required")?.to_string();
                let (dx, dy) = (args.get("dx").and_then(Value::as_f64), args.get("dy").and_then(Value::as_f64));
                let (Some(dx), Some(dy)) = (dx, dy) else { return Err("dx and dy are required numbers".into()) };
                let steps = args.get("steps").and_then(Value::as_u64).filter(|n| *n > 0).unwrap_or(12) as u32;
                let delay = args.get("stepDelayMs").and_then(Value::as_u64).unwrap_or(0);
                self.find_app(args, false)?.page().drag(&reference, dx, dy, steps, delay)?;
                Ok(json!({ "dragged": reference, "dx": args["dx"], "dy": args["dy"] }))
            }
            "stroke" => {
                let reference = text(args, "ref").ok_or("ref is required")?.to_string();
                let human = args.get("human").and_then(Value::as_bool) == Some(true);
                let transform = args.get("transform").filter(|t| t.is_object());
                let mut points: Vec<(f64, f64)> = args
                    .get("points")
                    .and_then(Value::as_array)
                    .ok_or("points is required")?
                    .iter()
                    .map(|p| Ok((p["x"].as_f64().ok_or("path point is not finite")?, p["y"].as_f64().ok_or("path point is not finite")?)))
                    .collect::<Result<_, &str>>()?;
                let count = points.len();
                if let Some(t) = transform {
                    let offset = t.get("offset").filter(|o| o.is_object()).map(|o| (o["x"].as_f64().unwrap_or(0.0), o["y"].as_f64().unwrap_or(0.0)));
                    points = cdp::attached::transform_path(
                        points,
                        &cdp::attached::PathTransform {
                            truncate: t.get("truncate").and_then(Value::as_f64),
                            reverse: t.get("reverse").and_then(Value::as_bool) == Some(true),
                            scale: t.get("scale").and_then(Value::as_f64),
                            offset,
                            wobble: t.get("wobble").and_then(Value::as_f64),
                            wobble_seed: t.get("wobbleSeed").and_then(Value::as_u64).map(|s| s as u32),
                        },
                    )?;
                }
                let pixels = text(args, "units") == Some("px");
                let curve = args.get("curve").and_then(Value::as_bool) == Some(true);
                let per_segment = args.get("samplesPerSegment").and_then(Value::as_u64).unwrap_or(12) as u32;
                let delay = args.get("stepDelayMs").and_then(Value::as_u64).unwrap_or(8);
                let b = self.find_app(args, false)?;
                let touch = b.session.as_ref().is_some_and(|s| s.touch()) && args.get("mouse").and_then(Value::as_bool) != Some(true);
                let (samples, drawn) = b.page().stroke(&reference, &points, pixels, curve, per_segment, delay, human, touch)?;
                Ok(json!({ "stroked": reference, "points": count, "samples": samples, "sent": drawn.sent, "coalesced": drawn.moves - drawn.sent, "maxMoveMs": drawn.max_move_ms, "ms": drawn.ms, "pointer": if touch { "touch" } else { "mouse" } }))
            }
            "audioInject" | "cameraInject" => {
                let audio = method == "audioInject";
                let noun = if audio { "audio-inject" } else { "camera-inject" };
                let path = text(args, "path").ok_or("path is required")?.to_string();
                if !std::path::Path::new(&path).exists() {
                    return Err(format!("{noun} file not found: {path}"));
                }
                let ext = std::path::Path::new(&path).extension().and_then(|e| e.to_str()).map(|e| format!(".{}", e.to_ascii_lowercase())).unwrap_or_default();
                let table: &[(&str, &str)] = if audio {
                    &[(".wav", "audio/wav"), (".mp3", "audio/mpeg"), (".m4a", "audio/mp4"), (".mp4", "audio/mp4"), (".aac", "audio/aac"), (".ogg", "audio/ogg"), (".opus", "audio/ogg"), (".webm", "audio/webm"), (".flac", "audio/flac")]
                } else {
                    &[(".png", "image/png"), (".jpg", "image/jpeg"), (".jpeg", "image/jpeg"), (".webp", "image/webp"), (".webm", "video/webm"), (".mp4", "video/mp4")]
                };
                let Some((_, mime)) = table.iter().find(|(e, _)| *e == ext) else {
                    let supported = table.iter().map(|(e, _)| *e).collect::<Vec<_>>().join(", ");
                    return Err(if audio {
                        format!("audio-inject cannot guess a media type for \"{}\" -- supported: {supported}", if ext.is_empty() { &path } else { &ext })
                    } else {
                        format!("camera-inject cannot show \"{}\" -- supported: {supported}", if ext.is_empty() { &path } else { &ext })
                    });
                };
                let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                let clip = json!({ "base64": cdp::BASE64.encode(&bytes), "mimeType": mime, "label": path });
                let page = self.find(args)?.page();
                if audio {
                    // A refusal set with `permission` stands only until this grants; the fake microphone needs the grant.
                    let _ = page.set_permission("microphone", "granted");
                }
                // replace: one pending clip, as a phone's microphone has one input, and a fresh consumed list for it
                let reset = if args.get("replace").and_then(Value::as_bool) == Some(true) { "const m = globalThis.__mcpFakeMic; if (m) { m.queue.length = 0; m.consumed.length = 0; }" } else { "" };
                let expression = if audio {
                    format!("(() => {{ const push = globalThis.__mcpFakeMicPush; if (typeof push !== 'function') return false; {reset} push({clip}); return true; }})()")
                } else {
                    format!("(async () => {{ const set = globalThis.__mcpFakeCameraSet; return typeof set === 'function' ? await set({clip}) : null; }})()")
                };
                let (mut frames, mut size) = (0, json!({ "width": 0, "height": 0 }));
                for frame in page.frames()? {
                    // A frame mid-navigation is skipped: the others still get the source.
                    if let Ok(done) = page.eval_in_frame(&frame.id, &expression) {
                        if audio && done == json!(true) {
                            frames += 1;
                        } else if !audio && done.is_object() {
                            frames += 1;
                            size = done;
                        }
                    }
                }
                if frames == 0 {
                    return Err(if audio {
                        "audio-inject found no frame with the fake microphone installed -- the init script lands on navigation, so navigate (or reload) after opening the browser".into()
                    } else {
                        "camera-inject: no frame has the fake camera installed -- is a page loaded?".into()
                    });
                }
                Ok(if audio {
                    json!({ "label": path, "mimeType": mime, "bytes": bytes.len(), "frames": frames })
                } else {
                    json!({ "path": path, "width": size["width"], "height": size["height"], "frames": frames })
                })
            }
            "tailnetForward" => {
                if args.get("middlewarePath").is_some() {
                    return Err("tailnet-forward --middleware runs a JavaScript module and is not supported by the Rust browser daemon".into());
                }
                let host = text(args, "host").ok_or("host is required")?.to_string();
                let port = args.get("port").and_then(Value::as_u64).and_then(|p| u16::try_from(p).ok()).ok_or("port is required")?;
                let local = args.get("localPort").and_then(Value::as_u64).and_then(|p| u16::try_from(p).ok());
                let (local_port, reused) = crate::tailnet_forward::ensure(&host, port, local)?;
                Ok(json!({ "url": format!("http://127.0.0.1:{local_port}"), "localPort": local_port, "reused": reused }))
            }
            "networkProxy" => {
                let pattern = text(args, "pattern").ok_or("pattern is required")?.to_string();
                let enable = args.get("enable").and_then(Value::as_bool).unwrap_or(true);
                let active = self.find(args)?.page().network_proxy(&pattern, enable)?;
                Ok(json!({ "active": active, "pattern": pattern }))
            }
            "networkStub" => {
                let pattern = text(args, "pattern").ok_or("pattern is required")?.replace("**", "*");
                let method = text(args, "method").map(|m| m.to_ascii_uppercase()).filter(|m| m != "*" && m != "ANY");
                let status = args.get("status").and_then(Value::as_i64).unwrap_or(200);
                let mut headers: Vec<(String, String)> = args.get("headers").and_then(Value::as_object).map(|h| h.iter().map(|(k, v)| (k.clone(), v.as_str().map_or_else(|| v.to_string(), str::to_string))).collect()).unwrap_or_default();
                let body = match text(args, "bodyFile") {
                    Some(f) => std::fs::read(f).map_err(|e| format!("cannot read the stub body {f}: {e}"))?,
                    None => text(args, "body").unwrap_or_default().as_bytes().to_vec(),
                };
                if let Some(ct) = text(args, "contentType") {
                    headers.retain(|(k, _)| !k.eq_ignore_ascii_case("content-type"));
                    headers.push(("Content-Type".into(), ct.to_string()));
                }
                let delay_ms = args.get("delayMs").and_then(Value::as_u64).unwrap_or(0);
                let bytes = body.len();
                self.find(args)?.page().network_stub(cdp::attached::Stub { method: method.clone(), pattern: pattern.clone(), status, headers, body, delay_ms })?;
                Ok(json!({ "pattern": pattern, "method": method, "status": status, "bytes": bytes, "delayMs": delay_ms }))
            }
            "networkUnstub" => {
                let pattern = text(args, "pattern");
                let removed = self.find(args)?.page().network_unstub(pattern)?;
                Ok(json!({ "removed": removed }))
            }
            "networkStubHits" => {
                let clear = args.get("clear").and_then(Value::as_bool).unwrap_or(false);
                Ok(json!({ "hits": self.find(args)?.page().network_stub_hits(clear) }))
            }
            "videoCheck" => {
                let video = text(args, "video").ok_or("video is required")?.to_string();
                let inputs = text(args, "inputs").map_or_else(|| harness_core::video_check::input_log_path_for(&video), str::to_string);
                harness_core::video_check::check_video_taps(&video, &inputs)
            }
            "checkClipping" => {
                let tolerance = args.get("tolerance").and_then(Value::as_f64).unwrap_or(0.98);
                self.find(args)?.page().check_clipping(tolerance)
            }
            "upload" => {
                let paths: Vec<String> = args.get("paths").and_then(Value::as_array).map(|a| a.iter().filter_map(|p| p.as_str().map(str::to_string)).collect()).unwrap_or_default();
                let clear = args.get("clear").and_then(Value::as_bool) == Some(true);
                let (armed, delivered) = self.find_app(args, false)?.page().upload(&paths, clear)?;
                Ok(json!({ "armed": armed, "delivered": delivered }))
            }
            "swipe" => {
                let (dx, dy) = (args.get("dx").and_then(Value::as_f64).unwrap_or(0.0), args.get("dy").and_then(Value::as_f64).unwrap_or(0.0));
                let duration = args.get("durationMs").and_then(Value::as_u64);
                // a short move flings, as a finger that lifts while moving does
                let fling = args.get("fling").and_then(Value::as_bool).unwrap_or_else(|| duration.is_some_and(|d| d < 250));
                let reference = text(args, "ref").map(str::to_string);
                let from = args.get("from").and_then(|f| Some((f["x"].as_f64()?, f["y"].as_f64()?)));
                let on_screen = args.get("screen").and_then(Value::as_bool) == Some(true);
                let b = self.find_app(args, true)?;
                let touch = b.session.as_ref().is_some_and(|s| s.touch());
                let (ox, oy, scale) = if on_screen { screen_origin(b) } else { (0.0, 0.0, 1.0) };
                let (dx, dy) = (dx * scale, dy * scale);
                let swiped = match (&reference, from) {
                    (Some(r), _) => b.page().swipe(cdp::attached::SwipeStart::Ref(r), dx, dy, fling, touch, duration)?,
                    (None, Some((x, y))) if on_screen => b.page().swipe(cdp::attached::SwipeStart::At { page: (ox + x * scale, oy + y * scale), frame: (x, y) }, dx, dy, fling, touch, duration)?,
                    (None, Some((x, y))) => b.page().swipe(cdp::attached::SwipeStart::From(x, y), dx, dy, fling, touch, duration)?,
                    (None, None) => return Err("swipe: pass --ref or --from".into()),
                };
                Ok(json!({
                    "from": { "x": swiped.from.0, "y": swiped.from.1 },
                    "to": { "x": swiped.to.0, "y": swiped.to.1 },
                    "ms": swiped.ms,
                    "scrolled": { "dx": swiped.scrolled.0, "dy": swiped.scrolled.1 },
                    "pointer": if touch { "touch" } else { "mouse" },
                }))
            }
            "scroll" => {
                let (x, y) = (args.get("x").and_then(Value::as_f64), args.get("y").and_then(Value::as_f64));
                let (Some(x), Some(y)) = (x, y) else { return Err("x and y are required numbers".into()) };
                self.find_app(args, false)?.page().eval(&format!("window.scrollBy({x}, {y})"))?;
                Ok(json!({ "scrolled": { "x": args["x"], "y": args["y"] } }))
            }
            "emulateMobile" => {
                let ua = text(args, "userAgent").ok_or("userAgent is required")?.to_string();
                let (user_agent, width, height) = self.find(args)?.page().emulate_mobile(&ua)?;
                Ok(json!({ "applied": true, "userAgent": user_agent, "width": width, "height": height }))
            }
            "fullscreenWindow" => {
                let (applied, width, height) = self.find(args)?.page().fullscreen_window();
                Ok(json!({ "applied": applied, "width": width, "height": height }))
            }
            "shellEval" => {
                let expression = text(args, "expression").ok_or("expression is required")?.to_string();
                let page = self.find(args)?.page();
                page.set_target(None);
                Ok(json!({ "result": page.eval_page(&expression)?.to_string() }))
            }
            "debugSet" => {
                let file = text(args, "file").ok_or("debug-set needs --file <source path>")?.to_string();
                let line = args.get("line").and_then(Value::as_u64).filter(|l| *l > 0).ok_or("debug-set needs --line <1-based line>")? as u32;
                let condition = text(args, "condition").map(str::to_string);
                let capture: Vec<String> = text(args, "capture").map(|c| c.split(',').map(|e| e.trim().to_string()).filter(|e| !e.is_empty()).collect()).unwrap_or_default();
                let defer = args.get("defer").and_then(Value::as_bool) == Some(true);
                let pause = args.get("pause").and_then(Value::as_bool) == Some(true);
                self.debugger(args)?.set(&file, line, condition, capture, defer, pause)
            }
            "debugHits" => {
                let clear = args.get("clear").and_then(Value::as_bool) == Some(true);
                let b = self.find(args)?;
                let hits = b.debugger.as_ref().map(|d| d.hits(clear)).unwrap_or_default();
                Ok(json!({ "count": hits.len(), "hits": hits }))
            }
            "debugClear" => {
                let b = self.find(args)?;
                match b.debugger.as_ref() {
                    Some(d) => d.clear(),
                    None => Ok(json!({ "cleared": 0 })),
                }
            }
            "debugPause" => {
                let ms = args.get("timeout").and_then(Value::as_u64).unwrap_or(5000);
                self.debugger(args)?.pause(ms)
            }
            "debugStep" => {
                let ms = args.get("timeout").and_then(Value::as_u64).unwrap_or(5000);
                let kind = text(args, "kind").unwrap_or("over").to_string();
                self.debugger(args)?.step(&kind, ms)
            }
            "debugResume" => self.debugger(args)?.resume(),
            "debugEval" => {
                let expression = text(args, "expression").ok_or("debug-eval needs an expression")?.to_string();
                self.debugger(args)?.eval(&expression)
            }
            "waitObserve" => {
                // The page script reports through a binding. Block here only while pumping the daemon's own tick, so focus
                // events (the mock keyboard) keep flowing: a pending in-page promise would starve them.
                let expression = text(args, "expression").ok_or("expression is required")?.to_string();
                let ms = args.get("timeout").and_then(Value::as_u64).unwrap_or(10_000);
                let outer = args.get("outer").and_then(Value::as_bool) == Some(true);
                let b = self.find_app(args, outer)?;
                let session = b.session.as_mut().ok_or("Shell not loaded, open first")?;
                session.device.wait_done = None;
                session.page.eval(&expression)?;
                let deadline = std::time::Instant::now() + std::time::Duration::from_millis(ms + 1500);
                loop {
                    session.tick();
                    if let Some(payload) = session.device.wait_done.take() {
                        return Ok(json!({ "result": payload }));
                    }
                    if std::time::Instant::now() >= deadline {
                        return Err("wait-observe: the page script never reported".into());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
            "outerEval" => {
                let expression = text(args, "expression").ok_or("expression is required")?.to_string();
                Ok(json!({ "result": self.find_app(args, true)?.page().eval(&expression)?.to_string() }))
            }
            "openedUrls" => {
                let clear = args.get("clear").and_then(Value::as_bool) == Some(true);
                let b = self.find(args)?;
                let Some(session) = b.session.as_mut() else { return Ok(json!({ "urls": [] })) };
                session.tick();
                let urls = session.device.opened_urls.clone();
                if clear {
                    session.device.opened_urls.clear();
                }
                Ok(json!({ "urls": urls }))
            }
            "permission" => {
                const KINDS: [&str; 6] = ["microphone", "camera", "clipboard-read", "clipboard-write", "geolocation", "notifications"];
                let kind = text(args, "kind").filter(|k| KINDS.contains(k)).ok_or("kind must be one of microphone, camera, clipboard-read, clipboard-write, geolocation, notifications")?.to_string();
                let state = text(args, "state").filter(|s| ["granted", "denied", "prompt"].contains(s)).ok_or("state must be one of granted, denied, prompt")?.to_string();
                self.find(args)?.page().set_permission(&kind, &state)?;
                Ok(json!({ "kind": kind, "state": state }))
            }
            "clipboard" => {
                let set = args.get("set").and_then(Value::as_str).map(str::to_string);
                let page = self.find(args)?.page();
                page.set_target(None);
                // The clipboard API answers only a focused document.
                page.bring_to_front()?;
                if let Some(text) = &set {
                    let literal = serde_json::to_string(text).map_err(|e| e.to_string())?;
                    page.eval_page(&format!("navigator.clipboard.writeText({literal})"))?;
                }
                let read = page.eval_page("navigator.clipboard.readText()")?;
                Ok(json!({ "text": read.as_str().unwrap_or("") }))
            }
            "indicateAppTap" => {
                let b = self.find(args)?;
                let (x, y) = (args["x"].as_f64().ok_or("indicateAppTap needs x")?, args["y"].as_f64().ok_or("indicateAppTap needs y")?);
                match args.get("indicator") {
                    Some(Value::Bool(false)) => {}
                    other => {
                        b.page().set_indicator(other.and_then(Value::as_str).map(str::to_string), false);
                        b.page().indicate_app_tap(x, y);
                        b.page().set_indicator(None, false);
                    }
                }
                Ok(Value::Null)
            }
            "indicateKeyPress" => {
                let b = self.find(args)?;
                let ch = args["char"].as_str().and_then(|c| c.chars().next()).ok_or("indicateKeyPress needs char")?;
                match args.get("indicator") {
                    Some(Value::Bool(false)) => {}
                    other => {
                        b.page().set_indicator(other.and_then(Value::as_str).map(str::to_string), false);
                        b.page().key_tint(ch);
                        b.page().set_indicator(None, false);
                    }
                }
                Ok(Value::Null)
            }
            "setDevice" => {
                let device = text(args, "device").ok_or("device is required")?.to_string();
                let orientation = text(args, "orientation").map(str::to_string);
                let b = self.find(args)?;
                let session = b.session.as_mut().ok_or("Shell not loaded, open first")?;
                let (device, orientation) = session.set_device(&device, orientation.as_deref())?;
                Ok(json!({ "device": device, "orientation": orientation }))
            }
            "consoleLogs" => {
                const LEVELS: [&str; 4] = ["debug", "info", "warn", "error"];
                let level = text(args, "level").unwrap_or("debug");
                let floor = LEVELS.iter().position(|l| *l == level).ok_or("level must be one of debug, info, warn, error")?;
                let clear = args.get("clear").and_then(Value::as_bool) == Some(true);
                let b = self.find(args)?;
                // a device frame's own page is not the app: its lines are left out
                let framed = b.session.as_ref().is_some_and(|s| s.framed());
                let page = b.page();
                let main = page.main_frame().to_string();
                let lines: Vec<String> = page
                    .console_logs(clear)
                    .into_iter()
                    .filter(|l| !(framed && l.frame.as_deref() == Some(main.as_str())))
                    .filter(|l| LEVELS.iter().position(|x| *x == l.level).unwrap_or(0) >= floor)
                    .map(|l| l.text)
                    .collect();
                Ok(json!({ "count": lines.len(), "lines": lines, "cleared": clear }))
            }
            "recordStart" => {
                let path = PathBuf::from(text(args, "path").ok_or("path is required")?);
                let b = self.find(args)?;
                if b.recording.is_some() {
                    return Err("recordStart: this session is already recording -- recordStop first".into());
                }
                let (vw, vh) = match &b.session {
                    Some(s) => s.viewport,
                    None => {
                        let v = b.page().eval_page("[innerWidth, innerHeight]")?;
                        (v[0].as_i64().unwrap_or(0), v[1].as_i64().unwrap_or(0))
                    }
                };
                if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                }
                let recording = b.page().start_recording(path.clone(), vw, vh)?;
                let log = PathBuf::from(harness_core::video_check::input_log_path_for(&path.to_string_lossy()));
                recording.log_inputs_to(log.clone())?;
                b.recording = Some(recording);
                b.input_log = Some(log.clone());
                b.summary.insert("video".into(), json!(true));
                Ok(json!({ "path": path.to_string_lossy(), "inputLog": log.to_string_lossy() }))
            }
            "recordStop" => {
                let b = self.find(args)?;
                let Some(recording) = b.recording.take() else { return Ok(json!({ "video": null })) };
                let path = recording.path.clone();
                let bytes = b.page().stop_recording(recording)?;
                b.summary.insert("video".into(), json!(false));
                Ok(json!({ "video": { "path": path.to_string_lossy(), "format": "mp4", "bytes": bytes }, "inputLog": b.input_log.as_ref().map(|l| l.to_string_lossy()) }))
            }
            "trace" => {
                let reference = text(args, "ref").ok_or("ref is required")?.to_string();
                let source = text(args, "source").ok_or("source is required")?.to_string();
                let human = args.get("human").and_then(Value::as_bool) == Some(true);
                let b = self.find_app(args, args.get("outer").and_then(Value::as_bool) == Some(true))?;
                let touch = b.session.as_ref().is_some_and(|s| s.touch());
                let page = b.page();
                let strokes: Vec<Vec<(f64, f64)>> = if source == "guide" {
                    let guide = page.guide_of(&reference)?;
                    let strokes = match guide {
                        Some(g) => harness_core::trace::svg_path_strokes(&g)?,
                        None => Vec::new(),
                    };
                    if strokes.is_empty() {
                        return Err(format!("trace: \"{reference}\" publishes no guide (data-active-guide is empty)"));
                    }
                    strokes
                } else {
                    // Cropped from a whole-viewport capture, once the pad holds still (a route transition moves it).
                    let mut found = None;
                    for _ in 0..30 {
                        let ([x, y, w, h], _) = page.element_box("trace", &reference)?;
                        let (png, vw) = page.viewport_png()?;
                        let ([x2, y2, w2, h2], _) = page.element_box("trace", &reference)?;
                        if (x - x2).abs() < 0.5 && (y - y2).abs() < 0.5 && (w - w2).abs() < 0.5 && (h - h2).abs() < 0.5 {
                            found = Some(harness_core::trace::template_strokes_in_viewport(&png, vw, [x, y, w, h])?);
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    let strokes = found.ok_or_else(|| format!("trace: \"{reference}\" kept moving for 3 s"))?;
                    if strokes.is_empty() {
                        return Err(format!("trace: no template shows on \"{reference}\""));
                    }
                    strokes
                };
                // Measured after the screenshot, in case the capture moved the page.
                let ([bx, by, _, _], _) = page.element_box("trace", &reference)?;
                let (mut samples, mut sent, mut coalesced, mut max_move_ms) = (0, 0, 0, 0);
                for (index, stroke) in strokes.iter().enumerate() {
                    if index > 0 {
                        std::thread::sleep(Duration::from_millis(PEN_LIFT_MS));
                    }
                    let on_page: Vec<(f64, f64)> = stroke.iter().map(|(x, y)| (bx + x, by + y)).collect();
                    let drawn = if human { cdp::attached::human_stroke_samples(&on_page)? } else { cdp::attached::stroke_samples(&on_page, false, 12)? };
                    let moved = page.draw_samples(&drawn, if human { cdp::attached::FRAME_MS } else { 8.0 }, touch, human)?;
                    samples += drawn.len();
                    (sent, coalesced, max_move_ms) = (sent + moved.sent, coalesced + moved.moves - moved.sent, max_move_ms.max(moved.max_move_ms));
                }
                Ok(json!({ "strokes": strokes.len(), "samples": samples, "sent": sent, "coalesced": coalesced, "maxMoveMs": max_move_ms }))
            }
            _ => Err(format!("Unknown command: {method}")),
        }
    }
}

/// Where the app's screen sits in the page, and its scale: the device frame's app frame, else the page itself.
fn screen_origin(b: &mut Browser) -> (f64, f64, f64) {
    if !b.session.as_ref().is_some_and(|s| s.framed()) {
        return (0.0, 0.0, 1.0);
    }
    const BOX: &str = "(() => { const f = document.querySelector('iframe[name=\"device-frame-app\"]'); if (!f) return null; const r = f.getBoundingClientRect(); const s = f.offsetWidth ? r.width / f.offsetWidth : 1; return [r.left + f.clientLeft * s, r.top + f.clientTop * s, s]; })()";
    match b.page().eval_page(BOX) {
        Ok(Value::Array(v)) if v.len() == 3 => (v[0].as_f64().unwrap_or(0.0), v[1].as_f64().unwrap_or(0.0), v[2].as_f64().filter(|s| *s > 0.0).unwrap_or(1.0)),
        _ => (0.0, 0.0, 1.0),
    }
}

fn respond_bytes(request: tiny_http::Request, status: u16, content_type: &str, body: Vec<u8>) {
    let header = tiny_http::Header::from_bytes("Content-Type", content_type).expect("a content type is a valid header");
    let _ = request.respond(tiny_http::Response::from_data(body).with_status_code(status).with_header(header));
}

fn respond(request: tiny_http::Request, status: u16, body: &Value) {
    respond_bytes(request, status, "application/json", body.to_string().into_bytes());
}

/// The built device-frame page (`tooling/browser-harness/dist`), found beside this binary or in the checkout.
pub(crate) fn harness_dist() -> PathBuf {
    let rel = "tooling/browser-harness/dist";
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.ancestors().map(|d| d.join(rel)).find(|p| p.is_dir()))
        .unwrap_or_else(|| wf_kit::root::repo_root().join(rel))
}

/// `/_mcp/device-frame[/path]`: static files of the harness, the page itself for the bare route and for any path that is
/// not a file (single-page routing), else the JSON 404.
fn serve_device_frame(request: tiny_http::Request, path: &str) {
    let dist = harness_dist();
    let rest = path.strip_prefix("/_mcp/device-frame").unwrap_or("");
    let mut segments: Vec<&str> = Vec::new();
    for seg in rest.split('/').filter(|s| !s.is_empty() && *s != ".") {
        if seg == ".." {
            segments.pop();
        } else {
            segments.push(seg);
        }
    }
    let wanted = if segments.is_empty() { dist.join("index.html") } else { segments.iter().fold(dist.clone(), |p, s| p.join(s)) };
    let file = if wanted.is_file() { wanted } else { dist.join("index.html") };
    match std::fs::read(&file) {
        Ok(bytes) if file.is_file() => respond_bytes(request, 200, &harness_core::staticd::content_type(&file), bytes),
        _ => respond(request, 404, &json!({ "ok": false, "error": "Not found" })),
    }
}

fn register(slot: &str, url: &str) {
    let record = json!({ "url": url, "pid": std::process::id(), "startedAt": rca_core::time::iso(wf_kit::time::now_ms()) });
    crate::slot::with_lock(|reg| {
        if reg["slots"][slot].is_null() {
            reg["slots"][slot] = json!({ "createdAt": utc_z(), "port_claims": {}, "device_claims": {} });
        }
        reg["slots"][slot]["browserMcp"] = record.clone();
    });
}

fn unregister(slot: &str, url: &str) {
    crate::slot::with_lock(|reg| {
        if reg["slots"][slot]["browserMcp"]["url"].as_str() == Some(url) {
            if let Some(s) = reg["slots"][slot].as_object_mut() {
                s.remove("browserMcp");
            }
        }
    });
}

pub fn run(args: &[String]) -> Option<i32> {
    let p = parse_spec(args, &Spec { values: &["port", "screenshot-dir"], flags: &["headless"], positional: Positional::None, ..Spec::NONE })?;
    let port: u16 = match p.val("port") {
        Some(v) => v.parse().ok()?,
        None => 0,
    };
    let cwd = std::env::current_dir().unwrap_or_default();
    let shots = cwd.join(p.val("screenshot-dir").unwrap_or(".screenshots"));
    let Ok(slot) = std::env::var("WF_BROWSER_SLOT") else {
        wf_kit::wf_err!("wf: browser-daemon needs WF_BROWSER_SLOT (the client sets it)");
        return Some(1);
    };
    let server = match tiny_http::Server::http(("127.0.0.1", port)) {
        Ok(s) => s,
        Err(e) => {
            wf_kit::wf_err!("wf: cannot bind 127.0.0.1:{port}: {e}");
            return Some(1);
        }
    };
    let url = format!("http://127.0.0.1:{}", server.server_addr().to_ip()?.port());
    let stop = Arc::new(AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        if signal_hook::flag::register(signal, Arc::clone(&stop)).is_err() {
            wf_kit::wf_err!("wf: cannot install the stop handler for signal {signal}");
            return Some(1);
        }
    }
    register(&slot, &url);
    println!("daemon ready {url}");
    let mut daemon = Daemon { url: url.clone(), screenshot_dir: shots, shots: 0, browsers: vec![], active: None };
    let (jobs, queue) = mpsc::channel::<Job>();
    // Connections are served on their own threads: the harness page fetches its assets from this server while a command
    // is waiting for it to load, so commands (run on this thread, in order) must not hold the listener.
    std::thread::spawn(move || {
        for request in server.incoming_requests() {
            let jobs = jobs.clone();
            std::thread::spawn(move || serve_request(request, &jobs));
        }
    });
    let mut stopped = None;
    let mut last_reap = std::time::Instant::now();
    // an idle daemon keeps its Chromium: one a test or a run forgot to stop held it for the 12 h guard
    let idle_after = Duration::from_secs(std::env::var("WF_BROWSER_DAEMON_IDLE_S").ok().and_then(|v| v.parse().ok()).unwrap_or(900));
    let mut last_job = std::time::Instant::now();
    'serve: while !stop.load(Ordering::SeqCst) {
        let job = queue.recv_timeout(Duration::from_millis(25));
        if job.is_ok() {
            last_job = std::time::Instant::now();
        } else if last_job.elapsed() > idle_after {
            wf_kit::wf_err!("wf: browser-daemon idle for {} s, stopping", idle_after.as_secs());
            break 'serve;
        }
        match job {
            Ok(Job::Status(reply)) => {
                let summaries = daemon.summaries();
                let _ = reply.send(json!({ "activeBrowserName": daemon.active, "browserCount": summaries.len(), "browsers": summaries }));
            }
            Ok(Job::Stop(reply)) => {
                stopped = Some(reply);
                break 'serve;
            }
            Ok(Job::Call(request, reply)) => {
                let method = request["method"].as_str().unwrap_or("");
                let call_args = if request["args"].is_object() { request["args"].clone() } else { json!({}) };
                let _ = reply.send(match daemon.dispatch(method, &call_args) {
                    Ok(result) => json!({ "ok": true, "result": result }),
                    Err(error) => json!({ "ok": false, "error": error }),
                });
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break 'serve,
        }
        for b in &mut daemon.browsers {
            if let Some(session) = b.session.as_mut() {
                session.tick();
            }
        }
        if last_reap.elapsed() >= std::time::Duration::from_secs(1) {
            last_reap = std::time::Instant::now();
            daemon.reap_dead_runs();
        }
    }
    for b in daemon.browsers.drain(..) {
        match (b.page, b.session) {
            (Some(p), _) => p.detach(),
            (None, Some(s)) => s.close(),
            (None, None) => {}
        }
    }
    unregister(&slot, &daemon.url);
    if let Some(reply) = stopped {
        let _ = reply.send(());
        // The connection thread writes the answer; the process must outlive it.
        std::thread::sleep(Duration::from_millis(200));
    }
    Some(0)
}

fn utc_z() -> String {
    let t = rca_core::time::iso(wf_kit::time::now_ms());
    format!("{}Z", &t[..t.len() - 5])
}
