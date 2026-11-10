//! Flow tests of the telemetry stack: the real subscriber and the real exporters, with the ingest replaced by a server on a local
//! port that records what it is sent. Given/When/Then.

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::routing::{get, post};
use axum::Router;
use serde_json::Value;
use service_kit::telemetry::{self, Destination, Source};
use service_kit::trace;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;

#[derive(Clone, Default)]
struct Ingest {
    received: Arc<Mutex<Vec<(String, String, Value)>>>,
}

async fn record(State(ingest): State<Ingest>, uri: axum::http::Uri, headers: HeaderMap, body: String) -> StatusCode {
    let auth = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
    let json = serde_json::from_str(&body).unwrap_or(Value::Null);
    ingest.received.lock().unwrap().push((uri.path().to_string(), auth, json));
    StatusCode::OK
}

async fn start_ingest() -> (Ingest, String) {
    let ingest = Ingest::default();
    let app = Router::new().route("/v1/logs", post(record)).route("/v1/traces", post(record)).with_state(ingest.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (ingest, base)
}

fn page() -> Router {
    Router::new().route(
        "/health",
        get(|| async {
            tracing::info!(cookie = "abc", note = "carries super-secret-value inside", "test.event");
            "ok"
        }),
    )
}

async fn serve(app: Router, dispatch: tracing::Dispatch, traceparent: Option<&str>) -> StatusCode {
    serve_at(app, dispatch, traceparent, "/health").await
}

async fn serve_at(app: Router, dispatch: tracing::Dispatch, traceparent: Option<&str>, uri: &str) -> StatusCode {
    let mut request = Request::builder().uri(uri);
    if let Some(value) = traceparent {
        request = request.header("traceparent", value);
    }
    app.layer(axum::middleware::from_fn(trace::log_request))
        .layer(trace::layer())
        .oneshot(request.body(Body::empty()).unwrap())
        .with_subscriber(dispatch)
        .await
        .unwrap()
        .status()
}

fn exported(ingest: &Ingest, path: &str) -> Vec<(String, Value)> {
    ingest.received.lock().unwrap().iter().filter(|(p, _, _)| p == path).map(|(_, auth, body)| (auth.clone(), body.clone())).collect()
}

fn source() -> Source {
    Source { service: "svc-test".into(), version: "1.2.3", commit: Some("abc1234".into()), host: Some("vm-main".into()) }
}

fn secrets() -> Vec<String> {
    vec!["super-secret-value".to_string()]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_an_ingest_when_a_request_with_a_traceparent_is_served_then_its_span_and_the_log_reach_it_with_the_bearer_token() {
    let (ingest, base) = start_ingest().await;
    let (dispatch, telemetry) = telemetry::build(source(), secrets(), Some(Destination::new(base, "tok-123")));

    let status = serve(page(), dispatch, Some("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")).await;
    tokio::task::spawn_blocking(move || telemetry.shutdown()).await.unwrap();

    let spans = exported(&ingest, "/v1/traces");
    let logs = exported(&ingest, "/v1/logs");
    let span_text = spans.iter().map(|(_, b)| b.to_string()).collect::<String>();
    let log_text = logs.iter().map(|(_, b)| b.to_string()).collect::<String>();
    assert_eq!(status, StatusCode::OK);
    assert!(spans.iter().all(|(auth, _)| auth == "Bearer tok-123"), "{spans:?}");
    assert!(span_text.contains("4bf92f3577b34da6a3ce929d0e0e4736"), "{span_text}");
    assert!(span_text.contains("00f067aa0ba902b7"), "the parent span id: {span_text}");
    assert!(span_text.contains("GET /health"), "{span_text}");
    assert!(span_text.contains("svc-test"), "the service name: {span_text}");
    assert!(log_text.contains("test.event"), "{log_text}");
    assert!(log_text.contains("4bf92f3577b34da6a3ce929d0e0e4736"), "the log carries its request's trace: {log_text}");
    // and the request itself is one http.request line in its trace, with method, path and status
    let records: Vec<Value> = logs
        .iter()
        .flat_map(|(_, b)| b["resourceLogs"].as_array().cloned().unwrap_or_default())
        .flat_map(|rl| rl["scopeLogs"].as_array().cloned().unwrap_or_default())
        .flat_map(|sl| sl["logRecords"].as_array().cloned().unwrap_or_default())
        .filter(|r| r["body"]["stringValue"] == "http.request")
        .collect();
    assert_eq!(records.len(), 1, "{log_text}");
    assert_eq!(records[0]["traceId"], "4bf92f3577b34da6a3ce929d0e0e4736");
    let attr = |key: &str| records[0]["attributes"].as_array().unwrap().iter().find(|a| a["key"] == key).map(|a| a["value"].clone()).unwrap_or_default();
    assert_eq!((attr("method"), attr("path")), (serde_json::json!({ "stringValue": "GET" }), serde_json::json!({ "stringValue": "/health" })));
    assert!(attr("status").to_string().contains("200"), "{}", attr("status"));
    // and what no call passes is inferred: the call site from the code, the version, commit and host from the source
    for inferred in ["code.filepath", "flow_telemetry.rs", "code.lineno", "code.namespace", "service.version", "1.2.3", "vcs.ref.head.revision", "abc1234", "host.name", "vm-main"] {
        assert!(log_text.contains(inferred), "{inferred}: {log_text}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_credential_when_it_is_logged_then_neither_the_field_value_nor_the_secret_text_reaches_the_ingest() {
    let (ingest, base) = start_ingest().await;
    let (dispatch, telemetry) = telemetry::build(source(), secrets(), Some(Destination::new(base, "tok-123")));

    serve(page(), dispatch, None).await;
    tokio::task::spawn_blocking(move || telemetry.shutdown()).await.unwrap();

    let log_text = exported(&ingest, "/v1/logs").iter().map(|(_, b)| b.to_string()).collect::<String>();
    assert!(log_text.contains("test.event"), "{log_text}");
    assert!(log_text.contains("[REDACTED]"), "{log_text}");
    assert!(!log_text.contains("super-secret-value"), "{log_text}");
    assert!(!log_text.contains("\"abc\""), "{log_text}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_parent_that_is_not_sampled_when_a_request_is_served_then_no_span_is_exported_for_it() {
    let (ingest, base) = start_ingest().await;
    let (dispatch, telemetry) = telemetry::build(source(), secrets(), Some(Destination::new(base, "tok-123")));

    serve(page(), dispatch, Some("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00")).await;
    tokio::task::spawn_blocking(move || telemetry.shutdown()).await.unwrap();

    let spans = exported(&ingest, "/v1/traces").iter().map(|(_, b)| b.to_string()).collect::<String>();
    assert!(!spans.contains("4bf92f3577b34da6a3ce929d0e0e4736"), "{spans}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_no_ingest_when_requests_are_served_then_nothing_is_exported_and_the_service_still_answers() {
    let (dispatch, telemetry) = telemetry::build(source(), secrets(), None);

    let status = serve(page(), dispatch, None).await;

    assert_eq!(status, StatusCode::OK);
    assert!(!telemetry.is_exporting());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_route_that_carries_a_token_in_its_path_when_it_is_called_then_neither_the_log_nor_the_span_holds_the_token() {
    let (ingest, base) = start_ingest().await;
    let (dispatch, telemetry) = telemetry::build(source(), secrets(), Some(Destination::new(base, "tok-123")));
    let hook = Router::new().route("/hooks/{id}/{token}", post(|| async { StatusCode::ACCEPTED }));
    let token = "wh_0123456789abcdef";
    let uri = format!("/hooks/sched-1/{token}");

    let request_uri = uri.clone();
    let app = hook.layer(axum::middleware::from_fn(trace::log_request)).layer(trace::layer());
    let status = app
        .oneshot(Request::builder().method("POST").uri(request_uri).body(Body::empty()).unwrap())
        .with_subscriber(dispatch)
        .await
        .unwrap()
        .status();
    tokio::task::spawn_blocking(move || telemetry.shutdown()).await.unwrap();

    let text = ["/v1/traces", "/v1/logs"].iter().flat_map(|p| exported(&ingest, p)).map(|(_, b)| b.to_string()).collect::<String>();
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(text.contains("/hooks/{id}/{token}"), "the route is still logged: {text}");
    assert!(!text.contains(token), "{text}");
}
