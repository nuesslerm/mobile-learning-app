//! The built leetcode-backend as a black box: its binary on a free port, LeetCode a fake server this test runs, the capture tool
//! another, the session file in a temp dir. Nothing real is reachable; what it answers and what it sends upstream is its contract.
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};
use service_kit::blackbox::Running;
use std::sync::{Arc, Mutex};

const BEARER: &str = "integration-bearer";
const ADMIN: &str = "integration-admin";
const COMMIT: &str = "0123456789abcdef";

/// What the fake LeetCode saw: one `(path, cookie, csrf)` per request.
type Seen = Arc<Mutex<Vec<(String, String, String)>>>;

async fn fake_leetcode() -> (String, Seen) {
    let seen: Seen = Default::default();
    async fn graphql(State(seen): State<Seen>, headers: HeaderMap) -> Json<Value> {
        let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        seen.lock().unwrap().push(("/graphql".to_string(), header("cookie"), header("x-csrftoken")));
        Json(json!({ "data": { "userStatus": { "isSignedIn": true, "username": "ada" } } }))
    }
    let app = Router::new().route("/graphql", post(graphql)).with_state(seen.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, seen)
}

async fn start(leetcode: &str) -> (Running, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let env = [
        ("GIT_COMMIT", COMMIT.to_string()),
        ("LEETCODE_BACKEND_SESSION_FILE", tmp.path().join("session.json").display().to_string()),
        ("LEETCODE_ORIGIN", leetcode.to_string()),
        // the capture tool is never reached by these tests: nothing listens there
        ("LEETCODE_CAPTURE_TOOL_BASE_URL", "http://127.0.0.1:9".to_string()),
        ("LEETCODE_CAPTURE_TRIGGER_SECRET", "integration-trigger".to_string()),
        ("LEETCODE_BACKEND_ADMIN_SECRET", ADMIN.to_string()),
        ("VM_GATEWAY_AUTH_SEED", BEARER.to_string()),
    ];
    (Running::start(env!("CARGO_BIN_EXE_leetcode-backend-rs"), "PORT", &env, "/health").await, tmp)
}

async fn call(service: &Running, method: &str, path: &str, bearer: Option<&str>, admin: Option<&str>, body: Option<Value>) -> (u16, Value) {
    let url = format!("{}/api/leetcode{path}", service.base);
    let mut request = if method == "GET" { service.client.get(url) } else { service.client.post(url).json(&body.unwrap_or(json!({}))) };
    if let Some(bearer) = bearer {
        request = request.header("authorization", format!("Bearer {bearer}"));
    }
    if let Some(admin) = admin {
        request = request.header("x-admin-secret", admin);
    }
    let response = request.send().await.unwrap_or_else(|e| panic!("{method} {path}: {e}\n{}", service.logs()));
    (response.status().as_u16(), response.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn given_the_service_when_health_is_asked_then_it_is_ok_and_names_the_commit_it_was_given() {
    let (leetcode, _) = fake_leetcode().await;
    let (service, _files) = start(&leetcode).await;
    // When health is asked without any credential
    let (status, health) = service.get("/health").await;
    // Then it is ok and names its commit
    assert_eq!((status, health["status"].as_str(), health["commit"].as_str()), (200, Some("ok"), Some(COMMIT)), "{health}");
}

#[tokio::test]
async fn given_the_service_when_the_leetcode_routes_are_called_without_the_bearer_then_each_refuses_with_401() {
    let (leetcode, seen) = fake_leetcode().await;
    let (service, _files) = start(&leetcode).await;
    // When each route is called with no bearer and with a wrong one
    for (method, path) in [("GET", "/user-status"), ("GET", "/login-session"), ("POST", "/test"), ("POST", "/submit"), ("POST", "/run"), ("POST", "/format"), ("POST", "/session")] {
        for bearer in [None, Some("not-the-bearer")] {
            let (status, body) = call(&service, method, path, bearer, Some(ADMIN), None).await;
            // Then it is 401 unauthorized, even with the admin secret
            assert_eq!((status, body["error"].as_str()), (401, Some("unauthorized")), "{method} {path} bearer={bearer:?}: {body}");
        }
    }
    // and LeetCode was never asked
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn given_no_stored_session_when_the_user_status_is_asked_then_it_says_not_signed_in_and_leetcode_is_not_asked() {
    let (leetcode, seen) = fake_leetcode().await;
    let (service, _files) = start(&leetcode).await;
    // When the user status is asked with the bearer and no session was ever stored
    let (status, body) = call(&service, "GET", "/user-status", Some(BEARER), None, None).await;
    // Then it answers signed out with no username, and LeetCode saw nothing
    assert_eq!((status, body["isSignedIn"].as_bool(), body["username"].is_null()), (200, Some(false), true), "{body}");
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn given_a_session_stored_with_the_admin_secret_when_the_user_status_is_asked_then_leetcode_is_asked_with_that_cookie() {
    let (leetcode, seen) = fake_leetcode().await;
    let (service, _files) = start(&leetcode).await;
    // When a session is stored without the admin secret, with a cookie that has no LEETCODE_SESSION, and then properly
    let (no_admin, _) = call(&service, "POST", "/session", Some(BEARER), None, Some(json!({ "cookie": "LEETCODE_SESSION=abc; csrftoken=xyz" }))).await;
    let (bad_cookie, bad_body) = call(&service, "POST", "/session", Some(BEARER), Some(ADMIN), Some(json!({ "cookie": "other=1" }))).await;
    let (stored, stored_body) = call(&service, "POST", "/session", Some(BEARER), Some(ADMIN), Some(json!({ "cookie": "LEETCODE_SESSION=abc; csrftoken=xyz" }))).await;
    // Then the first two are refused, the third is stored
    assert_eq!((no_admin, bad_cookie, stored, stored_body["ok"].as_bool()), (401, 400, 200, Some(true)), "{bad_body}\n{}", service.logs());
    // When the user status is asked
    let (status, user) = call(&service, "GET", "/user-status", Some(BEARER), None, None).await;
    // Then it answers what LeetCode said, and LeetCode was asked under the stored cookie and csrf token
    assert_eq!((status, user["isSignedIn"].as_bool(), user["username"].as_str()), (200, Some(true), Some("ada")), "{user}\n{}", service.logs());
    assert_eq!(seen.lock().unwrap().as_slice(), [("/graphql".to_string(), "LEETCODE_SESSION=abc; csrftoken=xyz".to_string(), "xyz".to_string())]);
}

#[tokio::test]
async fn given_the_service_when_a_login_capture_is_started_twice_then_the_second_is_a_409_and_the_session_says_it_is_active() {
    let (leetcode, _) = fake_leetcode().await;
    let (service, _files) = start(&leetcode).await;
    // When no capture is active, one is started with the admin secret (and once without), then started again
    let (idle, idle_body) = call(&service, "GET", "/login-session", Some(BEARER), None, None).await;
    let (refused, _) = call(&service, "POST", "/login-session", Some(BEARER), None, None).await;
    let (first, first_body) = call(&service, "POST", "/login-session", Some(BEARER), Some(ADMIN), None).await;
    let (second, second_body) = call(&service, "POST", "/login-session", Some(BEARER), Some(ADMIN), None).await;
    let (active, active_body) = call(&service, "GET", "/login-session", Some(BEARER), None, None).await;
    // Then it was idle, the unauthenticated start is refused, the first start is accepted, the second conflicts, and it is now active
    assert_eq!((idle, idle_body["active"].as_bool()), (200, Some(false)), "{idle_body}");
    assert_eq!((refused, first, first_body["started"].as_bool()), (401, 200, Some(true)), "{first_body}");
    assert_eq!((second, second_body["error"].as_str()), (409, Some("capture_already_active")), "{second_body}");
    assert_eq!((active, active_body["active"].as_bool()), (200, Some(true)), "{active_body}");
}
