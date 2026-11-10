//! Against the deployed service (REPORTING_SERVICE_BASE_URL, default http://reporting-service:8791): what it logs reaches BetterStack.

#[tokio::test]
#[ignore = "against the deployed service: run by test:verify:deployed"]
async fn given_a_request_in_a_fresh_trace_when_the_deployed_service_answers_then_its_log_line_reaches_betterstack() {
    let base = std::env::var("REPORTING_SERVICE_BASE_URL").unwrap_or_else(|_| "http://reporting-service:8791".to_string());
    let line = service_kit::e2e::logged_request(&format!("{base}/health")).await;
    service_kit::e2e::assert_inferred(&line, "reporting-service");
}

fn base() -> String {
    std::env::var("REPORTING_SERVICE_BASE_URL").unwrap_or_else(|_| "http://reporting-service:8791".to_string())
}

async fn get(path: &str) -> (u16, serde_json::Value) {
    let response = reqwest::get(format!("{}{path}", base())).await.unwrap_or_else(|e| panic!("GET {path}: {e}"));
    let status = response.status().as_u16();
    (status, response.json().await.unwrap_or(serde_json::Value::Null))
}

#[tokio::test]
#[ignore = "against the deployed service: run by test:verify:deployed"]
async fn given_the_deployed_service_when_health_is_asked_then_it_is_ok_with_its_rows_and_the_pushed_commit() {
    // When health is asked
    let (status, health) = get("/health").await;
    // Then the service is ok, has stored rows and runs the commit the pipeline expects
    assert_eq!((status, health["status"].as_str()), (200, Some("ok")), "{health}");
    assert!(health["rows"].is_u64(), "{health}");
    service_kit::e2e::assert_serves_commit(&health, "reporting-service");
}

#[tokio::test]
#[ignore = "against the deployed service: run by test:verify:deployed"]
async fn given_the_deployed_service_when_a_metrics_window_is_asked_then_it_answers_points_and_refuses_an_unknown_window() {
    // When a series is asked for the default window and then for a window that does not exist
    let (status, series) = get("/api/metrics/series").await;
    let (refused_status, refused) = get("/api/metrics/series?window=not-a-window").await;
    // Then the default window answers its points, and the unknown one is a 400 naming the windows it accepts
    assert!(status == 200 && series["window"].is_string() && series["points"].is_array(), "{series}");
    assert!(refused_status == 400 && refused["error"] == "invalid_window" && refused["accepted"].as_array().is_some_and(|a| !a.is_empty()), "{refused}");
}

#[tokio::test]
#[ignore = "against the deployed service: run by test:verify:deployed"]
async fn given_the_deployed_service_when_the_report_pages_are_asked_then_current_headroom_and_plans_answer_their_shape() {
    // When the current samples, the headroom report and the plan list are asked
    let ((current_status, current), (headroom_status, headroom), (plans_status, plans)) = (get("/api/metrics/current").await, get("/api/headroom").await, get("/api/plans").await);
    // Then each answers 200 with its documented shape
    assert!(current_status == 200 && current["samples"].is_array(), "{current}");
    assert!(headroom_status == 200 && headroom.is_object(), "{headroom}");
    assert!(plans_status == 200 && plans["plans"].is_array(), "{plans}");
}

#[tokio::test]
#[ignore = "against the deployed service: run by test:verify:deployed"]
async fn given_the_deployed_service_when_an_unknown_path_or_a_write_is_sent_then_it_is_not_found() {
    // When a path that is not served is read and a write is sent to a served one
    let (missing, body) = get("/api/not-served").await;
    let write = reqwest::Client::new().post(format!("{}/api/headroom", base())).send().await.unwrap().status().as_u16();
    // Then both are 404: the service only reads
    assert_eq!((missing, body["error"].as_str(), write), (404, Some("not_found"), 404), "{body}");
}
