//! Flow tests of leetcode-backend: every request enters through `api::app`, the builder `main` serves, in process (no socket). The
//! ports are doubled at construction: leetcode.com and the capture tool by in-memory fakes (or, for the adapters' own protocol, by
//! local listeners), the clock by a fixed one, the session file in a temporary directory. The code runner is the real one, because
//! its harness scripts are this service's code. Given/When/Then.

#[path = "flow/restored_state.rs"]
mod restored_state;
#[path = "flow/upstream.rs"]
mod upstream;

use axum::body::{Body, Bytes};
use axum::extract::ConnectInfo;
use axum::http::request::Builder;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use leetcode_backend::api::{self, Settings};
use leetcode_backend::infrastructure::{http_client, FileSessionStore, HttpCaptureTool, HttpLeetcode, ProcessRunner};
use leetcode_backend_core::application::ports::{CaptureTool, Leetcode, Ports};
use leetcode_backend_core::testing::{FakeCapture, FakeLeetcode, FixedClock};
use secrecy::SecretString;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;
use upstream::{eventually, Upstream};

const HOUR: u64 = 60 * 60 * 1000;
const NOW: u64 = 10 * HOUR;
const CAPTURE_TTL: u64 = 5 * 60 * 1000;
const BEARER: &str = "gateway-seed";
const ADMIN: &str = "admin-secret";
const CAPTURE_SECRET: &str = "capture-secret";
const CALLER: [u8; 4] = [127, 0, 0, 1];

struct World {
    app: Router,
    dir: TempDir,
    leetcode: Arc<FakeLeetcode>,
    capture: Arc<FakeCapture>,
    clock: Arc<FixedClock>,
}

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

impl World {
    fn new() -> Self {
        Self::limited(api::REQUESTS_PER_MINUTE)
    }

    /// The service with its own per-caller limit, so a refusal does not depend on how fast this host sends.
    fn limited(requests_per_minute: u32) -> Self {
        let (leetcode, capture) = (FakeLeetcode::signed_in(), FakeCapture::new());
        Self::over_limited(leetcode.clone(), capture.clone(), leetcode, capture, requests_per_minute)
    }

    /// The real leetcode.com and capture-tool adapters, dialling the given local listeners.
    fn with_adapters(leetcode_url: &str, capture_url: &str) -> Self {
        let leetcode = Arc::new(HttpLeetcode::new(http_client(), leetcode_url));
        let capture = Arc::new(HttpCaptureTool::new(http_client(), capture_url, SecretString::from(CAPTURE_SECRET)));
        Self::over_limited(leetcode, capture, FakeLeetcode::signed_in(), FakeCapture::new(), api::REQUESTS_PER_MINUTE)
    }

    fn over_limited(leetcode: Arc<dyn Leetcode>, capture: Arc<dyn CaptureTool>, fake_leetcode: Arc<FakeLeetcode>, fake_capture: Arc<FakeCapture>, requests_per_minute: u32) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let clock = FixedClock::at(NOW);
        let ports = Ports {
            store: Arc::new(FileSessionStore::new(dir.path().join("session.json"))),
            leetcode,
            capture,
            runner: Arc::new(ProcessRunner),
            clock: clock.clone(),
        };
        let settings = Settings { commit: "abc123".into(), bearer: SecretString::from(BEARER), admin: SecretString::from(ADMIN), requests_per_minute };
        Self { app: api::app(ports, settings), dir, leetcode: fake_leetcode, capture: fake_capture, clock }
    }

    fn session_file(&self) -> PathBuf {
        self.dir.path().join("session.json")
    }

    fn store(&self, session: Value) {
        self.store_raw(&session.to_string());
    }

    fn store_raw(&self, text: &str) {
        std::fs::write(self.session_file(), text).unwrap();
    }

    fn stored(&self) -> Option<Value> {
        std::fs::read_to_string(self.session_file()).ok().map(|text| serde_json::from_str(&text).unwrap())
    }

    async fn send(&self, request: Request<Body>) -> Answer {
        self.send_from(CALLER, request).await
    }

    /// `request` as if from `peer`: the rate limit keys on the address `main`'s server takes from the socket.
    async fn send_from(&self, peer: [u8; 4], mut request: Request<Body>) -> Answer {
        request.extensions_mut().insert(ConnectInfo(SocketAddr::from((peer, 40000))));
        let response = self.app.clone().oneshot(request).await.unwrap();
        let (status, headers) = (response.status(), response.headers().clone());
        let text = String::from_utf8(response.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
        Answer { status, headers, text }
    }
}

fn session(id: &str, csrf: &str, verified_at: u64) -> Value {
    json!({ "sessionId": id, "csrfToken": csrf, "capturedAt": verified_at, "lastVerifiedAt": verified_at, "status": "active" })
}

fn anonymous(method: &str, path: &str) -> Builder {
    Request::builder().method(method).uri(path)
}

fn call(method: &str, path: &str) -> Builder {
    anonymous(method, path).header("authorization", format!("Bearer {BEARER}"))
}

fn admin(method: &str, path: &str) -> Builder {
    call(method, path).header("x-admin-secret", ADMIN)
}

fn empty(builder: Builder) -> Request<Body> {
    builder.body(Body::empty()).unwrap()
}

fn with_json(builder: Builder, body: Value) -> Request<Body> {
    raw(builder, "application/json", body.to_string())
}

fn raw(builder: Builder, content_type: &str, body: impl Into<Body>) -> Request<Body> {
    builder.header("content-type", content_type).body(body.into()).unwrap()
}

fn test_body(slug: &str) -> Value {
    json!({ "slug": slug, "lang": "python3", "questionId": 1, "code": "class Solution: pass", "dataInput": "[2,7]\n9" })
}

fn submit_body(slug: &str) -> Value {
    json!({ "slug": slug, "lang": "python3", "questionId": 1, "code": "class Solution: pass" })
}

fn unauthorized(answer: &Answer) -> bool {
    answer.status == StatusCode::UNAUTHORIZED && answer.json() == json!({ "error": "unauthorized" })
}

#[tokio::test]
async fn given_callers_with_and_without_credentials_when_they_probe_the_edge_then_cors_auth_body_rules_and_the_rate_limit_hold_and_nothing_reaches_leetcode() {
    let w = World::new();
    w.store(session("sid", "csrf", NOW));
    let with_admin_header = |builder: Builder, secret: Option<&str>| match secret {
        Some(value) => builder.header("x-admin-secret", value),
        None => builder,
    };
    let misfits = [
        ("/api/leetcode/test", json!({ "slug": "a", "lang": "python3", "questionId": 1, "code": "x" })),
        ("/api/leetcode/test", json!({ "slug": "", "lang": "python3", "questionId": 1, "code": "x", "dataInput": "" })),
        ("/api/leetcode/test", json!({ "slug": "a", "lang": "python3", "questionId": "1", "code": "x", "dataInput": "" })),
        ("/api/leetcode/submit", json!({ "slug": "a", "lang": "python3", "code": "x" })),
        ("/api/leetcode/submit", json!({ "slug": "a", "lang": "", "questionId": 1, "code": "x" })),
        ("/api/leetcode/run", json!({ "code": "x", "methodName": "f", "testCases": [] })),
        ("/api/leetcode/run", json!({ "lang": "python3", "code": "x", "methodName": "", "testCases": [] })),
        ("/api/leetcode/run", json!({ "lang": "python3", "code": "x", "methodName": "f", "testCases": "no" })),
        ("/api/leetcode/run", json!({ "lang": "python3", "code": "x", "methodName": "f", "testCases": [{ "inputs": {}, "expected": 1, "compareMode": "fuzzy" }] })),
        ("/api/leetcode/format", json!({ "code": "x" })),
        ("/api/leetcode/format", json!({ "lang": "", "code": "x" })),
        ("/api/leetcode/format", json!({ "lang": "python3" })),
        ("/api/leetcode/format", json!("just a string")),
    ];

    let health = w.send(empty(anonymous("GET", "/health").header("origin", "https://app.example"))).await;
    let head = w.send(empty(anonymous("HEAD", "/health"))).await;
    let preflight = w
        .send(empty(
            anonymous("OPTIONS", "/api/leetcode/test")
                .header("origin", "https://app.example")
                .header("access-control-request-method", "POST")
                .header("access-control-request-headers", "content-type,x-admin-secret"),
        ))
        .await;
    let unrouted = [w.send(empty(anonymous("GET", "/HEALTH/"))).await, w.send(empty(anonymous("GET", "/nope"))).await];
    let mut without_bearer = Vec::new();
    for authorization in [None, Some("Bearer not-the-seed"), Some("Bearer x"), Some("Basic Z2F0ZXdheS1zZWVk"), Some(BEARER)] {
        let request = anonymous("GET", "/api/leetcode/user-status");
        let request = match authorization {
            Some(value) => request.header("authorization", value),
            None => request,
        };
        without_bearer.push(w.send(empty(request)).await);
    }
    let with_bearer = w.send(empty(call("GET", "/api/leetcode/login-session"))).await;
    let mut without_admin = Vec::new();
    for secret in [None, Some("nope"), Some("admin-secret-and-more")] {
        let cookie = json!({ "cookie": "LEETCODE_SESSION=a" });
        without_admin.push(w.send(with_json(with_admin_header(call("POST", "/api/leetcode/session"), secret), cookie)).await);
        without_admin.push(w.send(raw(with_admin_header(call("POST", "/api/leetcode/session"), secret), "application/json", "{not json")).await);
        without_admin.push(w.send(empty(with_admin_header(call("POST", "/api/leetcode/login-session"), secret))).await);
        without_admin.push(w.send(empty(with_admin_header(call("GET", "/api/leetcode/nope"), secret))).await);
    }
    let admin_without_bearer =
        w.send(with_json(with_admin_header(anonymous("POST", "/api/leetcode/session"), Some(ADMIN)), json!({ "cookie": "LEETCODE_SESSION=a" }))).await;
    let unrouted_with_admin = w.send(empty(admin("GET", "/api/leetcode/nope"))).await;
    let mut unprocessable = Vec::new();
    for (path, body) in misfits {
        unprocessable.push(w.send(with_json(call("POST", path), body)).await);
    }
    let names_the_field = w.send(with_json(call("POST", "/api/leetcode/format"), json!({ "lang": 1 }))).await;
    let malformed = w.send(raw(call("POST", "/api/leetcode/format"), "application/json", r#"{"lang": "#)).await;
    let not_json = w.send(raw(call("POST", "/api/leetcode/format"), "text/plain", r#"{"lang":"python3","code":"x"}"#)).await;
    let too_large = w.send(with_json(call("POST", "/api/leetcode/format"), json!({ "lang": "python3", "code": "x".repeat(1_100_000) }))).await;
    let capture_after = w.send(empty(call("GET", "/api/leetcode/login-session"))).await;
    // a limit of 3 a minute refills one request every 20 s, so the fourth is refused however slowly this host sends
    let limited = World::limited(3);
    let other_caller = [10, 0, 0, 7];
    let mut within_limit = Vec::new();
    for _ in 0..3 {
        within_limit.push(limited.send_from(other_caller, empty(anonymous("GET", "/health"))).await.status);
    }
    let over_limit = limited.send_from(other_caller, empty(anonymous("GET", "/health"))).await;
    let first_caller_still_admitted = limited.send(empty(anonymous("GET", "/health"))).await;

    assert_eq!((health.status, health.json()), (StatusCode::OK, json!({ "status": "ok", "commit": "abc123" })));
    assert_eq!(health.header("access-control-allow-origin"), "*");
    assert_eq!((head.status, head.text.as_str()), (StatusCode::OK, ""));
    assert_eq!(preflight.status, StatusCode::OK);
    assert_eq!(
        [preflight.header("access-control-allow-origin"), preflight.header("access-control-allow-methods"), preflight.header("access-control-allow-headers")],
        ["*", "GET,HEAD,PUT,PATCH,POST,DELETE", "content-type,x-admin-secret"]
    );
    assert!(unrouted.iter().all(|answer| answer.status == StatusCode::NOT_FOUND));
    assert!(without_bearer.iter().all(unauthorized));
    assert_eq!(with_bearer.status, StatusCode::OK);
    assert!(without_admin.iter().chain([&admin_without_bearer]).all(unauthorized));
    assert_eq!((unrouted_with_admin.status, unrouted_with_admin.json()), (StatusCode::NOT_FOUND, json!({ "error": "not_found" })));
    for answer in &unprocessable {
        assert_eq!(answer.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", answer.text);
        assert!(!answer.text.is_empty());
    }
    assert_eq!(names_the_field.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(names_the_field.text.contains("lang"), "{}", names_the_field.text);
    assert_eq!(
        [malformed.status, not_json.status, too_large.status],
        [StatusCode::BAD_REQUEST, StatusCode::UNSUPPORTED_MEDIA_TYPE, StatusCode::PAYLOAD_TOO_LARGE]
    );
    assert_eq!(w.stored(), Some(session("sid", "csrf", NOW)));
    assert_eq!(capture_after.json(), json!({ "active": false, "expiresAt": null }));
    assert!(w.leetcode.calls().is_empty());
    assert!(within_limit.iter().all(|status| *status == StatusCode::OK));
    assert_eq!(over_limit.status, StatusCode::TOO_MANY_REQUESTS);
    let retry_after: u64 = over_limit.header("retry-after").parse().unwrap();
    assert!(retry_after <= 60, "{retry_after}");
    assert_eq!(first_caller_still_admitted.status, StatusCode::OK);
}

#[tokio::test]
async fn given_no_session_when_a_person_captures_one_uses_it_and_lets_it_go_stale_then_every_step_answers_as_the_session_allows() {
    let w = World::new();
    let post_cookie = |cookie: &str| with_json(admin("POST", "/api/leetcode/session"), json!({ "cookie": cookie }));
    let user_status = || empty(call("GET", "/api/leetcode/user-status"));
    let signed_out = json!({ "isSignedIn": false, "username": null });

    let no_session_status = w.send(user_status()).await;
    let no_session_test = w.send(with_json(call("POST", "/api/leetcode/test"), test_body("two-sum"))).await;
    let asked_without_capture = w.capture.asked();
    let idle = w.send(empty(call("GET", "/api/leetcode/login-session"))).await;
    let started = w.send(empty(admin("POST", "/api/leetcode/login-session"))).await;
    let refused_start = w.send(empty(admin("POST", "/api/leetcode/login-session"))).await;
    let active = w.send(empty(call("GET", "/api/leetcode/login-session"))).await;
    let during_capture = w.send(user_status()).await;
    let asked_during_capture = w.capture.asked();
    w.clock.advance(CAPTURE_TTL);
    let after_expiry = w.send(empty(call("GET", "/api/leetcode/login-session"))).await;
    let asked_after_expiry = {
        w.send(user_status()).await;
        w.capture.asked()
    };
    let restarted = w.send(empty(admin("POST", "/api/leetcode/login-session"))).await;
    let now = NOW + CAPTURE_TTL;

    let no_field = w.send(with_json(admin("POST", "/api/leetcode/session"), json!({}))).await;
    let empty_cookie = w.send(post_cookie("")).await;
    let without_session_value = w.send(post_cookie("csrftoken=only-the-token")).await;
    let nothing_stored = w.stored();
    w.send(post_cookie("LEETCODE_SESSION=only")).await;
    let only_session_value = w.stored();
    w.send(post_cookie("foo=1; LEETCODE_SESSION=a=b==; csrftoken=c=d;  ; bare")).await;
    let equals_kept = w.stored();
    let set = w.send(post_cookie("foo=1; LEETCODE_SESSION=sess-abc; csrftoken=tok-xyz")).await;
    let captured = w.stored();

    let fresh_status = w.send(user_status()).await;
    let test = w.send(with_json(call("POST", "/api/leetcode/test"), test_body("two-sum"))).await;
    let submit = w.send(with_json(call("POST", "/api/leetcode/submit"), submit_body("two-sum"))).await;
    let poll = w.send(empty(call("GET", "/api/leetcode/poll/a%20b"))).await;
    let fresh_calls = w.leetcode.calls();
    w.leetcode.answer_relay(Err("LeetCode request failed (429): slow down".to_string()));
    let relay_failures = [
        w.send(with_json(call("POST", "/api/leetcode/test"), test_body("two-sum"))).await,
        w.send(with_json(call("POST", "/api/leetcode/submit"), submit_body("two-sum"))).await,
        w.send(empty(call("GET", "/api/leetcode/poll/9"))).await,
    ];

    let mut with_unknown_field = w.stored().unwrap();
    with_unknown_field["extra"] = json!([1, 2]);
    w.store(with_unknown_field);
    w.clock.advance(2 * HOUR);
    let reverified_status = w.send(user_status()).await;
    let reverified = w.stored().unwrap();
    let calls_before_outage = w.leetcode.calls().len();
    w.clock.advance(2 * HOUR);
    w.leetcode.answer_status(Err("LeetCode request failed (503): maintenance".to_string()));
    let outage = w.send(user_status()).await;
    w.leetcode.answer_status(Ok(json!({ "isSignedIn": false })));
    let asked_before_rejection = w.capture.asked();
    let rejected_status = w.send(user_status()).await;
    let rejected = w.stored().unwrap();
    let asked_after_rejection = w.capture.asked();

    w.store_raw("{not json");
    let broken_file = w.send(user_status()).await;
    w.store_raw("[1]");
    let not_an_object = w.send(user_status()).await;
    w.store_raw("null");
    let null_file = w.send(user_status()).await;
    let health = w.send(empty(anonymous("GET", "/health"))).await;

    assert_eq!((no_session_status.status, no_session_status.json()), (StatusCode::OK, signed_out.clone()));
    assert_eq!(
        (no_session_test.status, no_session_test.json()),
        (StatusCode::UNAUTHORIZED, json!({ "error": "no_session", "message": "Run a login-capture session first" }))
    );
    assert_eq!(asked_without_capture, 2);
    assert_eq!(idle.json(), json!({ "active": false, "expiresAt": null }));
    assert_eq!((started.status, started.json()), (StatusCode::OK, json!({ "started": true, "expiresAt": NOW + CAPTURE_TTL })));
    assert_eq!(
        (refused_start.status, refused_start.json()),
        (StatusCode::CONFLICT, json!({ "error": "capture_already_active", "expiresAt": NOW + CAPTURE_TTL }))
    );
    assert_eq!(active.json(), json!({ "active": true, "expiresAt": NOW + CAPTURE_TTL }));
    assert_eq!((during_capture.json(), asked_during_capture), (signed_out.clone(), 2));
    assert_eq!((after_expiry.json(), asked_after_expiry), (json!({ "active": false, "expiresAt": null }), 3));
    assert_eq!(restarted.json(), json!({ "started": true, "expiresAt": now + CAPTURE_TTL }));

    assert_eq!(no_field.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(no_field.text.contains("cookie"), "{}", no_field.text);
    assert_eq!(empty_cookie.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        (without_session_value.status, without_session_value.json()),
        (StatusCode::BAD_REQUEST, json!({ "error": "Cookie string has no LEETCODE_SESSION value" }))
    );
    assert_eq!(nothing_stored, None);
    assert_eq!(only_session_value, Some(session("only", "", now)));
    assert_eq!(equals_kept, Some(session("a=b==", "c=d", now)));
    assert_eq!((set.status, set.json()), (StatusCode::OK, json!({ "ok": true })));
    assert_eq!(captured, Some(session("sess-abc", "tok-xyz", now)));

    assert_eq!(fresh_status.json(), json!({ "isSignedIn": true, "username": "ada" }));
    for answer in [&test, &submit, &poll] {
        assert_eq!((answer.status, answer.json()), (StatusCode::OK, json!({ "interpret_id": "i-1" })));
    }
    assert_eq!(fresh_calls, ["user_status", "test two-sum", "submit two-sum", "poll a b"]);
    for answer in &relay_failures {
        assert_eq!((answer.status, answer.json()), (StatusCode::BAD_GATEWAY, json!({ "error": "LeetCode request failed (429): slow down" })));
    }

    assert_eq!(reverified_status.json(), json!({ "isSignedIn": true, "username": "ada" }));
    assert_eq!(&w.leetcode.calls()[7..calls_before_outage], ["user_status", "user_status"]);
    assert_eq!(
        (&reverified["lastVerifiedAt"], &reverified["capturedAt"], &reverified["status"], &reverified["extra"]),
        (&json!(now + 2 * HOUR), &json!(now), &json!("active"), &json!([1, 2]))
    );
    assert_eq!((outage.status, outage.json()), (StatusCode::BAD_GATEWAY, json!({ "error": "LeetCode request failed (503): maintenance" })));
    assert_eq!(rejected_status.json(), signed_out);
    assert_eq!((&rejected["status"], &rejected["extra"]), (&json!("expired"), &json!([1, 2])));
    assert_eq!(asked_after_rejection, asked_before_rejection + 1);

    assert_eq!(broken_file.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(broken_file.json()["error"].as_str().is_some_and(|error| !error.is_empty()), "{}", broken_file.text);
    assert_eq!(
        (not_an_object.status, not_an_object.json()),
        (StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": "session file is not a JSON object" }))
    );
    assert_eq!((null_file.status, null_file.json()), (StatusCode::OK, json!({ "isSignedIn": false, "username": null })));
    assert_eq!(health.status, StatusCode::OK);
}

#[tokio::test]
async fn given_the_real_adapters_when_a_caller_goes_from_no_session_to_relayed_runs_then_the_capture_tool_and_leetcode_see_their_own_protocols() {
    let (leetcode, tool) = (Upstream::start().await, Upstream::start().await);
    let w = World::with_adapters(&leetcode.url, &tool.url);
    let check = json!({ "state": "SUCCESS", "run_success": true, "status_msg": "Accepted", "total_correct": 3, "total_testcases": 3 });
    let answer = check.clone();
    tool.reply(|_| (200, "{}".to_string()));
    leetcode.reply(move |seen| match seen.path.as_str() {
        "/graphql" => (200, json!({ "data": { "userStatus": { "isSignedIn": true, "username": "bob" } } }).to_string()),
        "/problems/two-sum/interpret_solution/" => (200, json!({ "interpret_id": "i-1", "interpret_expected_id": "e-1" }).to_string()),
        "/problems/two-sum/submit/" => (200, json!({ "submission_id": "777" }).to_string()),
        _ => (200, answer.to_string()),
    });

    let without_session = w.send(empty(call("GET", "/api/leetcode/user-status"))).await;
    let triggered = eventually(|| tool.seen().len() == 1).await;
    tool.reply(|_| (500, json!({ "error": "down" }).to_string()));
    let tool_down = w.send(empty(call("GET", "/api/leetcode/user-status"))).await;
    let triggered_again = eventually(|| tool.seen().len() == 2).await;
    w.store(session("sess-123", "csrf-456", NOW));
    let status = w.send(empty(call("GET", "/api/leetcode/user-status"))).await;
    let test = w.send(with_json(call("POST", "/api/leetcode/test"), test_body("two-sum"))).await;
    let submit = w.send(with_json(call("POST", "/api/leetcode/submit"), submit_body("two-sum"))).await;
    let poll = w.send(empty(call("GET", "/api/leetcode/poll/abc-1"))).await;
    w.send(empty(call("GET", "/api/leetcode/poll/a%20b"))).await;
    w.send(empty(call("GET", "/api/leetcode/poll/a%2Fb%3Fc"))).await;
    let dot_segments = [
        w.send(empty(call("GET", "/api/leetcode/poll/.."))).await,
        w.send(with_json(call("POST", "/api/leetcode/test"), test_body(".."))).await,
    ];
    let seen = leetcode.seen();
    leetcode.reply(|_| (200, json!({ "data": { "userStatus": { "isSignedIn": false } } }).to_string()));
    let without_user = w.send(empty(call("GET", "/api/leetcode/user-status"))).await;
    leetcode.reply(|_| (500, "boom".to_string()));
    let failed = [
        w.send(with_json(call("POST", "/api/leetcode/test"), test_body("s"))).await,
        w.send(with_json(call("POST", "/api/leetcode/submit"), submit_body("s"))).await,
        w.send(empty(call("GET", "/api/leetcode/poll/9"))).await,
    ];
    leetcode.reply(|_| (200, "<html>not json</html>".to_string()));
    let garbled = w.send(empty(call("GET", "/api/leetcode/poll/9"))).await;

    let signed_out = json!({ "isSignedIn": false, "username": null });
    assert_eq!((without_session.status, without_session.json()), (StatusCode::OK, signed_out.clone()));
    assert_eq!((tool_down.status, tool_down.json()), (StatusCode::OK, signed_out.clone()));
    assert!(triggered && triggered_again);
    for trigger in tool.seen() {
        assert_eq!((trigger.method.as_str(), trigger.path.as_str(), trigger.header("x-capture-trigger-secret")), ("POST", "/trigger", CAPTURE_SECRET));
    }
    assert_eq!(status.json(), json!({ "isSignedIn": true, "username": "bob" }));
    assert_eq!(test.json(), json!({ "interpret_id": "i-1", "interpret_expected_id": "e-1" }));
    assert_eq!(submit.json(), json!({ "submission_id": "777" }));
    assert_eq!(poll.json(), check);
    let requests: Vec<_> = seen.iter().map(|s| (s.method.as_str(), s.path.as_str())).collect();
    assert_eq!(
        requests,
        [
            ("POST", "/graphql"),
            ("POST", "/problems/two-sum/interpret_solution/"),
            ("POST", "/problems/two-sum/submit/"),
            ("GET", "/submissions/detail/abc-1/check/"),
            ("GET", "/submissions/detail/a%20b/check/"),
            ("GET", "/submissions/detail/a%2Fb%3Fc/check/"),
        ]
    );
    for request in &seen {
        assert_eq!(
            [request.header("content-type"), request.header("x-requested-with"), request.header("x-csrftoken"), request.header("cookie")],
            ["application/json", "XMLHttpRequest", "csrf-456", "LEETCODE_SESSION=sess-123; csrftoken=csrf-456"]
        );
    }
    assert_eq!(seen[0].json(), json!({ "query": "query globalData { userStatus { isSignedIn username } }", "variables": {} }));
    assert_eq!(
        seen[1].json(),
        json!({ "lang": "python3", "question_id": 1, "typed_code": "class Solution: pass", "data_input": "[2,7]\n9", "test_mode": false })
    );
    assert_eq!(
        seen[2].json(),
        json!({ "lang": "python3", "question_id": 1, "typed_code": "class Solution: pass", "test_mode": false, "judge_type": "large" })
    );
    for answer in &dot_segments {
        assert_eq!((answer.status, answer.json()), (StatusCode::BAD_GATEWAY, json!({ "error": "Invalid path segment: .." })));
    }
    assert_eq!(without_user.json(), signed_out);
    for answer in &failed {
        assert_eq!((answer.status, answer.json()), (StatusCode::BAD_GATEWAY, json!({ "error": "LeetCode request failed (500): boom" })));
    }
    assert_eq!(garbled.status, StatusCode::BAD_GATEWAY);
    assert!(garbled.json()["error"].as_str().unwrap().starts_with("LeetCode answered with something that is not JSON"), "{}", garbled.text);
}

const TWO_SUM: &str = "
class Solution:
    def twoSum(self, nums, target):
        seen = {}
        for i, n in enumerate(nums):
            if target - n in seen:
                return [seen[target - n], i]
            seen[n] = i
";

const IDENTITY: &str = "class Solution:\n    def f(self, x):\n        return x\n";

fn run(lang: &str, code: &str, method: &str, cases: Value) -> Request<Body> {
    with_json(call("POST", "/api/leetcode/run"), json!({ "lang": lang, "code": code, "methodName": method, "testCases": cases }))
}

fn format(lang: &str, code: &str) -> Request<Body> {
    with_json(call("POST", "/api/leetcode/format"), json!({ "lang": lang, "code": code }))
}

fn case(inputs: Value, expected: Value) -> Value {
    json!({ "inputs": inputs, "expected": expected })
}

#[tokio::test]
async fn given_no_leetcode_session_when_code_is_run_and_formatted_then_this_host_runs_it_case_by_case_and_reports_each_outcome() {
    if !std::process::Command::new("python3").arg("--version").output().is_ok_and(|out| out.status.success()) {
        eprintln!("skipped: python3 is not installed, and the runner needs it");
        return;
    }
    let w = Arc::new(World::new());
    let sleeper = "import time\nclass Solution:\n    def f(self, x):\n        time.sleep(x)\n        return x\n";
    let slow = tokio::spawn({
        let w = w.clone();
        let request = run("python3", sleeper, "f", json!([case(json!({ "x": 10 }), json!(10)), case(json!({ "x": 0 }), json!(0))]));
        async move { w.send(request).await.json() }
    });
    let halves = [r#"{"lang":"ru"#, r#"by","code":"x"}"#].map(|half| Ok::<_, std::io::Error>(Bytes::from(half)));

    let solved = w
        .send(run(
            "python3",
            TWO_SUM,
            "twoSum",
            json!([case(json!({ "nums": [2, 7, 11, 15], "target": 9 }), json!([0, 1])), case(json!({ "nums": [3, 2, 4], "target": 6 }), json!([1, 2]))]),
        ))
        .await;
    let wrong = w.send(run("python3", TWO_SUM, "twoSum", json!([case(json!({ "nums": [2, 7], "target": 9 }), json!([1, 0]))]))).await.json();
    let printing = w
        .send(run("python3", "class Solution:\n    def f(self, x):\n        print('hi', x)\n        return x\n", "f", json!([case(json!({ "x": 5 }), json!(5))])))
        .await
        .json();
    let raising = w
        .send(run(
            "python3",
            "class Solution:\n    def f(self, x):\n        return 10 // x\n",
            "f",
            json!([case(json!({ "x": 0 }), json!(1)), case(json!({ "x": 5 }), json!(2))]),
        ))
        .await
        .json();
    let compared = w
        .send(run(
            "python3",
            IDENTITY,
            "f",
            json!([
                { "inputs": { "x": [3, 1, 2] }, "expected": [1, 2, 3], "compareMode": "sorted" },
                { "inputs": { "x": [[3, 1], [2]] }, "expected": [[1, 3], [2]], "compareMode": "setOfSorted" },
                case(json!({ "x": [3, 1, 2] }), json!([1, 2, 3])),
            ]),
        ))
        .await
        .json();
    let list_node = w
        .send(run("python3", "class Solution:\n    def f(self, xs):\n        return ListNode.from_list(xs).to_list()\n", "f", json!([case(json!({ "xs": [1, 2, 3] }), json!([1, 2, 3]))])))
        .await
        .json();
    let not_compiling = w.send(run("python3", "def (:", "twoSum", json!([]))).await;
    let no_solution = w.send(run("python3", "x = 1", "twoSum", json!([case(json!({}), json!(1))]))).await.json();
    let ruby = w.send(run("ruby", "x", "f", json!([]))).await;
    let javascript = w
        .send(run("javascript", "class Solution { add(a, b) { return a + b; } }", "add", json!([case(json!({ "a": 1, "b": 2 }), json!(3))])))
        .await
        .json();
    let formatted = w.send(format("python3", "x=1\ny  =  2")).await;
    let ruby_format = w.send(format("ruby", "x")).await.json();
    let chunked = w.send(raw(call("POST", "/api/leetcode/format"), "application/json", Body::from_stream(tokio_stream::iter(halves)))).await;
    let timed_out = slow.await.unwrap();

    let (status, solved) = (solved.status, solved.json());
    let first = &solved["results"][0];
    assert_eq!((status, &solved["ok"]), (StatusCode::OK, &json!(true)));
    assert_eq!(
        [&first["pass"], &first["input"], &first["actual"], &first["expected"], &first["stdout"], &first["stderr"], &first["error"]],
        [&json!(true), &json!({ "nums": [2, 7, 11, 15], "target": 9 }), &json!([0, 1]), &json!([0, 1]), &json!(""), &json!(""), &Value::Null]
    );
    assert!(first["elapsed_ms"].is_number());
    assert_eq!(solved["results"][1]["pass"], json!(true));
    let wrong_case = &wrong["results"][0];
    assert_eq!(
        [&wrong["ok"], &wrong_case["pass"], &wrong_case["actual"], &wrong_case["expected"], &wrong_case["error"]],
        [&json!(true), &json!(false), &json!([0, 1]), &json!([1, 0]), &Value::Null]
    );
    let printed = &printing["results"][0];
    assert_eq!([&printed["pass"], &printed["stdout"], &printed["stderr"]], [&json!(true), &json!("hi 5\n"), &json!("")]);
    assert_eq!([&raising["ok"], &raising["results"][0]["pass"], &raising["results"][1]["pass"]], [&json!(true), &json!(false), &json!(true)]);
    assert!(raising["results"][0]["error"].as_str().unwrap().starts_with("ZeroDivisionError: "), "{raising}");
    let passes: Vec<&Value> = compared["results"].as_array().unwrap().iter().map(|result| &result["pass"]).collect();
    assert_eq!(passes, [&json!(true), &json!(true), &json!(false)]);
    assert_eq!(list_node["results"][0]["pass"], json!(true));
    let (status, not_compiling) = (not_compiling.status, not_compiling.json());
    assert_eq!((status, &not_compiling["ok"], &not_compiling["results"]), (StatusCode::OK, &json!(false), &json!([])));
    assert!(not_compiling["error"].as_str().unwrap().starts_with("CompileError: "), "{not_compiling}");
    assert_eq!(no_solution, json!({ "ok": false, "results": [], "error": "No Solution class defined" }));
    assert_eq!((ruby.status, ruby.json()), (StatusCode::OK, json!({ "ok": false, "results": [], "error": "Unsupported language: ruby" })));
    let added = &javascript["results"][0];
    assert_eq!([&javascript["ok"], &added["pass"], &added["actual"], &added["error"]], [&json!(true), &json!(true), &json!(3), &Value::Null]);
    let (status, formatted) = (formatted.status, formatted.json());
    assert_eq!((status, &formatted["ok"], formatted["formatted"].as_str().unwrap().trim_end()), (StatusCode::OK, &json!(true), "x = 1\ny = 2"));
    assert_eq!(ruby_format, json!({ "ok": false, "formatted": "", "error": "Unsupported language: ruby" }));
    assert_eq!((chunked.status, chunked.json()), (StatusCode::OK, ruby_format));
    assert_eq!([&timed_out["ok"], &timed_out["results"][0]["pass"], &timed_out["results"][1]["pass"]], [&json!(true), &json!(false), &json!(true)]);
    assert_eq!(timed_out["results"][0]["error"], json!("TimeoutError: Code execution timed out (5s limit)"));
    assert!(w.leetcode.calls().is_empty());
    assert_eq!(w.capture.asked(), 0);
}
