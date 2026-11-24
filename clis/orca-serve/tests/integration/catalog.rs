use serde_json::{json, Value};
use std::path::Path;

#[allow(dead_code, unused_imports)]
#[path = "../flow/support.rs"]
mod fixture;

#[test]
#[ignore = "wire catalog measured through wf test run --performance"]
fn given_the_pinned_catalog_when_each_host_call_is_dispatched_then_its_outcome_is_recorded() {
    let (dispatch, exporter) = super::performance::exporter();
    let _guard = tracing::dispatcher::set_default(&dispatch);
    let host = fixture::harness();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("cassettes/orca-api");
    for (row, input) in exec_testkit::performance_catalog(&dir) {
        let name = row["name"].as_str().unwrap();
        let kind = row["kind"].as_str().unwrap();
        if kind == "http" {
            assert!(row["skipped"].as_str().unwrap().contains("third-party"));
            continue;
        }
        let span = tracing::info_span!("catalog.call", otel.name = %format!("catalog.orca.{kind}.{name}"), api.outcome = tracing::field::Empty).entered();
        if kind == "transport" {
            assert_eq!(name, "e2ee.handshake");
            let _phone = fixture::paired(&host);
            span.record("api.outcome", "ok");
            continue;
        }
        assert_eq!(kind, "rpc");
        let mut conn = host.host.connection(&host.device.id, &host.device.token, vec![]);
        let params = input["requests"].as_array().and_then(|requests| requests.iter().find(|req| req["method"] == name)).and_then(|r| r.get("params")).cloned().unwrap_or(json!({}));
        let frames = conn.on_message(&json!({"id":"catalog","method":name,"params":params}).to_string());
        let refused = frames.iter().map(|f| serde_json::from_str::<Value>(f).unwrap()).any(|f| f["ok"] == false);
        span.record("api.outcome", if refused { "refused" } else { "ok" });
    }
    exporter.shutdown();
}
