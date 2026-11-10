//! The fast path to the device's UI tree. `uiautomator dump` starts a JVM, waits for the UI to go idle and walks every node on
//! each call (3 to 12 s on the emulator). This talks to openatx/uiautomator2's on-device server (`u2.jar`, MIT, run with
//! `app_process`; one UiAutomation connection that stays up) over HTTP JSON-RPC instead: a selector query answers in
//! tens of milliseconds on the device, and `waitForExists` waits on the device in one round trip.
//! `WF_ANDROID_UIA=dump` keeps the old dump path; `WF_U2_URL` points at an already running server (tests).
use crate::android_input::Node;
use harness_core::device::Sel;
use cli_http::Request;
use serde_json::{json, Value};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const JAR: &[u8] = include_bytes!("../assets/u2.jar");
const DEVICE_JAR: &str = "/data/local/tmp/u2.jar";
const SERVER_PORT: u16 = 9008;

static URL: Mutex<Option<(String, String)>> = Mutex::new(None);

/// Every `wf android ...` call is its own process: the forwarded port is remembered in a file so only the first one pays for
/// `adb forward` and the server check; a call that cannot reach the server forgets it and sets up again.
fn cache_file(serial: &str) -> std::path::PathBuf {
    let safe: String = serial.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    std::env::temp_dir().join(format!("wf-u2-{safe}.url"))
}

pub(crate) fn enabled() -> bool {
    std::env::var("WF_ANDROID_UIA").map_or(true, |v| v != "dump")
}

fn adb(serial: &str, args: &[&str]) -> Option<String> {
    let o = Command::new("adb").arg("-s").arg(serial).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn pong(url: &str) -> bool {
    Request::get(&format!("{url}/ping")).direct().timeout(Duration::from_secs(3)).send().is_ok_and(|r| r.ok() && r.text().contains("pong"))
}

/// The server's base URL for `serial`: forwarded once per process, pushed and started when it is not running.
fn base(serial: &str) -> Result<String, String> {
    base_with(serial, false)
}

fn base_with(serial: &str, restart: bool) -> Result<String, String> {
    if let Ok(url) = std::env::var("WF_U2_URL") {
        if restart {
            let _ = adb(serial, &["shell", "pkill", "-f", "com.wetest.uia2"]);
        }
        return Ok(url);
    }
    if let Some((s, url)) = URL.lock().map_err(|e| e.to_string())?.clone().filter(|_| !restart) {
        if s == serial {
            return Ok(url);
        }
    }
    if let Ok(url) = std::fs::read_to_string(cache_file(serial)).map_err(|_| ()).and_then(|u| if restart { Err(()) } else { Ok(u) }) {
        let url = url.trim().to_string();
        if !url.is_empty() {
            *URL.lock().map_err(|e| e.to_string())? = Some((serial.to_string(), url.clone()));
            return Ok(url);
        }
    }
    let port = adb(serial, &["forward", "tcp:0", &format!("tcp:{SERVER_PORT}")]).and_then(|p| p.parse::<u16>().ok()).ok_or("adb forward to the UI server failed")?;
    let url = format!("http://127.0.0.1:{port}");
    if restart {
        // The server answers /ping but its UiAutomation connection is gone (NullPointerException on every call): kill it first.
        let _ = adb(serial, &["shell", "pkill", "-f", "com.wetest.uia2"]);
        std::thread::sleep(Duration::from_millis(800));
    }
    if restart || !pong(&url) {
        let have = adb(serial, &["shell", "ls", DEVICE_JAR]).is_some_and(|o| o.contains("u2.jar") && !o.contains("No such"));
        if !have {
            let local = std::env::temp_dir().join(format!("wf-u2-{}.jar", std::process::id()));
            std::fs::write(&local, JAR).map_err(|e| format!("cannot write the UI server jar: {e}"))?;
            let pushed = adb(serial, &["push", &local.display().to_string(), DEVICE_JAR]);
            let _ = std::fs::remove_file(&local);
            pushed.ok_or("adb push of the UI server failed")?;
        }
        // Two shells: one frees a UiAutomation connection a stale `uiautomator dump` may hold, the second starts the server detached.
        let _ = adb(serial, &["shell", "pkill", "-f", "uiautomator"]);
        adb(serial, &["shell", &format!("CLASSPATH={DEVICE_JAR} nohup app_process / com.wetest.uia2.Main >/dev/null 2>&1 &")]);
        let end = Instant::now() + Duration::from_secs(20);
        std::thread::sleep(Duration::from_millis(800));
        while !pong(&url) {
            if Instant::now() >= end {
                return Err("the on-device UI server did not answer within 20 s".into());
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    }
    configure(&url);
    let _ = std::fs::write(cache_file(serial), &url);
    *URL.lock().map_err(|e| e.to_string())? = Some((serial.to_string(), url.clone()));
    Ok(url)
}

/// UiAutomator waits for 500 ms without accessibility events before every node lookup, and objInfo makes about 16 of them;
/// scripts wait for what they need with `wait`, so the server skips it (a find took 0.85 s on average on 15 Oct).
fn configure(url: &str) {
    let call = |method: &str, params: Value| {
        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
        Request::post(&format!("{url}/jsonrpc/0")).direct().timeout(Duration::from_secs(5)).json(&body).send().ok().and_then(|r| serde_json::from_str::<Value>(&r.text()).ok())
    };
    match call("getConfigurator", json!([])).map(|v| v["result"].clone()).filter(Value::is_object) {
        Some(current) => {
            if call("setConfigurator", json!([without_idle_wait(&current)])).is_none_or(|v| v.get("error").is_some()) {
                wf_kit::wf_err!("wf: the UI server kept its idle wait (setConfigurator failed)");
            }
        }
        None => wf_kit::wf_err!("wf: the UI server kept its idle wait (getConfigurator failed)"),
    }
}

fn without_idle_wait(current: &Value) -> Value {
    let mut next = current.clone();
    next["waitForIdleTimeout"] = json!(0);
    next
}

fn forget(serial: &str) {
    let _ = std::fs::remove_file(cache_file(serial));
    if let Ok(mut g) = URL.lock() {
        *g = None;
    }
}

/// `rpc`, and once more on a fresh server when the first call failed for a reason other than "no such view".
fn rpc(serial: &str, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
    match rpc_once(serial, method, params.clone(), timeout) {
        Err(e) if !is_not_found(&e) && std::env::var("WF_U2_URL").is_err() => {
            forget(serial);
            base_with(serial, true)?;
            rpc_once(serial, method, params, timeout)
        }
        other => other,
    }
}

fn rpc_once(serial: &str, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
    let mut span = harness_core::spans::begin("inner", &format!("u2 {method}"), None);
    let out = rpc_untimed(serial, method, params, timeout);
    span.outcome = match &out {
        Ok(_) => "ok",
        Err(e) if is_not_found(e) => "miss",
        Err(_) => "error",
    };
    out
}

fn rpc_untimed(serial: &str, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
    let post = |url: &str| Request::post(&format!("{url}/jsonrpc/0")).direct().timeout(timeout).json(&body).send();
    let r = match post(&base(serial)?) {
        Ok(r) => r,
        Err(_) => {
            // The cached forward or the server is gone: set both up again once.
            forget(serial);
            post(&base(serial)?)?
        }
    };
    let v: Value = serde_json::from_str(&r.text()).map_err(|e| format!("UI server: unreadable reply: {e}"))?;
    match v.get("error") {
        Some(e) => Err(format!("UI server: {method}: {}", e["data"].as_str().or(e["message"].as_str()).unwrap_or("error").lines().next().unwrap_or(""))),
        None => Ok(v["result"].clone()),
    }
}

/// The server answers a selector with no match with an error whose message is the selector itself.
fn is_not_found(e: &str) -> bool {
    e.contains("UiSelector[") || e.contains("NotFound")
}

const TEXT_MATCHES: u64 = 0x04;
const CLASS_NAME: u64 = 0x10;
const DESCRIPTION_MATCHES: u64 = 0x0100;
const RESOURCE_ID_MATCHES: u64 = 0x40_0000;
const INSTANCE: u64 = 0x0100_0000;

fn with(mut s: Value, mask: u64, field: &str, value: Value) -> Value {
    s["mask"] = json!(s["mask"].as_u64().unwrap_or(0) | mask);
    s[field] = value;
    s
}

/// The UiSelectors for a selector, the text before the accessibility label. Every given field must hold, and the server's Java
/// engine matches each pattern against the whole value.
pub(crate) fn selectors(sel: &Sel) -> Vec<Value> {
    let mut base = json!({ "mask": 0, "childOrSibling": [], "childOrSiblingSelector": [] });
    if let Some(id) = &sel.id {
        base = with(base, RESOURCE_ID_MATCHES, "resourceIdMatches", json!(id.java_id()));
    }
    match &sel.text {
        Some(t) => vec![with(base.clone(), TEXT_MATCHES, "textMatches", json!(t.java())), with(base, DESCRIPTION_MATCHES, "descriptionMatches", json!(t.java()))],
        None => vec![base],
    }
}

fn rect(v: &Value) -> Option<[i64; 4]> {
    let n = |k: &str| v[k].as_i64();
    Some([n("left")?, n("top")?, n("right")?, n("bottom")?])
}

fn node(info: &Value, at: (usize, usize)) -> Option<Node> {
    let bounds = rect(&info["bounds"])?;
    let s = |k: &str| info[k].as_str().unwrap_or_default().to_string();
    Some(Node { bounds, visible: rect(&info["visibleBounds"]).unwrap_or(bounds), text: s("text"), desc: s("contentDescription"), class: s("className"), id: s("resourceName"), inner: None, at: Some(at) })
}

/// A dead UiAutomation connection answers every selector with "no match"; `deviceInfo` fails then, so a miss is only a miss
/// when it does not. On a dead server this restarts it and says so, and the caller asks again.
pub(crate) fn revived(serial: &str) -> Result<bool, String> {
    if rpc_once(serial, "deviceInfo", json!([]), Duration::from_secs(10)).is_ok() {
        return Ok(false);
    }
    forget(serial);
    base_with(serial, true)?;
    Ok(true)
}

/// The element a selector names, a miss checked against a dead server: see `look`.
pub(crate) fn locate(serial: &str, sel: &Sel) -> Result<Option<Node>, String> {
    let first = look(serial, sel)?;
    if first.is_none() && revived(serial)? {
        return look(serial, sel);
    }
    Ok(first)
}

/// One look: every visible match in tree order (`objInfoOfAllInstances`), the text's before the label's, and of those
/// Maestro's deepest. A web view's text is the EditText inside it (the code editor's buffer).
pub(crate) fn look(serial: &str, sel: &Sel) -> Result<Option<Node>, String> {
    for (v, s) in selectors(sel).into_iter().enumerate() {
        let all = match rpc(serial, "objInfoOfAllInstances", json!([s]), Duration::from_secs(20)) {
            Ok(all) => all,
            Err(e) if is_not_found(&e) => continue,
            Err(e) => return Err(e),
        };
        let found: Vec<Node> = all.as_array().into_iter().flatten().enumerate().filter_map(|(k, info)| node(info, (v, k))).filter(Node::shows).collect();
        let Some(mut n) = crate::android_input::deepest(found, |a, b| inside(&a.bounds, &b.bounds)) else { continue };
        if n.class.contains("WebView") {
            n.inner = inner_edit_text(serial, &s, n.at.map_or(0, |a| a.1))?;
        }
        return Ok(Some(n));
    }
    Ok(None)
}

fn inside(a: &[i64; 4], b: &[i64; 4]) -> bool {
    a[0] >= b[0] && a[1] >= b[1] && a[2] <= b[2] && a[3] <= b[3]
}

fn instance(s: &Value, k: usize) -> Value {
    with(s.clone(), INSTANCE, "instance", json!(k))
}

/// The text of the first EditText whose centre lies inside `b`. The first EditText is read on its own first: that one
/// call shows a typed key within half a second, where the all-instances read lagged about 2.5 s (CI run 39535538289).
pub(crate) fn field_inside(serial: &str, b: [i64; 4]) -> Result<Option<String>, String> {
    let edit = with(json!({ "mask": 0, "childOrSibling": [], "childOrSiblingSelector": [] }), CLASS_NAME, "className", json!("android.widget.EditText"));
    match rpc(serial, "objInfo", json!([edit.clone()]), Duration::from_secs(20)) {
        Ok(info) => {
            if let Some(n) = node(&info, (0, 0)).filter(|n| crate::android_input::centre_in(&n.bounds, &b)) {
                return Ok(Some(n.text));
            }
        }
        Err(e) if is_not_found(&e) => return Ok(None),
        Err(e) => return Err(e),
    }
    let all = match rpc(serial, "objInfoOfAllInstances", json!([edit]), Duration::from_secs(20)) {
        Ok(all) => all,
        Err(e) if is_not_found(&e) => return Ok(None),
        Err(e) => return Err(e),
    };
    Ok(all.as_array().into_iter().flatten().filter_map(|info| node(info, (0, 0))).find(|n| crate::android_input::centre_in(&n.bounds, &b)).map(|n| n.text))
}

/// The text of the first EditText inside the `k`th node `s` matches.
fn inner_edit_text(serial: &str, s: &Value, k: usize) -> Result<Option<String>, String> {
    let mut parent = if k == 0 { s.clone() } else { instance(s, k) };
    parent["childOrSibling"] = json!(["child"]);
    parent["childOrSiblingSelector"] = json!([with(json!({ "mask": 0, "childOrSibling": [], "childOrSiblingSelector": [] }), CLASS_NAME, "className", json!("android.widget.EditText"))]);
    match rpc(serial, "objInfo", json!([parent]), Duration::from_secs(20)) {
        Ok(info) => Ok(info["text"].as_str().map(str::to_string)),
        Err(e) if is_not_found(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

/// How many nodes match, visible or not: the text's, plus the label's whose text does not match (counted already).
pub(crate) fn count(serial: &str, sel: &Sel) -> Result<usize, String> {
    let mut sels = selectors(sel).into_iter();
    let first = sels.next().map_or(Ok(0), |s| match rpc(serial, "count", json!([s]), Duration::from_secs(20)) {
        Ok(c) => Ok(c.as_u64().unwrap_or(0) as usize),
        Err(e) if is_not_found(&e) => Ok(0),
        Err(e) => Err(e),
    })?;
    let (Some(labels), Some(text)) = (sels.next(), &sel.text) else { return Ok(first) };
    let all = match rpc(serial, "objInfoOfAllInstances", json!([labels]), Duration::from_secs(20)) {
        Ok(all) => all,
        Err(e) if is_not_found(&e) => return Ok(first),
        Err(e) => return Err(e),
    };
    Ok(first + all.as_array().into_iter().flatten().filter(|info| !text.is_match(info["text"].as_str().unwrap_or_default())).count())
}

/// The window hierarchy as `uiautomator dump` writes it. An unknown overload must not read as a dead server, so no restart.
pub(crate) fn hierarchy(serial: &str) -> Result<String, String> {
    let dumped = rpc_once(serial, "dumpWindowHierarchy", json!([false, 50]), Duration::from_secs(30)).or_else(|_| rpc_once(serial, "dumpWindowHierarchy", json!([false]), Duration::from_secs(30)))?;
    dumped.as_str().map(str::to_string).ok_or_else(|| format!("UI server: dumpWindowHierarchy answered {dumped}"))
}

/// Presses at the first point, drags through the rest and lifts at the last, inside the device's own process: an `input
/// motionevent` per sample starts a JVM each (about a second on the emulator).
pub(crate) fn swipe_points(serial: &str, samples: &[(f64, f64)]) -> Result<(), String> {
    let paced = pace(samples);
    let flat: Vec<i64> = paced.iter().flat_map(|(x, y)| [x.round() as i64, y.round() as i64]).collect();
    let timeout = Duration::from_secs(20 + 3 * paced.len() as u64);
    match rpc(serial, "swipePoints", json!([flat, PACE_STEPS]), timeout)?.as_bool() {
        Some(true) => Ok(()),
        other => Err(format!("swipePoints answered {other:?}")),
    }
}

/// One step per segment makes the server inject the whole stroke in about 100 ms: a React Native pad then gets two or three
/// touch events instead of the stroke and scores it "too short". Two steps cost about 90 ms per point on the emulator, so a
/// stroke keeps the points that carry its shape (within `TOLERANCE_PX`), split so no segment is long, at most `MAX_POINTS`
/// (a few seconds, like a hand); a short one is padded to `MIN_POINTS`. A fixed stride cut the corners of long one-stroke letters (இ scored 84%).
const PACE_STEPS: i64 = 2;
const MAX_POINTS: usize = 60;
const MIN_POINTS: usize = 8;
const TOLERANCE_PX: f64 = 2.0;
const MAX_SEGMENT_PX: f64 = 30.0;

pub(crate) fn pace(samples: &[(f64, f64)]) -> Vec<(f64, f64)> {
    if samples.len() < 2 {
        return samples.to_vec();
    }
    let mut tolerance = TOLERANCE_PX;
    let mut kept = simplify(samples, tolerance);
    while kept.len() > MAX_POINTS / 2 {
        tolerance *= 1.5;
        kept = simplify(samples, tolerance);
    }
    // A long segment is two injected steps: the pad can lose the last one before the lift and score the stroke "too short"
    // (ஈ, job 1a139a1cb39-72ee2), so no segment is longer than `MAX_SEGMENT_PX`, and the stroke rests on its last point.
    let length: f64 = kept.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).sum();
    let segment = MAX_SEGMENT_PX.max(length / (MAX_POINTS - 2) as f64);
    let mut out = vec![kept[0]];
    for w in kept.windows(2) {
        let pieces = (((w[1].0 - w[0].0).hypot(w[1].1 - w[0].1) / segment).ceil() as usize).max(1);
        for i in 1..=pieces {
            let t = i as f64 / pieces as f64;
            out.push((w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t));
        }
    }
    while out.len() < MIN_POINTS - 1 {
        let (a, b) = (out[out.len() - 2], out[out.len() - 1]);
        out.insert(out.len() - 1, ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0));
    }
    let last = out[out.len() - 1];
    out.push(last);
    out
}

/// Ramer-Douglas-Peucker: the fewest points whose polyline stays within `tolerance` of the path.
fn simplify(path: &[(f64, f64)], tolerance: f64) -> Vec<(f64, f64)> {
    let mut keep = vec![false; path.len()];
    keep[0] = true;
    keep[path.len() - 1] = true;
    let mut spans = vec![(0, path.len() - 1)];
    while let Some((first, last)) = spans.pop() {
        let (a, b) = (path[first], path[last]);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = dx.hypot(dy);
        let far = (first + 1..last)
            .map(|i| {
                let p = path[i];
                let d = if len == 0.0 { (p.0 - a.0).hypot(p.1 - a.1) } else { (dy * (p.0 - a.0) - dx * (p.1 - a.1)).abs() / len };
                (i, d)
            })
            .max_by(|x, y| x.1.total_cmp(&y.1));
        if let Some((i, _)) = far.filter(|f| f.1 > tolerance) {
            keep[i] = true;
            spans.push((first, i));
            spans.push((i, last));
        }
    }
    path.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

/// Finds the target and taps it in one call on the device: a layout that moves between a lookup and a tap cannot make the
/// tap miss (the Run Local button moved 87 px under one on 18 Oct). `at` is the (selector, instance) a lookup took, so the
/// tap lands on that match. False when nothing matches.
pub(crate) fn click_target(serial: &str, sel: &Sel, at: Option<(usize, usize)>) -> Result<bool, String> {
    let all = selectors(sel);
    let picked: Vec<Value> = match at {
        Some((v, k)) if v < all.len() => vec![instance(&all[v], k)],
        _ => all,
    };
    for sel in picked {
        match rpc(serial, "click", json!([sel]), Duration::from_secs(20)) {
            // false: UiObject.click pressed the node but saw no content change within its 500 ms acknowledgment; a fallback
            // tap would press it twice (a mic pressed twice started a second recording, CI runs 39540461741, 39543792431)
            Ok(_) => return Ok(true),
            Err(e) if is_not_found(&e) => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(false)
}

/// A tap at a point, inside the device's own process (`input tap` starts a JVM).
pub(crate) fn click(serial: &str, x: f64, y: f64) -> Result<(), String> {
    rpc(serial, "click", json!([x.round() as i64, y.round() as i64]), Duration::from_secs(20)).map(|_| ())
}

/// A swipe between two points lasting about `ms`: the server paces by steps, and each step costs tens of milliseconds on the
/// emulator, so the step count is kept small.
pub(crate) fn swipe(serial: &str, from: (f64, f64), to: (f64, f64), ms: u64) -> Result<(), String> {
    let steps = (ms / 50).clamp(2, 10);
    let flat: Vec<i64> = vec![from.0.round() as i64, from.1.round() as i64, to.0.round() as i64, to.1.round() as i64];
    match rpc(serial, "swipePoints", json!([flat, steps]), Duration::from_secs(30))?.as_bool() {
        Some(true) => Ok(()),
        other => Err(format!("swipePoints answered {other:?}")),
    }
}

/// One key by its Android keycode, inside the device's own process (`input keyevent` starts a JVM, about 0.6 s a key).
pub(crate) fn press_key(serial: &str, code: u32) -> Result<(), String> {
    match rpc(serial, "pressKeyCode", json!([code]), Duration::from_secs(10))?.as_bool() {
        Some(true) => Ok(()),
        other => Err(format!("pressKeyCode answered {other:?}")),
    }
}


#[cfg(test)]
mod tests {
    use super::pace;

    fn longest(points: &[(f64, f64)]) -> f64 {
        points.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).fold(0.0, f64::max)
    }

    #[test]
    fn given_a_stroke_when_paced_for_the_server_then_it_keeps_its_shape_in_at_most_60_short_segments_resting_on_its_end() {
        for (path, at_least) in [(vec![(10.0, 10.0), (400.0, 10.0), (400.0, 800.0), (10.0, 800.0)], 40), (vec![(100.0, 100.0), (100.0, 250.0), (100.0, 400.0)], 8), (vec![(100.0, 200.0), (150.0, 250.0), (200.0, 200.0)], 8)] {
            let paced = pace(&path);
            let n = paced.len();
            assert!((at_least..=60).contains(&n), "{n} points for {path:?}");
            assert!(longest(&paced) <= 30.01, "a segment over 30 px: {paced:?}");
            assert_eq!((paced[0], paced[n - 1], paced[n - 2]), (path[0], path[path.len() - 1], path[path.len() - 1]), "starts on its first point and rests on its last");
        }
        assert!(pace(&[(150.0, 250.0)]).len() == 1, "a tap stays a tap");
    }
}
