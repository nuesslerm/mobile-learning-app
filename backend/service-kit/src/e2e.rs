//! For each service's deployment verification test: one request carrying a fresh W3C trace id, then BetterStack's backend logs table is
//! polled until the service's `http.request` line in that trace arrives, which must carry what the telemetry crate infers.
use serde_json::Value;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(180);

/// From the environment, else from the vault through `wf secret get`.
pub fn setting(key: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| {
            let out = std::process::Command::new("wf").args(["secret", "get", key]).output().ok()?;
            out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|v| !v.is_empty())
        })
        .unwrap_or_else(|| panic!("{key} is unset and `wf secret get {key}` gave nothing; the deployed test needs it"))
}

/// Requests `url` in a fresh trace and returns the service's `http.request` line for it from BetterStack.
pub async fn logged_request(url: &str) -> Value {
    let trace = uuid::Uuid::new_v4().simple().to_string();
    let parent = uuid::Uuid::new_v4().simple().to_string();
    let client = reqwest::Client::new();
    client
        .get(url)
        .header("traceparent", format!("00-{trace}-{}-01", &parent[..16]))
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .unwrap_or_else(|e| panic!("GET {url}: {e}"));
    let (host, user, password, table) = (setting("CH_BACKEND_HOST"), setting("CH_BACKEND_USER"), setting("CH_BACKEND_PASSWORD"), setting("CH_BACKEND_TABLE"));
    let sql = format!(
        "SELECT raw FROM remote({table}) WHERE dt > now() - INTERVAL 30 MINUTE AND JSONExtractString(raw, 'message') = 'http.request' AND JSONExtractString(raw, 'span', 'trace_id') = '{trace}' LIMIT 1 FORMAT JSONEachRow"
    );
    let started = Instant::now();
    loop {
        let response = client
            .post(format!("https://{host}/"))
            .basic_auth(&user, Some(&password))
            .header("Content-Type", "text/plain")
            .body(sql.clone())
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .unwrap_or_else(|e| panic!("BetterStack query: {e}"));
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        assert!(status.is_success(), "BetterStack query answered {status}: {body}");
        if let Some(row) = body.lines().next().and_then(|l| serde_json::from_str::<Value>(l).ok()) {
            return match &row["raw"] {
                Value::String(raw) => serde_json::from_str(raw).unwrap_or_else(|e| panic!("raw is not JSON ({e}): {raw}")),
                other => other.clone(),
            };
        }
        assert!(started.elapsed() < WAIT, "no http.request line in trace {trace} from {url} reached BetterStack within {}s", WAIT.as_secs());
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

/// The commit `service` should run, from `WF_EXPECTED_REVISIONS` (a JSON object by service name, which `wf test run --stage
/// deployment-verification-tests` sets): the pushed one, or the one its image was built from while a push left its graph alone.
fn expected_in(revisions: &str, service: &str) -> Option<String> {
    let revisions: Value = serde_json::from_str(revisions).unwrap_or_else(|e| panic!("WF_EXPECTED_REVISIONS is not a JSON object ({e}): {revisions}"));
    revisions[service].as_str().filter(|s| !s.is_empty()).map(str::to_string)
}

/// A `/health` answer carries the commit the service was built from: it must be the one `WF_EXPECTED_REVISIONS` names for `service`
/// (the pushed commit, or the one its image was built from while a push left its graph alone). Without the variable it only has to name one.
pub fn assert_serves_commit(health: &Value, service: &str) {
    let commit = health["commit"].as_str().unwrap_or_default();
    assert!(!commit.is_empty(), "/health names no commit: {health}");
    if let Some(expected) = std::env::var("WF_EXPECTED_REVISIONS").ok().filter(|v| !v.is_empty()).and_then(|v| expected_in(&v, service)) {
        assert_revision(commit, &expected, health);
    }
}

fn assert_revision(revision: &str, expected: &str, line: &Value) {
    assert!(expected.starts_with(revision) || revision.starts_with(expected), "built from {revision}, expected {expected}: {line}");
}

/// The line names `service`, its version, commit (the one `WF_EXPECTED_REVISIONS` names for it, when set), host and OS, and the code that wrote it.
pub fn assert_inferred(line: &Value, service: &str) {
    let resource = &line["resource"]["attributes"];
    let attributes = &line["attributes"];
    let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
    assert_eq!(resource["service.name"], service, "{line}");
    for key in ["service.version", "vcs.ref.head.revision", "host.name", "os.type"] {
        assert!(!text(&resource[key]).is_empty(), "no {key}: {line}");
    }
    if let Some(expected) = std::env::var("WF_EXPECTED_REVISIONS").ok().filter(|v| !v.is_empty()).and_then(|v| expected_in(&v, service)) {
        assert_revision(&text(&resource["vcs.ref.head.revision"]), &expected, line);
    }
    assert!(text(&attributes["code.filepath"]).ends_with(".rs"), "no code.filepath: {line}");
    assert!(!text(&attributes["code.namespace"]).is_empty(), "no code.namespace: {line}");
    assert!(attributes["code.lineno"].as_i64().or_else(|| text(&attributes["code.lineno"]).parse().ok()).unwrap_or(0) > 0, "no code.lineno: {line}");
    assert!(!attributes["status"].is_null() && !text(&attributes["method"]).is_empty(), "no method or status: {line}");
}

#[cfg(test)]
mod tests {
    use super::{assert_revision, expected_in};
    use serde_json::json;

    const PUSHED: &str = "ef90df980456dfb6cf479933f56393be15458446";
    const BUILT: &str = "9895dc084c039cab48fa2c3aa35c942aa412bc5e";

    #[test]
    fn given_the_commits_wf_expects_when_a_service_runs_one_of_them_then_only_that_one_passes() {
        let expected = json!({ "reporting-service": &BUILT[..7], "leetcode-backend": PUSHED, "empty": "" }).to_string();
        assert_eq!(expected_in(&expected, "reporting-service").as_deref(), Some(&BUILT[..7]));
        assert_eq!((expected_in(&expected, "empty"), expected_in(&expected, "unlisted")), (None, None));
        assert_revision(BUILT, &BUILT[..7], &json!({}));
        assert_revision(PUSHED, expected_in(&expected, "leetcode-backend").as_deref().unwrap(), &json!({}));
    }

    #[test]
    #[should_panic(expected = "built from 9895dc084c039cab48fa2c3aa35c942aa412bc5e, expected ef90df98")]
    fn given_a_service_older_than_the_expected_commit_when_its_line_is_checked_then_it_fails() {
        assert_revision(BUILT, &PUSHED[..8], &json!({}));
    }
}
