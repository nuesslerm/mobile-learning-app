//! Against the deployed service (LEETCODE_CAPTURE_BASE_URL, default http://leetcode-capture:8792): what it logs reaches BetterStack.

#[tokio::test]
#[ignore = "against the deployed service: run by test:verify:deployed"]
async fn given_a_request_in_a_fresh_trace_when_the_deployed_service_answers_then_its_log_line_reaches_betterstack() {
    let base = std::env::var("LEETCODE_CAPTURE_BASE_URL").unwrap_or_else(|_| "http://leetcode-capture:8792".to_string());
    // The listener serves no `/`; its 404 still passes the trace layer and is logged.
    let line = service_kit::e2e::logged_request(&format!("{base}/")).await;
    service_kit::e2e::assert_inferred(&line, "leetcode-auth-cookie-capture");
}

fn base() -> String {
    std::env::var("LEETCODE_CAPTURE_BASE_URL").unwrap_or_else(|_| "http://leetcode-capture:8792".to_string())
}

#[tokio::test]
#[ignore = "against the deployed service: run by test:verify:deployed"]
async fn given_the_deployed_listener_when_a_capture_is_triggered_without_its_secret_then_it_is_refused_and_nothing_starts() {
    let client = reqwest::Client::new();
    // When a trigger arrives with no secret and with a wrong one
    for secret in [None, Some("not-the-secret")] {
        let mut request = client.post(format!("{}/trigger", base()));
        if let Some(secret) = secret {
            request = request.header("x-capture-trigger-secret", secret);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let body: serde_json::Value = response.json().await.unwrap_or(serde_json::Value::Null);
        // Then it is 401 unauthorized, not accepted
        assert_eq!((status, body["error"].as_str(), body.get("accepted")), (401, Some("unauthorized"), None), "secret={secret:?}: {body}");
    }
}

#[tokio::test]
#[ignore = "against the deployed service: run by test:verify:deployed"]
async fn given_the_deployed_listener_when_anything_else_is_asked_then_it_serves_only_the_trigger() {
    // When the trigger is read, and a path that is not served is asked
    let read = reqwest::get(format!("{}/trigger", base())).await.unwrap().status().as_u16();
    let other = reqwest::get(format!("{}/anything", base())).await.unwrap().status().as_u16();
    // Then both are 404: the listener serves a POST to /trigger and nothing else
    assert_eq!((read, other), (404, 404));
}
