//! The doubles of this crate's port, for the flow tests of every crate that drives a browser or reads a run: a fake multi-browser
//! daemon, a launched-browser session, and the plain HTTP calls and run-report reads those tests share.
mod browser_session;
mod mock_daemon;

pub use browser_session::{browsers_dir, call_at, chromium_gate, ChromiumGate, daemon_url, require_browser, BrowserSession};
pub use mock_daemon::MockDaemon;

fn http_agent(timeout_secs: u64) -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .proxy(None)
        .timeout_global(Some(std::time::Duration::from_secs(timeout_secs)))
        .build()
        .into()
}

/// One plain HTTP GET of `path` on `host_port`: the status and the body.
pub fn http_get(host_port: &str, path: &str) -> (u16, String) {
    match http_agent(10).get(format!("http://{host_port}{path}")).call() {
        Ok(mut response) => (response.status().as_u16(), response.body_mut().read_to_string().unwrap_or_default()),
        Err(_) => (0, String::new()),
    }
}

/// One plain HTTP POST of a JSON body; the response body.
pub(crate) fn http_post_json(host_port: &str, path: &str, body: &str) -> String {
    match http_agent(60).post(format!("http://{host_port}{path}")).content_type("application/json").send(body) {
        Ok(mut response) => response.body_mut().read_to_string().unwrap_or_default(),
        Err(_) => String::new(),
    }
}

/// From a test run's report.json: the outcome of the first wait after each `draw_strokes_on`, and the ms between one draw's
/// end and the next one's start.
pub fn after_each_draw(report: &serde_json::Value) -> (Vec<String>, Vec<i64>) {
    let calls: Vec<&serde_json::Value> = report["timing"]["timeline"].as_array().unwrap().iter().filter(|e| e["kind"] == "call").collect();
    let draws: Vec<usize> = (0..calls.len()).filter(|&i| calls[i]["name"] == "draw_strokes_on").collect();
    let waits = draws
        .iter()
        .map(|&i| calls[i + 1..].iter().find(|c| c["name"].as_str().is_some_and(|n| n.starts_with("extended_wait_until"))).map_or("none", |c| c["outcome"].as_str().unwrap()).to_string())
        .collect();
    let at = |i: usize, k: &str| calls[i][k].as_i64().unwrap();
    (waits, draws.windows(2).map(|w| at(w[1], "t0") - at(w[0], "t0") - at(w[0], "ms")).collect())
}
