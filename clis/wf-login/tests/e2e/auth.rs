//! `wf secret login listener` driving a real desktop Chromium through a whole login. Integration tier: the flow tests (`flow/auth.rs`) double the browser.
use serde_json::{json, Value};
use std::net::TcpListener;
use std::time::Duration;
use crate::rig::*;

fn chrome() -> Option<String> {
    cdp::launch::find_chromium().map(|p| p.display().to_string())
}

/// `(width, height)` from a JPEG's first start-of-frame marker.
fn jpeg_size(bytes: &[u8]) -> (u32, u32) {
    let mut at = 2;
    while at + 9 < bytes.len() {
        let (marker, len) = (bytes[at + 1], u32::from(bytes[at + 2]) << 8 | u32::from(bytes[at + 3]));
        if bytes[at] == 0xFF && (0xC0..=0xC2).contains(&marker) {
            return (u32::from(bytes[at + 7]) << 8 | u32::from(bytes[at + 8]), u32::from(bytes[at + 5]) << 8 | u32::from(bytes[at + 6]));
        }
        at += 2 + len as usize;
    }
    panic!("no start-of-frame marker");
}

#[test]
fn the_listener_drives_a_real_desktop_browser_through_a_login_and_delivers_the_captured_cookies() {
    let chrome = chrome().expect("no Chromium: set WF_CHROMIUM or install one under PLAYWRIGHT_BROWSERS_PATH ");
    // Given a site that sets a session cookie and shows a consent banner and an email field, a delivery sink, and the desktop's
    // Chromium (what `s6-setuidgid <user> sh -c 'exec chromium "$1"'` starts) headless on a known CDP port
    let c = Container::new();
    let site = Recorder::start(|req| {
        if req.url.starts_with("/login") {
            Answer {
                status: 200,
                headers: vec![("set-cookie".into(), "SID=abc; Path=/".into()), ("set-cookie".into(), "csrftoken=xyz; Path=/".into()), ("content-type".into(), "text/html".into())],
                body: b"<!doctype html><meta name=viewport content=\"width=device-width,initial-scale=1\"><body><button onclick=\"this.remove()\">Reject all</button><form><input type=\"email\" name=\"email\"></form></body>".to_vec(),
            }
        } else {
            ok("ok")
        }
    });
    let sink = Recorder::start(|_| ok("{}"));
    let cdp_port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    c.stub("s6-setuidgid", "shift\nexec \"$@\"");
    let dir = c.dir.path().display().to_string();
    c.stub("chromium", &format!("echo $$ > \"{dir}/chrome.pid\"; exec \"{chrome}\" --headless=new --no-sandbox --disable-gpu --remote-debugging-port={cdp_port} --user-data-dir=\"{dir}/profile\" \"$@\""));
    let l = c.up(&[("VNC_AUTH_CDP_ENDPOINT", Some(&format!("http://127.0.0.1:{cdp_port}"))), ("SINK_URL", Some(&format!("http://127.0.0.1:{}", sink.port))), ("SINK_SECRET", Some("sink-secret"))]);
    let spec = |id: &str, require: Value, deliver: Value| json!({ "spec": { "id": id, "loginUrl": format!("http://127.0.0.1:{}/login", site.port), "authErrorPatterns": ["401"], "capture": { "kind": "cookies", "urls": [format!("http://127.0.0.1:{}", site.port)], "require": require }, "deliver": deliver } });
    let kill_chrome = || {
        if let Some(pid) = std::fs::read_to_string(format!("{dir}/chrome.pid")).ok().and_then(|t| t.trim().parse::<i32>().ok()) {
            let _ = std::process::Command::new("kill").args(["-KILL", &pid.to_string()]).status();
        }
    };

    // When a login-URL session starts with no browser running
    let started = l.authed("/auth/start", spec("site", json!(["SID", "csrftoken"]), json!({ "kind": "http", "url": "${SINK_URL}/store", "headers": { "x-sink-secret": "${SINK_SECRET}" }, "jsonBody": { "cookie": "${credential}" } }))).json();
    let (session, link) = (text(&started["sessionId"]), token_of(&started["linkUrl"]));
    l.wait_for("vnc_auth.desktop_launcher.navigated", Duration::from_secs(60));
    // Then the desktop's Chromium is cold-started at the page, the consent banner is rejected on the human's behalf, and the
    // email field is prefilled
    assert!(site.seen().iter().any(|r| r.url == "/login"));
    let checked = l.wait_for("vnc_auth.desktop_launcher.cookie_banner_checked", Duration::from_secs(30));
    assert_eq!((text(&checked["dismissed"]), text(&checked["action"]), text(&checked["label"])), ("true".into(), "reject".into(), "Reject all".into()));
    assert_eq!(text(&l.wait_for("vnc_auth.desktop_launcher.prefilled", Duration::from_secs(20))["result"]), "{\"email\":true}");

    // When the viewer asks for a frame and sends a tap, a scroll, a key and a key it cannot send
    let input = |event: &str| fetch(l.gate, "POST", &format!("/input?t={link}"), &[("content-type", "application/json")], Some(event)).status;
    let frame = l.gate(&format!("/frame?t={link}"), &[]);
    let sent = [input(r#"{"type":"tap","x":20,"y":20}"#), input(r#"{"type":"scroll","x":20,"y":20,"dy":40}"#), input(r#"{"type":"key","key":"Enter"}"#), input(r#"{"type":"key","key":"F13"}"#)];
    // Then the frame is a JPEG of the whole phone viewport, flagged editable while the prefilled email field holds the focus, and
    // the events are 204 (400 for the unsupported key)
    assert_eq!((frame.status, frame.header("content-type"), frame.header("x-editable")), (200, Some("image/jpeg"), Some("1")));
    assert_eq!(&frame.body[..3], &[0xFF, 0xD8, 0xFF], "a JPEG");
    assert_eq!(jpeg_size(&frame.body), (393, 852), "the whole phone viewport, not the headless window");
    assert!(frame.header("x-frame-seq").is_some_and(|n| n.parse::<u64>().is_ok_and(|n| n >= 1)), "every frame is numbered for the long poll");
    assert_eq!(sent, [204, 204, 204, 400]);

    // When an allowlisted magic link is launched
    let launched = l.authed("/auth/launch-url", json!({ "url": "https://claude.ai/magic-link#frag-9k2x" }));
    // Then it is accepted and its fragment never reaches the log
    assert_eq!((launched.status, launched.json()["ok"].clone()), (200, json!(true)));

    // When the human has finished and the session is completed
    let done = l.authed("/auth/done", json!({ "sessionId": session }));
    // Then the required cookies are read out of the browser and delivered with the spec's own headers
    assert_eq!((done.status, done.json()), (200, json!({ "ok": true, "persisted": true })));
    let delivered = sink.seen();
    assert_eq!(delivered.len(), 1);
    assert_eq!((delivered[0].method.as_str(), delivered[0].url.as_str(), delivered[0].headers["x-sink-secret"].as_str()), ("POST", "/store", "sink-secret"));
    assert_eq!(serde_json::from_str::<Value>(&delivered[0].body).unwrap(), json!({ "cookie": "SID=abc; csrftoken=xyz" }));

    // When a session requires a cookie the browser never got, or delivers to a variable the listener lacks
    let missing = l.authed("/auth/start", spec("site-missing", json!(["SID", "NOPE"]), json!({ "kind": "http", "url": "${SINK_URL}/store" }))).json();
    let missing_done = l.authed("/auth/done", json!({ "sessionId": missing["sessionId"] }));
    let unset = l.authed("/auth/start", spec("site-env", json!(["SID"]), json!({ "kind": "http", "url": "${SINK_URL}/store", "headers": { "authorization": "Bearer ${NOT_SET_ANYWHERE}" } }))).json();
    let unset_done = l.authed("/auth/done", json!({ "sessionId": unset["sessionId"] }));
    // Then each is 502 naming the cookie or the variable, and nothing more is delivered
    assert_eq!(missing_done.status, 502);
    assert_eq!((missing_done.json()["error"].clone(), missing_done.json()["detail"].clone()), (json!("capture completed but the credential could not be stored"), json!("no session cookie found in the login browser: missing NOPE")));
    assert_eq!(unset_done.status, 502);
    assert!(unset_done.json()["detail"].to_string().contains("${NOT_SET_ANYWHERE}"), "{}", unset_done.text());
    assert_eq!(sink.seen().len(), 1);

    // And no link token, cookie value, trigger secret, delivery secret or link fragment ever reached the log
    let everything = Value::Array(l.logs()).to_string();
    assert!(link.len() > 30);
    for secret in [link.as_str(), "csrftoken=xyz", "SID=abc", SECRET, "sink-secret", "frag-9k2x"] {
        assert!(!everything.contains(secret), "{secret} reached the log");
    }
    kill_chrome();
}
