//! Flow tests of the capture listener: every trigger enters through `api::app`, the builder `main` serves, in process (no socket).
//! The ports are doubled at construction: in-memory fakes for the attempt's outcomes, and for the adapters' own protocols local
//! listeners (the backend, Telegram, the `wf browser` daemon) plus stub programs (`wf`, Xvfb, x11vnc, websockify) in a temporary
//! directory. Time is a fake clock that moves only when the attempt sleeps. Given/When/Then.

#[path = "flow/upstream.rs"]
mod upstream;

use async_trait::async_trait;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::Router;
use capture_core::application::ports::{Backend, PortError, Ports, Registration};
use capture_core::application::Capture;
use capture_core::domain::config::{CaptureConfig, Mode};
use capture_core::domain::cookies::Cookie;
use capture_core::testing::{Events, FakeBackend, FakeBrowser, FakeClock, FakeDisplays, FakeNotifier};
use capture_service::api;
use capture_service::infrastructure::config::Config;
use capture_service::infrastructure::{http_client, HttpBackend, HttpNotifier, WfBrowser, XvfbStacks};
use clap::Parser;
use http_body_util::BodyExt;
use secrecy::SecretString;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Semaphore;
use tower::ServiceExt;
use upstream::{eventually, Seen, Upstream};

const SECRET: &str = "trigger-secret";
const ADMIN: &str = "admin-secret";
const NOTIFY: &str = "notify-secret";
const CALLER: [u8; 4] = [127, 0, 0, 1];

struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    text: String,
}

impl Answer {
    fn json(&self) -> Value {
        serde_json::from_str(&self.text).unwrap_or(Value::Null)
    }

    fn header(&self, name: &str) -> &str {
        self.headers.get(name).and_then(|value| value.to_str().ok()).unwrap_or_default()
    }
}

fn config(mode: Mode) -> CaptureConfig {
    CaptureConfig {
        mode,
        novnc_port: 6081,
        capture_timeout: Duration::from_secs(10),
        poll_interval: Duration::from_secs(2),
        login_url: "https://leetcode.test/accounts/login".into(),
    }
}

fn app(mode: Mode, ports: Ports) -> Router {
    api::app(Arc::new(Capture::new(config(mode), ports)), SecretString::from(SECRET))
}

async fn send(app: &Router, peer: [u8; 4], method: &str, path: &str, secret: Option<&str>) -> Answer {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(secret) = secret {
        request = request.header("x-capture-trigger-secret", secret);
    }
    let mut request = request.body(Body::empty()).unwrap();
    // The rate limit keys on the peer address, which `main`'s server adds from the socket.
    request.extensions_mut().insert(ConnectInfo(SocketAddr::from((peer, 40000))));
    let response = app.clone().oneshot(request).await.unwrap();
    let (status, headers) = (response.status(), response.headers().clone());
    let text = String::from_utf8(response.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    Answer { status, headers, text }
}

async fn trigger(app: &Router) -> Answer {
    send(app, CALLER, "POST", "/trigger", Some(SECRET)).await
}

/// A trigger once the previous attempt has let go: its last notice is sent a moment before it clears its running flag, and
/// a trigger in that gap is answered 200 "alreadyRunning".
async fn trigger_when_idle(app: &Router) -> StatusCode {
    for _ in 0..100 {
        let answer = trigger(app).await;
        if answer.status != StatusCode::OK {
            return answer.status;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    StatusCode::OK
}

fn session_cookies() -> Vec<Cookie> {
    vec![Cookie { name: "LEETCODE_SESSION".into(), value: "s1".into() }, Cookie { name: "csrftoken".into(), value: "c1".into() }]
}

fn fakes(events: &Events, backend: Arc<dyn Backend>, browser: Arc<FakeBrowser>, notifier_fails: bool) -> Ports {
    Ports {
        backend,
        notifier: Arc::new(FakeNotifier { events: events.clone(), fail: notifier_fails }),
        browsers: Arc::new(browser),
        displays: Arc::new(FakeDisplays { events: events.clone() }),
        clock: Arc::new(FakeClock::default()),
    }
}

/// A backend whose registration waits until the test lets one through, so an attempt can be held in flight; it then reports a
/// capture already active, which ends the attempt.
struct HeldBackend {
    events: Events,
    gate: Semaphore,
}

#[async_trait]
impl Backend for HeldBackend {
    async fn register(&self) -> Result<Registration, PortError> {
        self.events.push("backend.register");
        self.gate.acquire().await.unwrap().forget();
        Ok(Registration { started: false, expires_at: 1 })
    }

    async fn submit_cookie(&self, _cookie: &str) -> Result<(), PortError> {
        unreachable!("an attempt that is already active pushes nothing")
    }
}

#[tokio::test]
async fn given_callers_with_and_without_the_secret_when_they_trigger_then_only_post_trigger_with_it_starts_one_attempt_at_a_time_within_the_rate_limit() {
    let events = Events::default();
    let backend = Arc::new(HeldBackend { events: events.clone(), gate: Semaphore::new(0) });
    let app = app(Mode::Local, fakes(&events, backend.clone(), FakeBrowser::new(&events, vec![true], session_cookies()), false));
    let other_caller = [10, 0, 0, 7];

    let other_method = send(&app, CALLER, "GET", "/trigger", Some(SECRET)).await;
    let other_path = send(&app, CALLER, "POST", "/nope", Some(SECRET)).await;
    let without_secret = send(&app, CALLER, "POST", "/trigger", None).await;
    let wrong_secret = send(&app, CALLER, "POST", "/trigger", Some("nope")).await;
    let started_by_refusals = events.all();
    let accepted = trigger(&app).await;
    let registering = eventually(|| events.all() == ["backend.register"]).await;
    let while_running = trigger(&app).await;
    backend.gate.add_permits(1);
    let mut after = trigger(&app).await;
    for _ in 0..100 {
        if after.status == StatusCode::ACCEPTED {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        after = trigger(&app).await;
    }
    backend.gate.add_permits(1);
    let mut limited = Vec::new();
    for _ in 0..30 {
        limited.push(send(&app, other_caller, "POST", "/trigger", Some("wrong")).await.status);
    }
    let over_limit = send(&app, other_caller, "POST", "/trigger", Some("wrong")).await;
    let first_caller_still_heard = send(&app, CALLER, "POST", "/trigger", Some("wrong")).await;

    assert_eq!((other_method.status, other_method.text.as_str()), (StatusCode::NOT_FOUND, ""));
    assert_eq!((other_path.status, other_path.text.as_str()), (StatusCode::NOT_FOUND, ""));
    for refused in [&without_secret, &wrong_secret] {
        assert_eq!((refused.status, refused.json()), (StatusCode::UNAUTHORIZED, json!({ "error": "unauthorized" })));
        assert_eq!(refused.header("content-type"), "application/json");
    }
    assert!(started_by_refusals.is_empty());
    assert_eq!((accepted.status, accepted.json()), (StatusCode::ACCEPTED, json!({ "accepted": true })));
    assert!(registering);
    assert_eq!((while_running.status, while_running.json()), (StatusCode::OK, json!({ "accepted": true, "alreadyRunning": true })));
    assert_eq!((after.status, after.json()), (StatusCode::ACCEPTED, json!({ "accepted": true })));
    assert!(limited.iter().all(|status| *status == StatusCode::UNAUTHORIZED));
    assert_eq!((over_limit.status, over_limit.json()), (StatusCode::TOO_MANY_REQUESTS, json!({ "error": "rate_limited" })));
    let retry_after: u64 = over_limit.header("retry-after").parse().unwrap();
    assert!((1..=60).contains(&retry_after), "{retry_after}");
    assert_eq!(first_caller_still_heard.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn given_each_way_a_sign_in_can_go_when_an_attempt_is_triggered_then_its_steps_run_in_order_and_everything_opened_is_closed() {
    let succeed = Events::default();
    let remote = Events::default();
    let nobody = Events::default();
    let no_cookies = Events::default();
    let no_csrf = Events::default();
    let no_browser = Events::default();
    let only_csrf = vec![Cookie { name: "other".into(), value: "x".into() }, Cookie { name: "csrftoken".into(), value: "c1".into() }];
    let worlds = [
        (Mode::Local, &succeed, FakeBrowser::new(&succeed, vec![false, false, true], session_cookies())),
        (Mode::Remote, &remote, FakeBrowser::new(&remote, vec![true], session_cookies())),
        (Mode::Local, &nobody, FakeBrowser::new(&nobody, vec![false], session_cookies())),
        (Mode::Local, &no_cookies, FakeBrowser::new(&no_cookies, vec![true], only_csrf)),
        (Mode::Local, &no_csrf, FakeBrowser::new(&no_csrf, vec![true], session_cookies()[..1].to_vec())),
        (Mode::Local, &no_browser, FakeBrowser::failing_to_launch(&no_browser, "open failed")),
    ];

    let mut answers = Vec::new();
    for (mode, events, browser) in worlds {
        let app = app(mode, fakes(events, Arc::new(FakeBackend::new(events, true)), browser, false));
        answers.push(trigger(&app).await.status);
        let ended = |events: &Events| events.all().last().is_some_and(|last| last == "browser.close" || last == "display.teardown" || last.starts_with("notify LeetCode login-capture failed"));
        assert!(eventually(|| ended(events)).await, "{:?}", events.all());
    }

    assert!(answers.iter().all(|status| *status == StatusCode::ACCEPTED));
    assert_eq!(
        succeed.all(),
        [
            "backend.register",
            "browser.launch display=- url=https://leetcode.test/accounts/login",
            "notify LeetCode session expired — log in now: this machine directly (a Chromium window has opened)",
            "browser.check",
            "browser.check",
            "browser.check",
            "browser.cookies",
            "backend.submit LEETCODE_SESSION=s1; csrftoken=c1",
            "notify LeetCode session captured and pushed to the backend successfully.",
            "browser.close",
        ]
    );
    let remote = remote.all();
    assert_eq!(remote[..3], ["backend.register", "display.start 6081", "browser.launch display=:99 url=https://leetcode.test/accounts/login"]);
    assert_eq!(remote[3], "notify LeetCode session expired — log in now: http://leetcode-capture:6081/vnc.html");
    assert_eq!(remote[remote.len() - 2..], ["browser.close", "display.teardown"]);
    let nobody = nobody.all();
    assert_eq!(nobody.iter().filter(|event| *event == "browser.check").count(), 6, "one check at 0 s, then every 2 s until the 10 s deadline");
    assert_eq!(nobody[nobody.len() - 2..], ["notify LeetCode login-capture timed out waiting for sign-in.", "browser.close"]);
    assert!(!nobody.iter().any(|event| event.starts_with("backend.submit") || event == "browser.cookies"));
    for incomplete in [no_cookies.all(), no_csrf.all()] {
        assert_eq!(incomplete[incomplete.len() - 2..], ["notify LeetCode sign-in detected but session cookies were not found.", "browser.close"]);
        assert!(!incomplete.iter().any(|event| event.starts_with("backend.submit")));
    }
    assert_eq!(
        no_browser.all(),
        ["backend.register", "browser.launch display=- url=https://leetcode.test/accounts/login", "notify LeetCode login-capture failed: open failed"]
    );
}

/// What the stub `wf browser` daemon answers: the sign-in replies `eval` walks through (the last repeats), the cookies, and a
/// method that fails.
#[derive(Default)]
struct Script {
    sign_in: Vec<Value>,
    evals: usize,
    cookies: Value,
    fail_on: Option<&'static str>,
}

fn daemon_reply(script: &Mutex<Script>, seen: &Seen) -> (u16, String) {
    let call = seen.json();
    let method = call["method"].as_str().unwrap_or_default();
    let mut script = script.lock().unwrap();
    let answer = if script.fail_on == Some(method) {
        json!({ "ok": false, "error": format!("{method} failed") })
    } else if method == "eval" {
        let reply = script.sign_in[script.evals.min(script.sign_in.len() - 1)].clone();
        script.evals += 1;
        json!({ "ok": true, "result": { "result": reply.to_string() } })
    } else if method == "cookies" {
        json!({ "ok": true, "result": { "cookies": script.cookies } })
    } else {
        json!({ "ok": true, "result": {} })
    };
    (200, answer.to_string())
}

fn signed_in() -> Value {
    json!({ "data": { "userStatus": { "isSignedIn": true, "username": "u" } } })
}

fn write_program(dir: &Path, name: &str, text: &str) {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn methods(daemon: &Upstream) -> Vec<String> {
    daemon.seen().iter().map(|seen| seen.json()["method"].as_str().unwrap_or_default().to_string()).collect()
}

fn texts(notifier: &Upstream) -> Vec<String> {
    notifier.seen().iter().map(|seen| seen.json()["text"].as_str().unwrap_or_default().to_string()).collect()
}

#[tokio::test]
async fn given_the_real_adapters_when_attempts_run_remote_and_then_local_then_the_backend_telegram_the_browser_daemon_and_the_display_see_their_own_protocols() {
    let (backend, notifier, daemon) = (Upstream::start().await, Upstream::start().await, Upstream::start().await);
    let bin = tempfile::tempdir().unwrap();
    let tools_log = bin.path().join("tools.log");
    std::fs::write(&tools_log, "").unwrap();
    let stub_tool = format!(
        "#!/bin/sh\necho \"start $(basename \"$0\") $*\" >> '{log}'\ntrap 'echo \"term $(basename \"$0\")\" >> '{log}'; exit 0' TERM INT\nwhile :; do sleep 0.05; done\n",
        log = tools_log.display()
    );
    for tool in ["Xvfb", "x11vnc", "websockify"] {
        write_program(bin.path(), tool, &stub_tool);
    }
    let wf_reply = format!("starting\\n{{\"result\":{{\"url\":\"http://127.0.0.1:1\"}}}}\\n{{\"result\":{{\"url\":\"{}\"}}}}\\nnot json", daemon.url);
    write_program(bin.path(), "wf", &format!("#!/bin/sh\nif [ \"$3\" = start ]; then printf '{wf_reply}\\n'; else echo 'no daemon yet'; fi\n"));
    let script = Arc::new(Mutex::new(Script::default()));
    let daemon_script = script.clone();
    daemon.reply(move |seen| daemon_reply(&daemon_script, seen));
    notifier.reply(|_| (200, "{}".to_string()));
    let registered_then = |session: (u16, Value)| {
        move |seen: &Seen| match seen.path.as_str() {
            "/api/leetcode/login-session" => (200, json!({ "expiresAt": 1 }).to_string()),
            _ => (session.0, session.1.to_string()),
        }
    };
    let ports = |mode_bin: &Path| Ports {
        backend: Arc::new(HttpBackend::new(http_client(), backend.url.clone(), SecretString::from(ADMIN))),
        notifier: Arc::new(HttpNotifier::new(http_client(), notifier.url.clone(), SecretString::from(NOTIFY))),
        browsers: Arc::new(WfBrowser::with_program(http_client(), mode_bin.join("wf").display().to_string())),
        displays: Arc::new(XvfbStacks::in_dir(mode_bin)),
        clock: Arc::new(FakeClock::default()),
    };
    let tools = || std::fs::read_to_string(&tools_log).unwrap().lines().map(str::to_string).collect::<Vec<_>>();
    {
        let mut script = script.lock().unwrap();
        script.sign_in = vec![json!({ "data": null }), json!({ "data": { "userStatus": { "isSignedIn": "yes" } } }), json!({}), signed_in()];
        script.cookies = json!([{ "name": "other", "value": "x" }, { "name": "csrftoken", "value": "csrf-xyz" }, { "name": "LEETCODE_SESSION", "value": "sess-abc" }]);
    }
    backend.reply(registered_then((200, json!({ "ok": true }))));

    let remote = app(Mode::Remote, ports(bin.path()));
    let remote_accepted = trigger(&remote).await;
    let remote_done = eventually(|| tools().len() == 6 && methods(&daemon).last().is_some_and(|m| m == "close")).await;
    let (remote_backend, remote_daemon, remote_texts, remote_notices) = (backend.seen(), daemon.seen(), texts(&notifier), notifier.seen());

    let local = app(Mode::Local, ports(bin.path()));
    let mut local_answers = Vec::new();
    backend.reply(|_| (409, json!({ "expiresAt": 4242 }).to_string()));
    local_answers.push(trigger_when_idle(&local).await);
    let already_active_registered = eventually(|| backend.seen().len() == remote_backend.len() + 1).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let after_already_active = (daemon.seen().len(), notifier.seen().len());
    backend.reply(|_| (500, json!({ "expiresAt": 0 }).to_string()));
    local_answers.push(trigger_when_idle(&local).await);
    let refused_registered = eventually(|| backend.seen().len() == remote_backend.len() + 2).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let after_refused_registration = (daemon.seen().len(), notifier.seen().len());
    script.lock().unwrap().evals = 0;
    backend.reply(registered_then((500, json!("boom"))));
    local_answers.push(trigger_when_idle(&local).await);
    let cookie_refused_told = eventually(|| texts(&notifier).len() == remote_texts.len() + 2 && methods(&daemon).last().is_some_and(|m| m == "close")).await;
    let local_daemon = daemon.seen()[remote_daemon.len()..].to_vec();
    script.lock().unwrap().fail_on = Some("open");
    local_answers.push(trigger_when_idle(&local).await);
    let open_failure_told = eventually(|| texts(&notifier).len() == remote_texts.len() + 3).await;
    let pushes_before_notifier_outage = backend.seen().iter().filter(|seen| seen.path == "/api/leetcode/session").count();
    script.lock().unwrap().fail_on = None;
    notifier.reply(|_| (500, "{}".to_string()));
    backend.reply(registered_then((200, json!({ "ok": true }))));
    local_answers.push(trigger_when_idle(&local).await);
    let pushed_despite_notifier =
        eventually(|| backend.seen().iter().filter(|seen| seen.path == "/api/leetcode/session").count() == pushes_before_notifier_outage + 1).await;

    assert_eq!((remote_accepted.status, remote_accepted.json()), (StatusCode::ACCEPTED, json!({ "accepted": true })));
    assert!(remote_done, "{:?} {:?}", tools(), methods(&daemon));
    assert_eq!(
        remote_backend.iter().map(|seen| (seen.method.as_str(), seen.path.as_str(), seen.header("x-admin-secret"))).collect::<Vec<_>>(),
        [("POST", "/api/leetcode/login-session", ADMIN), ("POST", "/api/leetcode/session", ADMIN)]
    );
    assert_eq!(remote_backend[1].header("content-type"), "application/json");
    assert_eq!(remote_backend[1].json(), json!({ "cookie": "LEETCODE_SESSION=sess-abc; csrftoken=csrf-xyz" }));
    let calls: Vec<Value> = remote_daemon.iter().map(Seen::json).collect();
    let remote_methods: Vec<&str> = calls.iter().map(|call| call["method"].as_str().unwrap()).collect();
    assert_eq!(remote_methods, ["open", "navigate", "eval", "eval", "eval", "eval", "cookies", "close"]);
    let open = &calls[0]["args"];
    assert_eq!(
        [&open["name"], &open["renderMode"], &open["keyboardMode"], &open["headless"], &open["launchArgs"], &open["launchEnv"]["DISPLAY"]],
        [&json!("leetcode-auth-capture"), &json!("plain"), &json!("none"), &json!(false), &json!(["--no-sandbox"]), &json!(":99")]
    );
    assert_eq!(calls[1]["args"], json!({ "name": "leetcode-auth-capture", "url": "https://leetcode.test/accounts/login" }));
    assert!(calls[2]["args"]["expression"].as_str().unwrap().contains("userStatus { isSignedIn username }"));
    assert_eq!(calls[7]["args"], json!({ "name": "leetcode-auth-capture" }));
    assert_eq!(
        remote_texts,
        ["LeetCode session expired — log in now: http://leetcode-capture:6081/vnc.html", "LeetCode session captured and pushed to the backend successfully."]
    );
    for notice in &remote_notices {
        assert_eq!((notice.method.as_str(), notice.path.as_str(), notice.header("x-notify-secret"), notice.header("content-type")), ("POST", "/notify", NOTIFY, "application/json"));
    }
    assert_eq!(
        tools(),
        [
            "start Xvfb :99 -screen 0 1280x800x24",
            "start x11vnc -display :99 -rfbport 5900 -forever -shared -nopw",
            "start websockify --web=/usr/share/novnc 6081 localhost:5900",
            "term websockify",
            "term x11vnc",
            "term Xvfb",
        ]
    );

    assert!(local_answers.iter().all(|status| *status == StatusCode::ACCEPTED), "{local_answers:?}");
    assert!(already_active_registered && refused_registered);
    assert_eq!(after_already_active, (remote_daemon.len(), remote_notices.len()));
    assert_eq!(after_refused_registration, (remote_daemon.len(), remote_notices.len()));
    assert!(cookie_refused_told && open_failure_told && pushed_despite_notifier);
    let local_open = &local_daemon[0].json()["args"];
    assert_eq!(*local_open, json!({ "name": "leetcode-auth-capture", "renderMode": "plain", "keyboardMode": "none", "headless": false }));
    assert_eq!(
        texts(&notifier)[remote_texts.len()..remote_texts.len() + 3],
        [
            "LeetCode session expired — log in now: this machine directly (a Chromium window has opened)",
            "LeetCode login-capture failed: POST /session failed (500): \"boom\"",
            "LeetCode login-capture failed: open failed",
        ]
    );
    assert_eq!(tools().len(), 6);
}

#[test]
fn given_a_required_setting_that_is_empty_or_a_mode_that_does_not_exist_when_the_listener_starts_then_it_refuses_and_names_the_variable() {
    let complete = [
        ("--backend-base-url", "http://backend"),
        ("--admin-secret", ADMIN),
        ("--notify-base-url", "http://notify"),
        ("--notify-secret", NOTIFY),
        ("--trigger-secret", SECRET),
        ("--mode", "local"),
    ];
    let parse = |flag: &str, value: &str| {
        let args = complete.iter().flat_map(|(name, default)| [name.to_string(), if *name == flag { value.to_string() } else { default.to_string() }]);
        Config::try_parse_from(std::iter::once("leetcode-auth-cookie-capture-rs".to_string()).chain(args))
    };
    let refusals = [
        ("--backend-base-url", "", "LEETCODE_BACKEND_BASE_URL"),
        ("--admin-secret", "", "LEETCODE_BACKEND_ADMIN_SECRET"),
        ("--notify-base-url", "", "TELEGRAM_NOTIFY_BASE_URL"),
        ("--notify-secret", "", "TELEGRAM_NOTIFY_SECRET"),
        ("--trigger-secret", "", "LEETCODE_CAPTURE_TRIGGER_SECRET"),
        ("--mode", "sideways", "CAPTURE_MODE"),
    ];

    let accepted = parse("", "");
    let refused: Vec<(String, &str)> = refusals.iter().map(|(flag, value, variable)| (parse(flag, value).err().map(|e| e.to_string()).unwrap_or_default(), *variable)).collect();

    assert!(accepted.is_ok());
    for (error, variable) in refused {
        assert!(error.contains(variable), "{variable}: {error}");
    }
}
