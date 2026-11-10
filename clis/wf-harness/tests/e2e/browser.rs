//! `wf browser` against a real Chromium: the Rust daemon driving a phone-framed page, a Chromium someone else launched and the
//! daemon attached to, and the recording check on a real ffmpeg video. Integration tier: these run after the CLIs are built, one real
//! Chromium at a time, and fail naming what is missing when there is none. The mock-daemon journeys are `flow/browser.rs`.
use crate::support::{after_each_draw, call_at, chromium_gate, daemon_url, http_get, repo_root, require_browser, run_wf, Aux, BrowserSession, MockReply, MockServer, RunOpts, RunResult};
use regex::Regex;
use serde_json::{json, Value};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("json {text:?}: {e}"))
}

/// A verb's one JSON line with its timing zeroed.
fn line(r: &RunResult) -> Value {
    let mut v = parse(r.stdout.trim());
    if v.get("ms").is_some() {
        v["ms"] = json!(0);
    }
    v
}


#[test]
fn check_taps_finds_the_tap_indicator_in_a_recording_for_each_logged_tap() {
    assert!(Command::new("ffmpeg").arg("-version").output().is_ok_and(|o| o.status.success()), "ffmpeg is not installed (CI installs it in the e2e jobs' setup)");
    // Given a one-second clip with a tap-indicator-blue dot, and its tap log beside it
    let dir = tempfile::Builder::new().prefix("wf-flow-check-taps-").tempdir().unwrap();
    let video = dir.path().join("clip.mp4");
    assert!(Command::new("ffmpeg").args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "color=c=white:s=200x200:d=1:r=10", "-vf", "drawbox=x=40:y=40:w=30:h=30:color=0x2f7dff:t=fill", "-pix_fmt", "yuv420p"]).arg(&video).status().unwrap().success());
    let taps = |list: &[(u32, u32, u32)]| std::fs::write(dir.path().join("clip.inputs.jsonl"), list.iter().map(|(t, x, y)| json!({ "kind": "tap", "t": t, "x": x, "y": y }).to_string() + "\n").collect::<String>()).unwrap();
    let check = |path: &Path, extra: &[&str]| {
        let mut args = vec!["browser", "check-taps", path.to_str().unwrap()];
        args.extend_from_slice(extra);
        run_wf(&args, &RunOpts::default().cwd(dir.path()).env("WF_REGISTRY_DIR", &dir.path().join("registry").display().to_string()))
    };

    // When the only tap is on the dot
    taps(&[(200, 55, 55)]);
    let pass = check(&video, &["--json"]);
    // Then it passes, with no daemon running
    assert_eq!((pass.exit_code, line(&pass)["result"].clone()), (0, json!({ "taps": 1, "dotsFound": 1, "pass": true, "missing": [] })));

    // When a second tap is away from the dot, and when the recording is not there
    taps(&[(200, 55, 55), (400, 150, 150)]);
    let fail = check(&video, &["--inputs", &dir.path().join("clip.inputs.jsonl").display().to_string()]);
    let missing = check(&dir.path().join("nope.mp4"), &[]);
    // Then it fails naming the tap's time with the check as evidence, or names the missing file
    let failed = line(&fail);
    assert_eq!((fail.exit_code, failed["error"].clone(), failed["evidence"]["missing"][0].as_f64()), (1, json!("1 of 2 taps show a dot; no dot for the taps at 400 ms"), Some(400.0)));
    assert!(missing.exit_code == 1 && line(&missing)["error"].as_str().unwrap().contains("video-check: no file at"), "{}", missing.stdout);
}

fn browser_session(prefix: &str, page_server: &str) -> BrowserSession {
    require_browser();
    BrowserSession::start(prefix, page_server)
}

fn serve_page(page: &str) -> String {
    format!("const http = require('node:http'); const page = {}; const s = http.createServer((q, r) => {{ r.writeHead(200, {{ 'content-type': 'text/html' }}); r.end(page); }}); s.listen(0, '127.0.0.1', () => console.log(s.address().port));", json!(page))
}

const PHONE_PAGE: &str = "<!doctype html><title>app</title><meta name=viewport content=\"width=device-width,initial-scale=1\"><body style=\"margin:0\"><button id=b style=\"margin:20px;padding:10px 20px\">Go</button><input id=i placeholder=\"name\" style=\"margin:20px\"><div id=d style=\"width:100px;height:60px;background:red;margin:20px\"></div><p>hello</p><script>window.log=[];for (const t of ['click','touchstart','touchend','mousedown','mouseup']) addEventListener(t, e => log.push(t + ':' + (e.target.id||e.target.tagName)));addEventListener('keydown', e => log.push('key:' + e.key));window.strokes=[];for (const t of ['touchstart','touchmove','touchend']) addEventListener(t, e => { if (!window.slow) return; if (t === 'touchmove') { const t0 = performance.now(); while (performance.now() - t0 < 50); } strokes.push([t[5], e.touches[0] ? e.touches[0].clientX : null]); }, { passive: false });</script></body>";

const STATE: &str = "JSON.stringify({ w: innerWidth, h: innerHeight, dpr: devicePixelRatio, ua: navigator.userAgent, tp: navigator.maxTouchPoints, plat: navigator.platform, coarse: matchMedia(\"(pointer:coarse)\").matches, dark: matchMedia(\"(prefers-color-scheme: dark)\").matches })";

const INSETS: &str = "JSON.stringify([\"top\", \"right\", \"bottom\", \"left\"].map((side) => { const d = document.createElement(\"div\"); d.style.cssText = `position:fixed;padding-${side}:env(safe-area-inset-${side})`; document.body.appendChild(d); return getComputedStyle(d)[`padding${side[0].toUpperCase()}${side.slice(1)}`]; }))";

fn png_size(path: &str) -> (u32, u32) {
    let bytes = std::fs::read(path).unwrap();
    (u32::from_be_bytes(bytes[16..20].try_into().unwrap()), u32::from_be_bytes(bytes[20..24].try_into().unwrap()))
}

#[test]
fn a_phone_framed_session_looks_and_behaves_like_the_device_it_was_opened_as() {
    let s = browser_session("wf-flow-browser-phone-", &serve_page(PHONE_PAGE));

    // When the default device is started and a link opened on it
    let mut opened = s.result(&["start-device", "--device", "a"]);
    let nav = s.result(&["open-link", &s.base, "--device", "a"]);
    // Then the summary and the app frame are an iPhone 15 Pro: viewport, pixel ratio, user agent, touch and safe areas
    opened.as_object_mut().unwrap().remove("systemFont");
    assert_eq!(opened, json!({ "name": "a", "engine": "chromium", "renderMode": "device-frame", "devicePreset": "iphone-15-pro", "orientation": "portrait", "keyboardMode": "mock", "colorScheme": "system", "video": false, "active": true, "targetUrl": null }));
    assert_eq!(nav, json!({ "link": s.base }));
    assert_eq!(s.eval_json("a", STATE), json!({ "w": 393, "h": 852, "dpr": 3, "ua": "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148", "tp": 5, "plat": "iPhone", "coarse": true, "dark": false }));
    assert_eq!(s.eval_json("a", INSETS), json!(["59px", "0px", "34px", "0px"]));

    // When the page is described, and an input takes and loses focus
    let snapshot = s.rpc("snapshot", json!({ "name": "a" }))["yaml"].as_str().unwrap().to_string();
    let keyboard = "JSON.stringify([window.__MCP_KEYBOARD_VISIBLE__, window.__MCP_KEYBOARD_MODE__, window.__MCP_KEYBOARD_HEIGHT__, document.activeElement.id])";
    s.rpc("eval", json!({ "name": "a", "expression": "document.getElementById(\"i\").focus(), \"focused\"" }));
    std::thread::sleep(Duration::from_millis(1000));
    let shown = s.eval_json("a", keyboard);
    s.rpc("eval", json!({ "name": "a", "expression": "document.getElementById(\"i\").blur(), \"blurred\"" }));
    std::thread::sleep(Duration::from_millis(1000));
    let hidden = s.eval_json("a", "JSON.stringify([window.__MCP_KEYBOARD_VISIBLE__, window.__MCP_KEYBOARD_HEIGHT__])");
    // Then the app frame's content is listed with refs, and the app frame is told the mock keyboard's height and mode
    let normal = Regex::new(r"\[ref=f\d+e").unwrap().replace_all(&snapshot, "[ref=e").replace(" [active]", "");
    assert_eq!(normal, ["- generic [ref=e1]:", "  - button \"Go\" [ref=e2]", "  - textbox \"name\" [ref=e3]", "  - paragraph [ref=e5]: hello"].join("\n"));
    assert_eq!((shown, hidden), (json!([true, "overlay", 270, "i"]), json!([false, 0])));

    // When the app is tapped, typed into, keyed, dragged, and a missing element is tapped
    let tap = |r: &str| s.rpc("tap", json!({ "name": "a", "ref": r, "indicator": false }));
    let (tap_button, tap_input) = (tap("#b"), tap("#i"));
    let typed = s.rpc("type", json!({ "name": "a", "ref": "#i", "text": "hello", "indicator": false }));
    let pressed = s.rpc("press", json!({ "name": "a", "key": "Backspace", "repeat": 2 }));
    let dragged = s.rpc("drag", json!({ "name": "a", "ref": "#d", "dx": 30, "dy": 0, "indicator": false }));
    let missing = s.call("tap", json!({ "name": "a", "ref": "#nope", "indicator": false }));
    // Then a phone sees touches then clicks, keys and mouse drags, and the missing ref fails naming it
    for (t, r) in [(&tap_button, "#b"), (&tap_input, "#i")] {
        assert!(t["tapped"] == r && t["pointer"] == "touch" && t["holdMs"].as_f64().unwrap() > 0.0, "{t}");
    }
    assert_eq!((typed["typed"].clone(), pressed, dragged["dragged"].clone()), (json!("#i"), json!({ "key": "Backspace", "repeat": 2 }), json!("#d")));
    assert_eq!(
        s.eval_json("a", "JSON.stringify([window.log, document.getElementById(\"i\").value])"),
        json!([["touchstart:b", "touchend:b", "mousedown:b", "mouseup:b", "click:b", "touchstart:i", "touchend:i", "mousedown:i", "mouseup:i", "click:i", "key:Backspace", "key:Backspace", "mousedown:d", "mouseup:d", "click:d"], "hel"])
    );
    assert!(missing.as_ref().unwrap_err().contains("tap: no element resolved for ref \"#nope\""), "{missing:?}");

    // When a human stroke 300 px long is drawn while the page spends 50 ms on every touch move
    s.rpc("eval", json!({ "name": "a", "expression": "window.slow = true, \"slow\"" }));
    let stroke = s.rpc("stroke", json!({ "name": "a", "ref": "#d", "units": "px", "points": [{ "x": 0, "y": 30 }, { "x": 300, "y": 30 }], "human": true, "indicator": false }));
    let felt = s.eval_json("a", "JSON.stringify(window.strokes)");
    // Then the finger keeps a hand's pace (its planned 0.73 s, not 50 ms a sample) by sending only the latest of the moves
    // already due, as a touchscreen coalesces them: one touch down at the path's start, its last point, one lift; the result
    // counts the moves sent (the page may coalesce more itself) and coalesced, and the 50 ms each move blocked
    let (samples, ms) = (stroke["samples"].as_u64().unwrap(), stroke["ms"].as_u64().unwrap());
    let felt = felt.as_array().unwrap();
    let kinds: String = felt.iter().map(|e| e[0].as_str().unwrap()).collect();
    assert!(stroke["pointer"] == "touch" && samples == 44 && (700..1400).contains(&ms), "{stroke}");
    let (sent, coalesced) = (stroke["sent"].as_u64().unwrap(), stroke["coalesced"].as_u64().unwrap());
    assert!(kinds.matches('m').count() <= sent as usize && coalesced > 0 && sent + coalesced == samples - 1 && stroke["maxMoveMs"].as_u64() >= Some(50), "{stroke} {kinds}");
    assert!(kinds.starts_with("sm") && kinds.ends_with("me") && kinds.matches('s').count() == 1 && kinds.matches('e').count() == 1 && kinds.len() < samples as usize + 1, "{felt:?}");
    let x = |e: &Value| e[1].as_f64().unwrap();
    assert!((x(&felt[0]) - 20.0).abs() <= 1.0 && (x(&felt[felt.len() - 2]) - 320.0).abs() <= 1.0, "{felt:?}");

    // When the indicator commands are called through the daemon (no CLI verb: scripts that dispatch their own events use them)
    let markers = || s.eval_json("a", "JSON.stringify(document.querySelectorAll(\"[data-testid=wf-browser-tap-indicator]\").length)").as_i64().unwrap();
    let before = markers();
    let hidden_tap = s.call("indicateAppTap", json!({ "name": "a", "x": 40, "y": 60, "indicator": false }));
    let after_hidden = markers();
    let shown_tap = s.call("indicateAppTap", json!({ "name": "a", "x": 40, "y": 60 }));
    let key = s.call("indicateKeyPress", json!({ "name": "a", "char": "q" }));
    let no_char = s.call("indicateKeyPress", json!({ "name": "a" }));
    // Then a tap marker lands in the app frame only when indicators are on, and a key indicator needs its char
    assert_eq!((before, hidden_tap.is_ok(), after_hidden, shown_tap.is_ok(), markers()), (0, true, 0, true, 1));
    assert_eq!((key.is_ok(), no_char), (true, Err("indicateKeyPress needs char".to_string())));
    // and the markers' script is installed in the frame once, so a later marker sends only its call
    assert_eq!(s.eval_json("a", "JSON.stringify(typeof window.__wfMarks)"), json!("object"));

    // When a landscape Pixel 8 with a dark scheme is opened beside it, and screenshotted
    let pixel = s.open("p", json!({ "devicePreset": "pixel-8", "orientation": "landscape", "colorScheme": "dark" }));
    s.rpc("navigate", json!({ "name": "p", "url": s.base }));
    let shot = s.rpc("screenshot", json!({ "name": "p" }));
    // Then its app frame is rotated, Android-identified and dark, and the PNG is the fitted shell at the device pixel ratio
    assert_eq!([pixel["devicePreset"].as_str(), pixel["orientation"].as_str(), pixel["colorScheme"].as_str()], [Some("pixel-8"), Some("landscape"), Some("dark")]);
    assert_eq!(without_chrome_major(s.eval_json("p", STATE)), json!({ "w": 915, "h": 412, "dpr": 2.625, "ua": "Mozilla/5.0 (Linux; Android 14; Pixel 8 Build/AP2A.240805.005; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/N.0.0.0 Mobile Safari/537.36", "tp": 5, "plat": "Linux armv81", "coarse": true, "dark": true }));
    assert_eq!(s.eval_json("p", INSETS), json!(["0px", "24px", "0px", "16px"]));
    assert_eq!(png_size(shot["path"].as_str().unwrap()), (2468, 1250));

    // When a screen size that does not exist is asked for, and a device is stopped and started again
    let unknown = s.wf(&["start-device", "--device", "x", "--screen-size", "nokia-3310"]);
    let close = s.result(&["stop-device", "--device", "p"]);
    let list = s.result(&["list-devices"]);
    let again = s.wf(&["start-device", "--device", "p"]);
    // Then the unknown size is named with the known ones, the stopped device is gone and its name free again
    assert_eq!(unknown.exit_code, 2);
    assert!(line(&unknown)["error"].as_str().unwrap().contains("unknown screen size 'nokia-3310': one of iphone-se"), "{}", unknown.stdout);
    assert_eq!(close, json!({ "closed": ["p"] }));
    assert!(!list["devices"].to_string().contains("\"p\"") && list["daemon"]["running"] == true, "{list}");
    assert_eq!(again.exit_code, 0, "{}", again.stdout);

    // When a browser is started with launch flags and an environment through the daemon
    let launched = s.call("open", json!({ "name": "l", "renderMode": "plain", "keyboardMode": "none", "headless": true, "launchArgs": ["--force-prefers-reduced-motion"], "launchEnv": { "TZ": "Asia/Tokyo" } }));
    s.rpc("navigate", json!({ "name": "l", "url": s.base }));
    // Then the page sees the flag and the environment
    assert!(launched.is_ok(), "{launched:?}");
    assert_eq!(s.eval_json("l", "JSON.stringify({ reduced: matchMedia(\"(prefers-reduced-motion: reduce)\").matches, tz: Intl.DateTimeFormat().resolvedOptions().timeZone })"), json!({ "reduced": true, "tz": "Asia/Tokyo" }));
}

const STATE_PAGE: &str = "<!doctype html><title>app</title><body style=\"margin:0\">\n<a id=\"l\" target=\"_blank\" href=\"https://example.com/x\" style=\"display:block;padding:20px\">link</a>\n<button id=\"w\" onclick=\"window.open('/rel')\" style=\"margin:20px;padding:10px\">win</button>\n<button id=\"n\" onclick=\"window.open('https://example.org/y','_blank','noopener')\" style=\"margin:20px;padding:10px\">noopener</button>\n</body>";

#[test]
fn a_phone_framed_session_exposes_the_page_state_an_agent_inspects() {
    let s = browser_session("wf-flow-browser-state-", &serve_page(STATE_PAGE));
    let value = |method: &str, name: &str, expression: &str| parse(s.rpc(method, json!({ "name": name, "expression": expression }))["result"].as_str().unwrap());
    s.open("a", json!({}));
    s.rpc("navigate", json!({ "name": "a", "url": s.origin }));

    // When expressions run in the shell, the app frame and the outer frame
    // Then each runs in its own document
    assert_eq!((value("shellEval", "a", "typeof window.__shell"), value("eval", "a", "document.title"), value("outerEval", "a", "document.title")), (json!("object"), json!("app"), json!("app")));
    assert_eq!(value("eval", "a", "typeof window.__shell"), json!("undefined"));

    // When links and window.open calls are tapped, and the list is read, cleared and read again
    for r in ["#l", "#w", "#n"] {
        s.rpc("tap", json!({ "name": "a", "ref": r }));
    }
    let opened = |clear: bool| s.rpc("openedUrls", json!({ "name": "a", "clear": clear }))["urls"].as_array().unwrap().clone();
    let urls = opened(false);
    let cleared = opened(true);
    // Then each URL is listed with how it was opened, recorded instead of opened
    let shape: Vec<Value> = urls.iter().map(|u| json!({ "url": u["url"], "via": u["via"] })).collect();
    assert_eq!(shape, [json!({ "url": "https://example.com/x", "via": "anchor" }), json!({ "url": format!("{}/rel", s.origin), "via": "window.open" }), json!({ "url": "https://example.org/y", "via": "linking" })]);
    assert!(urls.iter().all(|u| u["at"].as_f64().is_some_and(|at| at > 1e12)));
    assert_eq!((cleared.len(), opened(false).len()), (3, 0));

    // When the clipboard permissions are granted and the clipboard is set and read
    let read = s.rpc("permission", json!({ "name": "a", "kind": "clipboard-read", "state": "granted" }));
    let write = s.rpc("permission", json!({ "name": "a", "kind": "clipboard-write", "state": "granted" }));
    let set = s.rpc("clipboard", json!({ "name": "a", "set": "hello tamil" }));
    let back = s.rpc("clipboard", json!({ "name": "a" }));
    // Then the text round-trips
    assert_eq!((read, write), (json!({ "kind": "clipboard-read", "state": "granted" }), json!({ "kind": "clipboard-write", "state": "granted" })));
    assert_eq!((set, back), (json!({ "text": "hello tamil" }), json!({ "text": "hello tamil" })));

    // When an element is stroked on the iPhone
    s.rpc("eval", json!({ "name": "a", "expression": "window.log = []; for (const t of ['touchstart', 'touchmove', 'touchend']) document.addEventListener(t, (e) => window.log.push(t))" }));
    let stroked = s.rpc("stroke", json!({ "name": "a", "ref": "#w", "indicator": false, "points": [{ "x": 0.1, "y": 0.5 }, { "x": 0.9, "y": 0.5 }] }));
    let log: Vec<String> = serde_json::from_str(value("eval", "a", "JSON.stringify(window.log)").as_str().unwrap()).unwrap();
    // Then the app sees a finger
    assert_eq!(stroked["pointer"], "touch");
    assert!(log[0] == "touchstart" && log.last().unwrap() == "touchend" && log.iter().filter(|t| *t == "touchmove").count() > 2, "{log:?}");

    // When a page that spends 60 ms on every touch move has a scroller, and is swiped for 600 ms and flung for 120 ms
    s.rpc("eval", json!({ "name": "a", "expression": "document.body.insertAdjacentHTML('beforeend', '<div id=wf-sc style=\"position:fixed;top:150px;left:0;width:300px;height:300px;overflow:auto\"><div style=\"height:20000px\"></div></div>'); window.swipeLog = []; for (const t of ['touchstart', 'touchmove', 'touchend']) document.addEventListener(t, () => { window.swipeLog.push(t); if (t === 'touchmove') { const end = performance.now() + 60; while (performance.now() < end); } }, { passive: false })" }));
    let swiped = s.rpc("swipe", json!({ "name": "a", "from": { "x": 150, "y": 400 }, "dx": 0, "dy": -150, "durationMs": 600, "indicator": false }));
    let log: Vec<String> = serde_json::from_str(value("eval", "a", "JSON.stringify(window.swipeLog)").as_str().unwrap()).unwrap();
    let flung = s.rpc("swipe", json!({ "name": "a", "from": { "x": 150, "y": 400 }, "dx": 0, "dy": -150, "durationMs": 120, "indicator": false }));
    // Then the drag keeps its pace with fewer moves, as a touchscreen coalesces them (600 ms and the 120 ms rest, not 36 moves
    // of 60 ms) and stops under the finger; the fling keeps every frame's stamp, so the content coasts past the finger
    assert_eq!(swiped["pointer"], "touch");
    assert!(swiped["ms"].as_u64().unwrap() < 1100, "{swiped}");
    assert!(log[0] == "touchstart" && log.last().unwrap() == "touchend" && log.iter().filter(|t| *t == "touchmove").count() >= 3, "{log:?}");
    assert!(swiped["scrolled"]["dy"].as_f64().unwrap() <= 160.0, "{swiped}");
    assert!(flung["scrolled"]["dy"].as_f64().unwrap() > 300.0, "{flung}");

    // When the page inserts a button the moment its ref is looked up, so no frame has laid it out yet, and the ref is tapped
    s.rpc("eval", json!({ "name": "a", "expression": "window.__wfRefs = { get e1() { document.body.insertAdjacentHTML('beforeend', '<button id=wf-new onclick=\"window.newTapped = true\" style=\"position:fixed;top:200px;left:0;width:200px;height:40px\">new</button>'); return document.getElementById('wf-new'); } }" }));
    let fresh = s.call("tap", json!({ "name": "a", "ref": "e1" }));
    // Then the tap finds its box and lands on it
    assert!(fresh.is_ok() && value("eval", "a", "JSON.stringify(window.newTapped === true)") == json!("true"), "{fresh:?}");

    // When an input is focused by a tap, a plain area is tapped, and the page blurs the input
    let visible = || {
        std::thread::sleep(Duration::from_millis(700));
        value("shellEval", "a", "JSON.stringify(window.__shell.getStatus().keyboardVisible)")
    };
    s.rpc("eval", json!({ "name": "a", "expression": "document.body.insertAdjacentHTML('beforeend', '<input id=wf-t style=position:fixed;top:0;left:0;width:200px;height:40px><div id=wf-d style=position:fixed;top:100px;left:0;width:200px;height:40px>plain</div>')" }));
    s.rpc("tap", json!({ "name": "a", "ref": "#wf-t" }));
    let shown = visible();
    s.rpc("tap", json!({ "name": "a", "ref": "#wf-d" }));
    let after_plain = visible();
    s.rpc("eval", json!({ "name": "a", "expression": "document.getElementById('wf-t').blur()" }));
    // Then the keyboard stays up past the plain tap, as on Android, and goes down with the blur
    assert_eq!((shown, after_plain, visible()), (json!("true"), json!("true"), json!("false")));

    // When the iPhone switches to a landscape Pixel 8
    let switched = s.rpc("setDevice", json!({ "name": "a", "device": "pixel-8", "orientation": "landscape" }));
    // Then the app frame is resized, rotated and Android-identified, keeping the scale the browser launched with
    assert_eq!(switched, json!({ "device": "pixel-8", "orientation": "landscape" }));
    assert_eq!(value("eval", "a", "JSON.stringify({ w: innerWidth, h: innerHeight, dpr: devicePixelRatio, plat: navigator.platform, ua: navigator.userAgent.includes(\"Pixel 8\") })"), json!("{\"w\":915,\"h\":412,\"dpr\":3,\"plat\":\"Linux armv81\",\"ua\":true}"));

    // When a plain device is started and opens the page
    s.wf(&["start-device", "--device", "plain", "--plain"]);
    s.wf(&["open-link", &s.origin, "--device", "plain"]);
    // Then it loads directly without the device frame
    assert_eq!((value("eval", "plain", "document.title"), value("eval", "plain", "typeof window.__shell")), (json!("app"), json!("undefined")));
    assert!(s.result(&["list-devices"])["devices"].as_array().unwrap().iter().any(|b| b["name"] == "plain" && b["renderMode"] == "plain"));

    // When a device records, taps and stops recording
    s.wf(&["start-device", "--device", "v"]);
    s.wf(&["open-link", &s.origin, "--device", "v"]);
    let video = s.root.path().join("v.mp4").display().to_string();
    s.result(&["start-recording", &video, "--device", "v"]);
    s.result(&["tap-on", "--text", "win", "--device", "v"]);
    // check-taps looks for the dot in the 1500 ms after the tap: a loaded recorder that stops right after it has no such frame
    std::thread::sleep(Duration::from_millis(1600));
    let files = s.result(&["stop-recording", "--device", "v"]);
    let check = s.wf(&["check-taps", &video]);
    // Then the recording and its input log are written, and check-taps finds the tap's indicator
    assert_eq!(files, json!({ "files": [video] }));
    assert!(std::fs::metadata(&video).unwrap().len() > 1000);
    assert!(Path::new(&video.replace(".mp4", ".inputs.jsonl")).exists());
    assert_eq!((check.exit_code, line(&check)["result"]["pass"].clone()), (0, json!(true)), "{}", check.stdout);

    // When a page asks for the microphone with an audio file, and the file is missing or not a WAV
    let wav = s.repo.join("tone.wav");
    assert!(Command::new("ffmpeg").args(["-y", "-loglevel", "error", "-f", "lavfi", "-i", "sine=frequency=440:duration=1"]).arg(&wav).status().unwrap().success());
    std::fs::write(s.repo.join("tone.mp3"), "x").unwrap();
    let missing = s.try_open("m", json!({ "audioFile": s.repo.join("nope.wav").display().to_string() }));
    let not_wav = s.try_open("n", json!({ "audioFile": s.repo.join("tone.mp3").display().to_string() }));
    s.open("mic", json!({ "audioFile": wav.display().to_string() }));
    s.rpc("navigate", json!({ "name": "mic", "url": s.origin }));
    let tracks = s.rpc("eval", json!({ "name": "mic", "expression": "navigator.mediaDevices.getUserMedia({ audio: true }).then((s) => s.getAudioTracks().length)" }));
    // Then the page gets a stream, and the others are refused
    assert!(missing.as_ref().unwrap_err().contains("--audio-file not found") && not_wav.as_ref().unwrap_err().contains("--audio-file must be a .wav"), "{missing:?} {not_wav:?}");
    assert!(tracks.to_string().contains('1'), "{tracks}");

    // When a page answers 401 and the session has HTTP credentials
    let auth = Aux::spawn("require('http').createServer((q,r)=>{const ok=q.headers.authorization==='Basic '+Buffer.from('u:p').toString('base64');if(!ok){r.writeHead(401,{'WWW-Authenticate':'Basic realm=\"t\"'});return r.end('denied')}r.end('welcome u')}).listen(0,'127.0.0.1',function(){console.log(this.address().port)})");
    s.open("c", json!({ "httpCredentials": { "username": "u", "password": "p" } }));
    s.rpc("navigate", json!({ "name": "c", "url": format!("http://127.0.0.1:{}/", auth.port) }));
    let mut body = Value::String(String::new());
    for _ in 0..20 {
        if body.to_string().contains("welcome") {
            break;
        }
        if let Ok(r) = s.call("eval", json!({ "name": "c", "expression": "document.body.innerText" })) {
            body = r["result"].clone();
        }
    }
    // Then the browser retries with them and loads the page
    assert!(body.to_string().contains("welcome u"), "{body}");

    // When pages load an outside host behind HTTPS_PROXY, and a host on the tailnet
    let proxy = Aux::spawn("require('http').createServer((q,r)=>{console.log(q.url);r.end('via proxy')}).listen(0,'127.0.0.1',function(){console.log('port '+this.address().port)})");
    let proxy_url = format!("http://127.0.0.1:{}", proxy.port);
    // Chromium resets a SOCKS connection after its greeting now and then while it starts: unhandled, that killed the listener
    let mut socks = Aux::spawn("require('net').createServer(c=>{c.on('error',()=>{});c.once('data',()=>{c.write(Buffer.from([5,0]));c.once('data',r=>{console.log(r.subarray(5,5+r[4]).toString()+':'+r.readUInt16BE(5+r[4]));c.destroy()})})}).listen(0,'127.0.0.1',function(){console.log('port '+this.address().port)})");
    let socks_port = socks.port.to_string();
    // a daemon takes its egress settings from the environment it starts in, so these sessions get their own
    let egress = BrowserSession::start("wf-flow-browser-egress-", &serve_page(STATE_PAGE));
    let env = [("HTTPS_PROXY", proxy_url.as_str()), ("WF_SOCKS_PORT", socks_port.as_str())];
    // (localhost first: the proxy logs in order, so once it has logged the outside page it has logged any local one)
    for args in [&["start-device", "--device", "px"][..], &["open-link", &egress.origin, "--device", "px"], &["open-link", "http://wf-egress.example/hello", "--device", "px"], &["start-device", "--device", "t", "--tailnet"], &["open-link", "http://wf-tail.test:81/x", "--device", "t"]] {
        let r = egress.wf_env(args, &env);
        assert_eq!(r.exit_code, 0, "{args:?}: {}{}", r.stdout, r.stderr);
    }
    let (outside, tailnet) = ("http://wf-egress.example/hello".to_string(), "wf-tail.test:81".to_string());
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !(proxy.seen().contains(&outside) && socks.seen().contains(&tailnet)) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    // Then the outside request goes through the proxy and localhost stays direct, and Chromium asks the SOCKS5 proxy by name
    assert!(proxy.seen().contains(&outside) && !proxy.seen().iter().any(|u| u.contains("127.0.0.1")), "{:?}", proxy.seen());
    assert!(socks.seen().contains(&tailnet), "{:?} (listener running: {}); app frame: {:?}", socks.seen(), socks.running(), egress.call("eval", json!({ "name": "t", "expression": "location.href" })));

    // When a session is named after a process that no longer exists
    assert!(s.try_open("run-2147483646", json!({})).is_ok());
    let gone = (0..30).any(|_| {
        std::thread::sleep(Duration::from_millis(300));
        !s.rpc("list", json!({})).to_string().contains("run-2147483646")
    });
    // Then the daemon reaps it and its browser
    assert!(gone, "{}", s.rpc("list", json!({})));
}

const OBSERVED_SCRIPT: &str = "function used() { return 1 + 1; }\nfunction neverCalled() { return 'x'.repeat(200) + 'unused padding to make the script clearly larger than what runs'; }\nwindow.usedResult = used();\n";

#[test]
fn a_phone_framed_session_measures_memory_and_coverage_for_a_benchmark() {
    let server = format!(
        "const http = require('node:http'); const js = {}; const page = {}; const s = http.createServer((q, r) => {{ if (q.url === '/s.js') {{ r.writeHead(200, {{ 'content-type': 'text/javascript' }}); setTimeout(() => r.end(js), 400); }} else {{ r.writeHead(200, {{ 'content-type': 'text/html' }}); r.end(page); }} }}); s.listen(0, '127.0.0.1', () => console.log(s.address().port));",
        json!(OBSERVED_SCRIPT),
        json!("<!doctype html><title>app</title><body><div id=root></div><script src=\"/s.js\"></script></body>")
    );
    let s = browser_session("wf-flow-browser-observe-", &server);
    let memory = |extra: Value| {
        let mut args = json!({ "name": "a" });
        for (k, v) in extra.as_object().into_iter().flatten() {
            args[k] = v.clone();
        }
        s.rpc("memory", args)
    };

    // When coverage starts before the first navigation, and stops after it; and stopping a session that never started it
    // (wf bench calls these RPC methods, which lost their CLI verbs)
    s.open("a", json!({}));
    let out = s.root.path().join("cov/ranges.json");
    let started = s.rpc("coverage", json!({ "name": "a", "action": "start" }));
    s.rpc("navigate", json!({ "name": "a", "url": s.base }));
    let stopped = s.rpc("coverage", json!({ "name": "a", "action": "stop", "out": out.display().to_string() }));
    s.open("b", json!({}));
    let never = s.call("coverage", json!({ "name": "b", "action": "stop" }));
    // Then the page script reports what ran, its ranges are written, and the other fails saying coverage was not started
    assert_eq!(started, json!({ "started": true }));
    let scripts = stopped["scripts"].as_array().unwrap();
    let script = scripts.iter().find(|x| x["url"].as_str().unwrap().ends_with("/s.js")).expect("the page script");
    assert_eq!(script["length"].as_u64(), Some(OBSERVED_SCRIPT.len() as u64));
    assert!(script["executedBytes"].as_u64().unwrap() > 0 && script["executedBytes"].as_u64().unwrap() < OBSERVED_SCRIPT.len() as u64);
    let file = parse(&std::fs::read_to_string(&out).unwrap());
    let ranges = file["scripts"].as_array().unwrap().iter().find(|x| x["url"].as_str().unwrap().ends_with("/s.js")).unwrap()["ranges"].as_array().unwrap().clone();
    assert!(!ranges.is_empty() && ranges.iter().all(|r| r[1].as_u64().unwrap() <= OBSERVED_SCRIPT.len() as u64 && r[0].as_u64().unwrap() < r[1].as_u64().unwrap()));
    assert!(never.as_ref().unwrap_err().contains("coverage was not started on this browser"), "{never:?}");
    // and navigate returned only after the app ran its script
    assert_eq!(s.rpc("eval", json!({ "name": "a", "expression": "window.usedResult" }))["result"], json!("2"));

    // When memory is sampled with a forced GC, after adding 600 nodes and 50 listeners, and with a heap snapshot
    let first = memory(json!({ "label": "first", "gc": true }));
    let before = first["targets"][0].clone();
    s.rpc("eval", json!({ "name": "a", "expression": "(() => { const root = document.getElementById('root'); for (let i = 0; i < 600; i++) { const d = document.createElement('div'); if (i < 50) d.addEventListener('click', () => {}); root.appendChild(d); } return 'ok'; })()" }));
    let after = memory(json!({ "gc": true }))["targets"][0].clone();
    let path = s.root.path().join("heap/a.heapsnapshot");
    let snapshot = memory(json!({ "snapshot": path.display().to_string() }));
    // Then the sample has the benchmark fields, the next one shows the nodes and listeners, and the snapshot is written and sized
    assert_eq!((first["label"].as_str(), first["gc"].clone(), first["scope"].as_str()), (Some("first"), json!(true), Some("lab-only")));
    assert!(first["at"].is_number());
    let mut keys: Vec<&String> = before.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, ["documents", "domNodes", "frames", "jsHeapTotalBytes", "jsHeapUsedBytes", "layoutObjects", "listeners", "target"]);
    assert!(before["jsHeapUsedBytes"].as_f64().unwrap() > 100_000.0 && before["domNodes"].as_f64().unwrap() > 10.0);
    assert!(after["domNodes"].as_f64().unwrap() - before["domNodes"].as_f64().unwrap() >= 600.0);
    assert!(after["listeners"].as_f64().unwrap() - before["listeners"].as_f64().unwrap() >= 50.0);
    assert_eq!((snapshot["snapshot"]["path"].as_str(), snapshot["snapshot"]["bytes"].as_u64()), (Some(path.to_str().unwrap()), Some(std::fs::metadata(&path).unwrap().len())));
    assert!(std::fs::read_to_string(&path).unwrap().starts_with("{\"snapshot\""));
}

const DEBUG_SERVER: &str = r#"
const http = require('node:http');
const map = { version: 3, file: 'app.js', sources: ['src/math.ts'], names: [], mappings: 'AAAA;AACA;AACA;AACA;AACA' };
const loopMap = { version: 3, file: 'loop.js', sources: ['src/loop.ts'], names: [], mappings: 'AAAA;AACA;AACA;AACA;AACA;AACA' };
const files = {
  '/page.html': ['text/html', '<!doctype html><title>dbg</title><body><button id="go" style="width:200px;height:80px" onclick="window.run()">go</button><script src="/app.js"></script></body>'],
  '/loop.html': ['text/html', '<!doctype html><title>loop</title><body><script src="/loop.js"></script></body>'],
  '/loop.js': ['text/javascript', 'var ticks = 0;\nfunction tick() {\n  var next = ticks + 1;\n  ticks = next;\n}\nsetInterval(tick, 1000);\n//# sourceMappingURL=/loop.js.map'],
  '/loop.js.map': ['application/json', JSON.stringify(loopMap)],
  '/app.js': ['text/javascript', 'function add(a, b) {\n  var sum = a + b;\n  return sum;\n}\nwindow.run = function () { return add(2, 3); };\n//# sourceMappingURL=/app.js.map'],
  '/app.js.map': ['application/json', JSON.stringify(map)],
};
const s = http.createServer((q, r) => { const f = files[q.url.split('?')[0]]; if (!f) { r.writeHead(404); r.end(); return; } r.writeHead(200, { 'content-type': f[0] }); r.end(f[1]); });
s.listen(0, '127.0.0.1', () => console.log(s.address().port));
"#;

const FINDER_PAGE: &str = "<!doctype html><title>finder</title><meta name=viewport content=\"width=device-width,initial-scale=1\"><body style=\"margin:0;font:16px sans-serif\">\
<div style=\"overflow:hidden;width:120px;height:60px;position:relative\"><button data-testid=\"Clipped\" onclick=\"log.push('clip')\" style=\"position:absolute;left:80px;top:0;width:100px;height:40px\">Clip</button></div>\
<p style=\"display:none\">Hello world</p><p data-testid=\"Hello\">Hello world</p>\
<input data-testid=\"Name\" value=\"Ada\" style=\"width:200px\">\
<button data-testid=\"Warn\" onclick=\"console.warn('clicked %s', 'it'); setTimeout(() => { const b = document.createElement('button'); b.dataset.testid = 'Late'; b.textContent = 'Late'; b.style.cssText = 'position:fixed;right:0;top:100px'; document.body.append(b); }, 300)\">Warn</button>\
<iframe data-testid=\"CodeEditorInput\" style=\"width:300px;height:80px;border:0\" srcdoc=\"<div class=cm-content><div class=cm-line>line one</div><div class=cm-line>line two</div></div>\"></iframe>\
<div data-testid=\"A\"><div data-testid=\"B\"><span data-testid=\"C\">c</span></div><div data-testid=\"D\">d</div></div><div data-testid=\"Plain\" style=\"width:40px;height:10px\"></div>\
<div data-testid=\"Row\" style=\"position:fixed;left:0;top:400px;width:300px;height:40px;overflow-x:auto;white-space:nowrap\"><span style=\"display:inline-block;width:500px\">row</span><button data-testid=\"End\">End</button></div>\
<button data-testid=\"Spawn\" style=\"position:fixed;right:0;top:160px\" onclick=\"const s = document.createElement('button'); s.dataset.testid = 'Slider'; s.textContent = 'S'; s.style.cssText = 'position:fixed;left:0;top:300px;width:30px;height:30px;padding:0'; s.onclick = () => log.push('slider@' + parseInt(s.style.left)); document.body.append(s); let n = 0; const t = setInterval(() => { n += 1; s.style.left = n * 40 + 'px'; if (n === 8) clearInterval(t); }, 60)\">Spawn</button>\
<button data-testid=\"Delay\" style=\"position:fixed;right:0;top:220px\" onclick=\"setTimeout(() => { const p = document.createElement('p'); p.dataset.testid = 'Delayed'; p.textContent = 'Delayed'; p.style.cssText = 'position:fixed;left:0;top:250px;margin:0'; document.body.append(p); }, 800)\">Delay</button>\
<div style=\"height:2000px\"></div><button data-testid=\"Below\">Below</button>\
<script>window.log = []; console.info('loaded'); console.warn('while loading');</script></body>";

#[test]
fn the_device_verbs_find_clip_tap_and_read_a_framed_page_without_writing_to_it() {
    let s = browser_session("wf-flow-browser-finder-", &serve_page(FINDER_PAGE));
    s.wf(&["start-device", "--device", "t"]);
    s.wf(&["open-link", &s.base, "--device", "t"]);
    let verb = |args: &[&str]| {
        let r = s.wf(args);
        let v: Value = serde_json::from_str(r.stdout.trim()).unwrap_or_else(|e| panic!("{args:?}: {e}: {}{}", r.stdout, r.stderr));
        (r.exit_code, v)
    };
    let result = |args: &[&str]| {
        let (code, v) = verb(args);
        assert_eq!(code, 0, "{args:?}: {v}");
        v["result"].clone()
    };
    let app = |expr: &str| -> Value { serde_json::from_str(parse(s.rpc("outerEval", json!({ "name": "t", "expression": expr }))["result"].as_str().unwrap()).as_str().unwrap()).unwrap() };

    // When a test's session is opened beside the hand-opened one, and elements are looked up with no --device
    let opened = s.rpc("open", json!({ "name": "owned", "renderMode": "plain", "headless": true, "owner": std::process::id() }));
    let clip = result(&["get-element", "--id", "Clipped"]);
    let hello = result(&["get-element", "hello WORLD"]);
    let name = result(&["get-element", "--id", "Name"]);
    let editor = result(&["get-element", "--id", "CodeEditorInput"]);
    let below = result(&["get-element", "--id", "Below"]);
    let nested = result(&["get-element", "--id", "[A-D]"]);
    let plain = result(&["get-element", "--id", "Plain"]);
    let buffer_matched = result(&["get-element", "--id", "CodeEditorInput", "--text", ".*line one.*"]);
    let line = result(&["get-element", "line one"]);
    // Then the owned session is left out, the clipped button is only its on-screen part, text matches whole and ignoring case
    // on the visible copy, an input reads as its value, the editor's web view as its lines, and an element below the fold is nil
    assert_eq!(opened["owner"], json!(std::process::id()), "{opened}");
    assert_eq!(clip, json!({ "bounds": { "left": 80, "top": 0, "right": 120, "bottom": 40, "cx": 100, "cy": 20 }, "text": "Clip" }));
    assert_eq!((hello["text"].clone(), hello["bounds"]["top"].as_i64().is_some_and(|t| t >= 60)), (json!("Hello world"), true), "{hello}");
    assert_eq!(name["text"], "Ada");
    assert_eq!(editor["text"], "line one\nline two");
    assert_eq!(below, Value::Null);
    // and of nested matches the first with no match inside it wins, an element with no text or label reads as null, and the
    // editor's buffer is its text but never matched, while a line of it matches inside the web view
    assert_eq!(nested["text"], "c", "{nested}");
    assert_eq!(plain["text"], Value::Null, "{plain}");
    assert_eq!(buffer_matched, Value::Null);
    assert_eq!(line["text"], "line one");
    assert!(app("JSON.stringify(document.querySelectorAll('[data-wf-text-target]').length)") == json!(0));

    // When a condition met at its third check is waited for, checked every 400 ms, on a page that does not change (its
    // documents already watched by a wait before it), and one met at its fourth, checked every 1000 ms, while the page
    // changes every 5 ms
    let third = ["extended-wait-until", "--condition", "(function() k = (k or 0) + 1; return k >= 3 and k end)()", "--interval", "400", "--timeout", "5000"];
    result(&third);
    let still = result(&third);
    app("JSON.stringify(window.churn = setInterval(() => { document.body.dataset.tick = String(Date.now()); }, 5))");
    let busy = result(&["extended-wait-until", "--condition", "(function() k = (k or 0) + 1; return k >= 4 and k end)()", "--interval", "1000", "--timeout", "5000"]);
    app("JSON.stringify(clearInterval(window.churn) || 0)");
    // Then with nothing changing each pause runs its whole interval, and while the page changes all the time a check still
    // comes at least 50 ms after the last (the interval's minimum), never back to back, yet long before the interval
    assert!(still["value"].as_f64() == Some(3.0) && still["ms"].as_u64().is_some_and(|ms| ms >= 790), "{still}");
    assert!(busy["value"].as_f64() == Some(4.0) && busy["ms"].as_u64().is_some_and(|ms| (150..2000).contains(&ms)), "{busy}");

    // When the clipped button is tapped, the field is typed into before and after it is tapped, and a button logs and adds one
    let tap = result(&["tap-on", "--id", "Clipped", "--device", "t"]);
    let (refused, why) = verb(&["input-text", "x"]);
    let keyboard_down = result(&["get-device-state", "keyboard"]);
    result(&["tap-on", "--id", "Name"]);
    let keyboard_up = result(&["get-device-state", "keyboard"]);
    result(&["input-text", "Q"]);
    result(&["tap-on", "--text", "warn"]);
    let late = result(&["extended-wait-until", "--visible", "--id", "Late", "--timeout", "5000"]);
    let gone = result(&["extended-wait-until", "--not-visible", "--id", "Below", "--timeout", "0"]);
    // Then the tap lands inside the visible part, typing waits for the keyboard the tap raised, the wait meets the late button,
    // and the console holds the page's lines from its load on, at their levels, without the device frame's own
    let (x, y) = (tap["x"].as_f64().unwrap(), tap["y"].as_f64().unwrap());
    assert!((88.0..=112.0).contains(&x) && (8.0..=32.0).contains(&y), "{tap}");
    assert_eq!(app("JSON.stringify(window.log)"), json!(["clip"]));
    assert!(refused == 1 && why["error"].as_str().unwrap().contains("the keyboard is down -- tap the field first"), "{why}");
    assert_eq!(keyboard_down, json!({ "visible": false, "bounds": null }));
    assert!(keyboard_up["visible"] == true && keyboard_up["bounds"]["bottom"].as_i64().unwrap() - keyboard_up["bounds"]["top"].as_i64().unwrap() == 270, "{keyboard_up}");
    assert!(app("JSON.stringify(document.querySelector('[data-testid=Name]').value)").as_str().unwrap().contains('Q'));
    assert!(late["met"] == true && late["value"]["text"] == "Late", "{late}");
    assert_eq!((gone["met"].clone(), gone["value"].clone()), (json!(true), json!(true)));
    let warn = result(&["get-logs"]);
    let info = result(&["get-logs", "--level", "info"]);
    assert_eq!(warn, json!(["warn: while loading", "warn: clicked it (at onclick)"]), "{warn}");
    assert!(info.as_array().unwrap().iter().any(|l| l == "info: loaded"), "{info}");
    assert_eq!(result(&["clear-logs"])["cleared"].as_u64().map(|n| n >= 2), Some(true));
    assert_eq!(result(&["get-logs", "--level", "debug"]), json!([]));

    // When the end of a row that scrolls sideways, 400 px down the screen, is scrolled into view by touch
    let (scrolled, end) = verb(&["scroll-until-visible", "--element-id", "End", "--from-id", "Row", "--direction", "RIGHT", "--device", "t"]);
    // Then the drag moved the row: the window covers the emulated screen (a headless one is 500x180, and a drag below it moved
    // nothing)
    assert!(scrolled == 0 && end["result"]["swipes"].as_u64().is_some_and(|n| n >= 1) && end["result"]["text"] == "End", "{end}");

    // When a button is tapped that slides in a second one (40 px every 60 ms, wider than it, for 480 ms), and the second is
    // tapped at once
    result(&["tap-on", "--id", "Spawn"]);
    result(&["tap-on", "--id", "Slider"]);
    // Then the tap waits in the page until two looks 100 ms apart agree on its box, and lands on it where it stopped
    assert_eq!(app("JSON.stringify(window.log.filter((l) => l.startsWith('slider')))"), json!(["slider@320"]));

    // When a button is tapped that adds an element 800 ms later, and a condition checked every 3 s waits for it
    result(&["tap-on", "--id", "Delay"]);
    let soon = result(&["extended-wait-until", "--condition", "device:get_element({id='Delayed'}) ~= nil", "--interval", "3000", "--timeout", "8000"]);
    // Then the page's change wakes the next check: it is met long before the 3 s a sleep between checks would take, with a
    // check per change (the tap markers fading, the element added), not a busy poll
    assert!(soon["met"] == true && soon["checks"].as_u64().is_some_and(|n| (2..=8).contains(&n)) && soon["ms"].as_u64().is_some_and(|ms| ms < 2000), "{soon}");

    // When a test's script reads an element below the fold, a clipped one, the editor buffer and the logs
    let lua = s.repo.join("below.lua");
    std::fs::write(&lua, format!("device:open_link('{}')\nlocal clip = device:get_element({{id='Clipped'}})\nlog(json.encode({{ seen = device:get_element({{id='Below'}}) ~= nil, clip = clip.bounds.right, editor = device:get_element({{id='CodeEditorInput'}}).text, logs = device:get_logs() }}))", s.base)).unwrap();
    let run = s.wf(&["test", &lua.display().to_string(), "--no-record"]);
    // Then get_element sees only what is on screen, with its on-screen part, and get_logs holds the warning logged while the
    // page loaded
    let line = run.stdout.lines().find(|l| l.starts_with('{')).unwrap_or_else(|| panic!("{}{}", run.stdout, run.stderr));
    let below: Value = serde_json::from_str(line).unwrap();
    assert_eq!((below["seen"].clone(), below["editor"].clone(), below["logs"].clone()), (json!(false), json!("line one\nline two"), json!(["warn: while loading"])), "{below}");
    assert!(below["clip"].as_i64().is_some_and(|r| r < 180), "the clipped element's bounds are its on-screen part: {below}");

    // When a run is stopped while a selector wait still has a minute to go
    let stop = s.repo.join("stop.lua");
    // (timed from the signal: on a loaded host the browser's start and open_link alone can take longer than the bound)
    let signalled = s.repo.join("stop.signalled");
    std::fs::write(&stop, format!("device:open_link('{}')\nos.execute('(sleep 2; touch {}; kill -TERM $PPID) &')\ndevice:extended_wait_until{{visible={{id='Never'}}, timeout=60000}}\nlog('never')", s.base, signalled.display())).unwrap();
    let stopped = s.wf(&["test", &stop.display().to_string(), "--no-record"]);
    let after_signal = std::fs::metadata(&signalled).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
    // Then the wait ends within a slice of the stop, with the stop's reason, instead of waiting out its timeout
    assert!(stopped.exit_code == 124 && after_signal.is_some_and(|d| d.as_secs() < 10) && stopped.stderr.contains("stopped: SIGTERM received") && !stopped.stdout.contains("never"), "{after_signal:?} after the signal: {}{}", stopped.stdout, stopped.stderr);

    // When a wait runs out
    let missed = s.wf_env(&["extended-wait-until", "--visible", "--id", "Nope", "--timeout", "200"], &[("WF_WAIT_DIAGNOSE_MS", "100")]);
    let failure: Value = serde_json::from_str(missed.stdout.trim()).unwrap_or_else(|e| panic!("{e}: {}", missed.stdout));
    // Then its evidence holds the page's hierarchy: the elements on screen with their ids and texts
    let hierarchy: Value = serde_json::from_str(&std::fs::read_to_string(failure["evidence"]["hierarchy"].as_str().unwrap()).unwrap()).unwrap();
    assert!(missed.exit_code == 1 && hierarchy["elements"].as_array().unwrap().iter().any(|e| e["id"] == "Clipped" && e["text"] == "Clip"), "{failure} {hierarchy}");

    // When the screen state and a pixel are read
    let screen = result(&["get-device-state", "screen"]);
    let shot = s.repo.join("shot.png").display().to_string();
    let saved = result(&["take-screenshot", &shot]);
    // Then the screen is the preset's, and the screenshot is it at the pixel ratio
    assert_eq!(screen, json!({ "width": 393, "height": 852, "dpr": 3 }));
    assert!(saved["width"].as_u64().unwrap() >= 393 * 3, "{saved}");
}

/// The lesson pad as e2e/tamil-writing.lua sees it: a three-stroke letter whose coach names the stroke whose guide shows, one
/// frame-ish (50 ms) after a stroke ends, and the score badge once the last is in.
const TAMIL_PAD_PAGE: &str = "<!doctype html><title>pad</title><meta name=viewport content=\"width=device-width,initial-scale=1\"><body style=\"margin:0;font:16px sans-serif\">\
<p data-testid=\"TamilPracticeStrokeStep\" style=\"margin:16px\"></p>\
<div data-testid=\"TamilPracticeCanvasSurface\" style=\"margin:16px;width:300px;height:300px;border:1px solid #ccc;touch-action:none\"><svg width=\"300\" height=\"300\"></svg></div>\
<script>const guides = ['M 30 60 L 270 60', 'M 150 90 L 150 260', 'M 30 280 L 270 280']; let index = 0;\
const pad = document.querySelector('[data-testid=TamilPracticeCanvasSurface]'), step = document.querySelector('[data-testid=TamilPracticeStrokeStep]');\
const show = () => { const g = guides[index] || ''; pad.dataset.activeGuide = g; const svg = pad.querySelector('svg');\
svg.innerHTML = g ? `<path data-testid=\"TamilPracticeCanvasActiveGuide\" d=\"${g}\" stroke=\"green\" stroke-width=\"10\" fill=\"none\"/>` : '';\
step.textContent = g ? `Stroke ${index + 1} of ${guides.length}` : `All ${guides.length} strokes written`;\
if (!g) { const b = document.createElement('span'); b.dataset.testid = 'TamilPracticeStrokeScore'; b.textContent = '100% ON THE LINE'; step.after(b); } };\
pad.addEventListener('pointerup', () => setTimeout(() => { index += 1; show(); }, 50)); show();</script></body>";

#[test]
fn a_traced_letter_goes_on_to_its_next_stroke_as_soon_as_that_strokes_guide_shows() {
    // Given the Tamil writing test case and a pad whose letter takes three strokes, the badge showing only after the last
    let s = browser_session("wf-flow-browser-tamil-", &serve_page(TAMIL_PAD_PAGE));
    std::fs::create_dir_all(s.repo.join("e2e")).unwrap();
    std::fs::copy(repo_root().join("e2e/tamil-writing.lua"), s.repo.join("e2e/tamil-writing.lua")).unwrap();

    // When the test case traces the letter
    let out = s.repo.join("out").display().to_string();
    let base = format!("--env=base={}", s.base);
    let r = s.wf(&["test", "e2e/tamil-writing.lua", "--no-record", "--test-output-dir", "rec", "--env", "part=consonant", &base, "--env", &format!("out={out}")]);

    // Then each stroke is drawn once, and the wait after it ends when the next stroke's guide shows rather than after its
    // 2 s bound: the user starts that stroke at once, and the last one ends at the badge, whose score is read
    let text = format!("{}{}", r.stdout, r.stderr);
    assert_eq!(r.exit_code, 0, "{text}");
    let report: Value = serde_json::from_str(&std::fs::read_to_string(s.repo.join("rec/report.json")).unwrap()).unwrap();
    let (waits, gaps) = after_each_draw(&report);
    assert_eq!(waits, ["hit", "hit", "hit"], "{gaps:?}");
    assert!(gaps.iter().all(|&ms| ms < 2000), "ms from one stroke to the next: {gaps:?}");
    assert!(text.contains("uyirmei-k letter 1 attempt 1: drew=true, next guide: Stroke 2 of 3") && text.contains("uyirmei-k letter 1 attempt 2: drew=true, next guide: Stroke 3 of 3"), "{text}");
    assert!(text.contains("uyirmei-k/1\t100% ON THE LINE\tok"), "{text}");
}

#[test]
fn an_agent_debugs_a_page_by_its_source_mapped_lines_without_touching_the_code() {
    let s = browser_session("wf-flow-browser-debug-", DEBUG_SERVER);
    let out = |args: &[&str]| {
        let o = s.wf(args);
        assert_eq!(o.exit_code, 0, "{args:?}: {}{}", o.stdout, o.stderr);
        line(&o)["result"].clone()
    };

    // Given a recorded page whose bundle maps to src/math.ts
    let video = s.root.path().join("v.mp4").display().to_string();
    s.open("a", json!({ "video": true, "videoPath": video }));
    s.rpc("navigate", json!({ "name": "a", "url": s.base }));

    // When an unknown file is asked for, then a logpoint on `return sum;` with two captures, and the page calls the function
    // (a logpoint is armed by the test, `test --logpoint`, over the daemon's debugSet)
    let unknown = s.call("debugSet", json!({ "name": "a", "file": "src/nope.ts", "line": 1 }));
    let set = s.rpc("debugSet", json!({ "name": "a", "file": "src/math.ts", "line": 3, "capture": "sum,a + b" }));
    let returned = s.eval_json("a", "JSON.stringify(window.run())");
    let hits = s.rpc("debugHits", json!({ "name": "a" }))["hits"].as_array().cloned().unwrap();
    // Then the unknown file names the files the page has maps for, the point resolves, the page is not held up, and one record
    // carries the stack at the original line, the locals and the captures, with no input action before it
    let unknown = unknown.unwrap_err();
    assert!(unknown.contains("src/math.ts") && unknown.contains("src/nope.ts"), "{unknown}");
    assert_eq!(set["resolved"], json!(true), "{set}");
    assert_eq!((returned, hits.len()), (json!(5), 1));
    let hit = &hits[0];
    assert_eq!((hit["file"].clone(), hit["line"].clone(), hit["function"].clone(), hit["frames"][0]["function"].clone(), hit["frames"][0]["file"].clone()), (json!("src/math.ts"), json!(3), json!("add"), json!("add"), json!("src/math.ts")));
    assert_eq!((hit["captures"]["sum"].clone(), hit["captures"]["a + b"].clone(), hit["locals"]["a"].clone(), hit["locals"]["b"].clone()), (json!(5), json!(5), json!(2), json!(3)));
    assert_eq!((hit["source"].clone(), hit["action"].clone()), (json!({ "file": "src/math.ts", "line": 3, "column": 1, "mapped": true }), json!(null)));
    assert!(hit["at_ms"].as_u64().is_some(), "{hit}");

    // When the button is tapped twice, and the recording stopped (a line is written once the video holds its frame)
    for _ in 0..2 {
        s.rpc("tap", json!({ "name": "a", "ref": "#go" }));
    }
    s.rpc("recordStop", json!({ "name": "a" }));
    // Then each new hit names the tap that caused it, as the input log beside the video does
    let actions: Vec<Value> = s.rpc("debugHits", json!({ "name": "a" }))["hits"].as_array().unwrap().iter().skip(1).map(|h| h["action"].clone()).collect();
    assert_eq!(actions, [json!(1), json!(2)]);
    let logged: Vec<Value> = std::fs::read_to_string(s.root.path().join("v.inputs.jsonl")).unwrap().lines().map(|l| parse(l)["action"].clone()).collect();
    assert_eq!(logged, [json!(1), json!(2)]);

    // When a breakpoint whose condition is false is set on another device
    s.wf(&["start-device", "--device", "c"]);
    s.wf(&["open-link", &s.base, "--device", "c"]);
    let conditional = out(&["set-breakpoint", "src/math.ts:3:a > 10", "--device", "c"]);
    s.eval_json("c", "JSON.stringify(window.run())");
    // Then it resolves, nothing holds the page and nothing is recorded
    assert_eq!(conditional["resolved"], json!(true), "{conditional}");
    assert_eq!(s.rpc("debugHits", json!({ "name": "c" }))["hits"], json!([]));

    // Given a page whose timer runs tick() (src/loop.ts) every second, slower than the device frame's own 300 ms timer
    s.wf(&["start-device", "--device", "l"]);
    s.wf(&["open-link", &format!("{}/loop.html", s.origin), "--device", "l"]);
    // When stepping and evaluating before any pause
    let early_step = s.wf(&["debug-step", "--device", "l"]);
    let early_eval = s.wf(&["debug-eval", "ticks", "--device", "l"]);
    // Then both fail saying it is not paused and how to pause
    let (step_error, eval_error) = (line(&early_step)["error"].as_str().unwrap_or_default().to_string(), line(&early_eval)["error"].as_str().unwrap_or_default().to_string());
    assert!(early_step.exit_code == 1 && step_error.contains("not paused") && step_error.contains("debug-pause") && step_error.contains("set-breakpoint"), "{}", early_step.stdout);
    assert!(early_eval.exit_code == 1 && eval_error.contains("not paused"), "{}", early_eval.stdout);

    // When it is paused, stepped over, read, stepped sideways, and resumed
    let paused = out(&["debug-pause", "--device", "l"]);
    let stepped = out(&["debug-step", "over", "--device", "l"]);
    let ticks = out(&["debug-eval", "ticks", "--device", "l"]);
    let next = out(&["debug-eval", "next", "--device", "l"]);
    let sideways = s.wf(&["debug-step", "sideways", "--device", "l"]);
    let resumed = out(&["debug-resume", "--device", "l"]);
    // Then it is held at tick's first statement in the original file, then on the next line where the local has its value; an
    // unknown step kind names the kinds; and once resumed the timer counts again
    assert_eq!((paused["paused"].clone(), paused["function"].clone(), paused["file"].clone(), paused["line"].clone(), paused["frames"][0]["mapped"].clone()), (json!(true), json!("tick"), json!("src/loop.ts"), json!(3), json!(true)), "{paused}");
    assert!(paused["locals"].as_object().is_some_and(|l| l.contains_key("next")), "{paused}");
    assert_eq!((stepped["function"].clone(), stepped["line"].clone()), (json!("tick"), json!(4)), "{stepped}");
    assert_eq!((next["type"].clone(), next["value"].as_i64(), next["file"].clone(), next["line"].clone()), (json!("number"), ticks["value"].as_i64().map(|t| t + 1), json!("src/loop.ts"), json!(4)), "{ticks} {next}");
    assert!(sideways.exit_code == 2 && line(&sideways)["error"].as_str().unwrap().contains("over, into or out"), "{}", sideways.stdout);
    assert_eq!(resumed, json!({ "resumed": true, "paused": false }));
    let before = s.eval_json("l", "JSON.stringify(window.ticks)");
    std::thread::sleep(Duration::from_millis(1200));
    assert!(s.eval_json("l", "JSON.stringify(window.ticks)").as_i64() > before.as_i64());
}

/// The UA names the installed Chromium's major version, which differs between a developer machine and the runner.
fn without_chrome_major(mut state: Value) -> Value {
    if let Some(ua) = state["ua"].as_str() {
        state["ua"] = json!(Regex::new(r"Chrome/\d+\.").unwrap().replace(ua, "Chrome/N.").into_owned());
    }
    state
}

fn chromium_path() -> Option<PathBuf> {
    let browsers = wf_harness::testing::browsers_dir();
    let mut dirs: Vec<String> = std::fs::read_dir(&browsers).ok()?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|d| d.starts_with("chromium-")).collect();
    dirs.sort();
    dirs.reverse();
    dirs.iter().flat_map(|d| ["chrome-linux/chrome", "chrome-linux64/chrome"].map(|rel| PathBuf::from(&browsers).join(d).join(rel))).find(|p| p.exists())
}

/// A Chromium someone else launched, a repo and a registry; the daemon's `open` with a `cdpEndpoint` attaches it (the RPC
/// the cookie-capture service calls).
struct Attached {
    _root: tempfile::TempDir,
    repo: PathBuf,
    reg_dir: PathBuf,
    endpoint: String,
    chrome: Child,
    _gate: crate::support::ChromiumGate,
}

impl Attached {
    fn start() -> Attached {
        require_browser();
        let gate = chromium_gate();
        let chromium = chromium_path().expect("a Chromium binary under PLAYWRIGHT_BROWSERS_PATH");
        let root = tempfile::Builder::new().prefix("wf-flow-browser-attach-").tempdir().unwrap();
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let chrome = Command::new(chromium)
            .args(["--headless=new", "--no-sandbox", "--disable-gpu", "--disable-dev-shm-usage", &format!("--remote-debugging-port={port}"), &format!("--user-data-dir={}", root.path().join("profile").display()), "--no-first-run", "about:blank"])
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(root.path().join("chromium.log")).expect("a log file for Chromium"))
            .spawn()
            .expect("start Chromium");
        let ready = (0..300).any(|_| {
            let up = TcpStream::connect(("127.0.0.1", port)).is_ok() && http_get(&format!("127.0.0.1:{port}"), "/json/version").0 == 200;
            if !up {
                std::thread::sleep(Duration::from_millis(100));
            }
            up
        });
        assert!(ready, "Chromium did not open its DevTools port");
        let (repo, reg_dir) = (root.path().join("repo"), root.path().join("reg"));
        std::fs::create_dir(&repo).unwrap();
        std::fs::create_dir(&reg_dir).unwrap();
        assert!(Command::new("git").args(["init", "-q", "-b", "main"]).current_dir(&repo).status().unwrap().success());
        std::fs::create_dir_all(repo.join("tooling/browser-harness")).unwrap();
        std::os::unix::fs::symlink(repo_root().join("tooling/browser-harness/dist"), repo.join("tooling/browser-harness/dist")).unwrap();
        Attached { _root: root, repo, reg_dir, endpoint: format!("http://127.0.0.1:{port}"), chrome, _gate: gate }
    }

    fn wf_env(&self, args: &[&str], env: &[(&str, &str)]) -> RunResult {
        let mut full = vec!["browser"];
        full.extend_from_slice(args);
        let mut opts = RunOpts::default().cwd(&self.repo).timeout(Duration::from_secs(60)).env("WF_REGISTRY_DIR", &self.reg_dir.display().to_string());
        for (key, value) in env {
            opts = opts.env(key, value);
        }
        run_wf(&full, &opts)
    }

    fn wf(&self, args: &[&str]) -> RunResult {
        self.wf_env(args, &[])
    }

    /// The daemon (`wf browser start` when none runs, in `env`), as @lib/browser-client finds it.
    fn daemon(&self, env: &[(&str, &str)]) -> String {
        if let Some(url) = daemon_url(&self.reg_dir) {
            return url;
        }
        let started = self.wf_env(&[&["start", "--json"][..]].concat(), env);
        parse(started.stdout.trim().lines().last().unwrap_or_default())["result"]["url"].as_str().expect("start names the daemon's url").to_string()
    }

    fn call(&self, method: &str, mut args: Value) -> Result<Value, String> {
        if args.get("name").is_none() && !matches!(method, "list" | "tailnetForward") {
            args["name"] = json!("a");
        }
        call_at(&self.daemon(&[]), method, args)
    }

    fn rpc(&self, method: &str, args: Value) -> Value {
        self.call(method, args.clone()).unwrap_or_else(|e| panic!("{method} {args}: {e}"))
    }

    fn open_in(&self, env: &[(&str, &str)]) -> Result<Value, String> {
        call_at(&self.daemon(env), "open", json!({ "name": "a", "devicePreset": "iphone-15-pro", "orientation": "portrait", "colorScheme": "system", "frame": "screen", "cdpEndpoint": self.endpoint }))
    }

    fn open(&self) -> Result<Value, String> {
        self.open_in(&[])
    }

    fn go(&self, url: &str) -> Result<Value, String> {
        self.call("navigate", json!({ "url": url }))
    }

    /// The page's value of `expression` as the eval RPC answers it: a JSON text.
    fn eval(&self, expression: &str) -> String {
        self.rpc("eval", json!({ "expression": expression }))["result"].as_str().unwrap_or_default().to_string()
    }

    fn eval_json(&self, expression: &str) -> Value {
        parse(parse(&self.eval(expression)).as_str().expect("a JSON text"))
    }

    /// `window.log` as `[kind, x, y]` rows.
    fn events(&self) -> Vec<(String, f64, f64)> {
        parse(&self.eval("window.log")).as_array().unwrap().iter().map(|e| (e[0].as_str().unwrap().to_string(), e[1].as_f64().unwrap_or(f64::NAN), e[2].as_f64().unwrap_or(f64::NAN))).collect()
    }
}

impl Drop for Attached {
    fn drop(&mut self) {
        if std::thread::panicking() {
            let log = std::fs::read_to_string(self._root.path().join("chromium.log")).unwrap_or_default();
            let tail: Vec<&str> = log.lines().rev().take(30).collect();
            eprintln!("chromium.log tail (newest first):\n{}", tail.join("\n"));
        }
        self.wf(&["stop"]);
        let _ = self.chrome.kill();
        let _ = self.chrome.wait();
    }
}

#[test]
fn a_browser_the_daemon_launches_is_given_no_flag_that_trusts_an_insecure_origin() {
        let a = Attached::start();
    // Given a Chromium that only records the command line it was started with, then exits
    let (log, stub) = (a.repo.join("chromium-args.txt"), a.repo.join("chromium-stub.sh"));
    std::fs::write(&stub, format!("#!/bin/sh\necho \"$@\" >> \"{}\"\nexit 1\n", log.display())).unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    // When the daemon is asked to open a browser of its own
    let opened = call_at(&a.daemon(&[("WF_CHROMIUM", stub.to_str().unwrap())]), "open", json!({ "name": "b", "renderMode": "plain" }));
    // Then it launched the stub with its usual flags and none that treats an origin as secure: a page served over plain http from
    // anywhere but loopback has no microphone or camera, as on a phone
    assert!(opened.as_ref().is_err_and(|e| e.contains("exited before opening its DevTools port")), "{opened:?}");
    let args = std::fs::read_to_string(&log).unwrap();
    assert!(args.contains("--autoplay-policy=no-user-gesture-required") && args.contains("--disable-dev-shm-usage"), "{args}");
    assert!(!args.contains("unsafely-treat-insecure-origin-as-secure"), "{args}");
}

fn attached_summary() -> Value {
    json!({ "name": "a", "engine": "chromium", "renderMode": "plain", "devicePreset": "iphone-15-pro", "orientation": "portrait", "keyboardMode": "none", "colorScheme": "system", "video": false, "active": true, "targetUrl": null, "systemFont": null })
}

fn snapshot_text(yaml: &Value) -> String {
    Regex::new(r"\[ref=f\d+e").unwrap().replace_all(yaml["yaml"].as_str().unwrap_or_default(), "[ref=e").replace(" [active]", "")
}

fn lines(items: &[&str]) -> String {
    items.join("\n")
}

#[test]
fn an_attached_session_reads_pages_and_manages_its_lifecycle() {
    let a = Attached::start();

    // When the session attaches, navigates and evaluates
    let opened = a.open();
    let nav = a.go("data:text/html,<title>t</title>");
    // Then the summary comes back in its field order, and values come back as JSON (undefined as null)
    let summary = opened.unwrap();
    assert_eq!((summary.clone(), summary.as_object().unwrap().keys().collect::<Vec<_>>()), (attached_summary(), attached_summary().as_object().unwrap().keys().collect::<Vec<_>>()));
    assert_eq!(nav, Ok(json!({ "navigated": "data:text/html,<title>t</title>" })));
    assert_eq!((a.eval("document.title"), a.eval("({a:1,b:[1,2]})"), a.eval("undefined")), ("\"t\"".to_string(), "{\"a\":1,\"b\":[1,2]}".to_string(), "null".to_string()));

    // When the sessions and the daemon are listed
    a.go("data:text/html,<p>x</p>").unwrap();
    let mut listed = attached_summary();
    listed["targetUrl"] = json!("data:text/html,<p>x</p>");
    let status = parse(&a.wf(&["status"]).stdout);
    let devices = parse(a.wf(&["list-devices"]).stdout.trim())["result"].clone();
    // Then the session shows its target URL and the daemon its URL and browsers
    assert_eq!(a.rpc("list", json!({})), json!([listed]));
    assert_eq!(devices["devices"], json!([listed]));
    assert!(Regex::new(r"^http://127\.0\.0\.1:\d+$").unwrap().is_match(status["url"].as_str().unwrap()));
    assert_eq!((status["activeBrowserName"].clone(), status["browserCount"].clone()), (json!("a"), json!(1)));
    assert_eq!(a.rpc("cookies", json!({})), json!({ "cookies": [] }));

    // When a page is screenshotted and scrolled
    a.go("data:text/html,<body style=\"margin:0;background:teal\"><div style=\"height:3000px\"></div>").unwrap();
    let shot = a.rpc("screenshot", json!({}));
    let scrolled = a.rpc("scroll", json!({ "x": 0, "y": 300 }));
    a.rpc("scroll", json!({ "x": 0, "y": 50 }));
    // Then a PNG lands under .screenshots, and the window moves by the amounts asked for
    let path = shot["path"].as_str().unwrap();
    assert!(Regex::new(r"/\.screenshots/app-0001-\d{4}-\d\d-\d\dT\d\d-\d\d-\d\d-\d{3}Z\.png$").unwrap().is_match(path), "{path}");
    assert_eq!(&std::fs::read(path).unwrap()[1..4], b"PNG");
    assert_eq!((scrolled["scrolled"].clone(), a.eval("window.scrollY")), (json!({ "x": 0, "y": 300 }), "350".to_string()));

    // When pages with structure, labels, states and tables are described
    a.go("data:text/html,<h1>Title</h1><button>Go</button><a href=\"/x\">link</a><input placeholder=\"name\"><ul><li>one</li><li>two</li></ul><p>para <b>bold</b></p>").unwrap();
    let simple = snapshot_text(&a.rpc("snapshot", json!({})));
    a.go("data:text/html,<nav><a href=\"/a\">A</a> <a href=\"/b\">B</a></nav><main><h2>Sec</h2><label>Email <input type=email value=\"x@y\"></label><input type=checkbox checked aria-label=\"agree\"><select aria-label=\"pick\"><option>one<option selected>two</select><textarea aria-label=\"t\">hi</textarea><img alt=\"logo\" src=\"x.png\"><button disabled>Off</button><div role=dialog aria-label=\"D\"><span>in dialog</span></div><div hidden>hidden</div><div style=\"display:none\">none</div>text node<table><tr><th>H</th></tr><tr><td>C</td></tr></table></main>").unwrap();
    let rich = snapshot_text(&a.rpc("snapshot", json!({})));
    // Then roles, names, states and refs are printed, hidden content left out
    assert_eq!(simple, lines(&["- generic [ref=e1]:", "  - heading \"Title\" [level=1] [ref=e2]", "  - button \"Go\" [ref=e3]", "  - link \"link\" [ref=e4] [cursor=pointer]:", "    - /url: /x", "  - textbox \"name\" [ref=e5]", "  - list [ref=e6]:", "    - listitem [ref=e7]: one", "    - listitem [ref=e8]: two", "  - paragraph [ref=e9]: para bold"]));
    assert_eq!(
        rich,
        lines(&[
            "- generic [ref=e1]:", "  - navigation [ref=e2]:", "    - link \"A\" [ref=e3] [cursor=pointer]:", "      - /url: /a", "    - link \"B\" [ref=e4] [cursor=pointer]:", "      - /url: /b", "  - main [ref=e5]:", "    - heading \"Sec\" [level=2] [ref=e6]", "    - generic [ref=e7]:", "      - text: Email", "      - textbox \"Email\" [ref=e8]: x@y",
            "    - checkbox \"agree\" [checked] [ref=e9]", "    - combobox \"pick\" [ref=e10]:", "      - option \"one\"", "      - option \"two\" [selected]", "    - textbox \"t\" [ref=e11]: hi", "    - img \"logo\" [ref=e12]", "    - button \"Off\" [disabled] [ref=e13]", "    - dialog \"D\" [ref=e14]: in dialog", "    - text: text node",
            "    - table [ref=e15]:", "      - rowgroup [ref=e16]:", "        - row [ref=e17]:", "          - columnheader \"H\" [ref=e18]", "        - row [ref=e19]:", "          - cell \"C\" [ref=e20]"
        ])
    );

    // When a page cut off at the right edge is checked for clipping
    a.go("data:text/html,<body style=\"margin:0\"><button class=ok style=\"width:100px;height:30px\">ok</button><button class=cut style=\"position:absolute;top:60px;left:99999px;width:100px;height:30px\">cut</button>").unwrap();
    let clipping = a.rpc("checkClipping", json!({}));
    // Then only the cut control is reported with its geometry
    let classes: Vec<&str> = clipping["clipped"].as_array().unwrap().iter().map(|c| c["cls"].as_str().unwrap()).collect();
    assert!(classes.contains(&"cut") && !classes.contains(&"ok"), "{clipping}");
    assert!(clipping["clippedCount"].as_f64() == Some(classes.len() as f64) && clipping["checked"].as_f64() >= clipping["clippedCount"].as_f64());

    // When the same name is opened twice, an unknown name is used, a closed port is navigated to, and a script throws
    let twice = a.open();
    let unknown = a.call("navigate", json!({ "name": "zz", "url": "about:blank" }));
    let closed_port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let refused = a.go(&format!("http://127.0.0.1:{closed_port}/"));
    let thrown = a.call("eval", json!({ "expression": "(() => { throw new Error('kaboom') })()" }));
    // Then each fails naming the browser, the known names, the net error or the thrown error
    assert!(twice.as_ref().unwrap_err().contains("Browser \"a\" already exists"), "{twice:?}");
    assert!(unknown.as_ref().unwrap_err().contains("No browser named \"zz\" (known: a)"), "{unknown:?}");
    assert!(Regex::new(r"^page\.goto: net::ERR_CONNECTION_REFUSED at ").unwrap().is_match(refused.as_ref().unwrap_err()), "{refused:?}");
    assert!(thrown.as_ref().unwrap_err().contains("kaboom"), "{thrown:?}");

    // When the device is stopped, opened again, and the daemon stopped
    let close = parse(a.wf(&["stop-device", "--device", "a"]).stdout.trim());
    let empty = a.rpc("list", json!({}));
    let reopened = a.open().unwrap();
    let stopped = a.wf(&["stop"]);
    // Then it is gone, attaches again, and the registry forgets the daemon
    assert_eq!((close["result"].clone(), empty, reopened), (json!({ "closed": ["a"] }), json!([]), attached_summary()));
    assert_eq!(stopped.stdout, "Browser daemon stopped\n");
    let registry = parse(&std::fs::read_to_string(a.reg_dir.join("registry.json")).unwrap());
    assert!(registry["slots"].as_object().unwrap().values().all(|s| s.get("browserMcp").is_none()));
}

const INTERACTIVE: &str = "data:text/html,<button id=b>Go</button><input id=i value=ab><script>window.log=[];document.addEventListener('click',e=>log.push('click:'+e.target.id));document.addEventListener('keydown',e=>log.push('key:'+e.key));</script>";

#[test]
fn an_attached_session_taps_types_drags_strokes_and_swipes_like_a_user() {
    let a = Attached::start();
    a.open().unwrap();

    // When an element is tapped by its snapshot ref and by a selector, and a selector matching nothing is tapped
    a.go(INTERACTIVE).unwrap();
    let snapshot = a.rpc("snapshot", json!({}))["yaml"].as_str().unwrap().to_string();
    let reference = Regex::new(r#"button "Go" \[ref=(f?\d*e\d+)\]"#).unwrap().captures(&snapshot).map(|c| c[1].to_string()).unwrap_or_else(|| "missing".into());
    let by_ref = a.rpc("tap", json!({ "ref": reference, "indicator": false }));
    let by_selector = a.rpc("tap", json!({ "ref": "#b", "indicator": false }));
    let nothing = a.call("tap", json!({ "ref": "#nope", "indicator": false }));
    let no_focus = a.call("press", json!({ "key": "Enter", "repeat": 1 }));
    // Then it is clicked at its centre with a mouse, the missing ref is named, and a key with nothing focused says to tap a field
    let at = |t: &Value| (t["tapped"].clone(), t["x"].as_f64(), t["y"].as_f64(), t["pointer"].clone());
    assert_eq!((at(&by_ref), at(&by_selector)), ((json!(reference), Some(25.0), Some(19.0), json!("mouse")), (json!("#b"), Some(25.0), Some(19.0), json!("mouse"))));
    assert_eq!(a.eval_json("JSON.stringify(window.log)"), json!(["click:b", "click:b"]));
    assert!(nothing.as_ref().unwrap_err().contains("tap: no element resolved for ref \"#nope\""), "{nothing:?}");
    assert!(no_focus.as_ref().unwrap_err().contains("press: nothing editable has focus -- tap a field first"), "{no_focus:?}");

    // When a field is typed into, keyed and submitted
    a.go(INTERACTIVE).unwrap();
    let typed = a.rpc("type", json!({ "ref": "#i", "text": "hello", "indicator": false }));
    let pressed = a.rpc("press", json!({ "key": "Backspace", "repeat": 2 }));
    let enter = a.rpc("press", json!({ "key": "Enter", "repeat": 1 }));
    let keyed = a.eval_json("JSON.stringify([window.log, document.getElementById(\"i\").value])");
    a.go(INTERACTIVE).unwrap();
    a.rpc("type", json!({ "ref": "#i", "text": "X", "submit": true, "indicator": false }));
    // Then the field and the key log follow
    assert_eq!((typed["typed"].clone(), pressed, enter), (json!("#i"), json!({ "key": "Backspace", "repeat": 2 }), json!({ "key": "Enter", "repeat": 1 })));
    assert_eq!(keyed, json!([["key:Backspace", "key:Backspace", "key:Enter"], "hel"]));
    assert_eq!(a.eval_json("JSON.stringify([window.log, document.getElementById(\"i\").value])"), json!([["key:Enter"], "X"]));

    // When text is typed at a caret, with human timing, and with the focus elsewhere
    a.go("data:text/html,<input id=i value=xy><input id=j><script>window.keys=[];i.addEventListener('keydown',e=>keys.push(e.key));i.focus();i.setSelectionRange(1,1)</script>").unwrap();
    let out = a.rpc("type", json!({ "ref": "#i", "text": "ab", "atCaret": true, "indicator": false }));
    let human = a.rpc("type", json!({ "ref": "#i", "text": "abc", "atCaret": true, "human": true, "indicator": false }));
    a.eval("j.focus()");
    let elsewhere = a.call("type", json!({ "ref": "#i", "text": "a", "atCaret": true }));
    // Then the keys go one by one into the caret, a thumb's gaps are 150 ms and more, and typing away from the focus is refused
    assert_eq!((out["typed"].as_str(), out["mode"].as_str(), out["keys"].as_f64()), (Some("#i"), Some("caret"), Some(2.0)));
    assert!(out["interKeyP50Ms"].as_f64().unwrap() >= 30.0 && human["interKeyP50Ms"].as_f64().unwrap() >= 150.0, "{out} {human}");
    assert!(a.eval("i.value").starts_with("\"xab"), "{}", a.eval("i.value"));
    assert!(elsewhere.as_ref().unwrap_err().contains("type --at-caret: focus is not in \"#i\" -- tap where the caret should go first"), "{elsewhere:?}");

    // When a box is dragged far and in two steps
    a.go("data:text/html,<div id=d style='position:absolute;left:20px;top:30px;width:60px;height:40px;background:red'></div><script>window.log=[];for(const t of ['mousedown','mousemove','mouseup'])addEventListener(t,e=>log.push(t[5]+':'+e.clientX+','+e.clientY+':'+e.buttons));</script>").unwrap();
    let long = a.rpc("drag", json!({ "ref": "#d", "dx": 120, "dy": 60, "indicator": false }));
    let short = a.rpc("drag", json!({ "ref": "#d", "dx": -10, "dy": 0, "steps": 2, "indicator": false }));
    // Then the page sees a press, equal moves and a release at the same coordinates
    assert_eq!((long["dragged"].clone(), short["dragged"].clone()), (json!("#d"), json!("#d")));
    let mut expected: Vec<String> = vec!["m:50,50:0".into(), "d:50,50:1".into()];
    expected.extend((0..12).map(|i| format!("m:{},{}:1", 60 + i * 10, 55 + i * 5)));
    expected.extend(["u:170,110:0", "m:50,50:0", "d:50,50:1", "m:45,50:1", "m:40,50:1", "u:40,50:0"].map(String::from));
    assert_eq!(a.eval_json("JSON.stringify(window.log)"), json!(expected));

    // When a path is stroked in fractions, then in pixels
    a.go("data:text/html,<div id=d style='position:absolute;left:20px;top:30px;width:100px;height:80px;background:red'></div><script>window.log=[];for(const t of ['mousedown','mousemove','mouseup'])addEventListener(t,e=>log.push(t[5]+':'+e.clientX.toFixed(0)+','+e.clientY.toFixed(0)+':'+e.buttons));</script>").unwrap();
    let fractions = a.rpc("stroke", json!({ "ref": "#d", "points": [{ "x": 0.1, "y": 0.9 }, { "x": 0.5, "y": 0.1 }, { "x": 0.9, "y": 0.9 }], "samplesPerSegment": 3, "indicator": false }));
    let pixels = a.rpc("stroke", json!({ "ref": "#d", "units": "px", "points": [{ "x": 0, "y": 0 }, { "x": 40, "y": 40 }], "samplesPerSegment": 4, "indicator": false }));
    // Then the page sees the sampled points
    let shape = |s: &Value| (s["stroked"].clone(), s["points"].clone(), s["samples"].clone(), s["pointer"].clone());
    assert_eq!((shape(&fractions), shape(&pixels)), ((json!("#d"), json!(3), json!(7), json!("mouse")), (json!("#d"), json!(2), json!(5), json!("mouse"))));
    assert_eq!(a.eval_json("JSON.stringify(window.log)"), json!(["m:30,102:0", "d:30,102:1", "m:43,80:1", "m:56,59:1", "m:70,38:1", "m:83,59:1", "m:96,80:1", "m:110,102:1", "u:110,102:0", "m:20,30:0", "d:20,30:1", "m:30,40:1", "m:40,50:1", "m:50,60:1", "m:60,70:1", "u:60,70:0"]));

    // When a stroke is drawn plainly and as a human, truncated, and wobbled
    a.go("data:text/html,<body style=\"margin:0\"><div id=b style=\"width:300px;height:100px\"></div><script>window.log=[];for(const t of ['mousedown','mousemove','mouseup'])document.addEventListener(t,e=>log.push([t,e.clientX,e.clientY]))</script>").unwrap();
    let two = json!([{ "x": 0, "y": 0 }, { "x": 1, "y": 1 }]);
    let plain = a.rpc("stroke", json!({ "ref": "#b", "indicator": false, "points": two }));
    let human = a.rpc("stroke", json!({ "ref": "#b", "indicator": false, "human": true, "points": two }));
    a.eval("window.log.length = 0");
    let five = json!([{ "x": 0, "y": 0.5 }, { "x": 0.25, "y": 0.5 }, { "x": 0.5, "y": 0.5 }, { "x": 0.75, "y": 0.5 }, { "x": 1, "y": 0.5 }]);
    a.rpc("stroke", json!({ "ref": "#b", "indicator": false, "points": five, "samplesPerSegment": 10, "transform": { "truncate": 0.5 } }));
    let half = a.events();
    a.eval("window.log.length = 0");
    a.rpc("stroke", json!({ "ref": "#b", "indicator": false, "points": five, "samplesPerSegment": 10, "transform": { "wobble": 0.05, "wobbleSeed": 3 } }));
    let wobbled = a.events();
    // Then the human stroke is sampled per frame with more samples, the truncated one stops halfway, and the wobble stays near the line
    assert!(human["samples"].as_f64() > plain["samples"].as_f64() && human["ms"].as_f64().unwrap() >= 240.0, "{plain} {human}");
    assert_eq!(half.last().unwrap(), &("mouseup".to_string(), 150.0, 50.0));
    assert_eq!(wobbled.iter().find(|e| e.0 == "mousedown").unwrap(), &("mousedown".to_string(), 0.0, 50.0));
    assert_eq!(wobbled.last().unwrap(), &("mouseup".to_string(), 300.0, 50.0));
    let moves: Vec<_> = wobbled.iter().filter(|e| e.0 == "mousemove").collect();
    assert!(moves.iter().any(|e| e.2 != 50.0) && moves.iter().all(|e| (e.2 - 50.0).abs() <= 5.0), "{moves:?}");

    // When a scrollable box is swiped from a point, and a swipe names neither a ref nor a point
    a.go("data:text/html,<body style=\"margin:0\"><div id=s style=\"height:400px;overflow:auto\"><div style=\"height:3000px\"></div></div>").unwrap();
    let swipe = a.rpc("swipe", json!({ "from": { "x": 100, "y": 300 }, "dx": 0, "dy": -200, "indicator": false }));
    let nowhere = a.call("swipe", json!({ "dx": 0, "dy": -50 }));
    // Then the scroller under the point moves by what the client reports, and the other says what is missing
    assert_eq!((swipe["from"]["x"].as_f64(), swipe["from"]["y"].as_f64(), swipe["to"]["x"].as_f64(), swipe["to"]["y"].as_f64(), swipe["scrolled"]["dx"].as_f64()), (Some(100.0), Some(300.0), Some(100.0), Some(100.0), Some(0.0)));
    let dy = swipe["scrolled"]["dy"].as_f64().unwrap();
    assert!(dy > 0.0 && a.eval("document.getElementById('s').scrollTop") == format!("{dy}"), "{swipe}");
    assert!(nowhere.as_ref().unwrap_err().contains("swipe: pass --ref or --from"), "{nowhere:?}");

    // When a tap, a drag, a stroke and a swipe run with indicators on, then off, then tinted
    a.go("data:text/html,<body style=\"margin:0\"><div id=b style=\"width:300px;height:100px;overflow:auto\"><div style=\"height:600px\"></div></div>").unwrap();
    let count = |id: &str| -> f64 { a.eval(&format!("document.querySelectorAll('[data-testid=\"{id}\"]').length")).trim().parse().unwrap() };
    a.rpc("tap", json!({ "ref": "#b" }));
    a.rpc("drag", json!({ "ref": "#b", "dx": 40, "dy": 0 }));
    a.rpc("stroke", json!({ "ref": "#b", "points": [{ "x": 0, "y": 0.5 }, { "x": 1, "y": 0.5 }] }));
    a.rpc("swipe", json!({ "from": { "x": 100, "y": 50 }, "dx": 0, "dy": -30 }));
    let shown = (count("wf-browser-tap-indicator"), count("wf-browser-drag-indicator"), count("wf-browser-stroke-indicator"));
    a.eval("document.querySelectorAll(\"[data-testid^=wf-browser]\").forEach(e=>e.remove())");
    a.rpc("tap", json!({ "ref": "#b", "indicator": false }));
    let off = count("wf-browser-tap-indicator");
    a.rpc("tap", json!({ "ref": "#b", "indicator": "#ff0000" }));
    let tint = a.eval("document.querySelector('[data-testid=\"wf-browser-tap-indicator\"]').style.background");
    // Then the page shows the marker of each kind, none when off, and the tint asked for
    assert_eq!((shown, off), ((1.0, 2.0, 1.0), 0.0));
    assert!(tint.contains("rgb(255, 0, 0)"), "{tint}");

    // When files are armed and a file input is tapped, and a missing file is armed
    let file = a.repo.join("note.txt");
    std::fs::write(&file, "hello").unwrap();
    let path = file.display().to_string();
    a.go("data:text/html,<input id=f type=file><script>document.getElementById(\"f\").addEventListener(\"change\",e=>{window.got=[...e.target.files].map(x=>x.name)})</script>").unwrap();
    let armed = a.rpc("upload", json!({ "paths": [path] }));
    a.rpc("tap", json!({ "ref": "#f" }));
    let missing = a.call("upload", json!({ "paths": ["/nope/missing.txt"] }));
    // Then the page receives them without a picker, and the missing file is named
    assert_eq!((armed, a.eval("window.got")), (json!({ "armed": [path], "delivered": [] }), "[\"note.txt\"]".to_string()));
    assert!(missing.as_ref().unwrap_err().contains("upload file not found: /nope/missing.txt"), "{missing:?}");
}

#[test]
fn an_attached_session_proxies_stubs_and_forwards_network_and_feeds_fake_media() {
    let a = Attached::start();
    a.open().unwrap();

    // When a pattern is proxied and the page fetches across origins
    let server = MockServer::start(None);
    let cors = |body: Value| {
        let mut reply = MockReply::json(200, body);
        reply.headers.insert("Access-Control-Allow-Origin".into(), "*".into());
        reply
    };
    server.routes(&[("GET /api", cors(json!({ "ok": 1 }))), ("POST /api/plan", cors(json!({ "live": true }))), ("GET /api/other", cors(json!({ "live": true })))]);
    a.go("data:text/html,<p>x</p>").unwrap();
    let base = format!("http://127.0.0.1:{}", server.port);
    let on = a.rpc("networkProxy", json!({ "pattern": "**/api", "enable": true }));
    let answer = a.eval(&format!("fetch('{base}/api').then(r => r.json())"));
    let proxied: Vec<_> = server.requests().into_iter().filter(|r| r.url == "/api").collect();
    let off = a.rpc("networkProxy", json!({ "pattern": "**/api", "enable": false }));
    // Then the request is sent without browser headers and the page reads the answer
    assert_eq!((on, answer, off), (json!({ "active": true, "pattern": "**/api" }), "{\"ok\":1}".to_string(), json!({ "active": false, "pattern": "**/api" })));
    assert!(proxied.len() == 1 && !proxied[0].headers.contains_key("sec-fetch-site"), "{proxied:?}");

    // When a route is stubbed over the proxy and the page posts to it and fetches another
    server.clear();
    a.rpc("networkProxy", json!({ "pattern": "*/api/*", "enable": true }));
    let stub = a.rpc("networkStub", json!({ "method": "POST", "pattern": "*/api/plan", "body": "{\"edits\":[1]}", "contentType": "application/json", "status": 201 }));
    let stubbed = a.eval(&format!("fetch('{base}/api/plan', {{method: 'POST', headers: {{'content-type': 'application/json'}}, body: '{{}}'}}).then(async r => r.status + ' ' + await r.text())"));
    let other = a.eval(&format!("fetch('{base}/api/other').then(r => r.json())"));
    let hits = a.rpc("networkStubHits", json!({ "clear": false }))["hits"].as_array().cloned().unwrap_or_default();
    let cleared = a.rpc("networkUnstub", json!({}));
    // Then the canned answer wins over the proxy, the other route reaches the server, and the hit is recorded
    assert_eq!((stub["status"].clone(), parse(&stubbed), other), (json!(201), json!("201 {\"edits\":[1]}"), "{\"live\":true}".to_string()));
    let seen: Vec<String> = server.requests().into_iter().map(|r| r.url).collect();
    assert!(!seen.contains(&"/api/plan".to_string()) && seen.contains(&"/api/other".to_string()), "{seen:?}");
    assert_eq!((hits.len(), hits[0]["method"].as_str(), hits[0]["status"].as_i64(), cleared["removed"].clone()), (1, Some("POST"), Some(201), json!(1)));

    // When a tailnet URL is navigated to through a SOCKS listener that joins the tailnet host to the server
    server.routes(&[("GET /hello", MockReply::json(200, json!({ "hi": "tailnet" })))]);
    let log = a.repo.join("socks.log");
    let socks = Aux::spawn(&format!(
        "const net=require('net'),fs=require('fs');net.createServer(c=>{{c.once('data',()=>{{c.write(Buffer.from([5,0]));c.once('data',r=>{{fs.appendFileSync({log:?},r.subarray(5,5+r[4]).toString()+':'+r.readUInt16BE(5+r[4])+'\\n');const up=net.connect({},'127.0.0.1',()=>{{c.write(Buffer.from([5,0,0,1,0,0,0,0,0,0]));c.pipe(up);up.pipe(c)}});up.on('error',()=>c.destroy())}})}})}}).listen(0,'127.0.0.1',function(){{console.log(this.address().port)}})",
        server.port,
        log = log.display().to_string()
    ));
    let socks_port = socks.port.to_string();
    a.wf(&["stop"]);
    a.open_in(&[("WF_SOCKS_PORT", socks_port.as_str())]).unwrap();
    let tailnet = a.go("http://100.101.102.103:8080/hello").unwrap();
    // Then the page loads through a loopback forward and the result names the original URL
    assert_eq!(tailnet["tailnetForwardedFrom"], json!("http://100.101.102.103:8080/hello"));
    assert!(Regex::new(r"^http://127\.0\.0\.1:\d+/hello$").unwrap().is_match(tailnet["navigated"].as_str().unwrap()), "{tailnet}");
    assert!(a.eval("document.body.innerText").contains("tailnet"));
    assert!(std::fs::read_to_string(&log).unwrap().contains("100.101.102.103:8080"));

    // When a navigation names an Expo path (nothing sends one now) with the old shared-Metro variable pointing at a server, and
    // then a navigation names that server by URL
    server.routes(&[("GET /hello", MockReply::json(200, json!({ "hi": "metro" })))]);
    let origin = format!("http://localhost:{}", server.port);
    a.wf(&["stop"]);
    a.open_in(&[("WF_SHARED_METRO_URL", origin.as_str())]).unwrap();
    let heard = server.requests().len();
    let by_path = a.call("navigate", json!({ "path": "hello" }));
    let heard_after_path = server.requests().len();
    let by_url = a.go(&format!("{origin}/hello"));
    // Then the path is refused naming --url and the server heard nothing, while the URL reaches it (so a request would have been seen)
    assert!(by_path.as_ref().is_err_and(|e| e.contains("Provide --url") && !e.contains("--path")), "{by_path:?}");
    assert_eq!(heard_after_path, heard);
    assert_eq!(by_url, Ok(json!({ "navigated": format!("{origin}/hello") })));
    assert!(server.requests().len() > heard_after_path && a.eval("document.body.innerText").contains("metro"));

    // When a clip goes to a page with the fake microphone, and a picture to one with the fake camera
    let wav = a.repo.join("clip.wav");
    std::fs::write(&wav, "RIFFfake").unwrap();
    let png = a.repo.join("pic.png");
    std::fs::write(&png, "png").unwrap();
    let (wav, png) = (wav.display().to_string(), png.display().to_string());
    a.go("data:text/html,<p>x</p>").unwrap();
    a.eval("globalThis.__mcpFakeMicPush = (c) => { window.pushed = [c.label, c.mimeType, c.base64]; return 1 }");
    a.eval("globalThis.__mcpFakeCameraSet = async (s) => ({ width: 640, height: 480 })");
    let audio = a.rpc("audioInject", json!({ "path": wav }));
    let camera = a.rpc("cameraInject", json!({ "path": png }));
    // Then the clip is queued in the frame that has the fake microphone, and the camera reports the picture's size
    use base64::Engine;
    assert_eq!(audio, json!({ "label": wav, "mimeType": "audio/wav", "bytes": 8, "frames": 1 }));
    assert_eq!(parse(&a.eval("window.pushed")), json!([wav, "audio/wav", base64::engine::general_purpose::STANDARD.encode("RIFFfake")]));
    assert_eq!(camera, json!({ "path": png, "width": 640, "height": 480, "frames": 1 }));

    // When a clip of an unknown type or a missing one is injected
    let odd = a.repo.join("clip.xyz");
    std::fs::write(&odd, "x").unwrap();
    let unknown = a.call("audioInject", json!({ "path": odd.display().to_string() }));
    let missing = a.call("audioInject", json!({ "path": "/nope.wav" }));
    // Then each fails with its own message
    assert!(unknown.as_ref().unwrap_err().contains("cannot guess a media type for \".xyz\""), "{unknown:?}");
    assert!(missing.as_ref().unwrap_err().contains("audio-inject file not found: /nope.wav"), "{missing:?}");
}

#[test]
fn an_attached_session_traces_a_lesson_pad_and_serves_the_device_frame() {
    let a = Attached::start();
    a.open().unwrap();

    // When a pad that publishes a two-part guide is traced
    a.go("data:text/html,<body style=\"margin:0\"><div id=pad data-active-guide=\"M 10 10 L 90 10 M 10 30 L 90 30\" style=\"width:100px;height:50px;margin:20px\"></div><script>window.log=[];for(const t of ['mousedown','mousemove','mouseup'])document.addEventListener(t,e=>log.push([t,e.clientX,e.clientY]))</script>").unwrap();
    let guide = a.rpc("trace", json!({ "ref": "#pad", "source": "guide", "indicator": false }));
    let log = a.events();
    // Then the page sees one press, moves and release per subpath, inside the pad
    assert!(guide["strokes"].as_f64() == Some(2.0) && guide["samples"].as_f64().unwrap() > 2.0, "{guide}");
    let of = |kind: &str| -> Vec<(String, f64, f64)> { log.iter().filter(|e| e.0 == kind).cloned().collect() };
    assert_eq!(of("mousedown"), [("mousedown".to_string(), 30.0, 30.0), ("mousedown".to_string(), 30.0, 50.0)]);
    assert_eq!(of("mouseup"), [("mouseup".to_string(), 110.0, 30.0), ("mouseup".to_string(), 110.0, 50.0)]);

    // When a pad showing a faint L template is traced, and one without a guide
    a.go("data:text/html,<body style=\"margin:0\"><div id=pad style=\"position:relative;width:200px;height:100px;margin:20px;background:white\"><div style=\"position:absolute;left:40px;top:15px;width:8px;height:60px;background:%23999\"></div><div style=\"position:absolute;left:40px;top:67px;width:50px;height:8px;background:%23999\"></div></div><script>window.log=[];document.addEventListener('mousedown',e=>log.push(['down',e.clientX,e.clientY]))</script>").unwrap();
    let template = a.rpc("trace", json!({ "ref": "#pad", "source": "template", "indicator": false }));
    let downs = a.events();
    a.go("data:text/html,<div id=pad>x</div>").unwrap();
    let no_guide = a.call("trace", json!({ "ref": "#pad", "source": "guide" }));
    // Then the template's stroke is read off the pixels and drawn from the L's upper end, and the bare pad says it has no guide
    assert_eq!((template["strokes"].as_f64(), downs.len()), (Some(1.0), 1));
    assert!(downs[0].1 >= 60.0 && downs[0].1 <= 68.0 && downs[0].2 <= 45.0, "{:?}", downs[0]);
    assert!(no_guide.as_ref().unwrap_err().contains("trace: \"#pad\" publishes no guide (data-active-guide is empty)"), "{no_guide:?}");

    // When the daemon's device-frame routes are requested (with the built harness)
    let dist = repo_root().join("tooling/browser-harness/dist");
    assert!(dist.join("index.html").exists(), "{} is not built: run `pnpm --filter @tooling/browser-harness run build`", dist.display());
    let host = a.daemon(&[]).trim_start_matches("http://").to_string();
    let get = |path: &str| {
        let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).proxy(None).timeout_global(Some(Duration::from_secs(20))).build().into();
        let mut response = agent.get(format!("http://{host}{path}")).call().unwrap();
        let content_type = response.headers().get("content-type").map(|v| v.to_str().unwrap_or_default().to_string());
        (response.status().as_u16(), content_type, response.body_mut().read_to_vec().unwrap())
    };
    let index = std::fs::read(dist.join("index.html")).unwrap();
    let asset = std::fs::read_dir(dist.join("assets")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).find(|f| f.ends_with(".js")).unwrap();
    let (page, spa, js, health, unknown) = (get("/_mcp/device-frame"), get("/_mcp/device-frame/some/client/route"), get(&format!("/_mcp/device-frame/assets/{asset}")), get("/health"), get("/nope"));
    // Then the page, the client-route fallback and the assets come from dist, health answers, and anything else is a JSON 404
    assert_eq!((page.0, page.1.as_deref()), (200, Some("text/html; charset=utf-8")));
    assert!(page.2 == index && spa.2 == index, "the device-frame page differs from dist/index.html");
    assert_eq!((js.0, js.1.as_deref()), (200, Some("text/javascript; charset=utf-8")));
    assert!(js.2 == std::fs::read(dist.join("assets").join(&asset)).unwrap());
    assert_eq!(parse(&String::from_utf8_lossy(&health.2)), json!({ "ok": true }));
    assert_eq!((unknown.0, parse(&String::from_utf8_lossy(&unknown.2))), (404, json!({ "ok": false, "error": "Not found" })));
}

#[test]
fn an_idle_daemon_stops_itself_so_no_forgotten_one_holds_its_browser() {
    // Given a daemon that nothing calls
    let mut daemon = std::process::Command::new(crate::support::wf_binary())
        .arg("browser-daemon")
        .env("WF_BROWSER_SLOT", format!("idle-{}", std::process::id()))
        .env("WF_BROWSER_DAEMON_IDLE_S", "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // When it has been idle longer than its limit
    let start = std::time::Instant::now();
    let status = loop {
        if let Some(status) = daemon.try_wait().unwrap() {
            break Some(status);
        }
        if start.elapsed() > std::time::Duration::from_secs(10) {
            let _ = daemon.kill();
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    // Then it has stopped by itself and said why
    let mut stderr = String::new();
    std::io::Read::read_to_string(daemon.stderr.as_mut().unwrap(), &mut stderr).unwrap();
    assert!(status.is_some_and(|s| s.success()) && stderr.contains("idle for 1 s, stopping"), "{status:?} {stderr}");
}
