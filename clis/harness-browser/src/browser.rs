//! `wf browser <command>`: the browser harness's CLI over the browser daemon found through the slot registry (a missing one
//! is started: `wf browser-daemon`). The device verbs are the command table's (`device::cli`); this file serves the daemon's
//! frozen lifecycle (`start|status|stop`), the session verbs (`start-device`, `stop-device`, `list-devices`, `hierarchy`),
//! the debugger, the tailnet verbs and `check-taps`; a dropped verb says why and what covers it, any other a usage error.
use rca_core::stats::js_json_pretty;
use serde_json::{json, Map, Value};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const DEVICES: &str = include_str!("browser_devices.json");
const DAEMON_MAX_LIFETIME_S: u64 = 12 * 60 * 60;

type Res<T> = Result<T, String>;

/* ---------- arguments (the shared clap wrapper) ---------- */

use wf_kit::args::{parse_spec, Parsed as Args, Positional, Spec};

/// `Number(text)` as JSON: NaN and infinities serialize as null.
fn num(text: &str) -> Value {
    let t = text.trim();
    let n = if t.is_empty() {
        0.0
    } else if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).map_or(f64::NAN, |v| v as f64)
    } else {
        match t {
            "Infinity" | "+Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            _ => t.parse::<f64>().ok().filter(|v| !t.to_lowercase().contains("inf") && !t.to_lowercase().contains("nan") && v.is_finite()).unwrap_or(f64::NAN),
        }
    };
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 9e15 { json!(n as i64) } else { serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number) }
}

/* ---------- registry and slot ---------- */

pub(crate) fn normalize_segment(seg: &str) -> String {
    let re = |s: &str, p: &str, to: &str| regex::Regex::new(p).expect("re").replace_all(s, to).into_owned();
    let s = re(seg, r"[^a-zA-Z0-9/]", "-");
    let s = re(&s, r"-+", "-");
    let s = re(&s, r"^-+|-+$", "");
    re(&s, r"^/+|/+$", "")
}

pub(crate) fn slot_id(app: &str) -> String {
    let branch = match normalize_segment(&wf_kit::proc::current_branch()) {
        b if b.is_empty() => "main".to_string(),
        b => b,
    };
    let app = normalize_segment(app);
    if app.is_empty() { format!("{branch}/default") } else { format!("{branch}/{app}/default") }
}

pub(crate) fn registry_dir() -> PathBuf {
    if let Ok(d) = std::env::var("WF_REGISTRY_DIR") {
        if !d.is_empty() {
            return PathBuf::from(d);
        }
    }
    let base = std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "~".into())).join(".local").join("state"));
    base.join("wf-registry")
}

pub(crate) fn registry_path() -> PathBuf {
    registry_dir().join("registry.json")
}

pub(crate) fn read_registry() -> Option<Value> {
    match std::fs::read_to_string(registry_path()) {
        Ok(t) => serde_json::from_str(&t).ok(),
        Err(_) => Some(json!({ "version": 3, "slots": {}, "devices": { "ios": {}, "android": {} } })),
    }
}

fn daemon_url(slot: &str) -> Option<String> {
    read_registry()?["slots"][slot]["browserMcp"]["url"].as_str().map(str::to_string)
}

fn clear_daemon(slot: &str) {
    let dir = registry_dir();
    let _ = std::fs::create_dir_all(&dir);
    let lock = dir.join("registry.lock");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if std::fs::OpenOptions::new().write(true).create_new(true).open(&lock).is_ok() {
            break;
        }
        if Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if let Some(mut reg) = read_registry() {
        if let Some(slot_obj) = reg["slots"][slot].as_object_mut() {
            slot_obj.remove("browserMcp");
            let tmp = dir.join(format!("registry.json.tmp.{}", std::process::id()));
            if std::fs::write(&tmp, format!("{}\n", serde_json::to_string_pretty(&reg).unwrap_or_default())).is_ok() {
                let _ = std::fs::rename(&tmp, registry_path());
            }
        }
    }
    let _ = std::fs::remove_file(&lock);
}

/* ---------- HTTP to the daemon ---------- */

pub fn http(method: &str, url: &str, body: Option<&str>, timeout: Option<Duration>) -> Result<(u16, String), String> {
    let mut request = cli_http::Request::new(method, url).direct();
    request = match timeout {
        Some(t) => request.timeout(t),
        None => request.without_timeout(),
    };
    if let Some(body) = body {
        request = request.header("Content-Type", "application/json").body(body);
    }
    let response = request.send()?;
    Ok((response.status, response.text()))
}

fn healthy(url: &str) -> bool {
    http("GET", &format!("{url}/health"), None, Some(Duration::from_secs(1))).is_ok_and(|(s, _)| (200..300).contains(&s))
}

/// The open sessions' summaries (`name`, `devicePreset`, `owner`, ...), for a device verb that names none.
pub(crate) fn sessions() -> Result<Vec<Value>, String> {
    let url = device_daemon(false)?;
    Ok(call_at(&url, "list", Map::new())?.as_array().cloned().unwrap_or_default())
}

/// The daemon a device driver posts to: the registered one, else (`start`, and unless `WF_BROWSER_NO_START`) a new one.
pub(crate) fn device_daemon(start: bool) -> Res<String> {
    let ctx = Ctx { slot: slot_id("mla-tamagui") };
    if let Some(url) = daemon_url(&ctx.slot).filter(|u| healthy(u)) {
        return Ok(url);
    }
    if !start {
        return Err("no browser daemon is running (wf browser start)".into());
    }
    if std::env::var_os("WF_BROWSER_NO_START").is_some() {
        return Err("no browser daemon is running, and WF_BROWSER_NO_START forbids starting one".into());
    }
    ensure_daemon(&ctx)
}

/// A device preset (`browser_devices.json`): `{id, viewport: {width, height}, dpr, supportsLandscape, ...}`.
pub(crate) fn preset(id: &str) -> Option<Value> {
    devices().into_iter().find(|d| d["id"].as_str() == Some(id))
}

/* ---------- daemon lifecycle ---------- */

struct Ctx {
    slot: String,
}

impl Ctx {
    /// `--json` reaches the command's own parse, which runs before anything is printed or a daemon is started; a daemon
    /// started with `MULTI_BROWSER_JSON=1` answers in JSON as well.
    fn json(&self) -> bool {
        wf_kit::args::json_seen() || std::env::var("MULTI_BROWSER_JSON").as_deref() == Ok("1")
    }
}

fn log_path() -> PathBuf {
    std::env::temp_dir().join("wf-browser-daemon.log")
}

fn spawn_guarded(ctx: &Ctx, extra: &[String]) -> Res<()> {
    let depth: u32 = std::env::var("WF_SPAWN_DEPTH").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    if depth >= 2 {
        return Err("browser daemon launch refused at spawn depth 2".into());
    }
    let log = std::fs::OpenOptions::new().create(true).append(true).open(log_path()).map_err(|e| e.to_string())?;
    let mut cmd = Command::new("timeout");
    cmd.arg(DAEMON_MAX_LIFETIME_S.to_string());
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    cmd.arg(exe).arg("browser-daemon").args(extra.iter().filter(|a| *a != "internal-daemon")).env("WF_BROWSER_SLOT", &ctx.slot);
    cmd
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log)
        .env("MULTI_BROWSER_JSON", if ctx.json() { "1" } else { "0" })
        .env("WF_SPAWN_DEPTH", (depth + 1).to_string());
    // SAFETY: setsid is async-signal-safe and runs in the forked child before exec, detaching it from this session.
    unsafe {
        cmd.pre_exec(|| nix::unistd::setsid().map(|_| ()).map_err(std::io::Error::from));
    }
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

fn wait_for_daemon(ctx: &Ctx) -> Res<String> {
    let timeout_ms = 15_000u64;
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    while Instant::now() < deadline {
        if let Some(url) = daemon_url(&ctx.slot) {
            if healthy(&url) {
                return Ok(url);
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let exists = read_registry().is_some_and(|r| r["slots"].get(&ctx.slot).is_some());
    Err(format!(
        "Timed out waiting for browser daemon after {timeout_ms}ms.\n  slot:     {}\n  in registry: {}\n  registry: {}\n  daemon log: {}",
        ctx.slot,
        if exists { "yes" } else { "no" },
        registry_path().display(),
        log_path().display()
    ))
}

fn ensure_daemon(ctx: &Ctx) -> Res<String> {
    if let Some(url) = daemon_url(&ctx.slot) {
        if healthy(&url) {
            return Ok(url);
        }
        clear_daemon(&ctx.slot);
    }
    spawn_guarded(ctx, &["internal-daemon".to_string()])?;
    wait_for_daemon(ctx)
}

/// One `POST /call` to the daemon at `url`: its result, or its error.
pub(crate) fn call_at(url: &str, method: &str, args: Map<String, Value>) -> Res<Value> {
    let body = json!({ "method": method, "args": Value::Object(args) }).to_string();
    let (_, text) = http("POST", &format!("{url}/call"), Some(&body), None)?;
    let reply: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if reply["ok"].as_bool() != Some(true) {
        return Err(reply["error"].as_str().map_or_else(|| "daemon command failed".to_string(), str::to_string));
    }
    Ok(reply.get("result").cloned().unwrap_or(Value::Null))
}

/* ---------- output ---------- */

fn success(ctx: &Ctx, result: Option<&Value>, human: Option<&dyn Fn(&Value) -> String>) {
    if ctx.json() {
        let mut line = Map::new();
        line.insert("level".into(), json!(30));
        line.insert("time".into(), json!(wf_kit::time::now_ms()));
        line.insert("pid".into(), json!(std::process::id()));
        line.insert("hostname".into(), json!(wf_kit::proc::hostname()));
        if let Some(r) = result.filter(|r| !r.is_null()) {
            line.insert("result".into(), r.clone());
        }
        line.insert("msg".into(), json!("ok"));
        println!("{}", Value::Object(line));
    } else if let (Some(h), Some(r)) = (human, result) {
        println!("{}", h(r));
    } else if let Some(r) = result.filter(|r| !r.is_null()) {
        match r {
            Value::String(s) => println!("{s}"),
            other => println!("{}", js_json_pretty(other)),
        }
    }
}

/// `${value}` for a JSON value, as a template literal prints it.
fn show(v: &Value) -> String {
    match v {
        Value::Null => "undefined".into(),
        Value::String(s) => s.clone(),
        Value::Number(n) => n.as_f64().map_or_else(|| n.to_string(), wf_kit::js::js_num),
        other => other.to_string(),
    }
}

fn f(r: &Value, key: &str) -> String {
    show(&r[key])
}

fn devices() -> Vec<Value> {
    serde_json::from_str(DEVICES).unwrap_or_default()
}

/* ---------- the daemon's lifecycle: frozen for @lib/browser-client and the cookie-capture service ---------- */

fn lifecycle(ctx: &Ctx, cmd: &str, rest: &[String]) -> Option<Res<i32>> {
    match cmd {
        "stop" => {
            parse_spec(rest, &Spec { positional: Positional::None, ..Spec::NONE })?;
            let Some(url) = daemon_url(&ctx.slot) else { return Some(Err("No browser daemon is running".into())) };
            match http("POST", &format!("{url}/stop"), None, None) {
                Ok((s, _)) if (200..300).contains(&s) => {}
                Ok((s, _)) => return Some(Err(format!("Daemon rejected stop: {s}"))),
                Err(e) => return Some(Err(e)),
            }
            clear_daemon(&ctx.slot);
            let text = json!("Browser daemon stopped");
            success(ctx, Some(&text), Some(&|_: &Value| "Browser daemon stopped".to_string()));
            Some(Ok(0))
        }
        "status" => {
            parse_spec(rest, &Spec { positional: Positional::None, ..Spec::NONE })?;
            let Some(url) = daemon_url(&ctx.slot) else { return Some(Err("No browser daemon is running".into())) };
            let (s, text) = match http("GET", &format!("{url}/status"), None, None) {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            };
            if !(200..300).contains(&s) {
                return Some(Err(format!("Daemon status failed: {s}")));
            }
            let session: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let mut out = Map::new();
            out.insert("url".into(), json!(url));
            if let Some(o) = session.as_object() {
                out.extend(o.clone());
            }
            success(ctx, Some(&Value::Object(out)), None);
            Some(Ok(0))
        }
        _ => {
            let a = parse_spec(rest, &Spec { values: &["port", "screenshot-dir"], flags: &["headless", "foreground"], positional: Positional::None, ..Spec::NONE })?;
            if a.flag("foreground") {
                return None;
            }
            let port = match a.val("port") {
                Some(p) => {
                    let n = num(p).as_i64()?;
                    if n != 0 && !(1024..=65535).contains(&n) {
                        return None;
                    }
                    n
                }
                None => 0,
            };
            let mut extra = vec!["internal-daemon".to_string(), "--port".into(), port.to_string()];
            if a.flag("headless") {
                extra.push("--headless".into());
            }
            if let Some(d) = a.val("screenshot-dir").filter(|d| !d.is_empty()) {
                extra.extend(["--screenshot-dir".into(), d.to_string()]);
            }
            let mode = if a.flag("headless") { "headless" } else { "headed" };
            // One daemon per slot: a second `start` reuses the live one instead of leaving another browser host behind.
            if let Some(url) = daemon_url(&ctx.slot).filter(|u| healthy(u)) {
                success(ctx, Some(&json!({ "url": url, "mode": mode })), None);
                return Some(Ok(0));
            }
            if let Err(e) = spawn_guarded(ctx, &extra) {
                return Some(Err(e));
            }
            let url = match wait_for_daemon(ctx) {
                Ok(u) => u,
                Err(e) => return Some(Err(e)),
            };
            success(ctx, Some(&json!({ "url": url, "mode": mode })), None);
            Some(Ok(0))
        }
    }
}

/* ---------- the verbs ---------- */

/// `wf browser`'s own verbs beside the command table's (`harness_core::device::api`): `wf check --dead-refs` checks each
/// `wf browser <verb>` it reads against both.
pub const VERBS: &[&str] = &[
    "check-taps", "debug-eval", "debug-pause", "debug-resume", "debug-step", "hierarchy", "list-devices", "set-breakpoint",
    "start", "start-device", "status", "stop", "stop-device", "tailnet-forward", "test", "tunnel",
];

const BY_RUN: &str = "state that lives for one run is Lua's: add_network_stub, get_network_stub_hits, get_debug_hits and test --logpoint";
const OWN_TEST_ONLY: &str = "only its own test called the verb; the daemon's RPC method stays for the clients that call it";

/// Verbs that are gone, each with why and what covers it (docs/plans/harness-api/spec.md, Dropped); their daemon RPC methods stay.
pub(crate) const DROPPED: &[(&str, &str, &str)] = &[
    ("camera-inject", "no screen uses a camera", ""),
    ("check-clipping", OWN_TEST_ONLY, ""),
    ("clipboard", "nothing pastes", "wf browser get-element; wf browser press-key Backspace"),
    ("cookies", OWN_TEST_ONLY, ""),
    ("coverage", OWN_TEST_ONLY, "wf bench"),
    ("debug-clear", OWN_TEST_ONLY, ""),
    ("debug-hits", BY_RUN, "device:get_debug_hits() in a test"),
    ("drag", "no caller", "wf browser swipe; wf browser scroll-until-visible"),
    ("eval", "it was the Lua driver's transport, which now calls the daemon in process; reads are typed verbs", "wf browser get-element; wf browser hierarchy; wf browser get-logs"),
    ("memory", OWN_TEST_ONLY, "wf bench"),
    ("network-proxy", "its one runbook mention said not to use it", ""),
    ("network-stub", BY_RUN, "device:add_network_stub(method, url_pattern, opts) in a test"),
    ("network-stub-hits", BY_RUN, "device:get_network_stub_hits() in a test"),
    ("opened-urls", OWN_TEST_ONLY, ""),
    ("outer-eval", "it was the Lua driver's transport, which now calls the daemon in process; reads are typed verbs", "wf browser get-element; wf browser get-logs --level info"),
    ("permission", "the voice gate taps the permission dialog as a user does", "wf browser tap-on"),
    ("preflight", OWN_TEST_ONLY, "wf browser start-device"),
    ("scroll", "no caller", "wf browser swipe; wf browser scroll-until-visible"),
    ("set-device", "no caller", "wf browser stop-device; wf browser start-device --screen-size P"),
    ("shell-eval", "it was the Lua driver's transport, which now calls the daemon in process; reads are typed verbs", "wf browser get-device-state"),
    ("stroke", "no script drew raw strokes", "wf browser draw-strokes-on"),
    ("upload", OWN_TEST_ONLY, ""),
    ("wait-observe", "it was the Lua driver's transport, which now calls the daemon in process", "wf browser extended-wait-until"),
];

fn emit(v: Value) {
    println!("{v}");
}

/// A verb's usage line: the first line of its committed help.
fn usage_of(verb: &str) -> String {
    wf_kit::help::text(&format!("browser {verb}")).and_then(|t| t.lines().next()).and_then(|l| l.strip_prefix("Usage: ")).map_or_else(|| format!("wf browser {verb}"), str::to_string)
}

fn usage_error(verb: &str, error: &str) -> i32 {
    emit(json!({ "ok": false, "command": verb, "platform": "browser", "error": error, "usage": usage_of(verb) }));
    2
}

fn frame(p: &Args) -> Result<String, String> {
    let f = p.val("frame").map(str::to_string).or_else(|| std::env::var("WF_BROWSER_FRAME").ok()).unwrap_or_else(|| "screen".into());
    if ["screen", "bezel", "document"].contains(&f.as_str()) {
        Ok(f)
    } else {
        Err(format!("--frame is screen, bezel or document, got '{f}'"))
    }
}

/// The screen preset a new session or test run gets: the one given, else WF_BROWSER_SCREEN_SIZE, else iphone-15-pro.
pub(crate) fn screen_size(given: Option<&str>) -> Result<String, String> {
    let id = given.map(str::to_string).or_else(|| std::env::var("WF_BROWSER_SCREEN_SIZE").ok().filter(|s| !s.is_empty())).unwrap_or_else(|| "iphone-15-pro".into());
    if preset(&id).is_none() {
        let known: Vec<String> = devices().iter().filter_map(|d| d["id"].as_str().map(str::to_string)).collect();
        return Err(format!("unknown screen size '{id}': one of {} (wf browser list-devices)", known.join(", ")));
    }
    Ok(id)
}

/// Why a verb did not answer: a usage error (exit 2) or a failure with the next step (exit 1).
enum Out {
    Usage(String),
    Failed { error: String, evidence: Value, next: String },
}

fn failed(error: impl Into<String>, next: &str) -> Out {
    Out::Failed { error: error.into(), evidence: Value::Null, next: next.into() }
}

/// One RPC to the running daemon (never started here: a session verb needs an open session).
fn rpc(method: &str, args: Value) -> Result<Value, Out> {
    let url = device_daemon(false).map_err(|e| failed(e, "wf browser start-device --device N"))?;
    let Value::Object(args) = args else { return Err(failed("bad arguments", "")) };
    call_at(&url, method, args).map_err(|e| failed(e, "wf browser list-devices"))
}

fn start_device(p: &Args) -> Result<Value, Out> {
    let name = p.val("device").ok_or_else(|| Out::Usage("--device N names the new session".into()))?;
    if preset(name).is_some() {
        return Err(Out::Usage(format!("--device names the session; the screen preset '{name}' goes in --screen-size")));
    }
    let size = screen_size(p.val("screen-size")).map_err(Out::Usage)?;
    let frame = frame(p).map_err(Out::Usage)?;
    let scheme = p.val("color-scheme").unwrap_or("system");
    if !["light", "dark", "system"].contains(&scheme) {
        return Err(Out::Usage(format!("--color-scheme is light, dark or system, got '{scheme}'")));
    }
    let url = device_daemon(true).map_err(|e| failed(e, "wf browser start, then start-device"))?;
    let mut m = json!({ "name": name, "devicePreset": size, "orientation": "portrait", "colorScheme": scheme, "frame": frame });
    if p.flag("tailnet") || std::env::var_os("WF_BROWSER_TAILNET").is_some() {
        m["tailnetProxy"] = json!(true);
    }
    if p.flag("plain") {
        m["renderMode"] = json!("plain");
    }
    if p.flag("headless") {
        m["headless"] = json!(true);
    }
    let Value::Object(m) = m else { return Err(failed("bad arguments", "")) };
    call_at(&url, "open", m).map_err(|e| failed(e, "wf browser list-devices"))
}

fn stop_device(p: &Args) -> Result<Value, Out> {
    let names: Vec<String> = if p.flag("all") {
        if p.val("device").is_some() {
            return Err(Out::Usage("give --device or --all, not both".into()));
        }
        sessions().map_err(|e| failed(e, "wf browser list-devices"))?.iter().filter_map(|s| s["name"].as_str().map(str::to_string)).collect()
    } else {
        vec![crate::driver::browser_session(p.val("device")).map_err(|(code, e)| if code == 2 { Out::Usage(e) } else { failed(e, "wf browser list-devices") })?]
    };
    for name in &names {
        rpc("close", json!({ "name": name }))?;
    }
    Ok(json!({ "closed": names }))
}

fn list_devices() -> Value {
    let url = daemon_url(&slot_id("mla-tamagui")).filter(|u| healthy(u));
    let devices = url.as_deref().and_then(|u| call_at(u, "list", Map::new()).ok()).unwrap_or_else(|| json!([]));
    json!({ "daemon": { "running": url.is_some(), "url": url }, "devices": devices, "screens": self::devices() })
}

fn check_taps(p: &Args) -> Result<Value, Out> {
    let video = p.positional.first().map(String::as_str).unwrap_or_default();
    let inputs = p.val("inputs").map_or_else(|| harness_core::video_check::input_log_path_for(video), str::to_string);
    let r = harness_core::video_check::check_video_taps(video, &inputs).map_err(|e| failed(e, "record with wf browser start-recording, then stop-recording"))?;
    if r["pass"].as_bool() == Some(true) {
        return Ok(r);
    }
    let missing: Vec<String> = r["missing"].as_array().map(|a| a.iter().map(show).collect()).unwrap_or_default();
    Err(Out::Failed {
        error: format!("{} of {} taps show a dot; no dot for the taps at {} ms", f(&r, "dotsFound"), f(&r, "taps"), missing.join(", ")),
        evidence: r,
        next: "look at the recording at each missing tap's time: a dot drawn too late, or a tap that never landed".into(),
    })
}

fn set_breakpoint(name: &str, p: &Args) -> Result<Value, Out> {
    let spec = p.positional.first().map(String::as_str).unwrap_or_default();
    let mut parts = spec.splitn(3, ':');
    let (file, line, condition) = (parts.next().unwrap_or(""), parts.next().and_then(|l| l.parse::<u32>().ok()), parts.next());
    let Some(line) = line.filter(|_| !file.is_empty()) else { return Err(Out::Usage(format!("want file:line[:condition], got '{spec}'"))) };
    let mut args = json!({ "name": name, "file": file, "line": line, "pause": true });
    if let Some(c) = condition {
        args["condition"] = json!(c);
    }
    rpc("debugSet", args)
}

fn debugger(verb: &str, name: &str, p: &Args) -> Result<Value, Out> {
    let mut args = json!({ "name": name });
    if let Some(t) = p.val("timeout") {
        args["timeout"] = num(t);
    }
    let method = match verb {
        "debug-pause" => "debugPause",
        "debug-resume" => "debugResume",
        "debug-step" => {
            let kind = p.positional.first().map_or("over", String::as_str);
            if !["over", "into", "out"].contains(&kind) {
                return Err(Out::Usage(format!("debug-step takes over, into or out, got '{kind}'")));
            }
            args["kind"] = json!(kind);
            "debugStep"
        }
        _ => {
            args["expression"] = json!(p.positional.first());
            "debugEval"
        }
    };
    rpc(method, args)
}

fn tailnet_forward(p: &Args) -> Result<Value, Out> {
    let (Some(host), Some(port)) = (p.val("host"), p.val("port")) else { return Err(Out::Usage("--host and --port are required".into())) };
    let url = device_daemon(true).map_err(|e| failed(e, "wf browser start"))?;
    let mut m = Map::new();
    m.insert("host".into(), json!(host));
    m.insert("port".into(), num(port));
    if let Some(l) = p.val("local-port") {
        m.insert("localPort".into(), num(l));
    }
    call_at(&url, "tailnetForward", m).map_err(|e| failed(e, "wf vm net up, then try again"))
}

fn spec(verb: &str) -> Spec {
    let (values, flags, positional): (&'static [&'static str], &'static [&'static str], Positional) = match verb {
        "start-device" => (&["device", "screen-size", "frame", "color-scheme"], &["tailnet", "plain", "headless"], Positional::None),
        "stop-device" => (&["device"], &["all"], Positional::None),
        "list-devices" => (&[], &[], Positional::None),
        "check-taps" => (&["inputs"], &[], Positional::Exactly(1)),
        "tailnet-forward" => (&["host", "port", "local-port"], &[], Positional::None),
        "set-breakpoint" | "debug-eval" => (&["device"], &[], Positional::Exactly(1)),
        "debug-step" => (&["device", "timeout"], &[], Positional::AtMost(1)),
        "debug-pause" | "debug-resume" => (&["device", "timeout"], &[], Positional::None),
        _ => (&["device"], &[], Positional::None),
    };
    Spec { values, flags, positional, ..Spec::NONE }
}

/// `wf browser <own verb>`: one JSON line, `{ok, command, platform, device, ms, result}` (exit 0), a failure with the next
/// step (exit 1) or a usage error (exit 2).
fn verb(verb: &str, rest: &[String]) -> i32 {
    let started = Instant::now();
    if rest.iter().any(|a| a == "--help" || a == "-h") {
        return wf_kit::help::text(&format!("browser {verb}")).map_or(2, |t| {
            print!("{t}");
            0
        });
    }
    let Some(p) = parse_spec(rest, &spec(verb)) else {
        let e = wf_kit::args::take_error().unwrap_or_default();
        let said: Vec<&str> = e.lines().map(str::trim).take_while(|l| !l.is_empty() && !l.starts_with("Usage:")).collect();
        return usage_error(verb, said.join(" ").trim_start_matches("error: "));
    };
    // the verbs on one session resolve it first: --device, else the only open one that no running test owns
    let on_session = matches!(verb, "hierarchy" | "set-breakpoint" | "debug-pause" | "debug-step" | "debug-eval" | "debug-resume");
    let device = match on_session.then(|| crate::driver::browser_session(p.val("device"))) {
        Some(Ok(name)) => Some(name),
        Some(Err((2, e))) => return usage_error(verb, &e),
        Some(Err((_, e))) => {
            emit(json!({ "ok": false, "command": verb, "platform": "browser", "device": null, "ms": started.elapsed().as_millis() as u64, "error": e, "next": "wf browser list-devices" }));
            return 1;
        }
        None => p.val("device").map(str::to_string),
    };
    let name = device.clone().unwrap_or_default();
    let out = match verb {
        "start-device" => start_device(&p),
        "stop-device" => stop_device(&p),
        "list-devices" => Ok(list_devices()),
        "hierarchy" => {
            use harness_core::device::Device;
            crate::driver::BrowserDevice::attach(name).hierarchy().map_err(|e| failed(e, "wf browser list-devices"))
        }
        "check-taps" => check_taps(&p),
        "set-breakpoint" => set_breakpoint(&name, &p),
        "tailnet-forward" => tailnet_forward(&p),
        _ => debugger(verb, &name, &p),
    };
    let ms = started.elapsed().as_millis() as u64;
    match out {
        Ok(result) => {
            emit(json!({ "ok": true, "command": verb, "platform": "browser", "device": device, "ms": ms, "result": result }));
            0
        }
        Err(Out::Usage(e)) => usage_error(verb, &e),
        Err(Out::Failed { error, evidence, next }) => {
            emit(json!({ "ok": false, "command": verb, "platform": "browser", "device": device, "ms": ms, "error": error, "evidence": evidence, "next": next }));
            1
        }
    }
}

pub fn run(args: &[String]) -> Option<i32> {
    let (cmd, rest) = args.split_first()?;
    match cmd.as_str() {
        "tunnel" => return crate::browser_tunnel::run(rest),
        "test" => return harness_core::run::test_for(&crate::driver::PLATFORM, rest),
        "start" | "status" | "stop" => {
            read_registry()?;
            let ctx = Ctx { slot: slot_id("mla-tamagui") };
            return match lifecycle(&ctx, cmd, rest)? {
                Ok(code) => Some(code),
                Err(message) => {
                    eprintln!("[wf browser] Error: {message}");
                    Some(1)
                }
            };
        }
        v if harness_core::device::cli::is_verb(v) => return Some(harness_core::device::cli::run(&crate::driver::PLATFORM, v, rest)),
        v if VERBS.contains(&v) => return Some(verb(v, rest)),
        v if v.starts_with('-') => return None,
        _ => {}
    }
    let Some((old, why, instead)) = DROPPED.iter().find(|(old, _, _)| old == cmd) else { return Some(harness_core::device::cli::unknown_verb("browser", cmd, VERBS)) };
    emit(json!({ "ok": false, "command": old, "platform": "browser", "error": "dropped", "why": why, "use": (!instead.is_empty()).then_some(instead) }));
    Some(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_the_verbs_when_read_then_each_name_is_served_once_and_every_dropped_one_points_at_a_verb_that_exists() {
        let mut all: Vec<&str> = VERBS.iter().copied().chain(DROPPED.iter().map(|d| d.0)).collect();
        let n = all.len();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), n, "a verb is both kept and dropped");
        assert!(all.iter().all(|v| harness_core::device::api::by_verb(v).is_none()), "a dropped verb shadows a table verb");
        for (old, _, instead) in DROPPED {
            for v in instead.split("; ").filter_map(|u| u.strip_prefix("wf browser ")).map(|u| u.split(' ').next().unwrap_or(u)) {
                assert!(VERBS.contains(&v) || harness_core::device::api::by_verb(v).is_some(), "{old}: {v}");
            }
        }
        crate::register_help();
        for v in VERBS.iter().filter(|v| **v != "test") {
            assert!(wf_kit::help::text(&format!("browser {v}")).is_some_and(|t| t.starts_with(&format!("Usage: wf browser {v}"))), "wf browser {v} has no help");
        }
    }

    #[test]
    fn given_a_screen_size_when_checked_then_only_a_known_preset_passes() {
        assert_eq!(screen_size(Some("pixel-8")), Ok("pixel-8".into()));
        assert!(screen_size(Some("nokia")).unwrap_err().contains("unknown screen size 'nokia': one of iphone-se"));
    }
}
