//! The browser driver: one named session of the browser daemon, driven in process over its `POST /call`. Selectors resolve
//! in read-only page JS (`browser_find.js`): the app page first, then its same-origin frames (the editor's web view), with
//! bounds clipped to the screen and to every clipping ancestor. Tap units are the app screen's CSS px.
use harness_core::device::{Device, Element, Key, Level, Logpoint, Part, Rect, Sel};
use serde_json::{json, Map, Value};
use std::cell::{Cell, OnceCell, RefCell};
use std::time::{Duration, Instant};

const FIND: &str = include_str!("browser_find.js");

/// The longest one page observer waits before the run's stop flag is read again.
const WAIT_SLICE_MS: u64 = 1000;

/// The soonest a page change brings a condition's next check, after the last one: the interval's own minimum.
const CHANGE_FLOOR_MS: u64 = 50;

/// The page finder called with `spec`: `{hit, count, els}`, or `{tree}` for `mode: 'tree'`.
fn finder(spec: &Value) -> String {
    FIND.trim().trim_end_matches(';').replace("WF_FIND_SPEC", &spec.to_string())
}

/// The page finder as a function of its spec.
fn find_fn() -> &'static str {
    FIND.trim().trim_end_matches(';').trim_end_matches("(WF_FIND_SPEC)")
}

/// The app's document and every same-origin frame inside it, as `docs()`.
const DOCS: &str = "const docs=()=>{const out=[document];const walk=(w)=>{for(const f of w.document.querySelectorAll('iframe')){try{const c=f.contentDocument;if(c){out.push(c);walk(f.contentWindow)}}catch(e){}}};walk(window);return out};";

/// The device frame's drawn keyboard (`__shell`), with its rectangle on the app's screen; null without a device frame.
const KEYBOARD: &str = "(() => { const st = window.__shell && typeof window.__shell.getStatus === 'function' ? window.__shell.getStatus() : null; if (!st) return null; const f = document.querySelector('iframe[name=\"device-frame-app\"]'); const k = document.querySelector('.mock-keyboard'); let bounds = null; if (st.keyboardVisible && k && f) { const fr = f.getBoundingClientRect(); const s = f.offsetWidth ? fr.width / f.offsetWidth : 1; const o = [fr.left + f.clientLeft * s, fr.top + f.clientTop * s]; const r = k.getBoundingClientRect(); const [left, top, right, bottom] = [r.left, r.top, r.right, r.bottom].map((v, i) => Math.round((v - o[i % 2]) / s)); bounds = { left, top, right, bottom }; } return { visible: !!st.keyboardVisible, bounds }; })()";

/// Without a device frame a phone raises its keyboard for the focused editable element, one inside a web view too.
const FOCUSED_EDITABLE: &str = "(()=>{let a=document.activeElement;while(a&&a.tagName==='IFRAME'){try{a=a.contentDocument.activeElement}catch{return true}}return !!a&&(a.isContentEditable||/^(INPUT|TEXTAREA)$/.test(a.tagName))})()";

const MICROPHONE: &str = "(globalThis.__mcpFakeMicState ? globalThis.__mcpFakeMicState() : null)";

const SUMMARY: &str = "(()=>{const ids=[...document.querySelectorAll('[data-testid]')].filter(e=>{const r=e.getBoundingClientRect();return r.width>0&&r.height>0}).map(e=>e.getAttribute('data-testid'));return {url:location.href,title:document.title,testids:[...new Set(ids)].slice(0,60),text:((document.body&&document.body.innerText)||'').slice(0,400)}})()";

/// One session of the browser daemon: every call is `POST /call {method, args: {name: <session>, ...}}`.
pub(crate) struct DaemonClient {
    pub(crate) session: String,
    /// Whether a missing daemon is started (a test's own session) or is an error (a CLI verb attaching to one).
    start: bool,
    url: OnceCell<String>,
}

impl DaemonClient {
    pub(crate) fn new(session: String, start: bool) -> Self {
        DaemonClient { session, start, url: OnceCell::new() }
    }

    fn url(&self) -> Result<&str, String> {
        if self.url.get().is_none() {
            let _ = self.url.set(crate::browser::device_daemon(self.start)?);
        }
        Ok(self.url.get().map(String::as_str).unwrap_or_default())
    }

    pub(crate) fn call(&self, method: &str, args: Value) -> Result<Value, String> {
        let mut args = match args {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        args.insert("name".into(), json!(self.session));
        let url = self.url()?.to_string();
        let started = Instant::now();
        let out = crate::browser::call_at(&url, method, args);
        harness_core::device::log_timing(method, started.elapsed());
        out
    }

    /// A page expression's value: `outer` in the app's frame, else in the device frame's own page.
    fn eval(&self, app: bool, expression: &str) -> Result<Value, String> {
        let r = self.call(if app { "outerEval" } else { "shellEval" }, json!({ "expression": expression }))?;
        let text = r["result"].as_str().unwrap_or("null");
        serde_json::from_str(text).map_err(|e| format!("{expression:.40}: unexpected value ({e}): {text:.200}"))
    }
}

pub(crate) struct BrowserDevice {
    client: DaemonClient,
    /// A test's own session: it opens it on the first open_link and closes it at the end. An attached one is left as it is.
    owned: bool,
    opened: Cell<bool>,
    /// Where a recording asked for before the session exists goes: the session records from its open.
    video: RefCell<Option<String>>,
    /// Logpoints and stubs declared before the session exists, armed as soon as it does (before the first page loads).
    points: RefCell<Vec<Logpoint>>,
    stubs: RefCell<Vec<Value>>,
    /// `{width, height, dpr}`, read once: the preset does not change under a run.
    screen: RefCell<Option<Value>>,
}

/// A selector as the page's finder takes it: each pattern as a RegExp source and flags.
fn spec(sel: &Sel, mode: &str) -> Value {
    let re = |p: &Option<harness_core::device::sel::Pattern>| p.as_ref().map(|p| json!([p.js().0, p.js().1]));
    json!({ "id": re(&sel.id), "text": re(&sel.text), "css": sel.css, "mode": mode })
}

fn rect(b: &Value) -> Option<Rect> {
    let n = |k: &str| b[k].as_i64();
    Some(Rect { left: n("left")?, top: n("top")?, right: n("right")?, bottom: n("bottom")? })
}

fn element(hit: &Value) -> Option<Element> {
    let bounds = rect(&hit["bounds"])?;
    Some(Element { bounds, full: rect(&hit["full"]).unwrap_or(bounds), text: hit["text"].as_str().map(str::to_string), frame: if hit["frame"] == "editor" { "editor" } else { "app" } })
}

/// A point in [0, 1) for a tap's jitter.
fn jitter() -> f64 {
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

/// A page between documents (a navigation, a reload) has no context to look in: nothing is on screen then.
fn between_documents(e: &str) -> bool {
    ["no execution context", "Execution context was destroyed", "Cannot find context", "Inspected target navigated"].iter().any(|m| e.contains(m))
}

fn absolute(path: &str) -> String {
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        return path.to_string();
    }
    std::env::current_dir().map(|d| d.join(p).display().to_string()).unwrap_or_else(|_| path.to_string())
}

fn dom_key(key: Key) -> &'static str {
    match key {
        Key::Enter => "Enter",
        Key::Backspace => "Backspace",
        Key::Tab => "Tab",
        Key::Left => "ArrowLeft",
        Key::Right => "ArrowRight",
        Key::Up => "ArrowUp",
        Key::Down => "ArrowDown",
        Key::CtrlEnd => "Control+End",
    }
}

impl BrowserDevice {
    /// A test's session, opened by its first open_link and closed at its end; `owner` marks it with this process.
    pub(crate) fn new(session: String) -> Self {
        BrowserDevice { client: DaemonClient::new(session, true), owned: true, opened: Cell::new(false), video: RefCell::default(), points: RefCell::default(), stubs: RefCell::default(), screen: RefCell::default() }
    }

    /// A session someone else opened (a CLI verb's): it is driven as it is and never closed here.
    pub(crate) fn attach(session: String) -> Self {
        BrowserDevice { client: DaemonClient::new(session, false), owned: false, opened: Cell::new(true), video: RefCell::default(), points: RefCell::default(), stubs: RefCell::default(), screen: RefCell::default() }
    }

    fn debug_set(&self, p: &Logpoint) -> Result<Value, String> {
        let mut args = json!({ "file": p.file, "line": p.line, "defer": p.defer });
        if let Some(c) = &p.condition {
            args["condition"] = json!(c);
        }
        if !p.capture.is_empty() {
            args["capture"] = json!(p.capture.join(","));
        }
        self.client.call("debugSet", args)
    }

    fn ensure_open(&self) -> Result<(), String> {
        if self.opened.get() {
            return Ok(());
        }
        let device = crate::browser::screen_size(None)?;
        // document: the frame's chrome keeps its own rows (a report or gallery is not the app under test)
        let frame = std::env::var("WF_BROWSER_FRAME").unwrap_or_else(|_| "screen".into());
        let mut args = json!({ "devicePreset": device, "orientation": "portrait", "colorScheme": "system", "frame": frame, "owner": std::process::id() });
        if std::env::var("WF_BROWSER_TAILNET").is_ok() {
            args["tailnetProxy"] = json!(true);
        }
        if let Some(v) = self.video.borrow().as_deref() {
            args["video"] = json!(true);
            args["videoPath"] = json!(absolute(v));
        }
        self.client.call("open", args)?;
        self.opened.set(true);
        let points: Vec<Logpoint> = self.points.borrow_mut().drain(..).collect();
        for p in &points {
            self.debug_set(p)?;
        }
        let pending: Vec<Value> = self.stubs.borrow_mut().drain(..).collect();
        for spec in pending {
            self.send_stub(&spec)?;
        }
        Ok(())
    }

    /// A stub (`commands::stub_spec`) as the daemon's networkStub takes it; a relative `body_file` is made absolute, as the
    /// daemon reads it from its own working directory.
    fn send_stub(&self, spec: &Value) -> Result<Value, String> {
        let mut args = json!({ "pattern": spec["pattern"].as_str().ok_or("stub needs a pattern")?.replace("**", "*") });
        if let Some(m) = spec["method"].as_str() {
            args["method"] = json!(m);
        }
        if let Some(st) = spec["status"].as_i64() {
            args["status"] = json!(st);
        }
        if !spec["json_body"].is_null() {
            args["body"] = json!(spec["json_body"].to_string());
            args["contentType"] = json!("application/json");
        } else if let Some(b) = spec["body"].as_str() {
            args["body"] = json!(b);
        }
        if let Some(f) = spec["body_file"].as_str() {
            args["bodyFile"] = json!(absolute(f));
        }
        if let Some(d) = spec["delay"].as_u64().filter(|d| *d > 0) {
            args["delayMs"] = json!(d);
        }
        if let Some(h) = spec["headers"].as_object() {
            args["headers"] = Value::Object(h.iter().map(|(k, v)| (k.clone(), json!(v.as_str().map_or_else(|| v.to_string(), str::to_string)))).collect());
        }
        self.client.call("networkStub", args)
    }

    fn find(&self, sel: &Sel, mode: &str) -> Result<Value, String> {
        let pick = if mode == "count" { "r.count" } else { "r.hit" };
        match self.client.eval(true, &format!("(() => {{ const r = {}; return {pick}; }})()", finder(&spec(sel, mode)))) {
            Err(e) if between_documents(&e) => Ok(Value::Null),
            other => other,
        }
    }

    /// The observer for a selector: one MutationObserver per reachable document, plus intersection and resize observers on
    /// the matches, re-running the finder (at most every 25 ms) until its visibility is `want` or `ms` pass.
    fn observer(sel: &Sel, want: bool, ms: u64) -> String {
        format!(
            "(()=>{{const find=()=>{};const want={want};const t0=performance.now();const els=new Set();const watched=new Set();const observers=[];\
let fin=false;let timer=0;let later=0;let last=-1e9;\
const done=(ok)=>{{if(fin)return;fin=true;clearTimeout(timer);clearTimeout(later);observers.forEach((o)=>o.disconnect());window.__wfWaitDone(JSON.stringify({{seen:ok,ms:Math.round(performance.now()-t0)}}))}};\
{DOCS}\
const track=(e)=>{{if(els.has(e))return;els.add(e);try{{io.observe(e);ro.observe(e)}}catch(err){{}}}};\
const seen=()=>{{const r=find();r.els.forEach(track);return (r.hit!==null)===want}};\
const check=()=>{{if(fin)return;const gap=performance.now()-last;if(gap<25){{if(!later)later=setTimeout(()=>{{later=0;check()}},25-gap);return}}last=performance.now();if(seen())return done(true);attach()}};\
const io=new IntersectionObserver(()=>check());const ro=new ResizeObserver(()=>check());observers.push(io,ro);\
const attach=()=>{{for(const d of docs()){{if(watched.has(d))continue;watched.add(d);const o=new MutationObserver(()=>check());o.observe(d,{{subtree:true,childList:true,attributes:true,characterData:true}});observers.push(o)}}}};\
last=performance.now();if(seen())return done(true);attach();timer=setTimeout(()=>done(seen()),{ms})}})()",
            finder(&spec(sel, "one"))
        )
    }

    /// The settle in the page: from `from` (`{bounds, full}`, else its own first look), a look every 100 ms until two in a row
    /// agree on the box, at most `ms`; reports `{hit}`, null when nothing was found or the box is still `from`'s.
    fn settler(sel: &Sel, from: &Value, ms: u64) -> String {
        format!(
            "(()=>{{const find=()=>{}.hit;const key=(h)=>[h.bounds,h.full].map((r)=>[r.left,r.top,r.right,r.bottom].join()).join();\
const from={from};let el=from||find();const done=(hit)=>window.__wfWaitDone(JSON.stringify({{hit:hit===from?null:hit}}));\
if(!el)return done(null);const end=performance.now()+{ms};\
const look=()=>{{const next=find();if(!next)return done(el);if(key(next)===key(el))return done(next);el=next;if(performance.now()>=end)return done(el);setTimeout(look,100)}};\
if(performance.now()>=end)return done(el);setTimeout(look,100)}})()",
            finder(&spec(sel, "one"))
        )
    }

    /// Reports `{changed}` on a change in the app's documents, no sooner than `floor` ms in, else `{changed: false}` after
    /// `ms`. A change since the last check began (`look_ms` ago), or in a document not watched until now, counts.
    fn change_watcher(ms: u64, floor: u64, look_ms: u64) -> String {
        format!(
            "(()=>{{const w=window.__wfChanges||(window.__wfChanges={{at:-1e9,docs:new WeakSet(),obs:[],wake:null,idle:0}});clearTimeout(w.idle);\
{DOCS}\
let unknown=false;for(const d of docs()){{if(!w.docs.has(d)){{unknown=true;w.docs.add(d);const o=new MutationObserver(()=>{{w.at=performance.now();if(w.wake)w.wake()}});o.observe(d,{{subtree:true,childList:true,attributes:true,characterData:true}});w.obs.push(o)}}}}\
const t0=performance.now();let timer=0,later=0,hit=false;\
const done=(changed)=>{{w.wake=null;clearTimeout(timer);clearTimeout(later);\
w.idle=setTimeout(()=>{{w.obs.forEach((o)=>o.disconnect());if(window.__wfChanges===w)delete window.__wfChanges}},30000);\
window.__wfWaitDone(JSON.stringify({{changed,ms:Math.round(performance.now()-t0)}}))}};\
const wake=()=>{{w.wake=null;hit=true;const left={floor}-(performance.now()-t0);if(left<=0)return done(true);later=setTimeout(()=>done(true),left)}};\
if(unknown||t0-w.at<={look_ms}+20)wake();else w.wake=wake;timer=setTimeout(()=>done(hit),{ms})}})()"
        )
    }

    /// The preset's screen, `{width, height, dpr}`: its CSS viewport (as oriented) and pixel ratio; a plain page's own.
    fn screen(&self) -> Result<Value, String> {
        if let Some(s) = self.screen.borrow().clone() {
            return Ok(s);
        }
        let list = self.client.call("list", json!({}))?;
        let me = list.as_array().into_iter().flatten().find(|s| s["name"].as_str() == Some(self.client.session.as_str())).cloned().unwrap_or(Value::Null);
        let preset = me["devicePreset"].as_str().filter(|_| me["renderMode"] == "device-frame").and_then(crate::browser::preset);
        let s = match preset {
            Some(p) => {
                let (w, h) = (p["viewport"]["width"].clone(), p["viewport"]["height"].clone());
                let turned = me["orientation"] == "landscape" && p["supportsLandscape"] == true;
                json!({ "width": if turned { h.clone() } else { w.clone() }, "height": if turned { w } else { h }, "dpr": p["dpr"] })
            }
            None => self.client.eval(true, "({ width: innerWidth, height: innerHeight, dpr: devicePixelRatio })")?,
        };
        if !s["width"].is_number() {
            return Err(format!("cannot read the screen size: {s}"));
        }
        *self.screen.borrow_mut() = Some(s.clone());
        Ok(s)
    }

    /// A page without the device frame's shell: a look every 100 ms.
    fn poll_visibility(&self, sel: &Sel, want: bool, end: Instant) -> Result<bool, String> {
        loop {
            if self.get_element(sel)?.is_some() == want {
                return Ok(true);
            }
            harness_core::run_stop::check()?;
            if Instant::now() >= end {
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn keyboard(&self) -> Result<Value, String> {
        let framed = self.client.eval(false, KEYBOARD)?;
        if framed.is_object() {
            return Ok(framed);
        }
        Ok(json!({ "visible": self.client.eval(true, FOCUSED_EDITABLE)? == Value::Bool(true), "bounds": null }))
    }

    fn microphone(&self) -> Result<Value, String> {
        let m = self.client.eval(true, MICROPHONE)?;
        let last = |k: &str| m[k].as_array().and_then(|a| if k == "queued" { a.first() } else { a.last() }).cloned();
        let consumed = m["consumed"].as_array().is_some_and(|a| !a.is_empty());
        Ok(json!({ "input": last("queued").or_else(|| last("consumed")), "consumed": consumed }))
    }
}

impl Device for BrowserDevice {
    fn platform(&self) -> &'static str {
        "browser"
    }

    fn open_link(&self, link: &str) -> Result<(), String> {
        self.ensure_open()?;
        self.client.call("navigate", json!({ "url": link }))?;
        Ok(())
    }

    fn tap_at(&self, x: f64, y: f64) -> Result<(), String> {
        self.client.call("tap", json!({ "x": x, "y": y })).map(|_| ())
    }

    fn tap_element(&self, el: &Element, sel: &Sel) -> Result<(f64, f64), String> {
        let b = el.bounds;
        let at = |lo: i64, hi: i64| lo as f64 + (hi - lo) as f64 * (0.2 + 0.6 * jitter());
        let (x, y) = (at(b.left, b.right), at(b.top, b.bottom));
        self.client.call("tap", json!({ "x": x, "y": y, "selector": sel.show() }))?;
        Ok((x, y))
    }

    fn swipe(&self, from: (f64, f64), to: (f64, f64), duration_ms: u64) -> Result<(), String> {
        self.client.call("swipe", json!({ "from": { "x": from.0, "y": from.1 }, "dx": to.0 - from.0, "dy": to.1 - from.1, "durationMs": duration_ms, "screen": true })).map(|_| ())
    }

    fn input_text(&self, text: &str) -> Result<(), String> {
        self.client.call("type", json!({ "ref": ":focus", "text": text, "atCaret": true, "human": true, "speed": harness_core::device::speed() })).map(|_| ())
    }

    fn press_key(&self, key: Key) -> Result<(), String> {
        self.client.call("press", json!({ "key": dom_key(key) })).map(|_| ())
    }

    fn back(&self) -> Result<(), String> {
        Err("not on browser: a page has no system Back key".into())
    }

    fn draw_strokes_on(&self, _el: &Element, sel: &Sel, source: &str, human: bool) -> Result<Value, String> {
        let hit = self.find(sel, "one")?;
        let path = hit["path"].as_str().ok_or_else(|| format!("draw_strokes_on: {} is not on screen", sel.show()))?;
        let r = self.client.call("trace", json!({ "ref": path, "source": source, "human": human, "outer": hit["frame"] == "app" }))?;
        Ok(json!({ "strokes": r["strokes"], "points": r["samples"] }))
    }

    fn set_microphone_input(&self, path: &str) -> Result<Value, String> {
        let r = self.client.call("audioInject", json!({ "path": absolute(path), "replace": true }))?;
        Ok(json!({ "path": path, "bytes": r["bytes"] }))
    }

    fn run_shell(&self, _cmd: &str) -> Result<(String, i32), String> {
        Err("not on browser: a page has no OS shell".into())
    }

    fn take_screenshot(&self, path: &str) -> Result<(u32, u32), String> {
        let r = self.client.call("screenshot", json!({}))?;
        let src = r["path"].as_str().ok_or("screenshot: the daemon named no file")?;
        if let Some(dir) = std::path::Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()) {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::copy(src, path).map_err(|e| format!("screenshot: cannot copy {src} to {path}: {e}"))?;
        harness_core::device::png_size(path)
    }

    /// Before the test's session exists the recording starts with it, so the video holds the first page.
    fn start_recording(&self, path: &str) -> Result<(), String> {
        if !self.opened.get() {
            *self.video.borrow_mut() = Some(path.to_string());
            return Ok(());
        }
        self.client.call("recordStart", json!({ "path": absolute(path) })).map(|_| ())
    }

    fn stop_recording(&self) -> Result<Vec<String>, String> {
        if !self.opened.get() {
            return Ok(vec![]);
        }
        let r = self.client.call("recordStop", json!({}))?;
        let Some(path) = r["video"]["path"].as_str() else { return Ok(vec![]) };
        let asked = self.video.borrow().clone().filter(|v| absolute(v) == path);
        match asked {
            Some(v) if !std::path::Path::new(&v).exists() => Err(format!("no video was written at {v}")),
            Some(v) => Ok(vec![v]),
            None => Ok(vec![path.to_string()]),
        }
    }

    fn get_element(&self, sel: &Sel) -> Result<Option<Element>, String> {
        if sel.point.is_some() {
            return Err("a point is not an element: look one up by id, text or css".into());
        }
        Ok(element(&self.find(sel, "one")?))
    }

    fn get_element_with(&self, sel: &Sel, also: &Sel, pct: f64) -> Result<(Option<Element>, Option<Option<Element>>), String> {
        if sel.point.is_some() || also.point.is_some() {
            return Err("a point is not an element: look one up by id, text or css".into());
        }
        // the region is looked up only while the element shows less than `pct` percent of its box, as `visible_pct` reckons it
        let both = format!(
            "(() => {{ const find = {}; const area = (r) => Math.max(0, r.right - r.left) * Math.max(0, r.bottom - r.top); const a = find({}).hit; const shown = a && area(a.full) > 0 ? Math.min(100, 100 * area(a.bounds) / area(a.full)) : 0; return [a, a && shown >= {pct} ? false : find({}).hit]; }})()",
            find_fn(),
            spec(sel, "one"),
            spec(also, "one")
        );
        let hits = match self.client.eval(true, &both) {
            // the region is looked up on its own then, as before the looks were one
            Err(e) if between_documents(&e) => json!([null, false]),
            other => other?,
        };
        Ok((element(&hits[0]), (hits[1] != false).then(|| element(&hits[1]))))
    }

    /// The looks run in the page between the daemon's own ticks, so a keyboard a focus raises still moves the box meanwhile.
    fn settled(&self, sel: &Sel, seen: Option<Element>, ms: u64) -> Result<Option<Element>, String> {
        if sel.point.is_some() {
            return Err("a point is not an element: look one up by id, text or css".into());
        }
        let from = seen.as_ref().map_or(Value::Null, |e| json!({ "bounds": e.bounds.json(), "full": e.full.json() }));
        let report = match self.client.call("waitObserve", json!({ "expression": Self::settler(sel, &from, ms), "timeout": ms + 100, "outer": true })) {
            Ok(r) => r,
            Err(e) if e.contains("Shell not loaded") || e.contains("never reported") || between_documents(&e) => return harness_core::device::settle_by_looks(self, sel, seen, ms),
            Err(e) => return Err(e),
        };
        let v: Value = report["result"].as_str().and_then(|t| serde_json::from_str(t).ok()).unwrap_or(Value::Null);
        Ok(element(&v["hit"]).or(seen))
    }

    fn count_matches(&self, sel: &Sel) -> Result<usize, String> {
        Ok(self.find(sel, "count")?.as_u64().unwrap_or(0) as usize)
    }

    fn get_device_state(&self, parts: &[Part]) -> Result<Value, String> {
        let mut out = Map::new();
        for part in parts {
            let v = match part {
                Part::Screen => self.screen()?,
                Part::Keyboard => self.keyboard()?,
                Part::Microphone => self.microphone()?,
            };
            out.insert(part.name().into(), v);
        }
        Ok(Value::Object(out))
    }

    fn get_logs(&self, level: Level) -> Result<Vec<String>, String> {
        if !self.opened.get() {
            return Ok(vec![]);
        }
        let name = match level {
            Level::Debug => "debug",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        };
        let r = self.client.call("consoleLogs", json!({ "level": name }))?;
        Ok(r["lines"].as_array().into_iter().flatten().filter_map(|l| l.as_str().map(str::to_string)).collect())
    }

    fn clear_logs(&self) -> Result<usize, String> {
        if !self.opened.get() {
            return Ok(0);
        }
        Ok(self.client.call("consoleLogs", json!({ "clear": true }))?["count"].as_u64().unwrap_or(0) as usize)
    }

    fn hierarchy(&self) -> Result<Value, String> {
        let tree = self.client.eval(true, &format!("({}).tree", finder(&json!({ "mode": "tree" }))))?;
        Ok(json!({ "platform": "browser", "elements": tree }))
    }

    /// One observer per slice of at most `WAIT_SLICE_MS`, so a stop is seen between slices rather than after the timeout.
    fn wait_visibility(&self, sel: &Sel, want: bool, ms: u64) -> Result<bool, String> {
        let end = Instant::now() + Duration::from_millis(ms);
        let mut slice = ms.min(WAIT_SLICE_MS);
        loop {
            let asked = Instant::now();
            // the daemon pumps its own events while the page script runs, so focus changes keep raising the keyboard
            let report = match self.client.call("waitObserve", json!({ "expression": Self::observer(sel, want, slice), "timeout": slice, "outer": true })) {
                Ok(r) => r,
                Err(e) if e.contains("Shell not loaded") => return self.poll_visibility(sel, want, end),
                Err(e) => return Err(e),
            };
            let v: Value = report["result"].as_str().and_then(|t| serde_json::from_str(t).ok()).unwrap_or(Value::Null);
            if v["seen"].as_bool() == Some(true) {
                return Ok(true);
            }
            // a page that answered "not yet" before its slice ran out is asked again only when the slice would have ended
            let slice_end = asked + Duration::from_millis(slice);
            loop {
                harness_core::run_stop::check()?;
                let left = slice_end.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    break;
                }
                std::thread::sleep(left.min(Duration::from_millis(100)));
            }
            if Instant::now() >= end {
                return Ok(false);
            }
            slice = (end.saturating_duration_since(Instant::now()).as_micros().div_ceil(1000) as u64).min(WAIT_SLICE_MS);
        }
    }

    fn wait_for_change(&self, ms: u64, look_ms: u64) -> Result<bool, String> {
        if !self.opened.get() {
            return Ok(false);
        }
        let started = Instant::now();
        let end = started + Duration::from_millis(ms);
        loop {
            harness_core::run_stop::check()?;
            let left = end.saturating_duration_since(Instant::now()).as_micros().div_ceil(1000) as u64;
            if left == 0 {
                return Ok(true);
            }
            let slice = left.min(WAIT_SLICE_MS);
            let floor = CHANGE_FLOOR_MS.saturating_sub(started.elapsed().as_millis() as u64);
            let watcher = Self::change_watcher(slice, floor, look_ms + started.elapsed().as_millis() as u64);
            let report = match self.client.call("waitObserve", json!({ "expression": watcher, "timeout": slice, "outer": true })) {
                Ok(r) => r,
                // the page went away under its watcher: a new document is a change
                Err(e) if e.contains("never reported") => return Ok(true),
                Err(e) if e.contains("Shell not loaded") || between_documents(&e) => return Ok(false),
                Err(e) => return Err(e),
            };
            let v: Value = report["result"].as_str().and_then(|t| serde_json::from_str(t).ok()).unwrap_or(Value::Null);
            match v["changed"].as_bool() {
                Some(true) => return Ok(true),
                Some(false) => {}
                None => return Ok(false),
            }
        }
    }

    /// A stub declared before the session exists is sent once it does, before the first page.
    fn add_network_stub(&self, spec: &Value) -> Result<Value, String> {
        if !self.opened.get() {
            self.stubs.borrow_mut().push(spec.clone());
            return Ok(json!({ "pattern": spec["pattern"], "pending": true }));
        }
        self.send_stub(spec)
    }

    fn get_network_stub_hits(&self) -> Result<Vec<Value>, String> {
        if !self.opened.get() {
            return Ok(vec![]);
        }
        Ok(self.client.call("networkStubHits", json!({ "clear": false }))?["hits"].as_array().cloned().unwrap_or_default())
    }

    fn get_debug_hits(&self) -> Result<Vec<Value>, String> {
        if !self.opened.get() {
            return Ok(vec![]);
        }
        Ok(self.client.call("debugHits", json!({ "clear": false }))?["hits"].as_array().cloned().unwrap_or_default())
    }

    /// Armed as soon as the session exists, before the first page loads.
    fn arm_logpoints(&self, points: &[Logpoint]) -> Result<(), String> {
        if self.opened.get() {
            return points.iter().try_for_each(|p| self.debug_set(p).map(|_| ()));
        }
        self.points.borrow_mut().extend(points.iter().cloned());
        Ok(())
    }

    fn summary(&self) -> Option<Value> {
        if !self.opened.get() {
            return None;
        }
        self.client.eval(true, SUMMARY).ok().filter(Value::is_object)
    }

    fn shutdown(&self) {
        if self.owned && self.opened.replace(false) {
            let _ = self.client.call("close", json!({}));
        }
    }
}

/// `wf browser`: the platform the run engine and the verb front end name nothing else of.
pub(crate) struct BrowserPlatform;

pub(crate) static PLATFORM: BrowserPlatform = BrowserPlatform;

impl harness_core::platform::Platform for BrowserPlatform {
    fn name(&self) -> &'static str {
        "browser"
    }

    fn usage_options(&self) -> (&'static str, &'static str) {
        (" [--logpoint file:line[:cond]]...", " [--screen-size P] [--frame screen|bezel|document]")
    }

    /// `--screen-size` and `--frame` become the driver's WF_BROWSER_SCREEN_SIZE and WF_BROWSER_FRAME.
    fn check_options(&self, p: &wf_kit::args::Parsed) -> Result<(), String> {
        std::env::set_var("WF_BROWSER_SCREEN_SIZE", crate::browser::screen_size(p.val("screen-size"))?);
        if let Some(f) = p.val("frame") {
            if !["screen", "bezel", "document"].contains(&f) {
                return Err(format!("--frame is screen, bezel or document, got '{f}'"));
            }
            std::env::set_var("WF_BROWSER_FRAME", f);
        }
        Ok(())
    }

    fn begin_run(&self, _: &str, _: i64, _: &wf_kit::args::Parsed) -> Box<dyn harness_core::platform::RunScope> {
        Box::new(harness_core::platform::NoScope)
    }

    fn open_run_device(&self, serial: Option<String>, inputs: String, f: &mut harness_core::run_report::Facts) -> std::rc::Rc<dyn Device> {
        f.device = std::env::var("WF_BROWSER_SCREEN_SIZE").ok();
        f.inputs = Some(std::path::PathBuf::from(inputs));
        std::rc::Rc::new(BrowserDevice::new(serial.unwrap_or_else(|| format!("run-{}", std::process::id()))))
    }

    fn refuse_option(&self, p: &wf_kit::args::Parsed) -> Option<String> {
        p.flag("cold-start").then(|| "--cold-start is for Android: a browser run opens a fresh page anyway (it loads in 1 to 3 s)".into())
    }

    fn before_script(&self, _: &dyn Device, _: &wf_kit::args::Parsed, _: &mut harness_core::run_report::Facts) {}

    fn pick(&self, given: Option<&str>) -> Result<(std::rc::Rc<dyn Device>, String), (i32, String)> {
        let name = browser_session(given)?;
        Ok((std::rc::Rc::new(BrowserDevice::attach(name.clone())), name))
    }
}

/// The browser session a verb drives: `--device`, else the only open one. Exit 2 when several are open, 1 when none is.
pub(crate) fn browser_session(given: Option<&str>) -> Result<String, (i32, String)> {
    if let Some(n) = given {
        return Ok(n.to_string());
    }
    let all = crate::browser::sessions().map_err(|e| (1, e))?;
    let name = |s: &Value| s["name"].as_str().unwrap_or_default().to_string();
    // a running test's session is driven only when --device names it: a verb typed by hand must not race the test
    let free: Vec<String> = all.iter().filter(|s| s["owner"].is_null()).map(name).collect();
    match all.iter().find(|s| !s["owner"].is_null()).filter(|_| free.is_empty()) {
        Some(owned) => Err((1, format!("the open browser session '{}' belongs to a running test (pid {}): pass --device {} to drive it anyway", name(owned), owned["owner"], name(owned)))),
        None => harness_core::device::cli::one(free, "no browser session is open (wf browser start-device --device N)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_selector_when_sent_to_the_page_then_each_pattern_is_a_regexp_and_a_hit_becomes_an_element() {
        let sel = Sel::from_json(&json!({ "id": "Go", "text": "Run (local)" })).unwrap();
        let sent = spec(&sel, "one");
        assert_eq!((sent["id"][1].clone(), sent["css"].clone(), sent["mode"].clone()), (json!("imsu"), Value::Null, json!("one")));
        assert!(sent["text"][0].as_str().unwrap().contains("(?:Run[ \\n]\\(local\\))"), "{sent}");
        let hit = json!({ "bounds": { "left": 80, "top": 0, "right": 120, "bottom": 40 }, "full": { "left": 80, "top": -40, "right": 120, "bottom": 40 }, "text": "Clip", "frame": "editor", "path": ":root" });
        let clipped = Element { bounds: Rect { left: 80, top: 0, right: 120, bottom: 40 }, full: Rect { left: 80, top: -40, right: 120, bottom: 40 }, text: Some("Clip".into()), frame: "editor" };
        assert_eq!(element(&hit), Some(clipped));
        assert_eq!(element(&Value::Null), None);
        assert_eq!((dom_key(Key::Left), dom_key(Key::CtrlEnd)), ("ArrowLeft", "Control+End"));
        assert!((0..100).map(|_| jitter()).all(|j| (0.0..1.0).contains(&j)));
    }
}
