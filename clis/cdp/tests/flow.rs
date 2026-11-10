//! Flows through the public client (`launch`, `Attached::attach` and its commands, `Cdp`, `json_endpoint`) against a
//! Chromium double on loopback; the upstream of a proxied request and the browser executable are doubles too.
#[path = "flow/doubles.rs"]
mod doubles;
#[path = "flow/fake_browser.rs"]
mod fake_browser;

use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use base64::Engine as _;
use cdp::attached::{device_frame_url, fitted_shell_size, Attached};
use cdp::{json_endpoint, Cdp};
use doubles::{fake_chromium, fake_ffmpeg, upstream, TempDir};
use fake_browser::{attach_reply, closed_port, png_gray, png_size, value, Call, FakeBrowser, NO_REPLY};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn page_calls(browser: &FakeBrowser, method: &str) -> Vec<Value> {
    browser.calls_to(method).into_iter().map(|c| c.params).collect()
}

#[test]
fn given_a_launched_browser_when_its_page_is_attached_and_driven_then_each_command_reaches_the_page_and_the_answers_come_back() {
    // Given a browser whose window can go fullscreen and a page that answers what the caller evaluates
    let window = Arc::new(Mutex::new(("maximized", 500, 900)));
    let windows_gone = Arc::new(AtomicBool::new(false));
    let (state, gone) = (Arc::clone(&window), Arc::clone(&windows_gone));
    let browser = FakeBrowser::start(move |call, _| {
        if let Some(reply) = attach_reply(call) {
            return Ok(reply);
        }
        let p = &call.params;
        match call.method.as_str() {
            "Page.navigate" if p["url"] == "http://unreachable.test/" => Ok(json!({ "frameId": "F0", "errorText": "net::ERR_CONNECTION_REFUSED" })),
            "Page.navigate" => Ok(json!({ "frameId": "F0", "loaderId": "L1" })),
            "Runtime.evaluate" => {
                match p["expression"].as_str().unwrap() {
                    "document.title" => value(json!("attached")),
                    "window.__wfLeaving = true" => value(json!(true)),
                    "!window.__wfLeaving && document.readyState === 'complete'" => value(json!(true)),
                    "Promise.resolve(40 + 2)" => value(json!(42)),
                    "syntax(" => Ok(json!({ "result": {}, "exceptionDetails": { "text": "SyntaxError: Unexpected end of input" } })),
                    e if e.contains("throw new Error('boom')") => Ok(json!({ "result": {}, "exceptionDetails": { "text": "Uncaught", "exception": { "description": "Error: boom\n    at <anonymous>:1:15" } } })),
                    e if e.contains("window.innerWidth - document.documentElement.clientWidth") => value(json!({ "w": 15, "h": 0 })),
                    e if e.starts_with("({ width: window.innerWidth") => value(json!({ "width": 500, "height": 900 })),
                    e => Err(format!("unexpected expression {e}")),
                }
            }
            "Storage.getCookies" => Ok(json!({ "cookies": [{ "name": "sid", "value": "secret", "domain": "127.0.0.1", "path": "/", "httpOnly": true, "size": 9 }] })),
            "Network.getCookies" if p["urls"][0].as_str().unwrap().starts_with("http://127.0.0.1") => Ok(json!({ "cookies": [{ "name": "sid", "value": "secret", "domain": "127.0.0.1", "path": "/", "httpOnly": true }] })),
            "Network.getCookies" => Ok(json!({ "cookies": [] })),
            "Page.getLayoutMetrics" => Ok(json!({ "cssVisualViewport": { "clientWidth": 485, "clientHeight": 900 }, "visualViewport": { "clientWidth": 970, "clientHeight": 1800 } })),
            "Emulation.clearDeviceMetricsOverride" | "Emulation.setDeviceMetricsOverride" | "Emulation.setUserAgentOverride" | "Emulation.setTouchEmulationEnabled" => Ok(json!({})),
            "Browser.getWindowForTarget" if gone.load(Ordering::SeqCst) => Err("Browser window not found".into()),
            "Browser.getWindowForTarget" => {
                let (s, w, h) = *state.lock().unwrap();
                Ok(json!({ "windowId": 7, "bounds": { "left": 0, "top": 0, "width": w, "height": h, "windowState": s } }))
            }
            "Browser.setWindowBounds" => {
                *state.lock().unwrap() = if p["bounds"]["windowState"] == "fullscreen" { ("fullscreen", 1280, 1024) } else { ("normal", 500, 900) };
                Ok(json!({}))
            }
            "Browser.getVersion" => Ok(json!({ "product": "HeadlessChrome/141.0.7390.37", "protocolVersion": "1.3" })),
            "Runtime.addBinding" | "Target.detachFromTarget" => Ok(json!({})),
            "Page.addScriptToEvaluateOnNewDocument" => Ok(json!({ "identifier": "1" })),
            other => Err(format!("'{other}' wasn't found")),
        }
    });
    let dir = TempDir::new("launch");

    // When a browser executable that dies at once is launched
    let broken = cdp::launch::launch(&fake_chromium(&dir, "broken", "exit 3"), &[], &[], true);
    // Then the launch fails naming why
    assert_eq!(broken.err().as_deref(), Some("the browser exited before opening its DevTools port"));

    // When the browser is launched with an argument and an environment variable
    let exe = fake_chromium(&dir, "chrome", &format!("echo 'DevTools listening on ws://127.0.0.1:{}/devtools/browser/fake' >&2\nexec sleep 60", browser.port));
    let launched = cdp::launch::launch(&exe, &["--window-size=500,900".into()], &[("FAKE_ENV".into(), "set".into())], true).unwrap();
    // Then it runs headless with a throwaway profile and an ephemeral DevTools port, and its endpoint is read from stderr;
    // a sideways drag at a scroller's start is no swipe-back that navigates the page away
    assert_eq!(launched.endpoint, browser.endpoint);
    let args = std::fs::read_to_string(dir.0.join("chrome.args")).unwrap();
    for arg in ["--headless=new", "--no-sandbox", "--remote-debugging-port=0", "--disable-features=OverscrollHistoryNavigation", "--overscroll-history-navigation=0", "--window-size=500,900", "about:blank", "env=set"] {
        assert!(args.lines().any(|l| l == arg), "{arg} in {args}");
    }
    assert!(args.contains(&format!("--user-data-dir={}", launched.profile.display())));
    assert!(launched.profile.starts_with(std::env::temp_dir()));

    // When its first page is attached
    let mut page = Attached::attach(&launched.endpoint).unwrap();
    // Then the real page (not DevTools' own) is attached with a flat session, and its domains are enabled on it
    let handshake: Vec<(String, Option<String>)> = browser.calls().into_iter().map(|c| (c.method, c.session)).collect();
    let s1 = Some("S1".to_string());
    assert_eq!(handshake, [("Target.getTargets".into(), None), ("Target.attachToTarget".into(), None), ("Page.enable".into(), s1.clone()), ("Runtime.enable".into(), s1.clone()), ("Network.enable".into(), s1.clone())]);
    assert_eq!(page_calls(&browser, "Target.attachToTarget"), [json!({ "targetId": "T1", "flatten": true })]);
    assert_eq!(page.session_id(), "S1");

    // When it navigates, evaluates and reads cookies
    page.navigate("data:text/html,<title>attached</title>").unwrap();
    let refused = page.navigate("http://unreachable.test/").unwrap_err();
    let title = page.eval("document.title").unwrap();
    let awaited = page.eval("Promise.resolve(40 + 2)").unwrap();
    let thrown = page.eval("(() => { throw new Error('boom') })()").unwrap_err();
    let syntax = page.eval("syntax(").unwrap_err();
    let all = page.cookies(None).unwrap();
    let scoped = page.cookies(Some(&["http://127.0.0.1:8/".to_string()])).unwrap();
    let none = page.cookies(Some(&["http://example.invalid/".to_string()])).unwrap();
    // Then the page's answers come back, a thrown error and a net error by their text, HttpOnly cookies included
    assert_eq!(refused, "page.goto: net::ERR_CONNECTION_REFUSED at http://unreachable.test/");
    assert_eq!((title, awaited), (json!("attached"), json!(42)));
    assert!(thrown.starts_with("Error: boom"), "{thrown}");
    assert_eq!(syntax, "SyntaxError: Unexpected end of input");
    // and a navigation to a new document returned only once that document had loaded
    assert!(page_calls(&browser, "Runtime.evaluate").iter().any(|e| e["expression"] == "!window.__wfLeaving && document.readyState === 'complete'"));
    for e in page_calls(&browser, "Runtime.evaluate") {
        assert_eq!((&e["returnByValue"], &e["awaitPromise"], e.get("contextId")), (&json!(true), &json!(true), None), "{e}");
    }
    assert_eq!(all, [json!({ "name": "sid", "value": "secret", "domain": "127.0.0.1", "path": "/" })]);
    assert_eq!((scoped.len(), none.len()), (1, 0));
    let cookie_calls: Vec<Call> = browser.calls().into_iter().filter(|c| c.method.ends_with("getCookies")).collect();
    assert_eq!(cookie_calls.iter().map(|c| (c.method.as_str(), c.session.as_deref())).collect::<Vec<_>>(), [("Storage.getCookies", None), ("Network.getCookies", Some("S1")), ("Network.getCookies", Some("S1"))]);
    assert_eq!(cookie_calls[1].params, json!({ "urls": ["http://127.0.0.1:8/"] }));

    // When the page is presented as a phone
    let (ua, w, h) = page.emulate_mobile("Mozilla/5.0 (iPhone) TestPhone").unwrap();
    // Then the settled viewport plus the scrollbar gutter is pinned, with the user agent and five touch points
    assert_eq!((ua.as_str(), w, h), ("Mozilla/5.0 (iPhone) TestPhone", 500, 900));
    assert_eq!(page_calls(&browser, "Emulation.setDeviceMetricsOverride"), [json!({ "width": 500.0, "height": 900.0, "screenWidth": 500.0, "screenHeight": 900.0, "deviceScaleFactor": 1, "mobile": true })]);
    assert_eq!(page_calls(&browser, "Emulation.setUserAgentOverride"), [json!({ "userAgent": "Mozilla/5.0 (iPhone) TestPhone" })]);
    assert_eq!(page_calls(&browser, "Emulation.setTouchEmulationEnabled"), [json!({ "enabled": true, "maxTouchPoints": 5 })]);

    // When the window is asked to go fullscreen, then again once it is gone
    let applied = page.fullscreen_window();
    windows_gone.store(true, Ordering::SeqCst);
    let failed = page.fullscreen_window();
    // Then it goes through normal to fullscreen and reports the new size, and a missing window never fails the caller
    assert_eq!(applied, (true, 1280, 1024));
    assert_eq!(page_calls(&browser, "Browser.setWindowBounds"), [json!({ "windowId": 7, "bounds": { "windowState": "normal" } }), json!({ "windowId": 7, "bounds": { "windowState": "fullscreen" } })]);
    assert_eq!(failed, (false, 0, 0));
    assert_eq!(page.browser_version().unwrap(), "141.0.7390.37");

    // When a binding and an init script are registered and the page calls the binding with no command in flight
    page.add_binding("reportLoad").unwrap();
    page.add_init_script("window.reportLoad('x')").unwrap();
    browser.emit(json!({ "method": "Runtime.bindingCalled", "params": { "name": "reportLoad", "payload": "{\"title\":\"bound\"}", "executionContextId": 1 }, "sessionId": "S1" }));
    let mut calls = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while calls.is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
        calls.extend(page.take_binding_calls());
    }
    // Then both reach the page and the call arrives once
    assert_eq!(page_calls(&browser, "Runtime.addBinding"), [json!({ "name": "reportLoad" })]);
    assert_eq!(page_calls(&browser, "Page.addScriptToEvaluateOnNewDocument"), [json!({ "source": "window.reportLoad('x')" })]);
    assert_eq!(calls, [json!({ "name": "reportLoad", "payload": "{\"title\":\"bound\"}", "executionContextId": 1 })]);
    assert!(page.take_binding_calls().is_empty());

    // When the page is detached and the browser closed
    page.detach();
    let profile = launched.profile.clone();
    std::fs::create_dir_all(&profile).unwrap();
    launched.close();
    // Then the session is detached on the browser connection and the throwaway profile is gone
    assert_eq!(browser.calls_to("Target.detachFromTarget").iter().map(|c| (c.params.clone(), c.session.clone())).collect::<Vec<_>>(), [(json!({ "sessionId": "S1" }), None)]);
    assert!(!profile.exists());
}

#[test]
fn given_a_page_when_screenshots_are_taken_then_viewport_and_full_page_pngs_come_back_and_a_tiled_capture_is_taken_again() {
    // Given a page 3000 CSS pixels tall that answers captures from a queue, else with a blank viewport
    let queue: Arc<Mutex<VecDeque<String>>> = Arc::default();
    let shots = Arc::clone(&queue);
    let browser = FakeBrowser::start(move |call, _| {
        if let Some(reply) = attach_reply(call) {
            return Ok(reply);
        }
        let p = &call.params;
        match call.method.as_str() {
            "Page.getLayoutMetrics" => Ok(json!({ "cssContentSize": { "x": 0, "y": 0, "width": 500, "height": 3000 }, "contentSize": { "x": 0, "y": 0, "width": 1000, "height": 6000 } })),
            "Page.captureScreenshot" if p["clip"].is_object() => {
                let (w, h) = (p["clip"]["width"].as_u64().unwrap() as usize, p["clip"]["height"].as_u64().unwrap() as usize);
                Ok(json!({ "data": STANDARD.encode(png_gray(w, h, |_, _| 90)) }))
            }
            "Page.captureScreenshot" => Ok(json!({ "data": shots.lock().unwrap().pop_front().unwrap_or_else(|| STANDARD_NO_PAD.encode(png_gray(500, 900, |_, _| 240))) })),
            other => Err(format!("'{other}' wasn't found")),
        }
    });
    let mut page = Attached::attach(&browser.endpoint).unwrap();

    // When the viewport and the full page are captured
    let viewport = page.screenshot_png(false).unwrap();
    let full = page.screenshot_png(true).unwrap();
    // Then both are PNGs (unpadded and padded base64 alike), the full page as tall as the document
    assert_eq!(&viewport[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    assert_eq!((png_size(&viewport), png_size(&full)), ((500, 900), (500, 3000)));
    let captures = page_calls(&browser, "Page.captureScreenshot");
    assert_eq!(captures, [json!({ "format": "png" }), json!({ "format": "png", "captureBeyondViewport": true, "clip": { "x": 0, "y": 0, "width": 500, "height": 3000, "scale": 1 } })]);

    // Given captures that are one band repeated down the image, and real pages that are not
    let cases: Vec<(&str, Vec<u8>, bool)> = vec![
        ("a dark bar repeated every 40 rows", png_gray(400, 900, |x, y| if y % 40 < 8 && (60..340).contains(&x) { 20 } else { 235 }), true),
        ("a band 15.6 rows tall, as an eighth-scale decode has it", png_gray(157, 344, |x, y| if (y as f64 % 15.6) < 3.0 && (20..137).contains(&x) { 30 } else { 230 }), true),
        ("the bezel's faint 31-row gradient", png_gray(360, 789, |x, y| (236 + (y % 31) / 8 + usize::from((60..300).contains(&x))) as u8), true),
        ("a page darkening slowly", png_gray(360, 789, |_, y| (250 - y / 40) as u8), false),
        ("a blank page", png_gray(400, 900, |_, _| 240), false),
        ("a page whose rows do not repeat", png_gray(400, 900, |x, y| ((x * 7 + y * y / 13) % 251) as u8), false),
    ];
    let retaken = png_gray(400, 900, |x, y| ((x * 3 + y * y / 7) % 251) as u8);
    for (name, first, tiled) in cases {
        queue.lock().unwrap().extend([STANDARD.encode(&first), STANDARD.encode(&retaken)]);
        let before = browser.calls_to("Page.captureScreenshot").len();
        // When the page is captured
        let got = page.screenshot_png(false).unwrap();
        // Then a tiled capture is taken again and the retake returned, a real one is returned as it came
        let taken = browser.calls_to("Page.captureScreenshot").len() - before;
        assert_eq!((taken, got == retaken), if tiled { (2, true) } else { (1, false) }, "{name}");
        queue.lock().unwrap().clear();
    }

    // When the browser answers with data that is not base64
    queue.lock().unwrap().push_back("a$b".into());
    // Then the capture fails rather than returning garbage
    assert!(page.screenshot_png(false).is_err());
}

#[test]
fn given_a_browser_that_never_answers_the_first_capture_when_the_page_is_recorded_then_later_frames_fill_the_video() {
    // Given a page that leaves its first capture unanswered (Chromium 151 on a fresh page) and answers the rest, and an
    // ffmpeg double that writes what it is piped
    let first = Arc::new(AtomicBool::new(true));
    let browser = FakeBrowser::start(move |call, _| {
        if let Some(reply) = attach_reply(call) {
            return Ok(reply);
        }
        match call.method.as_str() {
            "Page.captureScreenshot" if first.swap(false, Ordering::SeqCst) => Err(NO_REPLY.into()),
            "Page.captureScreenshot" => Ok(json!({ "data": STANDARD.encode(b"frame;") })),
            other => Err(format!("'{other}' wasn't found")),
        }
    });
    let dir = TempDir::new("record");
    std::env::set_var("FFMPEG_PATH", fake_ffmpeg());
    let mut page = Attached::attach(&browser.endpoint).unwrap();

    // When the page is recorded past the time a capture is given up
    let recording = page.start_recording(dir.0.join("run.mp4"), 390, 844).unwrap();
    std::thread::sleep(Duration::from_secs(6));
    let asked = Instant::now();
    let size = page.stop_recording(recording);

    // Then the frames after the unanswered capture are in the video, and stopping does not wait out a call timeout
    let video = std::fs::read_to_string(dir.0.join("run.mp4")).unwrap();
    assert!(size.is_ok() && video.starts_with("frame;") && video.len() > 10 * "frame;".len(), "{size:?}: {} bytes", video.len());
    assert!(asked.elapsed() < Duration::from_secs(4), "stopping took {:?}", asked.elapsed());
}

#[test]
fn given_captures_that_take_300_ms_when_an_input_is_logged_then_it_returns_at_once_and_its_time_is_a_frame_captured_after_it() {
    // Given a page whose every capture takes 300 ms (a loaded host) and shows the page as it was when the capture was asked
    // for, recorded with an input log
    let changed = Arc::new(AtomicBool::new(false));
    let shows = Arc::clone(&changed);
    let browser = FakeBrowser::start(move |call, _| {
        if let Some(reply) = attach_reply(call) {
            return Ok(reply);
        }
        match call.method.as_str() {
            "Page.captureScreenshot" => {
                let frame = if shows.load(Ordering::SeqCst) { "after;" } else { "before;" };
                std::thread::sleep(Duration::from_millis(300));
                Ok(json!({ "data": STANDARD.encode(frame) }))
            }
            other => Err(format!("'{other}' wasn't found")),
        }
    });
    let dir = TempDir::new("record-inputs");
    std::env::set_var("FFMPEG_PATH", fake_ffmpeg());
    let mut page = Attached::attach(&browser.endpoint).unwrap();
    let recording = page.start_recording(dir.0.join("run.mp4"), 390, 844).unwrap();
    let log = dir.0.join("run.inputs.jsonl");
    recording.log_inputs_to(log.clone()).unwrap();

    // When the page changes while a capture is under way, and an input is logged at that moment
    std::thread::sleep(Duration::from_millis(400));
    let at = recording.elapsed_ms();
    changed.store(true, Ordering::SeqCst);
    let asked = Instant::now();
    recording.log_input(asked, json!({ "t": null, "kind": "tap" }));
    let returned = asked.elapsed();
    let unwritten = std::fs::read_to_string(&log).unwrap();
    page.stop_recording(recording).unwrap();

    // Then logging does not wait for a capture, and the line, written once its frame landed, names a frame asked for after
    // the change, a whole capture later, not the moment of logging (whose frame is the one under way, from before it)
    assert!(returned < Duration::from_millis(50) && unwritten.is_empty(), "logging took {returned:?}; the log then: {unwritten:?}");
    let lines: Vec<Value> = std::fs::read_to_string(&log).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!((lines.len(), &lines[0]["kind"]), (1, &json!("tap")), "{lines:?}");
    let t = lines[0]["t"].as_u64().unwrap();
    assert!(t >= at + 300, "the change at {at} ms is shown at {t} ms");
    let video = std::fs::read_to_string(dir.0.join("run.mp4")).unwrap();
    assert!(video.starts_with("before;") && video.ends_with("after;"), "{video}");
}

#[test]
fn given_a_recording_whose_captures_stall_when_an_input_is_logged_then_stopping_writes_it_at_its_own_moment() {
    // Given a recorded page whose captures stop answering after the first frames (a loaded or stuck browser)
    let answered = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&answered);
    let browser = FakeBrowser::start(move |call, _| {
        if let Some(reply) = attach_reply(call) {
            return Ok(reply);
        }
        match call.method.as_str() {
            "Page.captureScreenshot" if count.fetch_add(1, Ordering::SeqCst) < 3 => Ok(json!({ "data": STANDARD.encode("frame;") })),
            "Page.captureScreenshot" => Err(NO_REPLY.into()),
            other => Err(format!("'{other}' wasn't found")),
        }
    });
    let dir = TempDir::new("record-stalled");
    std::env::set_var("FFMPEG_PATH", fake_ffmpeg());
    let mut page = Attached::attach(&browser.endpoint).unwrap();
    let recording = page.start_recording(dir.0.join("run.mp4"), 390, 844).unwrap();
    let log = dir.0.join("run.inputs.jsonl");
    recording.log_inputs_to(log.clone()).unwrap();

    // When an input is logged once no frame comes any more, and the recording is stopped
    while answered.load(Ordering::SeqCst) < 4 {
        std::thread::sleep(Duration::from_millis(10));
    }
    let at = recording.elapsed_ms();
    let asked = Instant::now();
    recording.log_input(asked, json!({ "t": null, "kind": "press" }));
    let returned = asked.elapsed();
    page.stop_recording(recording).unwrap();

    // Then logging did not wait, and stopping wrote the line at the moment it was logged on the video's clock
    assert!(returned < Duration::from_millis(50), "logging took {returned:?}");
    let lines: Vec<Value> = std::fs::read_to_string(&log).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.len(), 1, "{lines:?}");
    let t = lines[0]["t"].as_u64().unwrap();
    assert!((at..at + 50).contains(&t), "logged at {at} ms, written at {t} ms");
}

#[test]
fn given_a_focused_editor_when_ctrl_end_is_pressed_then_the_page_sees_end_with_ctrl_held() {
    // Given a page that takes key events
    let browser = FakeBrowser::start(|call, _| attach_reply(call).map_or_else(|| Ok(json!({})), Ok));
    let mut page = Attached::attach(&browser.endpoint).unwrap();

    // When Ctrl+End and End are pressed
    page.press_key("Control+End").unwrap();
    page.press_key("End").unwrap();

    // Then each reaches the page as the End key, the first with Ctrl held (a key handler reads `key` and `ctrlKey`)
    let keys: Vec<(String, Value, Value)> = page_calls(&browser, "Input.dispatchKeyEvent").iter().map(|p| (p["type"].as_str().unwrap().to_string(), p["key"].clone(), p["modifiers"].clone())).collect();
    let want = |t: &str, m: i64| (t.to_string(), json!("End"), json!(m));
    assert_eq!(keys, [want("rawKeyDown", 2), want("keyUp", 2), want("rawKeyDown", 0), want("keyUp", 0)]);
    assert!(page.press_key("Meta+End").unwrap_err().contains("unsupported key Meta+End"));
}

#[test]
fn given_a_device_frame_page_with_nested_frames_when_frames_are_listed_searched_and_evaluated_then_each_frame_answers_in_its_own_document() {
    // Given a page with nested frames whose app frame appears on the third look, each frame's main world announced
    let looks = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&looks);
    let browser = FakeBrowser::start(move |call, events| {
        if call.method == "Runtime.enable" {
            for (frame, id, default) in [("F0", 1, true), ("F1", 2, true), ("F2", 3, true), ("FA", 11, true), ("FE", 12, true), ("F1", 99, false)] {
                events.push(json!({ "method": "Runtime.executionContextCreated", "params": { "context": { "id": id, "origin": "", "name": "", "auxData": { "isDefault": default, "type": "default", "frameId": frame } } }, "sessionId": "S1" }));
            }
        }
        if let Some(reply) = attach_reply(call) {
            return Ok(reply);
        }
        let p = &call.params;
        let frame = |id: &str, parent: Option<&str>, name: &str, url: &str| json!({ "id": id, "parentId": parent, "name": name, "url": url });
        match call.method.as_str() {
            "Page.navigate" => Ok(json!({ "frameId": "F0" })),
            "Page.getFrameTree" => {
                let mut children = vec![json!({ "frame": frame("F1", Some("F0"), "c", "about:srcdoc"), "childFrames": [{ "frame": frame("F2", Some("F1"), "g", "about:srcdoc") }] })];
                if seen.fetch_add(1, Ordering::SeqCst) >= 2 {
                    children.push(json!({ "frame": frame("FA", Some("F0"), "device-frame-app", "http://localhost:8081/"), "childFrames": [{ "frame": frame("FE", Some("FA"), "ed", "about:srcdoc") }] }));
                }
                Ok(json!({ "frameTree": { "frame": frame("F0", None, "", "http://127.0.0.1:5000/_mcp/device-frame"), "childFrames": children } }))
            }
            "Runtime.evaluate" => {
                let expression = p["expression"].as_str().unwrap();
                match (p["contextId"].as_i64(), expression) {
                    (Some(id), e) if e.contains("contenteditable") => value(json!(id == 12)),
                    (Some(3), e) if e.contains("getElementById('who')") => value(json!("grandchild")),
                    (Some(id), "document.title") => value(json!(match id { 1 => "top", 2 => "middle", 3 => "inner", 11 => "app", _ => "editor" })),
                    (None, "document.title") => value(json!("top (main world)")),
                    (_, e) => Err(format!("unexpected expression {e}")),
                }
            }
            other => Err(format!("'{other}' wasn't found")),
        }
    });

    // Given the harness URL that frames the app as a device, and the window that fits it
    let url = device_frame_url("http://127.0.0.1:5000/", "http://localhost:8081/a b?x=1&y=é", "iphone-15-pro", "portrait", "system", "screen");
    // Then the URL is encoded as URLSearchParams does, and the window adds margins and the label, rounded up to even
    assert_eq!(url, "http://127.0.0.1:5000/_mcp/device-frame?target=http%3A%2F%2Flocalhost%3A8081%2Fa+b%3Fx%3D1%26y%3D%C3%A9&device=iphone-15-pro&orientation=portrait&colorScheme=system&frame=screen");
    assert_eq!((fitted_shell_size(393.0, 852.0), fitted_shell_size(100.2, 50.0)), ((418, 916), (126, 114)));

    // When the page opens it and lists its frames
    let mut page = Attached::attach(&browser.endpoint).unwrap();
    page.navigate(&url).unwrap();
    let frames = page.frames().unwrap();
    // Then each frame comes with its parent, name and URL, the main frame first
    let listed: Vec<(&str, Option<&str>, &str)> = frames.iter().map(|f| (f.id.as_str(), f.parent.as_deref(), f.name.as_str())).collect();
    assert_eq!(listed, [("F0", None, ""), ("F1", Some("F0"), "c"), ("F2", Some("F1"), "g")]);
    assert_eq!(frames[0].url, "http://127.0.0.1:5000/_mcp/device-frame");

    // When each frame evaluates, and a frame with no context is asked
    let titles: Vec<Value> = frames.iter().map(|f| page.eval_in_frame(&f.id, "document.title").unwrap()).collect();
    let who = page.eval_in_frame("F2", "document.getElementById('who').textContent").unwrap();
    let missing = page.eval_in_frame("no-such-frame", "1").unwrap_err();
    // Then each sees its own document through its main world, and the missing one fails by name
    assert_eq!(titles, [json!("top"), json!("middle"), json!("inner")]);
    assert_eq!(who, "grandchild");
    assert!(missing.contains("no execution context"), "{missing}");
    page.set_target(Some("F2".into()));
    assert_eq!(page.eval("document.title").unwrap(), "inner");
    page.set_target(None);
    assert_eq!(page.eval("document.title").unwrap(), "top (main world)");

    // When the app frame is waited for, and a frame that never comes is
    let found = page.find_frame_named("device-frame-app", Duration::from_secs(5)).unwrap();
    let absent = page.find_frame_named("nope", Duration::from_millis(200)).unwrap();
    // Then the app frame is found once it appears and the absent one times out
    assert_eq!(found.as_ref().map(|f| (f.id.as_str(), f.parent.as_deref())), Some(("FA", Some("F0"))));
    assert_eq!(absent, None);
    assert!(looks.load(Ordering::SeqCst) >= 4, "the search looked again until the frame appeared");

    // When an editable frame is searched under the app frame, and under the editor itself
    let editable = page.find_editable_frame("FA", 4).unwrap();
    let below = page.find_editable_frame("FE", 4).unwrap();
    // Then the editor frame is found and nothing is found below it
    assert_eq!(editable.map(|f| f.name), Some("ed".to_string()));
    assert_eq!(below, None);

    // When the browser drops a frame's context, then all of them
    browser.emit(json!({ "method": "Runtime.executionContextDestroyed", "params": { "executionContextId": 3 }, "sessionId": "S1" }));
    browser.emit(json!({ "method": "Runtime.bindingCalled", "params": { "name": "flush" }, "sessionId": "S1" }));
    wait_for_binding(&mut page);
    let dropped = page.eval_in_frame("F2", "document.title");
    let kept = page.eval_in_frame("F1", "document.title");
    browser.emit(json!({ "method": "Runtime.executionContextsCleared", "params": {}, "sessionId": "S1" }));
    browser.emit(json!({ "method": "Runtime.bindingCalled", "params": { "name": "flush" }, "sessionId": "S1" }));
    wait_for_binding(&mut page);
    // Then the dropped frame has no context, the others keep theirs until every one is cleared
    assert!(dropped.unwrap_err().contains("no execution context for frame F2"));
    assert_eq!(kept.unwrap(), "middle");
    assert!(page.eval_in_frame("F0", "document.title").is_err());
}

/// Waits until an event sent after the ones under test has been read, so they have been read too.
fn wait_for_binding(page: &mut Attached) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while page.take_binding_calls().is_empty() {
        assert!(Instant::now() < deadline, "the browser's events never arrived");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn given_proxied_urls_when_the_page_requests_them_then_this_process_fetches_them_and_hands_back_the_answer_or_a_502() {
    // Given a page whose paused requests are answered by this process, and an upstream server
    let browser = FakeBrowser::start(|call, _| match attach_reply(call) {
        Some(reply) => Ok(reply),
        None if call.method.starts_with("Fetch.") => Ok(json!({})),
        None => Err(format!("'{}' wasn't found", call.method)),
    });
    let (port, seen) = upstream();
    let closed = closed_port();
    let mut page = Attached::attach(&browser.endpoint).unwrap();
    let paused = |id: &str, method: &str, url: String| json!({ "method": "Fetch.requestPaused", "sessionId": "S1", "params": { "requestId": id, "request": { "url": url, "method": method, "headers": { "Accept-Encoding": "gzip, deflate, br, zstd", "X-From-Page": "1" } } } });
    let answer_to = |id: &str| browser.wait_for(|c| c.params["requestId"] == id);

    // When the upstream's URLs are routed through this process
    assert!(page.network_proxy(&format!("http://127.0.0.1:{port}/**"), true).unwrap());
    // Then the page pauses exactly those requests
    assert_eq!(page_calls(&browser, "Fetch.enable"), [json!({ "patterns": [{ "urlPattern": format!("http://127.0.0.1:{port}/*"), "requestStage": "Request" }], "handleAuthRequests": false })]);

    // When the page requests a proxied URL and one that is not
    browser.emit(paused("R1", "GET", format!("http://127.0.0.1:{port}/answer")));
    let fulfilled = answer_to("R1");
    browser.emit(paused("R2", "GET", "http://elsewhere.test/x".into()));
    let continued = answer_to("R2");
    // Then this process made the proxied request with its own client and handed the answer back on the page's session
    assert_eq!((fulfilled.method.as_str(), fulfilled.session.as_deref()), ("Fetch.fulfillRequest", Some("S1")));
    assert_eq!(fulfilled.params["responseCode"], 200);
    assert_eq!(STANDARD.decode(fulfilled.params["body"].as_str().unwrap()).unwrap(), b"from upstream");
    let headers = fulfilled.params["responseHeaders"].as_array().unwrap();
    assert!(headers.contains(&json!({ "name": "access-control-allow-origin", "value": "*" })), "{headers:?}");
    assert!(!headers.iter().any(|h| h["name"].as_str().unwrap().eq_ignore_ascii_case("content-length")), "{headers:?}");
    {
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, "/answer");
        let header = |name: &str| seen[0].1.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
        assert_eq!(header("accept-encoding"), Some("gzip"), "the browser asks for a longer list; this is the client of this process");
        assert_eq!(header("x-from-page"), Some("1"));
    }
    // And the other request went on to the network untouched
    assert_eq!((continued.method.as_str(), &continued.params), ("Fetch.continueRequest", &json!({ "requestId": "R2" })));

    // When an unreachable URL is proxied too and the page requests it
    page.network_proxy(&format!("http://127.0.0.1:{closed}/*"), true).unwrap();
    browser.emit(paused("R3", "GET", format!("http://127.0.0.1:{closed}/x")));
    let failed = answer_to("R3");
    // Then the page is answered 502 with the JSON error
    assert_eq!(failed.params["responseCode"], 502);
    let body: Value = serde_json::from_slice(&STANDARD.decode(failed.params["body"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(body["error"], "network-proxy-fetch-failed");
    assert_eq!(failed.params["responseHeaders"], json!([{ "name": "content-type", "value": "application/json" }]));

    // When both routes are removed
    page.network_proxy(&format!("http://127.0.0.1:{port}/**"), false).unwrap();
    page.network_proxy(&format!("http://127.0.0.1:{closed}/*"), false).unwrap();
    // Then the page stops pausing requests
    assert_eq!(browser.calls().last().map(|c| c.method.clone()).as_deref(), Some("Fetch.disable"));
}

#[test]
fn given_devtools_endpoints_when_the_raw_client_connects_and_calls_then_results_errors_and_events_reach_the_caller() {
    // Given a browser with no page of its own yet
    let browser = FakeBrowser::start(|call, _| match call.method.as_str() {
        "Browser.getVersion" => Ok(json!({ "product": "HeadlessChrome/141.0.7390.37" })),
        "Target.getTargets" => Ok(json!({ "targetInfos": [{ "targetId": "D1", "type": "page", "url": "devtools://devtools/bundled/inspector.html" }] })),
        "Target.createTarget" => Ok(json!({ "targetId": "T9" })),
        "Target.attachToTarget" => Ok(json!({ "sessionId": "S9" })),
        "Page.enable" | "Runtime.enable" | "Network.enable" => Ok(json!({})),
        other => Err(format!("'{other}' wasn't found")),
    });
    let (host, closed) = ("127.0.0.1", closed_port());

    // When its DevTools HTTP endpoint is read
    let version = json_endpoint(host, browser.port, "/json/version").unwrap();
    let list = json_endpoint(host, browser.port, "/json/list").unwrap();
    // Then the WebSocket URLs come back, and a missing path, a non-JSON answer and a closed port fail by name
    assert_eq!(version["webSocketDebuggerUrl"], browser.browser_ws.as_str());
    assert_eq!(list[0]["webSocketDebuggerUrl"], browser.page_ws.as_str());
    assert_eq!(json_endpoint(host, browser.port, "/json/nope").unwrap_err(), "/json/nope answered HTTP 404");
    assert!(json_endpoint(host, browser.port, "/json/garbled").unwrap_err().starts_with("/json/garbled is not JSON"));
    assert!(json_endpoint(host, closed, "/json/version").unwrap_err().contains(&format!("cannot reach 127.0.0.1:{closed}")));

    // When the client connects and calls a known method, an unknown one, and receives an event
    let cdp = Cdp::connect(&browser.page_ws).unwrap();
    let product = cdp.call("Browser.getVersion", json!({})).unwrap();
    let unknown = cdp.call_on("Nope.nothing", json!({}), Some("S9")).unwrap_err();
    browser.emit(json!({ "method": "Page.loadEventFired", "params": { "timestamp": 1.5 } }));
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut events = Vec::new();
    while events.is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
        events.extend(cdp.take_events());
    }
    // Then the result and the browser's error come back, the call carried its session, and the event was queued
    assert_eq!(product["product"], "HeadlessChrome/141.0.7390.37");
    assert_eq!(unknown, "Nope.nothing failed: 'Nope.nothing' wasn't found");
    assert_eq!(browser.calls_to("Nope.nothing")[0].session.as_deref(), Some("S9"));
    assert_eq!(events, [json!({ "method": "Page.loadEventFired", "params": { "timestamp": 1.5 } })]);

    // When a page is attached straight from the browser's WebSocket URL
    let page = Attached::attach(&browser.browser_ws).unwrap();
    // Then a blank page is opened for it, since the browser has none
    assert_eq!(page_calls(&browser, "Target.createTarget"), [json!({ "url": "about:blank" })]);
    assert_eq!(page_calls(&browser, "Target.attachToTarget"), [json!({ "targetId": "T9", "flatten": true })]);
    assert_eq!(page.session_id(), "S9");

    // When the endpoint is closed, or not a DevTools URL at all
    // Then each fails naming the endpoint
    assert!(Cdp::connect(&format!("ws://127.0.0.1:{closed}/devtools/browser/x")).err().unwrap().contains("cannot reach"));
    assert_eq!(Cdp::connect("http://127.0.0.1:9/").err().unwrap(), "not a ws:// URL: http://127.0.0.1:9/");
    assert_eq!(Attached::attach("ftp://127.0.0.1/").err().unwrap(), "unsupported CDP endpoint ftp://127.0.0.1/");
}
