//! A page of an already-running Chromium, driven over CDP: what the Playwright daemon calls an *attached* session
//! (`open --cdp-endpoint`). No Playwright side effects are reproduced (no fake mic or camera, no color-scheme binding,
//! no device frame): an attached window is a real one on a real display.
use crate::{json_endpoint, Cdp};
use base64::Engine as _;
use serde_json::{json, Value};
use std::time::Duration;

const SNAPSHOT_JS: &str = include_str!("snapshot.js");
const MEASURE_LAUNCH_JS: &str = include_str!("measure_launch.js");
const INDICATORS_JS: &str = include_str!("js/indicators.js");

/// The markers' script is about 10 KB, and a run draws one per typed key: a document gets it once, and a marker is then
/// one short call; after a navigation the first call misses and the second installs the script again.
fn marker_calls(kind: &str, args: &Value) -> [String; 2] {
    [
        format!("window.__wfMarks ? (window.__wfMarks.{kind}({args}), true) : false"),
        format!("(window.__wfMarks = ({INDICATORS_JS})).{kind}({args}), true"),
    ]
}

pub struct Attached {
    cdp: Cdp,
    target_id: String,
    session: String,
    /// Main-world execution context of each frame, kept from `Runtime.executionContext*` events.
    contexts: std::collections::HashMap<String, i64>,
    bindings: Vec<Value>,
    /// When set, `eval` (and what is built on it) runs in this frame instead of the main one.
    target: Option<String>,
    /// Scripts the page parsed since coverage started: script id to (url, length).
    parsed: Option<std::collections::HashMap<String, (String, usize)>>,
    /// Where `HeapProfiler.addHeapSnapshotChunk` events are written while a heap snapshot is taken.
    heap_out: Option<(std::fs::File, usize)>,
    /// Files the next file chooser is answered with, once `upload` arms them.
    armed_files: Option<Vec<String>>,
    /// The `<input type=file>` of a chooser that opened while nothing was armed.
    open_chooser: Option<i64>,
    delivered: Vec<String>,
    /// URL globs routed through `network_proxy`.
    proxied: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    /// The user and password answered to a 401/407 challenge (`open --http-credentials`).
    credentials: std::sync::Arc<std::sync::Mutex<Option<(String, String)>>>,
    /// Canned answers for matching requests (`network_stub`), checked before the proxy; the page never reaches the network.
    stubs: std::sync::Arc<std::sync::Mutex<Vec<Stub>>>,
    /// Each stubbed request answered: {method, url, status, pattern, at}.
    stub_hits: std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
    /// The on-page marker colour for the command being run: `None` for the default, `Some("")` never; `indicators_off`
    /// turns them off (`--no-indicator`).
    indicator_color: Option<String>,
    indicators_off: bool,
    /// The action id the next marker is numbered with (0: none); the daemon sets it before each input command.
    step: u64,
    /// When the action's first marker was on the page, so a recording can name the first frame that shows it.
    marked: Option<std::time::Instant>,
    /// Console lines and uncaught errors of every frame since the page was attached, oldest first (bounded).
    console: Vec<ConsoleLine>,
}

/// One `Runtime.consoleAPICalled` or `Runtime.exceptionThrown` of the page: `debug`, `info`, `warn` or `error`, the text as
/// `'<level>: <message> (at <frames>)'`, and the frame it came from when known.
#[derive(Clone, Debug, PartialEq)]
pub struct ConsoleLine {
    pub level: &'static str,
    pub text: String,
    pub frame: Option<String>,
}

const CONSOLE_LIMIT: usize = 5_000;
const WINDOW_CHROME_SLACK: i64 = 300;

pub enum SwipeStart<'a> {
    Ref(&'a str),
    From(f64, f64),
    /// A point given twice: in the page, where the finger lands, and in the frame `eval` targets, where the scroller is found.
    At { page: (f64, f64), frame: (f64, f64) },
}

pub struct Swiped {
    pub from: (i64, i64),
    pub to: (i64, i64),
    pub ms: u64,
    pub scrolled: (f64, f64),
}

/// How one press-move-release went: its milliseconds, the moves it planned and sent (a human finger on a busy page sends
/// only the latest of the moves already due), and the longest one move's dispatch blocked, the page's per-move cost.
pub struct Drawn {
    pub ms: u64,
    pub moves: usize,
    pub sent: usize,
    pub max_move_ms: u64,
}

/// One frame of the page, as `Page.getFrameTree` reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub id: String,
    pub parent: Option<String>,
    pub url: String,
    pub name: String,
}

/// `http://host:port` (as `--cdp-endpoint` is given) or a `ws://` URL, resolved to the browser's WebSocket URL.
pub fn browser_ws_url(endpoint: &str) -> Result<String, String> {
    if endpoint.starts_with("ws://") {
        return Ok(endpoint.to_string());
    }
    if !endpoint.starts_with("http://") {
        return Err(format!("unsupported CDP endpoint {endpoint}"));
    }
    let (host, port) = crate::host_and_port(endpoint)?;
    let version = json_endpoint(&host, port, "/json/version")?;
    version["webSocketDebuggerUrl"].as_str().map(str::to_string).ok_or_else(|| format!("{endpoint} did not report a webSocketDebuggerUrl"))
}

/// `*` matches any run of characters.
pub fn glob_match(glob: &str, text: &str) -> bool {
    let parts: Vec<&str> = glob.split('*').collect();
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if parts.len() == 1 {
        return glob == text;
    }
    if !text.starts_with(first) || !text.ends_with(last) || text.len() < first.len() + last.len() {
        return false;
    }
    let mut at = first.len();
    let end = text.len() - last.len();
    for part in &parts[1..parts.len() - 1] {
        match text[at..end].find(part) {
            Some(i) => at += i + part.len(),
            None => return false,
        }
    }
    true
}

/// One canned answer: a request whose method (any when None) and URL (a `*` glob) match gets this status, headers and body.
#[derive(Clone, Debug)]
pub struct Stub {
    pub method: Option<String>,
    pub pattern: String,
    pub status: i64,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub delay_ms: u64,
}

struct Routes {
    credentials: std::sync::Arc<std::sync::Mutex<Option<(String, String)>>>,
    proxied: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    stubs: std::sync::Arc<std::sync::Mutex<Vec<Stub>>>,
    hits: std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// The stub a request matches; the last one declared wins, so a script can override an earlier route.
pub fn matching_stub(stubs: &[Stub], method: &str, url: &str) -> Option<Stub> {
    stubs.iter().rev().find(|s| s.method.as_deref().is_none_or(|m| m.eq_ignore_ascii_case(method)) && glob_match(&s.pattern, url)).cloned()
}

fn answer_fetch(routes: &Routes, event: Value, fire: crate::Fire) {
    let (credentials, proxied) = (&routes.credentials, &routes.proxied);
    if event["method"] == "Fetch.authRequired" {
        let session = event["sessionId"].as_str().map(str::to_string);
        let id = event["params"]["requestId"].clone();
        let answer = match credentials.lock().ok().and_then(|c| c.clone()) {
            Some((u, p)) => json!({ "response": "ProvideCredentials", "username": u, "password": p }),
            None => json!({ "response": "Default" }),
        };
        let _ = fire.send("Fetch.continueWithAuth", json!({ "requestId": id, "authChallengeResponse": answer }), session.as_deref());
        return;
    }
    let params = &event["params"];
    let session = event["sessionId"].as_str().map(str::to_string);
    let id = params["requestId"].as_str().unwrap_or_default().to_string();
    let url = params["request"]["url"].as_str().unwrap_or_default();
    let method = params["request"]["method"].as_str().unwrap_or("GET");
    // a cross-origin POST is preceded by a CORS preflight; for a stubbed URL the stub answers it too, or the POST never leaves
    if method.eq_ignore_ascii_case("OPTIONS") && routes.stubs.lock().is_ok_and(|s| s.iter().any(|st| glob_match(&st.pattern, url))) {
        let asked = params["request"]["headers"].as_object().and_then(|h| h.iter().find(|(k, _)| k.eq_ignore_ascii_case("access-control-request-headers"))).and_then(|(_, v)| v.as_str()).unwrap_or("*").to_string();
        let headers = json!([
            { "name": "Access-Control-Allow-Origin", "value": "*" },
            { "name": "Access-Control-Allow-Methods", "value": "GET, POST, PUT, PATCH, DELETE, OPTIONS" },
            { "name": "Access-Control-Allow-Headers", "value": asked },
            { "name": "Access-Control-Max-Age", "value": "600" },
        ]);
        let _ = fire.send("Fetch.fulfillRequest", json!({ "requestId": id, "responseCode": 204, "responseHeaders": headers, "body": "" }), session.as_deref());
        return;
    }
    if let Some(stub) = routes.stubs.lock().ok().and_then(|s| matching_stub(&s, method, url)) {
        if stub.delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(stub.delay_ms));
        }
        let mut headers: Vec<Value> = stub.headers.iter().map(|(k, v)| json!({ "name": k, "value": v })).collect();
        // a cross-origin page reads a stubbed answer as it would read a server that allows it
        if !stub.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("access-control-allow-origin")) {
            headers.push(json!({ "name": "Access-Control-Allow-Origin", "value": "*" }));
        }
        let _ = fire.send(
            "Fetch.fulfillRequest",
            json!({ "requestId": id, "responseCode": stub.status, "responseHeaders": headers, "body": crate::BASE64.encode(&stub.body) }),
            session.as_deref(),
        );
        if let Ok(mut h) = routes.hits.lock() {
            h.push(json!({ "method": method, "url": url, "status": stub.status, "pattern": stub.pattern, "at": now_ms() }));
        }
        return;
    }
    if !proxied.lock().map(|p| p.iter().any(|g| glob_match(g, url))).unwrap_or(false) {
        let _ = fire.send("Fetch.continueRequest", json!({ "requestId": id }), session.as_deref());
        return;
    }
    let fulfil = |code: i64, headers: Vec<Value>, body: &[u8]| {
        let _ = fire.send(
            "Fetch.fulfillRequest",
            json!({ "requestId": id, "responseCode": code, "responseHeaders": headers, "body": crate::BASE64.encode(body) }),
            session.as_deref(),
        );
    };
    let request = &params["request"];
    let url = request["url"].as_str().unwrap_or_default();
    let result = (|| -> Result<(i64, Vec<Value>, Vec<u8>), String> {
        let mut fetch = cli_http::Request::new(request["method"].as_str().unwrap_or("GET"), url).timeout(std::time::Duration::from_secs(60));
        for (k, v) in request["headers"].as_object().into_iter().flatten() {
            if !matches!(k.to_ascii_lowercase().as_str(), "host" | "content-length" | "connection" | "accept-encoding") {
                fetch = fetch.header(k, v.as_str().unwrap_or_default());
            }
        }
        if let Some(post) = request["postData"].as_str() {
            fetch = fetch.body(post);
        }
        let response = fetch.send()?;
        let headers = response
            .headers
            .iter()
            .filter(|(k, _)| !matches!(k.to_ascii_lowercase().as_str(), "content-encoding" | "content-length" | "transfer-encoding" | "connection"))
            .map(|(k, v)| json!({ "name": k, "value": v }))
            .collect();
        Ok((i64::from(response.status), headers, response.body))
    })();
    match result {
        Ok((code, headers, body)) => fulfil(code, headers, &body),
        Err(message) => fulfil(
            502,
            vec![json!({ "name": "content-type", "value": "application/json" })],
            json!({ "error": "network-proxy-fetch-failed", "message": message }).to_string().as_bytes(),
        ),
    }
}

impl Attached {
    /// Attaches to the first page of the browser, opening a blank one when it has none.
    pub fn attach(endpoint: &str) -> Result<Attached, String> {
        let cdp = Cdp::connect(&browser_ws_url(endpoint)?)?;
        let targets = cdp.call("Target.getTargets", json!({}))?;
        let existing = targets["targetInfos"]
            .as_array()
            .and_then(|all| all.iter().find(|t| t["type"] == "page" && !t["url"].as_str().unwrap_or("").starts_with("devtools://")))
            .and_then(|t| t["targetId"].as_str().map(str::to_string));
        let target_id = match existing {
            Some(id) => id,
            None => cdp.call("Target.createTarget", json!({ "url": "about:blank" }))?["targetId"].as_str().ok_or("Target.createTarget returned no targetId")?.to_string(),
        };
        let session = cdp.call("Target.attachToTarget", json!({ "targetId": target_id, "flatten": true }))?["sessionId"].as_str().ok_or("Target.attachToTarget returned no sessionId")?.to_string();
        let mut attached = Attached { cdp, target_id, session, contexts: Default::default(), bindings: Vec::new(), target: None, parsed: None, heap_out: None, armed_files: None, open_chooser: None, delivered: Vec::new(), proxied: Default::default(), credentials: Default::default(), stubs: Default::default(), stub_hits: Default::default(), indicator_color: None, indicators_off: false, step: 0, marked: None, console: Vec::new() };
        for domain in ["Page", "Runtime", "Network"] {
            attached.page_call(&format!("{domain}.enable"), json!({}))?;
        }
        Ok(attached)
    }

    pub fn session_id(&self) -> String {
        self.session.clone()
    }

    pub fn cdp_handle(&self) -> &Cdp {
        &self.cdp
    }

    fn page_call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let session = self.session.clone();
        self.cdp.call_on(method, params, Some(&session))
    }

    pub fn navigate(&mut self, url: &str) -> Result<(), String> {
        // the old document carries a marker the new one lacks, so the wait cannot pass on the page being left
        let _ = self.eval_page("window.__wfLeaving = true");
        let reply = self.page_call("Page.navigate", json!({ "url": url }))?;
        if let Some(e) = reply["errorText"].as_str().filter(|e| !e.is_empty()) {
            return Err(format!("page.goto: {e} at {url}"));
        }
        // Page.navigate answers once the navigation starts; a new document (loaderId) is waited for until it has loaded
        if reply["loaderId"].as_str().is_none_or(str::is_empty) {
            return Ok(());
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if self.eval_page("!window.__wfLeaving && document.readyState === 'complete'").is_ok_and(|v| v == json!(true)) {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                // what the page was doing when the wait ended: still the old document (the navigation never committed) or a new one that never loaded
                let state = self.eval_page("JSON.stringify({ href: location.href, readyState: document.readyState, leaving: !!window.__wfLeaving })").ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| "the page did not answer".to_string());
                return Err(format!("page.goto: {url} did not finish loading within 30 s (page state: {state})"));
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    /// Makes `eval` run in `frame` (or back in the main frame with `None`).
    pub fn set_target(&mut self, frame: Option<String>) {
        self.target = frame;
    }

    /// The value of a JavaScript expression in the target frame (the main one unless `set_target` chose another).
    pub fn eval(&mut self, expression: &str) -> Result<Value, String> {
        match self.target.clone() {
            Some(frame) => self.eval_in_frame(&frame, expression),
            None => self.eval_page(expression),
        }
    }

    /// The value of a JavaScript expression in the main frame, awaiting a returned promise; a thrown error is the message.
    pub fn eval_page(&mut self, expression: &str) -> Result<Value, String> {
        let reply = self.page_call("Runtime.evaluate", json!({ "expression": expression, "returnByValue": true, "awaitPromise": true }))?;
        if let Some(ex) = reply.get("exceptionDetails") {
            let text = ex["exception"]["description"].as_str().or_else(|| ex["text"].as_str()).unwrap_or("evaluation failed");
            return Err(text.to_string());
        }
        Ok(reply["result"].get("value").cloned().unwrap_or(Value::Null))
    }

    /// Every cookie of the browser (HttpOnly included), or those that apply to `urls`.
    pub fn cookies(&mut self, urls: Option<&[String]>) -> Result<Vec<Value>, String> {
        let reply = match urls {
            Some(u) => self.page_call("Network.getCookies", json!({ "urls": u }))?,
            None => self.cdp.call("Storage.getCookies", json!({}))?,
        };
        Ok(reply["cookies"]
            .as_array()
            .map(|all| all.iter().map(|c| json!({ "name": c["name"], "value": c["value"], "domain": c["domain"], "path": c["path"] })).collect())
            .unwrap_or_default())
    }

    /// The page as a PNG; `full_page` captures the whole document, beyond the viewport.
    pub fn screenshot_png(&mut self, full_page: bool) -> Result<Vec<u8>, String> {
        let mut params = json!({ "format": "png" });
        if full_page {
            let m = self.page_call("Page.getLayoutMetrics", json!({}))?;
            let size = if m["cssContentSize"].is_object() { &m["cssContentSize"] } else { &m["contentSize"] };
            params["captureBeyondViewport"] = json!(true);
            params["clip"] = json!({ "x": 0, "y": 0, "width": size["width"], "height": size["height"], "scale": 1 });
        }
        // a capture that comes back tiled (the gap in tiled.rs) is taken again, a little later, up to five times
        let mut png = Vec::new();
        for attempt in 0..5 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(150));
            }
            let reply = self.page_call("Page.captureScreenshot", params.clone())?;
            png = crate::BASE64.decode(reply["data"].as_str().ok_or("Page.captureScreenshot returned no data")?).map_err(|e| e.to_string())?;
            if !crate::tiled::png_is_tiled(&png) {
                break;
            }
        }
        Ok(png)
    }

    /// The viewport as a JPEG: what a remote viewer polls, far smaller than a PNG of the same pixels.
    pub fn screenshot_jpeg(&mut self, quality: u8) -> Result<Vec<u8>, String> {
        let reply = self.page_call("Page.captureScreenshot", json!({ "format": "jpeg", "quality": quality.clamp(1, 100) }))?;
        crate::BASE64.decode(reply["data"].as_str().ok_or("Page.captureScreenshot returned no data")?).map_err(|e| e.to_string())
    }

    /// One left-button click at CSS pixel `(x, y)`: move, press, release.
    pub fn click(&mut self, x: f64, y: f64) -> Result<(), String> {
        self.page_call("Input.dispatchMouseEvent", json!({ "type": "mouseMoved", "x": x, "y": y }))?;
        self.page_call("Input.dispatchMouseEvent", json!({ "type": "mousePressed", "x": x, "y": y, "button": "left", "clickCount": 1 }))?;
        self.page_call("Input.dispatchMouseEvent", json!({ "type": "mouseReleased", "x": x, "y": y, "button": "left", "clickCount": 1 }))?;
        Ok(())
    }

    /// A wheel scroll of `delta_y` CSS pixels (positive scrolls down) with the pointer at `(x, y)`.
    pub fn wheel(&mut self, x: f64, y: f64, delta_y: f64) -> Result<(), String> {
        self.page_call("Input.dispatchMouseEvent", json!({ "type": "mouseWheel", "x": x, "y": y, "deltaX": 0, "deltaY": delta_y }))?;
        Ok(())
    }

    fn window_bounds(&mut self) -> Result<(i64, i64, i64, String), String> {
        let got = self.cdp.call("Browser.getWindowForTarget", json!({ "targetId": self.target_id }))?;
        Ok((
            got["windowId"].as_i64().ok_or("no windowId")?,
            got["bounds"]["width"].as_i64().unwrap_or(0),
            got["bounds"]["height"].as_i64().unwrap_or(0),
            got["bounds"]["windowState"].as_str().unwrap_or("normal").to_string(),
        ))
    }

    /// EWMH fullscreen over CDP. The state goes out on its own (Chrome rejects it beside bounds), through `normal` first.
    /// Never fails: `(false, 0, 0)` when the window cannot be changed.
    pub fn fullscreen_window(&mut self) -> (bool, i64, i64) {
        let mut run = || -> Result<(bool, i64, i64), String> {
            let (id, _, _, state) = self.window_bounds()?;
            if state != "normal" {
                self.cdp.call("Browser.setWindowBounds", json!({ "windowId": id, "bounds": { "windowState": "normal" } }))?;
            }
            self.cdp.call("Browser.setWindowBounds", json!({ "windowId": id, "bounds": { "windowState": "fullscreen" } }))?;
            let (_, w, h, after) = self.window_bounds()?;
            Ok((after == "fullscreen", w, h))
        };
        run().unwrap_or((false, 0, 0))
    }

    fn viewport(&mut self) -> Result<(f64, f64), String> {
        let m = self.page_call("Page.getLayoutMetrics", json!({}))?;
        let vv = if m["cssVisualViewport"].is_object() { &m["cssVisualViewport"] } else { &m["visualViewport"] };
        Ok((vv["clientWidth"].as_f64().unwrap_or(0.0), vv["clientHeight"].as_f64().unwrap_or(0.0)))
    }

    /// Presents the page as a mobile device without resizing its window: clear any earlier override, read the settled
    /// native viewport (two equal reads, at most eight tries), add the scrollbar gutter, then pin it with `mobile: true`.
    pub fn emulate_mobile(&mut self, user_agent: &str) -> Result<(String, i64, i64), String> {
        self.page_call("Emulation.clearDeviceMetricsOverride", json!({}))?;
        let mut pre = self.viewport()?;
        for _ in 0..8 {
            std::thread::sleep(Duration::from_millis(40));
            let next = self.viewport()?;
            if next == pre {
                break;
            }
            pre = next;
        }
        let gutter = self.eval("({ w: window.innerWidth - document.documentElement.clientWidth, h: window.innerHeight - document.documentElement.clientHeight })").unwrap_or(Value::Null);
        let (w, h) = (pre.0 + gutter["w"].as_f64().unwrap_or(0.0), pre.1 + gutter["h"].as_f64().unwrap_or(0.0));
        self.page_call("Emulation.setDeviceMetricsOverride", json!({ "width": w, "height": h, "screenWidth": w, "screenHeight": h, "deviceScaleFactor": 1, "mobile": true }))?;
        self.page_call("Emulation.setUserAgentOverride", json!({ "userAgent": user_agent }))?;
        self.page_call("Emulation.setTouchEmulationEnabled", json!({ "enabled": true, "maxTouchPoints": 5 }))?;
        let size = self.eval("({ width: window.innerWidth, height: window.innerHeight })")?;
        Ok((user_agent.to_string(), size["width"].as_i64().unwrap_or(0), size["height"].as_i64().unwrap_or(0)))
    }

    /// Forgets everything the browser holds for any site: cookies and the HTTP cache (a login page asks for the account again).
    pub fn clear_browser_data(&mut self) -> Result<(), String> {
        self.page_call("Network.clearBrowserCookies", json!({}))?;
        self.page_call("Network.clearBrowserCache", json!({}))?;
        Ok(())
    }

    /// Pins the page to a phone of `width` x `height` CSS pixels. A headless browser has no real window to measure (its virtual
    /// screen caps one at 800x600), so the size is stated, not read back as `emulate_mobile` does.
    pub fn emulate_phone(&mut self, user_agent: &str, width: i64, height: i64) -> Result<(), String> {
        self.page_call("Emulation.setDeviceMetricsOverride", json!({ "width": width, "height": height, "screenWidth": width, "screenHeight": height, "deviceScaleFactor": 1, "mobile": true }))?;
        self.page_call("Emulation.setUserAgentOverride", json!({ "userAgent": user_agent }))?;
        self.page_call("Emulation.setTouchEmulationEnabled", json!({ "enabled": true, "maxTouchPoints": 5 }))?;
        Ok(())
    }

    /// Calls the page made to bindings registered with `add_binding`, oldest first (`{name, payload, executionContextId}`).
    pub fn take_binding_calls(&mut self) -> Vec<Value> {
        self.track_contexts();
        std::mem::take(&mut self.bindings)
    }

    /// The console lines buffered since the page was attached (or last cleared); `clear` empties the buffer.
    pub fn console_logs(&mut self, clear: bool) -> Vec<ConsoleLine> {
        self.track_contexts();
        if clear {
            std::mem::take(&mut self.console)
        } else {
            self.console.clone()
        }
    }

    /// The id of the page's main frame (a page target's main frame shares the target's id).
    pub fn main_frame(&self) -> &str {
        &self.target_id
    }

    /// Registers `window.<name>(payload)` for every document of the page: the call arrives in `take_binding_calls`.
    pub fn add_binding(&mut self, name: &str) -> Result<(), String> {
        self.page_call("Runtime.addBinding", json!({ "name": name }))?;
        Ok(())
    }

    /// Runs a script in every new document of the page, before the page's own scripts.
    pub fn add_init_script(&mut self, source: &str) -> Result<(), String> {
        self.page_call("Page.addScriptToEvaluateOnNewDocument", json!({ "source": source }))?;
        Ok(())
    }

    fn track_contexts(&mut self) {
        let events = self.cdp.take_events();
        for e in events {
            match e["method"].as_str() {
                Some("Runtime.bindingCalled") => self.bindings.push(e["params"].clone()),
                Some("Runtime.executionContextCreated") => {
                    let c = &e["params"]["context"];
                    if c["auxData"]["isDefault"].as_bool() == Some(true) {
                        if let (Some(frame), Some(id)) = (c["auxData"]["frameId"].as_str(), c["id"].as_i64()) {
                            self.contexts.insert(frame.to_string(), id);
                        }
                    }
                }
                Some("Runtime.executionContextDestroyed") => {
                    let id = e["params"]["executionContextId"].as_i64();
                    self.contexts.retain(|_, v| Some(*v) != id);
                }
                Some("Runtime.executionContextsCleared") => self.contexts.clear(),
                Some("Runtime.consoleAPICalled" | "Runtime.exceptionThrown") => {
                    let (level, text) = console_text(&e);
                    let context = e["params"]["executionContextId"].as_i64().or_else(|| e["params"]["exceptionDetails"]["executionContextId"].as_i64());
                    let frame = context.and_then(|id| self.contexts.iter().find(|(_, v)| **v == id).map(|(f, _)| f.clone()));
                    if self.console.len() >= CONSOLE_LIMIT {
                        self.console.remove(0);
                    }
                    self.console.push(ConsoleLine { level, text, frame });
                }
                Some("Debugger.scriptParsed") => {
                    if let Some(parsed) = self.parsed.as_mut() {
                        let p = &e["params"];
                        if let Some(id) = p["scriptId"].as_str() {
                            parsed.insert(id.to_string(), (p["url"].as_str().unwrap_or("").to_string(), p["length"].as_u64().unwrap_or(0) as usize));
                        }
                    }
                }
                Some("Page.fileChooserOpened") => {
                    if let Some(node) = e["params"]["backendNodeId"].as_i64() {
                        self.open_chooser = Some(node);
                        self.answer_chooser();
                    }
                }
                Some("HeapProfiler.addHeapSnapshotChunk") => {
                    if let (Some((file, bytes)), Some(chunk)) = (self.heap_out.as_mut(), e["params"]["chunk"].as_str()) {
                        use std::io::Write;
                        if file.write_all(chunk.as_bytes()).is_ok() {
                            *bytes += chunk.len();
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Every visible interactive element's intersection with the viewport, measured in the page (the TypeScript daemon's
    /// `measureLaunchPage`, transpiled): `checked` and the elements below `tolerance`, worst first.
    pub fn check_clipping(&mut self, tolerance: f64) -> Result<Value, String> {
        let measured = self.eval_page(&format!("({MEASURE_LAUNCH_JS})()"))?;
        let mut clipped: Vec<(f64, Value)> = measured["elements"]
            .as_array()
            .map(|all| all.as_slice())
            .unwrap_or_default()
            .iter()
            .filter(|el| el["ratio"].as_f64().unwrap_or(1.0) < tolerance)
            .map(|el| {
                let ratio = (el["ratio"].as_f64().unwrap_or(0.0) * 1000.0).round() / 1000.0;
                (ratio, json!({ "tag": el["tag"], "cls": el["cls"], "intersectionRatio": ratio, "left": el["left"], "right": el["right"], "width": el["width"] }))
            })
            .collect();
        clipped.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        Ok(json!({ "checked": measured["checked"], "clippedCount": clipped.len(), "clipped": clipped.into_iter().map(|(_, v)| v).collect::<Vec<_>>() }))
    }

    /// The six launch invariants of an attached desktop window (what the TypeScript SDK's `checkLaunchInvariants` measured),
    /// each `{id, pass, measured, reason?}`. Never fails for a violated invariant, only when the page or the window cannot be
    /// reached. The mobile and fullscreen checks read CDP's and the X server's own answers, not page geometry.
    pub fn check_launch_invariants(&mut self, device: &str, expected_width: i64, tolerance: f64) -> Result<Value, String> {
        let layout = self.page_call("Page.getLayoutMetrics", json!({}))?;
        let (_, window_width, window_height, window_state) = self.window_bounds()?;
        let page = self.eval_page(&format!("({MEASURE_LAUNCH_JS})()"))?;
        let url = self.eval_page("location.href").ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        let num = |v: &Value, k: &str| v[k].as_f64().unwrap_or(0.0);
        let text = |v: &Value, k: &str| v[k].as_str().unwrap_or("").to_string();
        let flag = |v: &Value, k: &str| v[k].as_bool().unwrap_or(false);

        let viewport_width = num(&layout["layoutViewport"], "clientWidth");
        let expected = expected_width as f64;
        let scrollbar = num(&page, "scrollbarWidth");
        let effective = viewport_width + scrollbar;
        let exact = effective == expected;
        let visual_scale = layout["visualViewport"]["scale"].as_f64().unwrap_or(1.0);
        let body_width = num(&page, "bodyWidth");
        let page_overflow = visual_scale == 1.0 && body_width == expected;
        let mobile_pass = exact || page_overflow;
        let raw_exact = viewport_width == expected;
        let overflow = if raw_exact {
            "none"
        } else if scrollbar > 0.0 && exact {
            "theirs"
        } else if page_overflow {
            "page"
        } else {
            "ours"
        };
        let mut mobile = json!({
            "id": "mobile-viewport",
            "pass": mobile_pass,
            "measured": {
                "clientWidth": viewport_width, "expectedWidth": expected, "scrollbarWidth": scrollbar, "effectiveWidth": effective,
                "screenWidth": num(&page, "screenWidth"), "devicePixelRatio": num(&page, "devicePixelRatio"),
                "windowBounds": format!("{window_width}x{window_height} ({window_state})"),
                "visualViewportScale": visual_scale, "bodyWidth": body_width, "overflow": overflow,
            },
        });
        if !mobile_pass {
            mobile["reason"] = json!(format!("effective viewport width {effective} (layout {viewport_width} + scrollbar {scrollbar}) does not equal device width {expected}"));
        }

        let ewmh = read_ewmh_state();
        let fullscreen_pass = window_state == "fullscreen" && matches!(&ewmh, Ok((_, true)));
        let mut fullscreen = json!({
            "id": "fullscreen",
            "pass": fullscreen_pass,
            "measured": {
                "windowState": window_state, "windowWidth": window_width, "windowHeight": window_height,
                "ewmhState": ewmh.as_ref().map_or("UNREADABLE".to_string(), |(s, _)| s.clone()),
                "ewmhFullscreen": ewmh.as_ref().is_ok_and(|(_, f)| *f),
            },
        });
        if let Err(e) = &ewmh {
            fullscreen["measured"]["ewmhError"] = json!(e);
        }
        if !fullscreen_pass {
            fullscreen["reason"] = json!(match &ewmh {
                Ok(_) => format!("windowState \"{window_state}\" not \"fullscreen\", or _NET_WM_STATE lacks _NET_WM_STATE_FULLSCREEN"),
                Err(e) => format!("cannot read EWMH _NET_WM_STATE ground truth: {e}"),
            });
        }

        let elements: Vec<Value> = page["elements"].as_array().cloned().unwrap_or_default();
        let label = |el: &Value| {
            let cls = text(el, "cls");
            let token = cls.split(char::is_whitespace).next().unwrap_or("");
            if token.is_empty() { text(el, "tag") } else { format!("{}.{token}", text(el, "tag")) }
        };
        let clipped: Vec<&Value> = elements
            .iter()
            .filter(|el| {
                let (ratio, h_ratio) = (num(el, "ratio"), num(el, "hRatio"));
                !flag(el, "tiny") && (h_ratio < tolerance || (flag(el, "inFold") && ratio < tolerance && !(ratio == 0.0 && (flag(el, "painted") || flag(el, "occluded")))))
            })
            .collect();
        let occluded: Vec<&Value> = elements.iter().filter(|el| flag(el, "occluded")).collect();
        let tiny: Vec<&Value> = elements.iter().filter(|el| flag(el, "tiny")).collect();
        let clipped_labels = clipped
            .iter()
            .map(|el| {
                let reason = text(el, "reason");
                format!(
                    "{}={:.3}/h{:.3}@{},{} {}x{}{}{}",
                    label(el),
                    num(el, "ratio"),
                    num(el, "hRatio"),
                    num(el, "left"),
                    num(el, "top"),
                    num(el, "width"),
                    num(el, "bottom") - num(el, "top"),
                    if reason.is_empty() { String::new() } else { format!(" ({reason})") },
                    if flag(el, "painted") { " painted" } else { "" },
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let checked = num(&page, "checked");
        let mut inputs = json!({
            "id": "inputs-visible",
            "pass": clipped.is_empty(),
            "measured": {
                "checked": checked, "clippedCount": clipped.len(), "clipped": clipped_labels,
                "occludedCount": occluded.len(),
                "occluded": occluded.iter().map(|el| format!("{} ({})", label(el), text(el, "reason"))).collect::<Vec<_>>().join(", "),
                "tinyCount": tiny.len(), "tiny": tiny.iter().map(|el| label(el)).collect::<Vec<_>>().join(", "),
            },
        });
        if !clipped.is_empty() {
            inputs["reason"] = json!(format!("{} of {checked} interactive elements clipped: {clipped_labels}", clipped.len()));
        }

        let fab = page["fab"].is_object().then(|| &page["fab"]);
        let mut fab_present = json!({
            "id": "fab-present",
            "pass": fab.is_some(),
            "measured": {
                "hostPresent": flag(&page, "hostPresent"),
                "fabTop": fab.map_or(0.0, |f| num(f, "top")), "fabLeft": fab.map_or(0.0, |f| num(f, "left")),
                "fabWidth": fab.map_or(0.0, |f| num(f, "width")), "fabHeight": fab.map_or(0.0, |f| num(f, "height")),
            },
        });
        if fab.is_none() {
            fab_present["reason"] = json!(if flag(&page, "hostPresent") { "data-fab-* mirror rect missing or zero on #vnc-open-url-host" } else { "#vnc-open-url-host not found" });
        }

        let offenders: Vec<&Value> = match fab {
            Some(f) => elements
                .iter()
                .filter(|el| {
                    let (l, t, w, h) = (num(f, "left"), num(f, "top"), num(f, "width"), num(f, "height"));
                    l < num(el, "right") && l + w > num(el, "left") && t < num(el, "bottom") && t + h > num(el, "top")
                })
                .collect(),
            None => Vec::new(),
        };
        let overlap_pass = fab.is_none() || offenders.is_empty();
        let mut no_overlap = json!({
            "id": "fab-no-overlap",
            "pass": overlap_pass,
            "measured": {
                "fabRect": fab.map_or("missing".to_string(), |f| format!("left={} top={} {}x{}", num(f, "left"), num(f, "top"), num(f, "width"), num(f, "height"))),
                "candidatesChecked": checked, "overlapCount": offenders.len(),
                "offender": offenders.first().map_or("none".to_string(), |el| label(el)),
            },
        });
        if !overlap_pass {
            no_overlap["reason"] = json!(format!("FAB overlaps {}", offenders.iter().map(|el| label(el)).collect::<Vec<_>>().join(", ")));
        }

        let banner = flag(&page, "bannerFound");
        let mut consent = json!({ "id": "consent-dismissed", "pass": !banner, "measured": { "bannerFound": banner } });
        if banner {
            consent["measured"]["bannerText"] = json!(text(&page, "bannerText"));
            consent["measured"]["clickableLabels"] = json!(text(&page, "clickableLabels"));
            consent["reason"] = json!(format!("consent banner still showing after dismissal: \"{}\"", text(&page, "bannerText")));
        }

        let invariants = vec![mobile, fullscreen, inputs, fab_present, no_overlap, consent];
        let pass = invariants.iter().all(|i| i["pass"].as_bool() == Some(true));
        Ok(json!({ "pass": pass, "url": url, "device": device, "invariants": invariants }))
    }

    /// Routes requests matching `pattern` (a `*` glob) through this process: the browser's request is sent by this process, without
    /// the browser's network restrictions, and the real response is handed back. A failure answers 502 with a JSON error.
    pub fn network_proxy(&mut self, pattern: &str, enable: bool) -> Result<bool, String> {
        let glob = pattern.replace("**", "*");
        if let Ok(mut proxied) = self.proxied.lock() {
            proxied.retain(|p| p != &glob);
            if enable {
                proxied.push(glob);
            }
        }
        self.refresh_fetch()?;
        Ok(enable)
    }

    /// Answers requests matching `stub` with its canned response from now on (replacing a stub with the same method and pattern).
    pub fn network_stub(&mut self, stub: Stub) -> Result<(), String> {
        if let Ok(mut stubs) = self.stubs.lock() {
            stubs.retain(|s| !(s.pattern == stub.pattern && s.method == stub.method));
            stubs.push(stub);
        }
        self.refresh_fetch()
    }

    /// Removes the stubs for `pattern`, or all of them; requests go to the network again.
    pub fn network_unstub(&mut self, pattern: Option<&str>) -> Result<usize, String> {
        let removed = match self.stubs.lock() {
            Ok(mut s) => {
                let before = s.len();
                s.retain(|st| pattern.is_some_and(|p| p.replace("**", "*") != st.pattern));
                before - s.len()
            }
            Err(_) => 0,
        };
        self.refresh_fetch()?;
        Ok(removed)
    }

    /// The stubbed requests answered so far, oldest first; `clear` empties the list.
    pub fn network_stub_hits(&self, clear: bool) -> Vec<Value> {
        self.stub_hits.lock().map(|mut h| if clear { std::mem::take(&mut *h) } else { h.clone() }).unwrap_or_default()
    }

    /// Answers 401/407 challenges with these credentials (the Playwright context's `httpCredentials`): only a challenge
    /// is answered, nothing is sent ahead of one.
    pub fn set_http_credentials(&mut self, user: &str, password: &str) -> Result<(), String> {
        if let Ok(mut c) = self.credentials.lock() {
            *c = Some((user.to_string(), password.to_string()));
        }
        self.refresh_fetch()
    }

    /// One `Fetch.enable` for what needs it: the proxied URL globs pause their requests, and credentials alone only
    /// watch for auth challenges (a pattern nothing matches pauses nothing).
    fn refresh_fetch(&mut self) -> Result<(), String> {
        let has_credentials = self.credentials.lock().map(|c| c.is_some()).unwrap_or(false);
        let mut globs = self.proxied.lock().map(|p| p.clone()).unwrap_or_default();
        globs.extend(self.stubs.lock().map(|s| s.iter().map(|s| s.pattern.clone()).collect::<Vec<_>>()).unwrap_or_default());
        if globs.is_empty() && !has_credentials {
            self.page_call("Fetch.disable", json!({}))?;
            return Ok(());
        }
        // Chromium raises auth challenges only for requests it has paused, so credentials pause every request and the
        // handler lets the ones that are not proxied or stubbed straight through.
        let patterns: Vec<Value> = if has_credentials { vec![json!({ "urlPattern": "*", "requestStage": "Request" })] } else { globs.iter().map(|p| json!({ "urlPattern": p, "requestStage": "Request" })).collect() };
        self.page_call("Fetch.enable", json!({ "patterns": patterns, "handleAuthRequests": has_credentials }))?;
        let routes = std::sync::Arc::new(Routes { credentials: std::sync::Arc::clone(&self.credentials), proxied: std::sync::Arc::clone(&self.proxied), stubs: std::sync::Arc::clone(&self.stubs), hits: std::sync::Arc::clone(&self.stub_hits) });
        self.cdp.on_paused(std::sync::Arc::new(move |event, fire| answer_fetch(&routes, event, fire)));
        Ok(())
    }

    /// Tints the matching key of the harness's own on-screen mock keyboard (a no-op when it is not mounted).
    pub fn key_tint(&mut self, ch: char) {
        let key = match ch {
            ' ' | '\t' => "space".to_string(),
            '\n' => "enter".to_string(),
            c => c.to_lowercase().to_string(),
        };
        self.mark("keyTint", json!({ "key": key }));
    }

    /// A tap marker at (`x`, `y`) in the app frame's own viewport (the harness's `device-frame-app` frame, else the page),
    /// for interactions a script dispatched itself. Best effort, like every marker.
    pub fn indicate_app_tap(&mut self, x: f64, y: f64) {
        if self.indicators_off {
            return;
        }
        let args = json!({
            "x": x, "y": y, "n": self.step,
            "color": self.indicator_color.clone().unwrap_or_else(|| "#2f7dff".to_string()),
            "max": 5, "lingerMs": 1800, "fadeMs": 400, "durationMs": 450,
        });
        let frame = self.find_frame_named("device-frame-app", Duration::from_secs(10)).ok().flatten().map(|f| f.id);
        for expression in marker_calls("tap", &args) {
            let drawn = match &frame {
                Some(id) => self.eval_in_frame(id, &expression),
                None => self.eval_page(&expression),
            };
            if drawn == Ok(json!(true)) {
                break;
            }
        }
    }

    /// Sets what the next commands draw on the page: a colour, the default (`None`), or nothing (`off`).
    pub fn set_indicator(&mut self, color: Option<String>, off: bool) {
        self.indicator_color = color;
        self.indicators_off = off;
    }

    /// Numbers the markers drawn from now on with `n` (the input log's action id), so a frame says which step it shows.
    pub fn set_step(&mut self, n: u64) {
        self.step = n;
        self.marked = None;
    }

    /// When the current action's first marker was drawn, if it drew one.
    pub fn marked_at(&self) -> Option<std::time::Instant> {
        self.marked
    }

    /// One of the cosmetic on-page markers (`tap`, `drag`, `stroke`, `keyTint`), in the viewport's own pixels, so a
    /// screenshot or recording shows where an interaction landed. Best effort: a page mid-navigation must never fail the
    /// real interaction.
    fn mark(&mut self, kind: &str, mut args: Value) {
        if self.indicators_off {
            return;
        }
        let default = match kind {
            "drag" => "#f97316",
            "stroke" => "#a855f7",
            "keyTint" => "#22c55e",
            _ => "#2f7dff",
        };
        args["color"] = json!(self.indicator_color.clone().unwrap_or_else(|| default.to_string()));
        args["n"] = json!(self.step);
        for (k, v) in [("max", json!(5)), ("lingerMs", json!(1800)), ("fadeMs", json!(400)), ("durationMs", json!(450))] {
            if args[k].is_null() {
                args[k] = v;
            }
        }
        for expression in marker_calls(kind, &args) {
            if self.eval_page(&expression) == Ok(json!(true)) {
                self.marked.get_or_insert_with(std::time::Instant::now);
                break;
            }
        }
    }

    /// Answers an open file chooser with the armed files, if any.
    fn answer_chooser(&mut self) {
        let (Some(node), Some(files)) = (self.open_chooser, self.armed_files.clone()) else { return };
        if self.page_call("DOM.setFileInputFiles", json!({ "files": files, "backendNodeId": node })).is_ok() {
            self.open_chooser = None;
            self.armed_files = None;
            self.delivered = files;
        }
    }

    /// Waits up to `ms` for a file chooser to open and answers it when files are armed (a tap that opens the picker).
    pub fn settle_file_chooser(&mut self, ms: u64) {
        if self.armed_files.is_none() {
            return;
        }
        let deadline = std::time::Instant::now() + Duration::from_millis(ms);
        while std::time::Instant::now() < deadline && self.armed_files.is_some() {
            self.track_contexts();
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Arms `paths` for the next file chooser, answers one that is already open, or clears the arming. Returns what is
    /// armed and what was delivered.
    pub fn upload(&mut self, paths: &[String], clear: bool) -> Result<(Vec<String>, Vec<String>), String> {
        self.page_call("Page.setInterceptFileChooserDialog", json!({ "enabled": !(clear || paths.is_empty()) }))?;
        if clear || paths.is_empty() {
            self.armed_files = None;
            return Ok((Vec::new(), Vec::new()));
        }
        for p in paths {
            if !std::path::Path::new(p).exists() {
                return Err(format!("upload file not found: {p}"));
            }
        }
        self.track_contexts();
        self.delivered.clear();
        self.armed_files = Some(paths.to_vec());
        self.answer_chooser();
        if self.armed_files.is_none() {
            return Ok((Vec::new(), std::mem::take(&mut self.delivered)));
        }
        Ok((paths.to_vec(), Vec::new()))
    }

    /// Every frame of the page (the main frame first, then depth first), with its parent.
    pub fn frames(&mut self) -> Result<Vec<Frame>, String> {
        fn walk(node: &Value, parent: Option<&str>, out: &mut Vec<Frame>) {
            let f = &node["frame"];
            let id = f["id"].as_str().unwrap_or("").to_string();
            out.push(Frame { id: id.clone(), parent: parent.map(str::to_string), url: f["url"].as_str().unwrap_or("").to_string(), name: f["name"].as_str().unwrap_or("").to_string() });
            for child in node["childFrames"].as_array().into_iter().flatten() {
                walk(child, Some(&id), out);
            }
        }
        let tree = self.page_call("Page.getFrameTree", json!({}))?;
        let mut out = Vec::new();
        walk(&tree["frameTree"], None, &mut out);
        Ok(out)
    }

    /// Evaluates an expression in the main world of one frame, as `eval` does for the main frame.
    pub fn eval_in_frame(&mut self, frame_id: &str, expression: &str) -> Result<Value, String> {
        self.track_contexts();
        let context = *self.contexts.get(frame_id).ok_or_else(|| format!("no execution context for frame {frame_id}"))?;
        let reply = self.page_call("Runtime.evaluate", json!({ "expression": expression, "contextId": context, "returnByValue": true, "awaitPromise": true }))?;
        if let Some(ex) = reply.get("exceptionDetails") {
            return Err(ex["exception"]["description"].as_str().or_else(|| ex["text"].as_str()).unwrap_or("evaluation failed").to_string());
        }
        Ok(reply["result"].get("value").cloned().unwrap_or(Value::Null))
    }

    /// The frame named `name`, waiting up to `timeout` for it to appear (the harness names its app frame
    /// `device-frame-app`).
    pub fn find_frame_named(&mut self, name: &str, timeout: Duration) -> Result<Option<Frame>, String> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if let Some(f) = self.frames()?.into_iter().find(|f| f.name == name) {
                return Ok(Some(f));
            }
            if std::time::Instant::now() >= deadline {
                return Ok(None);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// The first descendant of `root` (depth first, at most `max_depth` levels) whose document holds something editable.
    pub fn find_editable_frame(&mut self, root: &str, max_depth: usize) -> Result<Option<Frame>, String> {
        let frames = self.frames()?;
        fn search(me: &mut Attached, frames: &[Frame], parent: &str, depth: usize, max_depth: usize) -> Option<Frame> {
            for child in frames.iter().filter(|f| f.parent.as_deref() == Some(parent)) {
                let editable = me
                    .eval_in_frame(&child.id, "(() => { try { return !!document.querySelector('[contenteditable=\"true\"], [contenteditable=\"\"], textarea[aria-multiline], textarea'); } catch { return false; } })()")
                    .ok()
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if editable {
                    return Some(child.clone());
                }
                if depth + 1 < max_depth {
                    if let Some(deeper) = search(me, frames, &child.id, depth + 1, max_depth) {
                        return Some(deeper);
                    }
                }
            }
            None
        }
        Ok(search(self, &frames, root, 0, max_depth))
    }

    /// The browser's version string (`Browser.getVersion`'s `product` after the slash, for example `141.0.7390.37`).
    pub fn browser_version(&mut self) -> Result<String, String> {
        let product = self.cdp.call("Browser.getVersion", json!({}))?["product"].as_str().unwrap_or("").to_string();
        Ok(product.rsplit('/').next().unwrap_or("").to_string())
    }

    pub fn bring_to_front(&mut self) -> Result<(), String> {
        self.page_call("Page.bringToFront", json!({}))?;
        Ok(())
    }

    /// Sets one permission for every origin of the page's browser context, the way a user answers the OS prompt
    /// (`granted`, `denied`, or `prompt` for never asked). Set on the browser connection, not `grantPermissions`,
    /// which would deny every permission it does not list.
    pub fn set_permission(&mut self, kind: &str, state: &str) -> Result<(), String> {
        let info = self.cdp.call("Target.getTargetInfo", json!({ "targetId": self.target_id }))?;
        let context = info["targetInfo"]["browserContextId"].as_str().ok_or("permission: the page reports no browser context")?.to_string();
        let params = json!({ "permission": { "name": kind }, "setting": state, "browserContextId": context });
        // The default browser context is addressed by leaving the id out; Chromium does not know it by its own id.
        if self.cdp.call("Browser.setPermission", params).is_err() {
            self.cdp.call("Browser.setPermission", json!({ "permission": { "name": kind }, "setting": state }))?;
        }
        Ok(())
    }

    /// `Performance.getMetrics` of the page, after a forced garbage collection when `gc` is set.
    pub fn memory_metrics(&mut self, gc: bool) -> Result<Vec<Value>, String> {
        if gc {
            self.page_call("HeapProfiler.enable", json!({}))?;
            self.page_call("HeapProfiler.collectGarbage", json!({}))?;
        }
        self.page_call("Performance.enable", json!({}))?;
        let reply = self.page_call("Performance.getMetrics", json!({}))?;
        Ok(reply["metrics"].as_array().cloned().unwrap_or_default())
    }

    /// Writes a V8 heap snapshot of the page to `path` and returns its size in bytes.
    pub fn heap_snapshot(&mut self, path: &std::path::Path) -> Result<usize, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        self.track_contexts();
        self.heap_out = Some((file, 0));
        let result = self.page_call("HeapProfiler.enable", json!({})).and_then(|_| self.page_call("HeapProfiler.takeHeapSnapshot", json!({ "reportProgress": false })));
        self.track_contexts();
        let bytes = self.heap_out.take().map_or(0, |(_, n)| n);
        result.map(|_| bytes)
    }

    /// Starts recording which JavaScript runs (V8 precise coverage); call before navigating to see start-up.
    pub fn coverage_start(&mut self) -> Result<(), String> {
        self.track_contexts();
        self.parsed = Some(Default::default());
        self.page_call("Debugger.enable", json!({}))?;
        self.page_call("Profiler.enable", json!({}))?;
        self.page_call("Profiler.startPreciseCoverage", json!({ "callCount": false, "detailed": true }))?;
        Ok(())
    }

    /// Stops the recording: per script, its url, length, executed characters and the executed `[start, end)` ranges.
    pub fn coverage_stop(&mut self) -> Result<Vec<Value>, String> {
        if self.parsed.is_none() {
            return Err("coverage was not started on this browser".into());
        }
        self.track_contexts();
        let taken = self.page_call("Profiler.takePreciseCoverage", json!({}))?;
        self.page_call("Profiler.stopPreciseCoverage", json!({}))?;
        self.page_call("Profiler.disable", json!({}))?;
        self.page_call("Debugger.disable", json!({}))?;
        self.track_contexts();
        let parsed = self.parsed.take().unwrap_or_default();
        let mut scripts = Vec::new();
        for entry in taken["result"].as_array().into_iter().flatten() {
            let url = entry["url"].as_str().unwrap_or("");
            let length = entry["scriptId"].as_str().and_then(|id| parsed.get(id)).map_or(0, |(_, n)| *n);
            if url.is_empty() || length == 0 {
                continue;
            }
            let ranges = executed_ranges(entry["functions"].as_array().map_or(&[][..], |f| &f[..]), length);
            let executed: usize = ranges.iter().map(|(from, to)| to - from).sum();
            scripts.push(json!({ "url": url, "length": length, "executedBytes": executed, "ranges": ranges.iter().map(|(a, b)| json!([a, b])).collect::<Vec<_>>() }));
        }
        Ok(scripts)
    }

    /// Pins the viewport: size, pixel ratio and the mobile flag (what a context's `viewport` and `isMobile` become).
    pub fn set_metrics(&mut self, width: i64, height: i64, dpr: f64, mobile: bool) -> Result<(), String> {
        self.page_call("Emulation.setDeviceMetricsOverride", json!({ "width": width, "height": height, "deviceScaleFactor": dpr, "mobile": mobile, "screenWidth": width, "screenHeight": height }))?;
        // a browser that cannot resize its window (an attached remote one) still emulates the viewport
        let _ = self.cover_window(width, height);
        Ok(())
    }

    /// Grows the browser window to cover a viewport of `width` x `height` CSS pixels. The compositor scrolls only inside the
    /// real window (a headless one is 500x180), so a touch drag below it moved nothing, whatever the emulated viewport said.
    pub fn cover_window(&mut self, width: i64, height: i64) -> Result<(), String> {
        // the window's own chrome (~140 px) eats into its bounds; a viewport taller than what is left captured tiled
        let (width, height) = (width + WINDOW_CHROME_SLACK, height + WINDOW_CHROME_SLACK);
        let w = self.page_call("Browser.getWindowForTarget", json!({}))?;
        let b = &w["bounds"];
        let (have_w, have_h) = (b["width"].as_i64().unwrap_or(0), b["height"].as_i64().unwrap_or(0));
        if have_w >= width && have_h >= height {
            return Ok(());
        }
        self.page_call("Browser.setWindowBounds", json!({ "windowId": w["windowId"], "bounds": { "width": have_w.max(width), "height": have_h.max(height) } }))?;
        Ok(())
    }

    /// The page's user agent, platform and optional client-hint metadata, with touch points.
    pub fn set_identity(&mut self, user_agent: &str, platform: Option<&str>, metadata: Option<&Value>, touch_points: i64) -> Result<(), String> {
        let mut params = json!({ "userAgent": user_agent });
        if let Some(p) = platform {
            params["platform"] = json!(p);
        }
        if let Some(m) = metadata {
            params["userAgentMetadata"] = m.clone();
        }
        self.page_call("Emulation.setUserAgentOverride", params)?;
        self.page_call("Emulation.setTouchEmulationEnabled", json!({ "enabled": true, "maxTouchPoints": touch_points }))?;
        Ok(())
    }

    pub fn set_safe_area(&mut self, insets: &Value) -> Result<(), String> {
        self.page_call("Emulation.setSafeAreaInsetsOverride", json!({ "insets": insets }))?;
        Ok(())
    }

    /// `prefers-color-scheme`: `light` or `dark`; anything else clears the override.
    pub fn set_color_scheme(&mut self, scheme: &str) -> Result<(), String> {
        let features = if scheme == "light" || scheme == "dark" { json!([{ "name": "prefers-color-scheme", "value": scheme }]) } else { json!([]) };
        self.page_call("Emulation.setEmulatedMedia", json!({ "features": features }))?;
        Ok(())
    }

    /// Context-wide grants and TLS leniency a launched session starts with.
    pub fn grant_clipboard_and_ignore_cert_errors(&mut self) -> Result<(), String> {
        self.cdp.call("Browser.grantPermissions", json!({ "permissions": ["clipboardReadWrite", "clipboardSanitizedWrite"] }))?;
        self.page_call("Security.setIgnoreCertificateErrors", json!({ "ignore": true }))?;
        Ok(())
    }

    /// Makes the page behave as focused whether or not its (headless) window is: `focus` events and `document.hasFocus()`.
    pub fn emulate_focus(&mut self) -> Result<(), String> {
        self.page_call("Emulation.setFocusEmulationEnabled", json!({ "enabled": true }))?;
        Ok(())
    }

    /// Sends one real key press or text to the focused element of the page.
    pub fn insert_text(&mut self, text: &str) -> Result<(), String> {
        self.page_call("Input.insertText", json!({ "text": text }))?;
        Ok(())
    }

    /// An accessibility-style snapshot of the page with `eN` refs (see `snapshot.js` for the supported subset).
    pub fn snapshot(&mut self) -> Result<String, String> {
        match self.eval(SNAPSHOT_JS)? {
            Value::String(s) => Ok(s),
            other => Err(format!("snapshot returned {other}")),
        }
    }

    /// The bounding box (x, y, w, h) in main-frame viewport coordinates and the viewport size (w, h) of the element for
    /// `reference` (an `eN` ref from `snapshot`, else a CSS selector), looked up in the target frame.
    fn bounding_box(&mut self, action: &str, reference: &str) -> Result<([f64; 4], [f64; 2]), String> {
        let quoted = Value::String(reference.to_string());
        let lookup = format!("(() => {{ const r = {quoted}; const el = /^e\\d+$/.test(r) ? (window.__wfRefs || {{}})[r] : document.querySelector(r); return el && el.isConnected ? el : null; }})()");
        let viewport = self.eval_page("({ vw: window.innerWidth, vh: window.innerHeight })")?;
        let vp = [viewport["vw"].as_f64().unwrap_or(0.0), viewport["vh"].as_f64().unwrap_or(0.0)];
        let missing = || format!("{action}: no element resolved for ref \"{reference}\"");
        let rect = match self.target.clone() {
            None => {
                let rect = self.eval_page(&format!("(() => {{ const el = ({lookup}); if (!el) return null; const b = el.getBoundingClientRect(); return {{ x: b.x, y: b.y, w: b.width, h: b.height }}; }})()"))?;
                if rect.is_null() {
                    return Err(missing());
                }
                [rect["x"].as_f64().unwrap_or(0.0), rect["y"].as_f64().unwrap_or(0.0), rect["w"].as_f64().unwrap_or(0.0), rect["h"].as_f64().unwrap_or(0.0)]
            }
            Some(frame) => {
                // An element inside a frame: ask the browser for its quads, which are in the main frame's coordinates.
                self.track_contexts();
                let context = *self.contexts.get(&frame).ok_or_else(|| format!("no execution context for frame {frame}"))?;
                // getContentQuads reads the frame's layout as it stands (none yet for a just-inserted element), so lay it out first
                let laid_out = format!("(() => {{ const el = ({lookup}); el?.getBoundingClientRect(); return el; }})()");
                let handle = self.page_call("Runtime.evaluate", json!({ "expression": laid_out, "contextId": context, "returnByValue": false }))?;
                let Some(object_id) = handle["result"]["objectId"].as_str() else { return Err(missing()) };
                self.page_call("DOM.enable", json!({}))?;
                let quads = self.page_call("DOM.getContentQuads", json!({ "objectId": object_id }))?;
                let Some(q) = quads["quads"].as_array().and_then(|all| all.first()).and_then(Value::as_array).filter(|q| q.len() == 8) else {
                    return Err(format!("{action}: element for ref \"{reference}\" has no bounding box (not visible?)"));
                };
                let xs: Vec<f64> = q.iter().step_by(2).filter_map(Value::as_f64).collect();
                let ys: Vec<f64> = q.iter().skip(1).step_by(2).filter_map(Value::as_f64).collect();
                let (min_x, max_x) = xs.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
                let (min_y, max_y) = ys.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
                [min_x, min_y, max_x - min_x, max_y - min_y]
            }
        };
        if rect[2] <= 0.0 || rect[3] <= 0.0 {
            return Err(format!("{action}: element for ref \"{reference}\" has no bounding box (not visible?)"));
        }
        Ok((rect, vp))
    }

    /// The centre of what is on screen of the element for `reference`.
    fn locate(&mut self, action: &str, reference: &str) -> Result<(f64, f64), String> {
        let ([x, y, w, h], [vw, vh]) = self.bounding_box(action, reference)?;
        let (left, top) = (x.max(0.0), y.max(0.0));
        let (right, bottom) = ((x + w).min(vw), (y + h).min(vh));
        if right <= left || bottom <= top {
            return Err(format!("{action}: \"{reference}\" is off the screen -- swipe it into view first"));
        }
        Ok(((left + right) / 2.0, (top + bottom) / 2.0))
    }

    /// Presses at the centre of the element's box, moves by (dx, dy) in `steps` equal moves (pausing `step_delay_ms`
    /// after each), and releases.
    pub fn drag(&mut self, reference: &str, dx: f64, dy: f64, steps: u32, step_delay_ms: u64) -> Result<(), String> {
        let ([x, y, w, h], _) = self.bounding_box("drag", reference)?;
        let (sx, sy) = (x + w / 2.0, y + h / 2.0);
        self.page_call("Input.dispatchMouseEvent", json!({ "type": "mouseMoved", "x": sx, "y": sy }))?;
        self.page_call("Input.dispatchMouseEvent", json!({ "type": "mousePressed", "x": sx, "y": sy, "button": "left", "buttons": 1, "clickCount": 1 }))?;
        for i in 1..=steps {
            let (mx, my) = (sx + dx * f64::from(i) / f64::from(steps), sy + dy * f64::from(i) / f64::from(steps));
            self.page_call("Input.dispatchMouseEvent", json!({ "type": "mouseMoved", "x": mx, "y": my, "button": "left", "buttons": 1 }))?;
            if step_delay_ms > 0 {
                std::thread::sleep(Duration::from_millis(step_delay_ms));
            }
        }
        self.page_call("Input.dispatchMouseEvent", json!({ "type": "mouseReleased", "x": sx + dx, "y": sy + dy, "button": "left", "clickCount": 1 }))?;
        self.mark("drag", json!({ "startX": sx, "startY": sy, "endX": sx + dx, "endY": sy + dy }));
        Ok(())
    }

    /// Walks the pointer along `points` (fractions of the element's box, or pixels) as one press-move-release, with the
    /// path sampled `samples_per_segment` times per segment (straight, or a Catmull-Rom curve); `human` re-times it as a
    /// hand would draw it. Returns the number of samples drawn and how the moves went.
    #[allow(clippy::too_many_arguments)]
    pub fn stroke(&mut self, reference: &str, points: &[(f64, f64)], pixels: bool, curve: bool, samples_per_segment: u32, step_delay_ms: u64, human: bool, touch: bool) -> Result<(usize, Drawn), String> {
        let ([bx, by, bw, bh], _) = self.bounding_box("stroke", reference)?;
        let path: Vec<(f64, f64)> = points.iter().map(|(x, y)| if pixels { (bx + x, by + y) } else { (bx + x * bw, by + y * bh) }).collect();
        let traced = stroke_samples(&path, curve, samples_per_segment)?;
        let samples = if human { human_stroke_samples(&traced)? } else { traced };
        let frame_ms = if human { FRAME_MS } else { step_delay_ms as f64 };
        let drawn = self.draw_samples(&samples, frame_ms, touch, human)?;
        Ok((samples.len(), drawn))
    }

    /// One press-move-release through `samples` (page pixels), `frame_ms` apart: a finger when `touch`, else the mouse. A
    /// `human` finger keeps the planned pace on a page slower than a frame per move.
    pub fn draw_samples(&mut self, samples: &[(f64, f64)], frame_ms: f64, touch: bool, human: bool) -> Result<Drawn, String> {
        let Some(&(sx, sy)) = samples.first() else { return Err("nothing to draw".into()) };
        let began = std::time::Instant::now();
        // The marker leads the finger (it draws itself on at the same pace) and its line is gone when the finger lifts, so
        // a screenshot of the ink taken right after the stroke shows the ink, not the marker (G2's word judge read the
        // marker's loops as the letter's, run g2-browser-1792028469). Its number stays beside the start a while longer.
        if samples.len() >= 2 {
            let points: Vec<Value> = samples.iter().map(|(x, y)| json!({ "x": x, "y": y })).collect();
            let planned_ms = (samples.len() as f64 - 1.0) * frame_ms + frame_ms / 2.0;
            self.mark("stroke", json!({ "points": points, "durationMs": planned_ms.max(1.0), "lingerMs": 0, "fadeMs": 120 }));
        }
        let pause = |ms: f64| {
            if ms > 0.0 {
                std::thread::sleep(Duration::from_secs_f64(ms / 1000.0));
            }
        };
        let mut drawn = Drawn { ms: 0, moves: samples.len() - 1, sent: 0, max_move_ms: 0 };
        let sent = |drawn: &mut Drawn, at: std::time::Instant| {
            drawn.sent += 1;
            drawn.max_move_ms = drawn.max_move_ms.max(at.elapsed().as_millis() as u64);
        };
        if touch {
            let epoch_ms = || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() * 1000.0);
            let finger = |me: &mut Attached, phase: &str, at: Option<(f64, f64)>, due_ms: f64| -> Result<(), String> {
                let points = match at {
                    Some((x, y)) => json!([{ "x": x, "y": y, "radiusX": 7, "radiusY": 7, "force": 0.6, "id": 1 }]),
                    None => json!([]),
                };
                me.page_call("Input.dispatchTouchEvent", json!({ "type": phase, "touchPoints": points, "timestamp": due_ms / 1000.0 }))?;
                Ok(())
            };
            // the pace runs on a monotonic clock: a wall-clock step must not read as a page that fell behind
            let (start, t0) = (epoch_ms(), std::time::Instant::now());
            let since = || t0.elapsed().as_secs_f64() * 1000.0;
            finger(self, "touchStart", Some((sx, sy)), start)?;
            let (moves, mut index, mut due) = (samples.len() - 1, 0, 0.0);
            while index < moves {
                // moves already past due go as one, the latest, as a touchscreen coalesces them on a busy page (each move
                // blocked 40-60 ms on CI); the last point is always sent
                let lagged = if human { ((since() / frame_ms) as usize).min(moves) } else { 0 };
                index = (index + 1).max(lagged);
                due = index as f64 * frame_ms;
                pause(due - since());
                let at = std::time::Instant::now();
                finger(self, "touchMove", Some(samples[index]), start + due)?;
                sent(&mut drawn, at);
            }
            let lift = due + frame_ms / 2.0;
            pause(lift - since());
            finger(self, "touchEnd", None, start + lift)?;
        } else {
            self.page_call("Input.dispatchMouseEvent", json!({ "type": "mouseMoved", "x": sx, "y": sy }))?;
            self.page_call("Input.dispatchMouseEvent", json!({ "type": "mousePressed", "x": sx, "y": sy, "button": "left", "buttons": 1, "clickCount": 1 }))?;
            let (mut lx, mut ly) = (sx, sy);
            let start = std::time::Instant::now();
            for (index, (x, y)) in samples.iter().skip(1).enumerate() {
                let due = (index as f64 + 1.0) * frame_ms;
                pause(due - start.elapsed().as_secs_f64() * 1000.0);
                let at = std::time::Instant::now();
                self.page_call("Input.dispatchMouseEvent", json!({ "type": "mouseMoved", "x": x, "y": y, "button": "left", "buttons": 1 }))?;
                sent(&mut drawn, at);
                (lx, ly) = (*x, *y);
            }
            self.page_call("Input.dispatchMouseEvent", json!({ "type": "mouseReleased", "x": lx, "y": ly, "button": "left", "clickCount": 1 }))?;
        }
        drawn.ms = began.elapsed().as_millis() as u64;
        Ok(drawn)
    }

    /// A PNG of the whole viewport and its width in CSS pixels. Unlike a clipped capture this does not depend on where the
    /// compositor puts the clip while a screencast runs.
    pub fn viewport_png(&mut self) -> Result<(Vec<u8>, f64), String> {
        let vw = self.eval_page("window.innerWidth")?.as_f64().unwrap_or(0.0);
        let shot = self.page_call("Page.captureScreenshot", json!({ "format": "png" }))?;
        let data = shot["data"].as_str().ok_or("Page.captureScreenshot returned no data")?;
        Ok((crate::BASE64.decode(data).map_err(|e| e.to_string())?, vw))
    }

    /// The box (x, y, w, h) and viewport size of the element for `reference`.
    pub fn element_box(&mut self, action: &str, reference: &str) -> Result<([f64; 4], [f64; 2]), String> {
        self.bounding_box(action, reference)
    }

    /// The `data-active-guide` attribute of the element for `reference`, if it has one.
    pub fn guide_of(&mut self, reference: &str) -> Result<Option<String>, String> {
        self.bounding_box("trace", reference)?;
        let quoted = Value::String(reference.to_string());
        let v = self.eval(&format!("(() => {{ const r = {quoted}; const el = /^e\\d+$/.test(r) ? (window.__wfRefs || {{}})[r] : document.querySelector(r); return el ? el.getAttribute('data-active-guide') : null; }})()"))?;
        Ok(v.as_str().filter(|g| !g.is_empty()).map(str::to_string))
    }

    /// A finger tap: a random point in the middle 60% of the visible part of the element, held 70 to 140 ms.
    /// Returns the point and the hold.
    pub fn tap_touch(&mut self, reference: &str) -> Result<(i64, i64, u64), String> {
        let ([bx, by, bw, bh], [vw, vh]) = self.bounding_box("tap", reference)?;
        let (left, top) = (bx.max(0.0), by.max(0.0));
        let (right, bottom) = ((bx + bw).min(vw), (by + bh).min(vh));
        if right <= left || bottom <= top {
            return Err(format!("tap: \"{reference}\" is off the screen -- swipe it into view first"));
        }
        let (rx, ry) = (random_unit(), random_unit());
        self.tap_point(left + (right - left) * (0.2 + 0.6 * rx), top + (bottom - top) * (0.2 + 0.6 * ry), true)
    }

    /// A tap at a page point: a finger held 70 to 140 ms when `touch`, else a mouse click. Returns the point and the hold.
    pub fn tap_point(&mut self, x: f64, y: f64, touch: bool) -> Result<(i64, i64, u64), String> {
        let vp = self.eval_page("[window.innerWidth, window.innerHeight]")?;
        let (vw, vh) = (vp[0].as_f64().unwrap_or(f64::MAX), vp[1].as_f64().unwrap_or(f64::MAX));
        if !(0.0..vw).contains(&x) || !(0.0..vh).contains(&y) {
            return Err(format!("tap: ({}, {}) is off the {vw}x{vh} page", x.round(), y.round()));
        }
        self.mark("tap", json!({ "x": x, "y": y }));
        let hold = if touch {
            let hold = (70.0 + 70.0 * random_unit()).round() as u64;
            let point = json!([{ "x": x, "y": y, "radiusX": 7, "radiusY": 7, "force": 0.6, "id": 1 }]);
            self.page_call("Input.dispatchTouchEvent", json!({ "type": "touchStart", "touchPoints": point }))?;
            std::thread::sleep(Duration::from_millis(hold));
            self.page_call("Input.dispatchTouchEvent", json!({ "type": "touchEnd", "touchPoints": [] }))?;
            hold
        } else {
            self.page_call("Input.dispatchMouseEvent", json!({ "type": "mouseMoved", "x": x, "y": y }))?;
            for kind in ["mousePressed", "mouseReleased"] {
                self.page_call("Input.dispatchMouseEvent", json!({ "type": kind, "x": x, "y": y, "button": "left", "clickCount": 1 }))?;
            }
            0
        };
        self.settle_file_chooser(600);
        Ok((x.round() as i64, y.round() as i64, hold))
    }

    /// A mouse click at the centre of the visible part of the element; returns the point.
    pub fn tap(&mut self, reference: &str) -> Result<(i64, i64), String> {
        let (x, y) = self.locate("tap", reference)?;
        self.mark("tap", json!({ "x": x, "y": y }));
        self.page_call("Input.dispatchMouseEvent", json!({ "type": "mouseMoved", "x": x, "y": y }))?;
        for kind in ["mousePressed", "mouseReleased"] {
            self.page_call("Input.dispatchMouseEvent", json!({ "type": kind, "x": x, "y": y, "button": "left", "clickCount": 1 }))?;
        }
        self.settle_file_chooser(600);
        Ok((x.round() as i64, y.round() as i64))
    }

    /// A finger swipe of (dx, dy) from the centre of what is on screen of `reference`, or from `from`: down, along the path
    /// one sample per frame, and up. A drag rests before lifting so the content stops; a fling lifts while moving so it
    /// coasts. Without touch (`touch` false) it scrolls with the mouse wheel. Returns the scroller's measured movement.
    /// `duration_ms` sets how long the finger moves (it is otherwise taken from the distance).
    pub fn swipe(&mut self, start: SwipeStart<'_>, dx: f64, dy: f64, fling: bool, touch: bool, duration_ms: Option<u64>) -> Result<Swiped, String> {
        let ((fx, fy), (ex, ey)) = match start {
            SwipeStart::Ref(r) => {
                let at = self.locate("swipe", r).map_err(|e| e.replace("is off the screen -- swipe it into view first", "is not on the screen"))?;
                (at, at)
            }
            SwipeStart::From(x, y) => ((x, y), (x, y)),
            SwipeStart::At { page, frame } => (page, frame),
        };
        self.eval(&format!(
            "(() => {{ const scrolls = (el) => {{ const s = getComputedStyle(el); return (/(auto|scroll)/.test(s.overflowY) && el.scrollHeight > el.clientHeight) || (/(auto|scroll)/.test(s.overflowX) && el.scrollWidth > el.clientWidth); }}; let el = document.elementFromPoint({ex}, {ey}); while (el && !scrolls(el)) el = el.parentElement; window.__wfSwipeScroller = el || document.scrollingElement || document.documentElement; }})()"
        ))?;
        const POSITION: &str = "({ left: window.__wfSwipeScroller.scrollLeft, top: window.__wfSwipeScroller.scrollTop })";
        let position = |me: &mut Attached| -> Result<(f64, f64), String> {
            let v = me.eval(POSITION)?;
            Ok((v["left"].as_f64().unwrap_or(0.0), v["top"].as_f64().unwrap_or(0.0)))
        };
        let before = position(self)?;
        let began = std::time::Instant::now();
        if touch {
            const FRAME_MS: f64 = 1000.0 / 60.0;
            let distance = dx.hypot(dy);
            let ms = match duration_ms {
                Some(d) => d as f64,
                None if fling => (distance / 2.5).clamp(80.0, 160.0),
                None => (distance / 0.9).clamp(250.0, 700.0),
            };
            let frames = ((ms / FRAME_MS).round() as usize).max(3);
            let rest_ms = if fling { 0.0 } else { 120.0 };
            let finger = |me: &mut Attached, phase: &str, at: Option<(f64, f64)>, due_ms: f64| -> Result<(), String> {
                let points = match at {
                    Some((x, y)) if phase == "touchStart" || phase == "touchMove" => json!([{ "x": x, "y": y, "radiusX": 7, "radiusY": 7, "force": 0.6, "id": 1 }]),
                    _ => json!([]),
                };
                me.page_call("Input.dispatchTouchEvent", json!({ "type": phase, "touchPoints": points, "timestamp": due_ms / 1000.0 }))?;
                Ok(())
            };
            let epoch_ms = || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() * 1000.0);
            let mut due = epoch_ms();
            finger(self, "touchStart", Some((fx, fy)), due)?;
            let start = due;
            let mut frame = 0;
            while frame < frames {
                // a drag on a page slower than a frame per move gets fewer moves, as a touchscreen coalesces them (each
                // move blocked ~55 ms on CI); a fling keeps every frame, since stamps 40 ms apart reset Chromium's velocity
                let lagged = if fling { 0 } else { (((epoch_ms() - start) / FRAME_MS) as usize).min(frames) };
                frame = (frame + 1).max(lagged);
                let tau = frame as f64 / frames as f64;
                let progress = if fling { tau * tau } else { 10.0 * tau.powi(3) - 15.0 * tau.powi(4) + 6.0 * tau.powi(5) };
                due = start + frame as f64 * FRAME_MS;
                let wait = due - epoch_ms();
                if wait > 0.0 {
                    std::thread::sleep(Duration::from_secs_f64(wait / 1000.0));
                }
                finger(self, "touchMove", Some((fx + dx * progress, fy + dy * progress)), due)?;
            }
            let lift = due + if rest_ms > 0.0 { rest_ms } else { FRAME_MS / 2.0 };
            let wait = lift - epoch_ms();
            if wait > 0.0 {
                std::thread::sleep(Duration::from_secs_f64(wait / 1000.0));
            }
            finger(self, "touchEnd", None, lift)?;
        } else {
            self.page_call("Input.dispatchMouseEvent", json!({ "type": "mouseMoved", "x": fx, "y": fy }))?;
            self.page_call("Input.dispatchMouseEvent", json!({ "type": "mouseWheel", "x": fx, "y": fy, "deltaX": -dx, "deltaY": -dy }))?;
        }
        let ms = began.elapsed().as_millis() as u64;
        // A fling coasts and a wheel scroll animates: wait for the content to stop.
        let mut after = position(self)?;
        let (mut still, deadline) = (0, std::time::Instant::now() + Duration::from_millis(2500));
        while still < 3 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
            let now = position(self)?;
            still = if now == after { still + 1 } else { 0 };
            after = now;
        }
        self.eval("delete window.__wfSwipeScroller")?;
        self.mark("drag", json!({ "startX": fx, "startY": fy, "endX": fx + dx, "endY": fy + dy }));
        Ok(Swiped { from: (fx.round() as i64, fy.round() as i64), to: ((fx + dx).round() as i64, (fy + dy).round() as i64), ms, scrolled: (after.0 - before.0, after.1 - before.1) })
    }

    /// Replaces the content of an editable element with `text` (focus, select all, insert), optionally pressing Enter.
    pub fn type_text(&mut self, reference: &str, text: &str, submit: bool) -> Result<String, String> {
        let quoted = Value::String(reference.to_string());
        let ready = self.eval(&format!(
            "(() => {{ const r = {quoted}; const el = /^e\\d+$/.test(r) ? (window.__wfRefs || {{}})[r] : document.querySelector(r); if (!el) return 'missing'; if (!(el.isContentEditable || ['INPUT', 'TEXTAREA'].includes(el.tagName))) return 'not-editable'; el.focus(); if (el.select) el.select(); else document.execCommand('selectAll'); return 'ok'; }})()"
        ))?;
        match ready.as_str() {
            Some("ok") => {}
            Some("not-editable") => return Err(format!("type: \"{reference}\" is not an editable field")),
            _ => return Err(format!("type: no element resolved for ref \"{reference}\"")),
        }
        if text.is_empty() {
            self.press_key("Backspace")?;
        } else {
            self.page_call("Input.insertText", json!({ "text": text }))?;
        }
        if submit {
            self.press_key("Enter")?;
        }
        let value = self.eval(&format!(
            "(() => {{ const r = {quoted}; const el = /^e\\d+$/.test(r) ? (window.__wfRefs || {{}})[r] : document.querySelector(r); return el ? (el.value !== undefined ? el.value : el.textContent) : ''; }})()"
        ))?;
        Ok(value.as_str().unwrap_or("").to_string())
    }

    /// Whether focus is inside the element for `reference`, in a focused document.
    pub fn ref_has_focus(&mut self, reference: &str) -> Result<bool, String> {
        let quoted = Value::String(reference.to_string());
        let v = self.eval(&format!(
            "(() => {{ const r = {quoted}; const el = /^e\\d+$/.test(r) ? (window.__wfRefs || {{}})[r] : document.querySelector(r); if (!el) return false; const active = el.ownerDocument.activeElement; return el.ownerDocument.hasFocus() && !!active && (el === active || el.contains(active)); }})()"
        ))?;
        Ok(v.as_bool().unwrap_or(false))
    }

    /// One character through the preset's keyboard: iOS sends `keydown` with the key then the text through the input
    /// method, Android (Gboard) sends `keydown` 229 `Unidentified` then the text, a desktop preset presses real keys.
    pub fn type_char(&mut self, ch: char, platform: &str) -> Result<(), String> {
        if ch == '\n' {
            return self.press_key("Enter");
        }
        let text = ch.to_string();
        if platform == "android" {
            self.page_call("Input.dispatchKeyEvent", json!({ "type": "rawKeyDown", "key": "Unidentified", "windowsVirtualKeyCode": 229 }))?;
            self.page_call("Input.insertText", json!({ "text": text }))?;
            self.page_call("Input.dispatchKeyEvent", json!({ "type": "keyUp", "key": "Unidentified", "windowsVirtualKeyCode": 229 }))?;
            return Ok(());
        }
        let (code, key_code) = char_key(ch);
        let named = json!({ "key": text, "code": code, "windowsVirtualKeyCode": key_code });
        let with = |extra: Value| {
            let mut v = named.clone();
            for (k, val) in extra.as_object().into_iter().flatten() {
                v[k] = val.clone();
            }
            v
        };
        if platform == "ios" {
            self.page_call("Input.dispatchKeyEvent", with(json!({ "type": "rawKeyDown" })))?;
            self.page_call("Input.insertText", json!({ "text": text }))?;
        } else {
            self.page_call("Input.dispatchKeyEvent", with(json!({ "type": "keyDown", "text": text, "unmodifiedText": text })))?;
        }
        self.page_call("Input.dispatchKeyEvent", with(json!({ "type": "keyUp" })))?;
        Ok(())
    }

    /// Whether the page has focus and the focused element takes text: what `press` requires before it sends a key.
    pub fn editable_has_focus(&mut self) -> Result<bool, String> {
        const CHECK: &str = "(() => { const a = document.activeElement; return document.hasFocus() && !!a && (a.isContentEditable || a.tagName === 'TEXTAREA' || a.tagName === 'INPUT'); })()";
        if self.target.is_some() {
            for frame in self.frames()? {
                if self.eval_in_frame(&frame.id, CHECK).ok().and_then(|v| v.as_bool()) == Some(true) {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        Ok(self
            .eval_page("(() => { const a = document.activeElement; return document.hasFocus() && !!a && (a.isContentEditable || a.tagName === 'TEXTAREA' || a.tagName === 'INPUT'); })()")?
            .as_bool()
            .unwrap_or(false))
    }

    /// One key press (down and up) to the focused element; `Control+End` holds Ctrl, as the page's key handlers expect.
    pub fn press_key(&mut self, key: &str) -> Result<(), String> {
        let (key, modifiers) = match key {
            "Control+End" => ("End", 2),
            other => (other, 0),
        };
        let (code, vk, text, command) = match (key, modifiers) {
            ("Enter", 0) => ("Enter", 13, Some("\r"), None),
            ("Backspace", 0) => ("Backspace", 8, None, Some("deleteBackward")),
            ("Tab", 0) => ("Tab", 9, None, None),
            ("Escape", 0) => ("Escape", 27, None, None),
            ("ArrowLeft", 0) => ("ArrowLeft", 37, None, Some("moveLeft")),
            ("ArrowUp", 0) => ("ArrowUp", 38, None, Some("moveUp")),
            ("ArrowRight", 0) => ("ArrowRight", 39, None, Some("moveRight")),
            ("ArrowDown", 0) => ("ArrowDown", 40, None, Some("moveDown")),
            ("End", 0) => ("End", 35, None, Some("moveToEndOfLine")),
            ("Home", 0) => ("Home", 36, None, Some("moveToBeginningOfLine")),
            ("End", 2) => ("End", 35, None, Some("moveToEndOfDocument")),
            _ => return Err(format!("unsupported key {key}")),
        };
        let mut down = json!({ "type": if text.is_some() { "keyDown" } else { "rawKeyDown" }, "key": key, "code": code, "modifiers": modifiers, "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk });
        if let Some(t) = text {
            down["text"] = json!(t);
        }
        if let Some(c) = command {
            down["commands"] = json!([c]);
        }
        self.page_call("Input.dispatchKeyEvent", down)?;
        self.page_call("Input.dispatchKeyEvent", json!({ "type": "keyUp", "key": key, "code": code, "modifiers": modifiers, "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk }))?;
        Ok(())
    }

    /// Detaches from the page; the browser keeps running (it is someone else's).
    pub fn detach(self) {
        let session = self.session.clone();
        let _ = self.cdp.call("Target.detachFromTarget", json!({ "sessionId": session }));
    }
}

/// The path's samples: each segment sampled `per_segment` times, straight or as a Catmull-Rom curve (two points are
/// always straight).
/// One sample per display frame, as a phone's touchscreen reports them.
pub const FRAME_MS: f64 = 1000.0 / 60.0;
const HAND_SPEED_PX_PER_S: f64 = 420.0;

fn minimum_jerk(tau: f64) -> f64 {
    10.0 * tau.powi(3) - 15.0 * tau.powi(4) + 6.0 * tau.powi(5)
}

/// A hand drawing along `path`: one sample per 60 Hz frame, progress following the minimum-jerk profile, at hand speed,
/// with 1-1.5 px of tremor that is gone by the last sample. The first sample is where the finger comes down.
pub fn human_stroke_samples(path: &[(f64, f64)]) -> Result<Vec<(f64, f64)>, String> {
    if path.len() < 2 {
        return Err("a stroke needs at least two points".into());
    }
    let length: f64 = path.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).sum();
    let ms = (length / HAND_SPEED_PX_PER_S * 1000.0).clamp(260.0, 2600.0);
    let frames = ((ms / FRAME_MS).round() as usize).max(2);
    let amplitude = 1.0 + 0.5 * random_unit();
    let phases: Vec<f64> = (0..4).map(|_| random_unit() * 2.0 * std::f64::consts::PI).collect();
    let wave = |hz: f64, phase: f64, s: f64| (2.0 * std::f64::consts::PI * hz * s + phase).sin();
    let point_along = |distance: f64| -> (f64, f64) {
        let mut left = distance;
        for (i, w) in path.windows(2).enumerate() {
            let segment = (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
            if left <= segment || i == path.len() - 2 {
                let t = if segment == 0.0 { 0.0 } else { (left / segment).min(1.0) };
                return (w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t);
            }
            left -= segment;
        }
        path[path.len() - 1]
    };
    let mut samples = vec![path[0]];
    for frame in 1..=frames {
        let at = point_along(length * minimum_jerk(frame as f64 / frames as f64));
        let s = frame as f64 * FRAME_MS / 1000.0;
        let (dx, dy) = if frame == frames {
            (0.0, 0.0)
        } else {
            (amplitude * (0.6 * wave(9.0, phases[0], s) + 0.4 * wave(4.3, phases[1], s)), amplitude * (0.6 * wave(10.0, phases[2], s) + 0.4 * wave(3.7, phases[3], s)))
        };
        samples.push((at.0 + dx, at.1 + dy));
    }
    Ok(samples)
}

/// The key a character is typed with, as a US layout names it: its `code` and virtual key code.
fn char_key(ch: char) -> (String, i64) {
    if ch.is_ascii_alphabetic() {
        return (format!("Key{}", ch.to_ascii_uppercase()), i64::from(ch.to_ascii_uppercase() as u8));
    }
    if ch.is_ascii_digit() {
        return (format!("Digit{ch}"), i64::from(ch as u8));
    }
    if ch == ' ' {
        return ("Space".into(), 32);
    }
    let code = match ch {
        ';' | ':' => 186,
        '=' | '+' => 187,
        ',' | '<' => 188,
        '-' | '_' => 189,
        '.' | '>' => 190,
        '/' | '?' => 191,
        '`' | '~' => 192,
        '[' | '{' => 219,
        '\\' | '|' => 220,
        ']' | '}' => 221,
        '\'' | '"' => 222,
        _ => 0,
    };
    (String::new(), code)
}

/// A thumb's pause before the next key: 150-270 ms, and longer after punctuation and after a new line.
pub fn human_key_gap_ms(previous: char) -> f64 {
    let gap = 150.0 + 120.0 * random_unit();
    if previous == '\n' {
        gap + 400.0 + 300.0 * random_unit()
    } else if ".,;:!?)]}".contains(previous) {
        gap + 200.0 + 200.0 * random_unit()
    } else {
        gap
    }
}

pub fn stroke_samples(points: &[(f64, f64)], curve: bool, per_segment: u32) -> Result<Vec<(f64, f64)>, String> {
    if points.len() < 2 {
        return Err(format!("stroke: a path needs at least 2 points, got {}", points.len()));
    }
    if per_segment < 1 {
        return Err(format!("stroke: samplesPerSegment must be a positive integer, got {per_segment}"));
    }
    if points.iter().any(|(x, y)| !x.is_finite() || !y.is_finite()) {
        return Err("stroke: path point is not finite".into());
    }
    let mut samples = vec![points[0]];
    for i in 0..points.len() - 1 {
        let (p1, p2) = (points[i], points[i + 1]);
        let p0 = if i > 0 { points[i - 1] } else { p1 };
        let p3 = points.get(i + 2).copied().unwrap_or(p2);
        for step in 1..=per_segment {
            let t = f64::from(step) / f64::from(per_segment);
            samples.push(if curve && points.len() > 2 {
                let (t2, t3) = (t * t, t * t * t);
                let axis = |a: f64, b: f64, c: f64, d: f64| 0.5 * (2.0 * b + (c - a) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3);
                (axis(p0.0, p1.0, p2.0, p3.0), axis(p0.1, p1.1, p2.1, p3.1))
            } else {
                (p1.0 + (p2.0 - p1.0) * t, p1.1 + (p2.1 - p1.1) * t)
            });
        }
    }
    Ok(samples)
}

/// The deterministic part of the stroke transform: reverse, then scale about the path centre, then offset.
/// The distortions a caller asks of a path, in the order that matters: truncate (keep the first fraction by length),
/// reverse, scale about the bounding box's centre, offset, then wobble (a seeded jitter that leaves the endpoints put).
#[derive(Default)]
pub struct PathTransform {
    pub truncate: Option<f64>,
    pub reverse: bool,
    pub scale: Option<f64>,
    pub offset: Option<(f64, f64)>,
    pub wobble: Option<f64>,
    pub wobble_seed: Option<u32>,
}

fn truncate_by_length(points: &[(f64, f64)], fraction: f64) -> Vec<(f64, f64)> {
    if points.len() < 2 {
        return points.to_vec();
    }
    let total: f64 = points.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).sum();
    if total == 0.0 {
        return points.to_vec();
    }
    let target = total * fraction;
    let mut kept = vec![points[0]];
    let mut walked = 0.0;
    for w in points.windows(2) {
        let span = (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
        if walked + span < target {
            kept.push(w[1]);
            walked += span;
            continue;
        }
        let t = if span == 0.0 { 0.0 } else { (target - walked) / span };
        kept.push((w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t));
        break;
    }
    // A single point is not a path; keep the next one so the pointer still moves.
    if kept.len() < 2 {
        kept.push(points[1]);
    }
    kept
}

/// A small deterministic PRNG, so a "shaky" stroke is the SAME shaky stroke.
fn mulberry32(seed: u32) -> impl FnMut() -> f64 {
    let mut a = seed;
    move || {
        a = a.wrapping_add(0x6d2b_79f5);
        let mut t = a;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        f64::from(t ^ (t >> 14)) / 4_294_967_296.0
    }
}

pub fn transform_path(path: Vec<(f64, f64)>, transform: &PathTransform) -> Result<Vec<(f64, f64)>, String> {
    let mut path = path;
    if let Some(f) = transform.truncate {
        if f.is_nan() || f <= 0.0 || f > 1.0 {
            return Err(format!("stroke: truncate must be in (0, 1], got {f}"));
        }
        path = truncate_by_length(&path, f);
    }
    if transform.reverse {
        path.reverse();
    }
    if let Some(s) = transform.scale {
        if !s.is_finite() || s == 0.0 {
            return Err(format!("stroke: scale must be a non-zero number, got {s}"));
        }
        let (min_x, max_x) = path.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), (x, _)| (lo.min(*x), hi.max(*x)));
        let (min_y, max_y) = path.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), (_, y)| (lo.min(*y), hi.max(*y)));
        let (cx, cy) = ((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);
        path = path.into_iter().map(|(x, y)| (cx + (x - cx) * s, cy + (y - cy) * s)).collect();
    }
    if let Some((ox, oy)) = transform.offset {
        path = path.into_iter().map(|(x, y)| (x + ox, y + oy)).collect();
    }
    if let Some(w) = transform.wobble.filter(|w| *w != 0.0) {
        let mut random = mulberry32(transform.wobble_seed.unwrap_or(1));
        let last = path.len() - 1;
        path = path
            .into_iter()
            .enumerate()
            .map(|(i, (x, y))| {
                // The endpoints stay put: a wobbly stroke should still start and end where it was aimed.
                if i == 0 || i == last {
                    (x, y)
                } else {
                    (x + (random() * 2.0 - 1.0) * w, y + (random() * 2.0 - 1.0) * w)
                }
            })
            .collect();
    }
    Ok(path)
}

/// A console event as one line, `'<level>: <message> (at <frames>)'`: printf-style substitutions applied to the first argument,
/// the calling functions from the stack (innermost first), and an uncaught error as `error: Uncaught Error: <message>`.
pub fn console_text(event: &Value) -> (&'static str, String) {
    let p = &event["params"];
    if event["method"] == "Runtime.exceptionThrown" {
        let d = &p["exceptionDetails"];
        let head = d["text"].as_str().unwrap_or("Uncaught");
        let what = d["exception"]["description"].as_str().and_then(|t| t.lines().next()).or_else(|| d["exception"]["value"].as_str()).unwrap_or("");
        let message = if what.is_empty() { head.to_string() } else { format!("{head} {what}") };
        return ("error", with_frames(format!("error: {message}"), &d["stackTrace"]));
    }
    let level = match p["type"].as_str().unwrap_or("log") {
        "warning" => "warn",
        "error" | "assert" => "error",
        "debug" => "debug",
        _ => "info",
    };
    let show = |a: &Value| -> String {
        match a["type"].as_str() {
            Some("string") => a["value"].as_str().unwrap_or_default().to_string(),
            Some("undefined") => "undefined".into(),
            _ if a["subtype"] == "null" => "null".into(),
            _ if a["subtype"] == "error" => a["description"].as_str().and_then(|t| t.lines().next()).unwrap_or("Error").to_string(),
            // an object logged as fields (the app's Logger) is its preview, not "Object": the message is the evidence
            _ if a["preview"]["properties"].is_array() && (a["subtype"].is_null() || a["subtype"] == "array") => preview_text(&a["preview"]),
            _ => match &a["value"] {
                Value::Null => a["unserializableValue"].as_str().or_else(|| a["description"].as_str()).unwrap_or("").to_string(),
                Value::String(s) => s.clone(),
                v => v.to_string(),
            },
        }
    };
    let args: Vec<&Value> = p["args"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    let mut parts = Vec::new();
    let mut rest = args.iter().skip(1);
    if let Some(first) = args.first() {
        if first["type"] == "string" {
            let fmt = first["value"].as_str().unwrap_or_default();
            let mut out = String::with_capacity(fmt.len());
            let mut chars = fmt.chars().peekable();
            while let Some(c) = chars.next() {
                match (c, chars.peek().copied()) {
                    ('%', Some('%')) => {
                        chars.next();
                        out.push('%');
                    }
                    ('%', Some(k @ ('s' | 'd' | 'i' | 'f' | 'o' | 'O' | 'c'))) => {
                        chars.next();
                        match rest.next() {
                            Some(a) if k != 'c' => out.push_str(&show(a)),
                            Some(_) => {}
                            None => {
                                out.push('%');
                                out.push(k);
                            }
                        }
                    }
                    _ => out.push(c),
                }
            }
            parts.push(out);
        } else {
            parts.push(show(first));
        }
    }
    parts.extend(rest.map(|a| show(a)));
    (level, with_frames(format!("{level}: {}", parts.join(" ")), &p["stackTrace"]))
}

fn preview_text(preview: &Value) -> String {
    let array = preview["subtype"] == "array";
    let mut fields: Vec<String> = preview["properties"].as_array().into_iter().flatten().map(|f| {
        let value = f["value"].as_str().unwrap_or_default();
        let value = if f["type"] == "string" { Value::from(value).to_string() } else { value.lines().next().unwrap_or_default().to_string() };
        if array { value } else { format!("{}: {value}", f["name"].as_str().unwrap_or_default()) }
    }).collect();
    if preview["overflow"] == true {
        fields.push("…".into());
    }
    if array { format!("[{}]", fields.join(", ")) } else { format!("{{{}}}", fields.join(", ")) }
}

fn with_frames(text: String, stack: &Value) -> String {
    let names: Vec<&str> = stack["callFrames"].as_array().into_iter().flatten().filter_map(|f| f["functionName"].as_str()).filter(|n| !n.is_empty()).take(10).collect();
    if names.is_empty() {
        text
    } else {
        format!("{text} (at {})", names.join(" < "))
    }
}

/// A pseudo-random number in [0, 1) (a hand's tap point and hold need no better).
fn random_unit() -> f64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static STATE: AtomicU64 = AtomicU64::new(0);
    let mut x = STATE.load(Ordering::Relaxed);
    if x == 0 {
        x = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0x9E37_79B9, |d| d.as_nanos() as u64) | 1;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    STATE.store(x, Ordering::Relaxed);
    (x >> 11) as f64 / (1u64 << 53) as f64
}

/// The URL of the harness page that frames `target_url` as a device (`buildDeviceFrameUrl`).
pub fn device_frame_url(base_origin: &str, target_url: &str, preset: &str, orientation: &str, color_scheme: &str, frame: &str) -> String {
    format!(
        "{}/_mcp/device-frame?target={}&device={}&orientation={}&colorScheme={}&frame={}",
        base_origin.trim_end_matches('/'),
        cli_http::form_encode(target_url),
        cli_http::form_encode(preset),
        cli_http::form_encode(orientation),
        cli_http::form_encode(color_scheme),
        cli_http::form_encode(frame)
    )
}

/// The browser window size that fits a device box: margins and the label allowance added, each rounded up to even.
pub fn fitted_shell_size(width: f64, height: f64) -> (i64, i64) {
    let even = |n: f64| ((n / 2.0).ceil() * 2.0) as i64;
    (even(width.ceil() + 24.0), even(height.ceil() + 24.0 + 40.0))
}

/// Offsets V8 ran, as `[start, end)` pairs: a nested range with count 0 carves a hole in its parent.
pub fn executed_ranges(functions: &[Value], length: usize) -> Vec<(usize, usize)> {
    let mut covered = vec![false; length];
    for function in functions {
        for range in function["ranges"].as_array().into_iter().flatten() {
            let start = (range["startOffset"].as_u64().unwrap_or(0) as usize).min(length);
            let end = (range["endOffset"].as_u64().unwrap_or(0) as usize).min(length);
            let on = range["count"].as_u64().unwrap_or(0) > 0;
            if start < end {
                covered[start..end].fill(on);
            }
        }
    }
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for i in 0..=length {
        let on = covered.get(i).copied().unwrap_or(false);
        match (on, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// The EWMH `_NET_WM_STATE` of the active window, read from the X server with `xprop`, independent of what CDP says the
/// window state is: the state string and whether it holds `_NET_WM_STATE_FULLSCREEN`. `not found` is a successful read of
/// an absent atom; only an unreachable X server is an error.
fn read_ewmh_state() -> Result<(String, bool), String> {
    if std::env::var_os("DISPLAY").is_none_or(|d| d.is_empty()) && std::env::var_os("WAYLAND_DISPLAY").is_none_or(|d| d.is_empty()) {
        return Err("no X display (DISPLAY/WAYLAND_DISPLAY unset)".to_string());
    }
    let run = |args: &[&str]| -> Result<String, String> {
        let out = std::process::Command::new("xprop")
            .args(args)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() { Ok(String::from_utf8_lossy(&out.stdout).into_owned()) } else { Err(format!("xprop exited {}", out.status)) }
    };
    let active = run(&["-root", "_NET_ACTIVE_WINDOW"]).map_err(|e| format!("cannot read EWMH state: {e}"))?;
    let xid = active
        .match_indices("0x")
        .find_map(|(i, _)| {
            let hex: String = active[i + 2..].chars().take_while(|c| c.is_ascii_hexdigit()).collect();
            (!hex.is_empty()).then(|| format!("0x{hex}"))
        })
        .ok_or("_NET_ACTIVE_WINDOW reports no window id")?;
    let state = run(&["-id", &xid, "_NET_WM_STATE"]).map_or("_NET_WM_STATE: not found".to_string(), |s| s.trim().to_string());
    let has_fullscreen = state.contains("_NET_WM_STATE_FULLSCREEN");
    Ok((state, has_fullscreen))
}

#[cfg(test)]
mod tests {
    use super::console_text;
    use serde_json::json;

    #[test]
    fn given_console_events_when_formatted_then_each_is_one_level_prefixed_line_with_its_callers() {
        let warn = json!({ "method": "Runtime.consoleAPICalled", "params": { "type": "warning", "args": [{ "type": "string", "value": "Warning: %s is %d%%%c done" }, { "type": "string", "value": "it" }, { "type": "number", "value": 5 }, { "type": "string", "value": "color:red" }, { "type": "object", "subtype": "null", "value": null }], "stackTrace": { "callFrames": [{ "functionName": "" }, { "functionName": "render" }, { "functionName": "App" }] } } });
        assert_eq!(console_text(&warn), ("warn", "warn: Warning: it is 5% done null (at render < App)".into()));
        let log = json!({ "method": "Runtime.consoleAPICalled", "params": { "type": "log", "args": [{ "type": "object", "description": "Object" }, { "type": "undefined" }] } });
        assert_eq!(console_text(&log), ("info", "info: Object undefined".into()));
        let fields = json!({ "method": "Runtime.consoleAPICalled", "params": { "type": "error", "args": [{ "type": "string", "value": "app.crash" }, { "type": "object", "description": "Object", "preview": { "type": "object", "overflow": true, "properties": [{ "name": "message", "type": "string", "value": "boom" }, { "name": "fatal", "type": "boolean", "value": "true" }, { "name": "tags", "type": "object", "subtype": "array", "value": "Array(2)" }] } }, { "type": "object", "description": "Array(2)", "preview": { "type": "object", "subtype": "array", "overflow": false, "properties": [{ "name": "0", "type": "number", "value": "1" }, { "name": "1", "type": "string", "value": "b" }] } }] } });
        assert_eq!(console_text(&fields), ("error", "error: app.crash {message: \"boom\", fatal: true, tags: Array(2), …} [1, \"b\"]".into()));
        let others = json!({ "method": "Runtime.consoleAPICalled", "params": { "type": "warning", "args": [{ "type": "object", "subtype": "regexp", "description": "/ab+c/gi", "preview": { "type": "object", "subtype": "regexp", "properties": [{ "name": "lastIndex", "type": "number", "value": "0" }] } }, { "type": "object", "subtype": "map", "description": "Map(2)", "preview": { "type": "object", "subtype": "map", "properties": [] } }, { "type": "object", "description": "Object", "preview": { "type": "object", "properties": [{ "name": "word", "type": "string", "value": "நன்றி \"x\"" }, { "name": "error", "type": "object", "subtype": "error", "value": "Error: boom\n    at f (a.js:1:1)" }] } }] } });
        assert_eq!(console_text(&others), ("warn", "warn: /ab+c/gi Map(2) {word: \"நன்றி \\\"x\\\"\", error: Error: boom}".into()));
        let thrown = json!({ "method": "Runtime.exceptionThrown", "params": { "exceptionDetails": { "text": "Uncaught", "exception": { "description": "Error: boom\n    at f (a.js:1:1)" }, "stackTrace": { "callFrames": [{ "functionName": "f" }] } } } });
        assert_eq!(console_text(&thrown), ("error", "error: Uncaught Error: boom (at f)".into()));
    }
}
