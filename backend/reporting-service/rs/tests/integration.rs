//! The built reporting-service as a black box: its binary on a free port, its repository a temp dir, its Docker socket a path nothing
//! listens on, its database an ORDS endpoint on loopback that records what it is sent. Nothing real is reachable; what it answers is its contract.
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::json;
use service_kit::blackbox::Running;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// An ORDS SQL endpoint that writes down every statement it is sent and the credentials it was sent with, and answers each as an
/// empty database would.
struct Ords {
    base: String,
    statements: Arc<Mutex<Vec<String>>>,
    auth: Arc<Mutex<Vec<String>>>,
}

async fn fake_ords() -> Ords {
    let (statements, auth) = (Arc::new(Mutex::new(Vec::new())), Arc::new(Mutex::new(Vec::new())));
    let router = Router::new().route(
        "/ords/health/_/sql",
        post({
            let (statements, auth) = (statements.clone(), auth.clone());
            move |headers: HeaderMap, body: String| async move {
                auth.lock().unwrap().push(headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("").to_string());
                let rows = if body.contains("COUNT(*)") {
                    json!([{ "n": 0 }])
                } else if body.contains("SUM(BYTES)") {
                    json!([{ "bytes": 0 }])
                } else {
                    json!([])
                };
                statements.lock().unwrap().push(body);
                Json(json!({ "items": [{ "result": 0, "resultSet": { "items": rows }, "response": [] }] }))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    Ords { base: format!("http://127.0.0.1:{port}/ords/health/health_reports/"), statements, auth }
}

/// The service and the temp dir that holds its repository (each test has its own: they run side by side), over an ORDS endpoint.
async fn start_with(host_interval_ms: u64) -> (Running, tempfile::TempDir, Ords) {
    // the other sampler intervals are long so that the first sample does not race the test, and the socket does not exist
    let ords = fake_ords().await;
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();
    let repo = dir.join("repo");
    std::fs::create_dir_all(repo.join("docs/plans/alpha")).unwrap();
    std::fs::write(repo.join("docs/plans/alpha/manifest.json"), json!({ "slug": "alpha", "units": [{ "id": "u1", "title": "First", "status": "done", "seconds": 12.0 }] }).to_string()).unwrap();
    let env = [
        ("HEALTH_REPORTS_ORDS_BASE_URL", ords.base.clone()),
        ("HEALTH_REPORTS_ORDS_PASSWORD", "not-a-secret".to_string()),
        ("DOCKER_SOCKET", dir.join("no-docker.sock").display().to_string()),
        ("REPORTING_REPO_ROOT", repo.display().to_string()),
        ("REPORTING_CONTAINERS_INTERVAL_MS", "3600000".to_string()),
        ("REPORTING_HOST_INTERVAL_MS", host_interval_ms.to_string()),
        ("REPORTING_STORAGE_INTERVAL_MS", "3600000".to_string()),
        ("REPORTING_PRUNE_INTERVAL_MS", "3600000".to_string()),
    ];
    (Running::start(env!("CARGO_BIN_EXE_reporting-service-rs"), "PORT", &env, "/health").await, tmp, ords)
}

async fn start() -> (Running, tempfile::TempDir) {
    let (service, files, _ords) = start_with(3_600_000).await;
    (service, files)
}

#[tokio::test]
async fn given_the_database_when_the_service_runs_then_it_keeps_talking_to_it_as_health_rpt() {
    // Given an ORDS endpoint, and a host sweep every 100 ms
    let (service, _files, ords) = start_with(100).await;
    // When the service has run for a moment
    let mut seen = 0;
    for _ in 0..50 {
        seen = ords.statements.lock().unwrap().len();
        if seen >= 4 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // Then it kept sending requests (setting the table up and then writing each sweep), every one authenticated as health_rpt
    assert!(seen >= 4, "only {seen} requests reached the database\n{}", service.logs());
    let expected = format!("Basic {}", base64_of("health_rpt:not-a-secret"));
    assert!(ords.auth.lock().unwrap().iter().all(|a| *a == expected), "{:?}", ords.auth.lock().unwrap());
}

fn base64_of(text: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in text.as_bytes().chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() { ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

#[tokio::test]
async fn given_the_service_started_clean_when_health_is_asked_then_it_is_ok_with_no_rows_and_its_uptime() {
    // Given a service with an empty database
    let (service, _files) = start().await;
    // When health is asked
    let (status, health) = service.get("/health").await;
    // Then it is ok, has no rows yet and reports an uptime
    assert_eq!((status, health["status"].as_str(), health["rows"].as_u64()), (200, Some("ok"), Some(0)), "{health}\n{}", service.logs());
    assert!(health["uptime"].is_u64(), "{health}");
}

#[tokio::test]
async fn given_the_service_when_a_metrics_window_is_asked_then_it_answers_points_and_refuses_an_unknown_window_naming_the_accepted_ones() {
    let (service, _files) = start().await;
    // When the default series, then a window that does not exist
    let (status, series) = service.get("/api/metrics/series").await;
    let (refused_status, refused) = service.get("/api/metrics/series?window=not-a-window").await;
    // Then the default is an empty series of its window, and the unknown window is a 400 with the accepted list
    assert!(status == 200 && series["window"].is_string() && series["points"].as_array().is_some_and(Vec::is_empty), "{series}");
    assert!(refused_status == 400 && refused["error"] == "invalid_window" && refused["accepted"].as_array().is_some_and(|a| !a.is_empty()), "{refused}");
}

#[tokio::test]
async fn given_a_repository_with_a_plan_when_the_plans_are_asked_then_the_plan_is_listed_and_its_detail_is_served() {
    let (service, _files) = start().await;
    // When the plan list and the detail of the plan in the repository are asked, and the detail of one that is not there
    let (status, plans) = service.get("/api/plans").await;
    let (detail_status, detail) = service.get("/api/plans/alpha").await;
    let (missing_status, missing) = service.get("/api/plans/nope").await;
    // Then the plan is listed and served, and the missing one is a 404
    assert!(status == 200 && plans["plans"].as_array().is_some_and(|p| p.iter().any(|p| p["slug"] == "alpha")), "{plans}");
    assert!(detail_status == 200 && detail["slug"] == "alpha", "{detail}");
    assert_eq!((missing_status, missing["error"].as_str()), (404, Some("not_found")), "{missing}");
}

#[tokio::test]
async fn given_the_service_when_an_unknown_path_is_read_or_a_write_is_sent_then_it_is_not_found() {
    let (service, _files) = start().await;
    // When an unknown path is read and a write is sent to a served one
    let (missing, body) = service.get("/api/not-served").await;
    let write = service.client.post(format!("{}/api/headroom", service.base)).send().await.unwrap().status().as_u16();
    // Then both are 404: the service only reads
    assert_eq!((missing, body["error"].as_str(), write), (404, Some("not_found"), 404), "{body}");
}
