//! The agent-facing debugger of a browser session: logpoints and breakpoints on original source files, armed without editing the
//! code. `debug-set` resolves `file:line` through the page's source maps, a hit is recorded as one JSON record (stack, locals,
//! captured expressions) and, unless the point asks to pause, the page is resumed at once. Everything an agent needs comes back
//! in that record; no console output has to be added to the code under test. `pause`/`step`/`eval`/`resume` hold the page at a
//! line and read it there; each hit carries the id of the last input action before it.
use base64::Engine as _;
use cdp::Cdp;
use serde_json::{json, Map, Value};
use sourcemap::SourceMap;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const MAX_FRAMES: usize = 8;
const MAX_LOCALS: usize = 40;
const MAX_TEXT: usize = 300;

struct Script {
    url: String,
    map: Option<SourceMap>,
}

#[derive(Clone)]
struct Spec {
    id: usize,
    file: String,
    line: u32,
    condition: Option<String>,
    capture: Vec<String>,
    pause: bool,
}

#[derive(Default)]
struct State {
    scripts: HashMap<String, Script>,
    specs: Vec<Spec>,
    /// Debugger breakpoint id -> spec id.
    armed: HashMap<String, usize>,
    hits: Vec<Value>,
    /// Source-map fetches still running: `set` waits for them so a point is never lost to a race.
    pending_maps: usize,
    /// Set by `pause`/`step` and by a `--pause` point: pauses are kept until `resume`.
    hold: bool,
    /// The `Debugger.paused` params of the pause the page is held in.
    paused: Option<Value>,
}

pub struct Debugger {
    cdp: Cdp,
    session: String,
    started: Instant,
    state: Mutex<State>,
    changed: Condvar,
    /// The id of the last input action the session ran (0: none yet), shared with the daemon.
    last_action: Arc<AtomicU64>,
}

/// The CDP method for `debug-step <kind>`.
pub fn step_method(kind: &str) -> Result<&'static str, String> {
    match kind {
        "over" | "" => Ok("Debugger.stepOver"),
        "into" => Ok("Debugger.stepInto"),
        "out" => Ok("Debugger.stepOut"),
        other => Err(format!("debug-step: unknown kind '{other}' (over, into, out)")),
    }
}

fn same_source(source: &str, wanted: &str) -> bool {
    let (s, w) = (source.trim_start_matches("./"), wanted.trim_start_matches("./"));
    s == w || s.ends_with(&format!("/{w}")) || w.ends_with(&format!("/{s}"))
}

fn origin_and_dir(url: &str) -> (String, String) {
    let after = url.find("://").map_or(0, |i| i + 3);
    let origin_end = url[after..].find('/').map_or(url.len(), |i| after + i);
    let dir_end = url.rfind('/').filter(|i| *i >= origin_end).unwrap_or(origin_end);
    (url[..origin_end].to_string(), url[..dir_end].to_string())
}

fn load_map(script_url: &str, map_url: &str) -> Option<SourceMap> {
    if let Some(data) = map_url.strip_prefix("data:") {
        let (_, payload) = data.split_once("base64,")?;
        let bytes = cdp::BASE64.decode(payload.trim()).ok()?;
        return parse_map(&bytes);
    }
    let (origin, dir) = origin_and_dir(script_url);
    let full = if map_url.contains("://") {
        map_url.to_string()
    } else if map_url.starts_with('/') {
        format!("{origin}{map_url}")
    } else {
        format!("{dir}/{map_url}")
    };
    let response = cli_http::Request::get(&full).direct().timeout(Duration::from_secs(10)).send().ok()?;
    if !response.ok() {
        return None;
    }
    parse_map(response.text().as_bytes())
}

/// Metro's maps carry `x_facebook_sources`, which `SourceMap::from_slice` refuses as another format; their mappings are standard.
pub fn parse_map(bytes: &[u8]) -> Option<SourceMap> {
    match sourcemap::decode_slice(bytes).ok()? {
        sourcemap::DecodedMap::Regular(m) => Some(m),
        sourcemap::DecodedMap::Hermes(h) => Some((*h).clone()),
        sourcemap::DecodedMap::Index(i) => i.flatten().ok(),
    }
}

fn short(v: &Value) -> Value {
    let text = |s: &str| if s.len() > MAX_TEXT { format!("{}...", &s[..MAX_TEXT]) } else { s.to_string() };
    match v {
        Value::String(s) => json!(text(s)),
        other => other.clone(),
    }
}

/// A remote object as one JSON value: the primitive itself, or its description.
fn plain(remote: &Value) -> Value {
    if let Some(v) = remote.get("value") {
        return short(v);
    }
    if remote["type"] == "undefined" {
        return json!("undefined");
    }
    if let Some(d) = remote["description"].as_str() {
        return json!(if d.len() > MAX_TEXT { format!("{}...", &d[..MAX_TEXT]) } else { d.to_string() });
    }
    Value::Null
}

impl Debugger {
    /// Starts debugging the page behind `cdp`/`session`: events from now on feed the debugger.
    pub fn attach(cdp: Cdp, session: String, last_action: Arc<AtomicU64>) -> Result<Arc<Debugger>, String> {
        let dbg = Arc::new(Debugger { cdp: cdp.clone(), session, started: Instant::now(), state: Mutex::new(State::default()), changed: Condvar::new(), last_action });
        let handler_dbg = Arc::clone(&dbg);
        cdp.on_debugger(Arc::new(move |event| handler_dbg.on_event(&event)));
        dbg.call("Debugger.enable", json!({}))?;
        // The device frame's 300 ms measure timer runs beside the page; on a loaded machine a pause landed in it.
        let _ = dbg.call("Debugger.setBlackboxPatterns", json!({ "patterns": ["/_mcp/"] }));
        Ok(dbg)
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        // a socket that is one page takes no session id
        self.cdp.call_on(method, params, Some(self.session.as_str()).filter(|s| !s.is_empty()))
    }

    fn on_event(self: &Arc<Self>, event: &Value) {
        match event["method"].as_str() {
            Some("Debugger.scriptParsed") => {
                let p = &event["params"];
                let (Some(id), Some(url)) = (p["scriptId"].as_str(), p["url"].as_str()) else { return };
                let map_url = p["sourceMapURL"].as_str().filter(|u| !u.is_empty());
                if url.is_empty() {
                    return;
                }
                let Ok(mut st) = self.state.lock() else { return };
                st.scripts.insert(id.to_string(), Script { url: url.to_string(), map: None });
                let Some(map_url) = map_url else { return };
                st.pending_maps += 1;
                drop(st);
                let me = Arc::clone(self);
                let (id, url, map_url) = (id.to_string(), url.to_string(), map_url.to_string());
                std::thread::spawn(move || {
                    let map = load_map(&url, &map_url);
                    if let Ok(mut st) = me.state.lock() {
                        if let Some(s) = st.scripts.get_mut(&id) {
                            s.map = map;
                        }
                        st.pending_maps = st.pending_maps.saturating_sub(1);
                    }
                    me.arm_for_script(&id);
                });
            }
            Some("Debugger.paused") => self.on_paused(&event["params"]),
            _ => {}
        }
    }

    /// Arms every spec whose file the script's map knows (also after a reload, when the script is parsed again).
    fn arm_for_script(&self, script_id: &str) {
        let (specs, locations): (Vec<Spec>, Vec<(usize, u32, u32)>) = {
            let Ok(st) = self.state.lock() else { return };
            let Some(map) = st.scripts.get(script_id).and_then(|s| s.map.as_ref()) else { return };
            let mut found = Vec::new();
            for spec in &st.specs {
                if let Some((l, c)) = generated_position(map, &spec.file, spec.line) {
                    found.push((spec.id, l, c));
                }
            }
            (st.specs.clone(), found)
        };
        for (spec_id, line, column) in locations {
            let Some(spec) = specs.iter().find(|s| s.id == spec_id) else { continue };
            let mut params = json!({ "location": { "scriptId": script_id, "lineNumber": line, "columnNumber": column } });
            if let Some(c) = &spec.condition {
                params["condition"] = json!(c);
            }
            if let Ok(reply) = self.call("Debugger.setBreakpoint", params) {
                if let (Some(bp), Ok(mut st)) = (reply["breakpointId"].as_str(), self.state.lock()) {
                    st.armed.insert(bp.to_string(), spec_id);
                }
            }
        }
    }

    /// Source maps load asynchronously after a script is parsed: lets them settle (bounded) before a file is judged unknown
    /// or a location reported unmapped.
    fn settle_maps(&self) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline && self.state.lock().map(|s| s.pending_maps > 0).unwrap_or(false) {
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Arms `file:line`; fails with the files the page has maps for when none knows it.
    pub fn set(&self, file: &str, line: u32, condition: Option<String>, capture: Vec<String>, defer: bool, pause: bool) -> Result<Value, String> {
        self.settle_maps();
        let (id, script_ids, available) = {
            let mut st = self.state.lock().map_err(|e| e.to_string())?;
            let id = st.specs.len() + 1;
            st.specs.push(Spec { id, file: file.to_string(), line, condition, capture, pause });
            let mut ids = Vec::new();
            let mut available: Vec<String> = Vec::new();
            for (sid, script) in &st.scripts {
                if let Some(map) = &script.map {
                    available.extend(map.sources().map(str::to_string));
                    if generated_position(map, file, line).is_some() {
                        ids.push(sid.clone());
                    }
                }
            }
            available.sort();
            available.dedup();
            (id, ids, available)
        };
        if script_ids.is_empty() && defer {
            // Kept: it is armed when a script whose map knows the file is parsed (lazy bundles, later navigations).
            return Ok(json!({ "id": id, "file": file, "line": line, "resolved": false, "scripts": 0 }));
        }
        if script_ids.is_empty() {
            if let Ok(mut st) = self.state.lock() {
                st.specs.retain(|s| s.id != id);
            }
            let shown: Vec<&String> = available.iter().filter(|s| !s.contains("node_modules")).take(40).collect();
            return Err(format!(
                "debug-set: no source map of this page contains {file} line {line}. Files with maps ({} total, node_modules hidden): {shown:?}. Use a path ending like one of these, or a line that has code; web exports need --source-maps.",
                available.len()
            ));
        }
        for sid in &script_ids {
            self.arm_for_script(sid);
        }
        Ok(json!({ "id": id, "file": file, "line": line, "resolved": true, "scripts": script_ids.len() }))
    }

    fn on_paused(&self, params: &Value) {
        let frames = params["callFrames"].as_array().cloned().unwrap_or_default();
        let hit_ids: Vec<String> = params["hitBreakpoints"].as_array().map(|a| a.iter().filter_map(|b| b.as_str().map(str::to_string)).collect()).unwrap_or_default();
        let spec = {
            let Ok(st) = self.state.lock() else { return };
            hit_ids.iter().find_map(|b| st.armed.get(b)).and_then(|id| st.specs.iter().find(|s| s.id == *id).cloned())
        };
        if let Some(spec) = &spec {
            let record = self.capture(spec, &frames);
            if let Ok(mut st) = self.state.lock() {
                st.hits.push(record);
            }
        }
        if let Ok(mut st) = self.state.lock() {
            if st.hold || spec.as_ref().is_some_and(|s| s.pause) {
                st.hold = true;
                st.paused = Some(params.clone());
                self.changed.notify_all();
                return;
            }
        }
        // A pause that is not one of ours (a `debugger;` statement, an exception) must never leave the page frozen.
        let _ = self.call("Debugger.resume", json!({}));
    }

    /// Waits up to `ms` for the page to be held in a pause.
    fn wait_paused(&self, ms: u64) -> Result<Option<Value>, String> {
        let deadline = Instant::now() + Duration::from_millis(ms);
        let mut st = self.state.lock().map_err(|e| e.to_string())?;
        while st.paused.is_none() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            st = self.changed.wait_timeout(st, left).map_err(|e| e.to_string())?.0;
        }
        Ok(st.paused.clone())
    }

    /// Where the held page is: reason, top-frame location, the mapped stack and the top frame's locals.
    fn describe(&self, params: &Value) -> Value {
        let frames = params["callFrames"].as_array().cloned().unwrap_or_default();
        let stack = self.frames(&frames);
        let top = stack.first().cloned().unwrap_or(Value::Null);
        json!({
            "paused": true,
            "reason": params["reason"],
            "function": top["function"],
            "file": top["file"],
            "line": top["line"],
            "column": top["column"],
            "frames": stack,
            "locals": self.locals(frames.first()),
            "action": self.action(),
        })
    }

    fn held_or_waiting(&self, verb: &str, ms: u64, started: Instant) -> Result<Value, String> {
        Ok(match self.wait_paused(ms)? {
            Some(params) => self.describe(&params),
            None => json!({
                "paused": false,
                "pending": true,
                "waitedMs": started.elapsed().as_millis() as u64,
                "hint": format!("{verb}: no JavaScript ran within {ms}ms; the page pauses at the next statement it runs (debug-resume cancels)"),
            }),
        })
    }

    /// Holds the page at the next statement it runs; answers where it stopped, or that it is still waiting after `ms`.
    pub fn pause(&self, ms: u64) -> Result<Value, String> {
        let started = Instant::now();
        let already = {
            let mut st = self.state.lock().map_err(|e| e.to_string())?;
            st.hold = true;
            st.paused.is_some()
        };
        if !already {
            self.call("Debugger.pause", json!({}))?;
        }
        self.settle_maps();
        self.held_or_waiting("debug-pause", ms, started)
    }

    /// One step (`over`, `into`, `out`) from the held pause; answers the new location.
    pub fn step(&self, kind: &str, ms: u64) -> Result<Value, String> {
        let method = step_method(kind)?;
        let started = Instant::now();
        if self.state.lock().map_err(|e| e.to_string())?.paused.take().is_none() {
            return Err("debug-step: the page is not paused (debug-pause, or set-breakpoint, first)".into());
        }
        self.call(method, json!({}))?;
        self.held_or_waiting("debug-step", ms, started)
    }

    /// Releases the page; a pending pause request is cancelled too.
    pub fn resume(&self) -> Result<Value, String> {
        let was_paused = {
            let mut st = self.state.lock().map_err(|e| e.to_string())?;
            st.hold = false;
            st.paused.take().is_some()
        };
        if was_paused {
            self.call("Debugger.resume", json!({}))?;
        }
        Ok(json!({ "resumed": was_paused, "paused": false }))
    }

    /// Evaluates `expr` in the held top frame's scope.
    pub fn eval(&self, expr: &str) -> Result<Value, String> {
        let paused = self.state.lock().map_err(|e| e.to_string())?.paused.clone();
        let Some(params) = paused else { return Err("debug-eval: the page is not paused (debug-pause, or set-breakpoint, first)".into()) };
        let frames = params["callFrames"].as_array().cloned().unwrap_or_default();
        let frame_id = frames.first().and_then(|f| f["callFrameId"].as_str()).ok_or("debug-eval: the pause has no call frame")?;
        let r = self.call("Debugger.evaluateOnCallFrame", json!({ "callFrameId": frame_id, "expression": expr, "returnByValue": true }))?;
        if r.get("exceptionDetails").is_some() {
            return Err(format!("debug-eval: {expr} threw: {}", r["result"]["description"].as_str().unwrap_or("an exception")));
        }
        let top = self.frames(&frames[..1]).pop().unwrap_or(Value::Null);
        Ok(json!({ "expression": expr, "value": plain(&r["result"]), "type": r["result"]["type"], "function": top["function"], "file": top["file"], "line": top["line"] }))
    }

    fn action(&self) -> Value {
        match self.last_action.load(Ordering::SeqCst) {
            0 => Value::Null,
            n => json!(n),
        }
    }

    /// The original `(file, line, column, mapped)` of a generated 0-based position; the script URL and the generated position
    /// (1-based) when no map covers it.
    fn original(&self, script_id: &str, line: i64, column: i64) -> (String, i64, i64, bool) {
        let Ok(st) = self.state.lock() else { return (String::new(), line + 1, column + 1, false) };
        let Some(script) = st.scripts.get(script_id) else { return (String::new(), line + 1, column + 1, false) };
        if let Some((file, l, c)) = script.map.as_ref().and_then(|m| original_position(m, line, column)) {
            return (file, l, c, true);
        }
        (script.url.clone(), line + 1, column + 1, false)
    }

    fn frames(&self, frames: &[Value]) -> Vec<Value> {
        frames
            .iter()
            .take(MAX_FRAMES)
            .map(|f| {
                let loc = &f["location"];
                let (file, line, col, mapped) = self.original(loc["scriptId"].as_str().unwrap_or(""), loc["lineNumber"].as_i64().unwrap_or(0), loc["columnNumber"].as_i64().unwrap_or(0));
                let name = f["functionName"].as_str().filter(|n| !n.is_empty()).unwrap_or("(anonymous)");
                json!({ "function": name, "file": file, "line": line, "column": col, "mapped": mapped })
            })
            .collect()
    }

    fn locals(&self, top: Option<&Value>) -> Map<String, Value> {
        let mut locals = Map::new();
        let Some(top) = top else { return locals };
        for scope in top["scopeChain"].as_array().into_iter().flatten().filter(|s| matches!(s["type"].as_str(), Some("local" | "block" | "closure"))).take(3) {
            let Some(object_id) = scope["object"]["objectId"].as_str() else { continue };
            let Ok(props) = self.call("Runtime.getProperties", json!({ "objectId": object_id, "ownProperties": true })) else { continue };
            for p in props["result"].as_array().into_iter().flatten().take(MAX_LOCALS) {
                if let Some(name) = p["name"].as_str().filter(|n| !n.starts_with("__") && *n != "this") {
                    locals.entry(name.to_string()).or_insert_with(|| plain(&p["value"]));
                }
            }
        }
        locals
    }

    fn capture(&self, spec: &Spec, frames: &[Value]) -> Value {
        let out_frames = self.frames(frames);
        let locals = self.locals(frames.first());
        let mut captures = Map::new();
        if let Some(frame_id) = frames.first().and_then(|f| f["callFrameId"].as_str()) {
            for expr in &spec.capture {
                let value = match self.call("Debugger.evaluateOnCallFrame", json!({ "callFrameId": frame_id, "expression": expr, "returnByValue": true })) {
                    Ok(r) if r.get("exceptionDetails").is_some() => json!({ "error": r["result"]["description"] }),
                    Ok(r) => plain(&r["result"]),
                    Err(e) => json!({ "error": e }),
                };
                captures.insert(expr.clone(), value);
            }
        }
        let wall = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        json!({
            "breakpoint": spec.id,
            "file": spec.file,
            "line": spec.line,
            "function": out_frames.first().map_or(Value::Null, |f| f["function"].clone()),
            "source": out_frames.first().map_or(Value::Null, |f| json!({ "file": f["file"], "line": f["line"], "column": f["column"], "mapped": f["mapped"] })),
            "action": self.action(),
            "at_ms": self.started.elapsed().as_millis() as u64,
            "wall_ms": wall,
            "frames": out_frames,
            "locals": locals,
            "captures": captures,
        })
    }

    /// The recorded hits, oldest first; `clear` empties the list.
    pub fn hits(&self, clear: bool) -> Vec<Value> {
        let Ok(mut st) = self.state.lock() else { return vec![] };
        if clear {
            std::mem::take(&mut st.hits)
        } else {
            st.hits.clone()
        }
    }

    /// Removes every point; the page runs free.
    pub fn clear(&self) -> Result<Value, String> {
        let ids: Vec<String> = {
            let mut st = self.state.lock().map_err(|e| e.to_string())?;
            st.specs.clear();
            st.armed.drain().map(|(k, _)| k).collect()
        };
        for id in &ids {
            let _ = self.call("Debugger.removeBreakpoint", json!({ "breakpointId": id }));
        }
        Ok(json!({ "cleared": ids.len() }))
    }
}

/// The generated (line, column) of the first token that maps to `file:line` (1-based line), if the map knows the file.
fn generated_position(map: &SourceMap, file: &str, line: u32) -> Option<(u32, u32)> {
    map.tokens()
        .filter(|t| t.get_source().is_some_and(|s| same_source(s, file)) && t.get_src_line() + 1 == line)
        .map(|t| (t.get_dst_line(), t.get_dst_col()))
        .min()
}

/// The original `(source, 1-based line, 1-based column)` of a generated 0-based position, if the map covers it.
fn original_position(map: &SourceMap, line: i64, column: i64) -> Option<(String, i64, i64)> {
    let token = map.lookup_token(line.max(0) as u32, column.max(0) as u32)?;
    let src = token.get_source()?;
    Some((src.to_string(), i64::from(token.get_src_line()) + 1, i64::from(token.get_src_col()) + 1))
}
