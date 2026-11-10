//! Against the deployed service (LEETCODE_BACKEND_BASE_URL, default http://leetcode-backend-prod:8789): what it logs reaches BetterStack.

#[tokio::test]
#[ignore = "against the deployed service: run by test:verify:deployed"]
async fn given_a_request_in_a_fresh_trace_when_the_deployed_service_answers_then_its_log_line_reaches_betterstack() {
    let base = std::env::var("LEETCODE_BACKEND_BASE_URL").unwrap_or_else(|_| "http://leetcode-backend-prod:8789".to_string());
    let line = service_kit::e2e::logged_request(&format!("{base}/health")).await;
    service_kit::e2e::assert_inferred(&line, "leetcode-backend");
}

fn base() -> String {
    std::env::var("LEETCODE_BACKEND_BASE_URL").unwrap_or_else(|_| "http://leetcode-backend-prod:8789".to_string())
}

#[tokio::test]
#[ignore = "against the deployed service: run by test:verify:deployed"]
async fn given_the_deployed_service_when_health_is_asked_then_it_is_ok_and_runs_the_pushed_commit() {
    // When health is asked
    let response = reqwest::get(format!("{}/health", base())).await.unwrap();
    let status = response.status().as_u16();
    let health: serde_json::Value = response.json().await.unwrap();
    // Then the service is ok and serves the commit the pipeline expects
    assert_eq!((status, health["status"].as_str()), (200, Some("ok")), "{health}");
    service_kit::e2e::assert_serves_commit(&health, "leetcode-backend");
}

#[tokio::test]
#[ignore = "against the deployed service: run by test:verify:deployed"]
async fn given_the_deployed_service_when_the_leetcode_api_is_called_without_the_bearer_then_every_route_refuses_it() {
    let client = reqwest::Client::new();
    // When each leetcode route is called with no bearer and with a wrong one
    for (method, path) in [("GET", "/user-status"), ("GET", "/login-session"), ("POST", "/test"), ("POST", "/submit"), ("POST", "/run"), ("POST", "/format"), ("POST", "/session")] {
        let url = format!("{}/api/leetcode{path}", base());
        let request = |wrong: bool| {
            let builder = if method == "GET" { client.get(&url) } else { client.post(&url).json(&serde_json::json!({})) };
            if wrong { builder.header("authorization", "Bearer not-the-bearer") } else { builder }
        };
        for wrong in [false, true] {
            let response = request(wrong).send().await.unwrap_or_else(|e| panic!("{method} {path}: {e}"));
            let status = response.status().as_u16();
            let body: serde_json::Value = response.json().await.unwrap_or(serde_json::Value::Null);
            // Then it is 401 unauthorized, before any body is read or any work is done
            assert_eq!((status, body["error"].as_str()), (401, Some("unauthorized")), "{method} {path} wrong={wrong}: {body}");
        }
    }
}

#[tokio::test]
#[ignore = "against the deployed service: run by test:verify:deployed"]
async fn given_the_deployed_service_when_an_unknown_path_is_asked_then_it_is_not_found() {
    // When a path that is not served is asked
    let status = reqwest::get(format!("{}/api/not-served", base())).await.unwrap().status().as_u16();
    // Then it is a 404
    assert_eq!(status, 404);
}
