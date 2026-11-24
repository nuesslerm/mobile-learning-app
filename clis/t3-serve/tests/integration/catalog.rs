use super::support::{self, Harness, PairingRequest};
use serde_json::{json, Value};
use std::path::Path;
use t3_core::{rpc::Connection, Principal};

#[test]
#[ignore = "wire catalog measured through wf test run --performance"]
fn given_the_pinned_catalog_when_each_host_call_is_dispatched_then_its_outcome_is_recorded() {
    let (dispatch, exporter) = super::performance::exporter();
    let _guard = tracing::dispatcher::set_default(&dispatch);
    let host = Harness::start();
    let scopes: Vec<String> = t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect();
    let principal = Principal { token_id: "catalog".into(), scopes: scopes.clone(), expires_at: i64::MAX };
    let pairing = host.pair(PairingRequest { scopes, ..PairingRequest::default() });
    let token = support::bearer_of(host.addr, &pairing.code);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("cassettes/t3-api");
    for (row, input) in exec_testkit::performance_catalog(&dir) {
        let name = row["name"].as_str().unwrap();
        let kind = row["kind"].as_str().unwrap();
        let span = tracing::info_span!("catalog.call", otel.name = %format!("catalog.t3.{kind}.{name}"), api.outcome = tracing::field::Empty, http.response.status_code = tracing::field::Empty).entered();
        let script = input["script"].as_array().into_iter().flatten();
        if kind == "http" {
            let req = script.filter_map(|s| s.get("http")).next();
            let path = row["path"].as_str().unwrap().split('/').map(|part| if part.starts_with(':') { "catalog" } else { part }).collect::<Vec<_>>().join("/");
            let body = req.and_then(|r| r.get("body")).map(Value::to_string).unwrap_or_default();
            let reply = support::http(host.addr, row["method"].as_str().unwrap(), &path, &[("Authorization", &format!("Bearer {token}")), ("Content-Type", "application/json")], &body);
            assert!((100..600).contains(&reply.status), "{name}");
            span.record("http.response.status_code", reply.status);
            span.record("api.outcome", if reply.status >= 400 { "refused" } else { "ok" });
            continue;
        }
        let tag = match kind { "rpc" => name, "command" => "orchestration.dispatchCommand", "mutation" => "projects.mutate", _ => panic!("unknown catalog kind {kind}") };
        let payload = script.filter_map(|s| s.get("ws-send")).find(|f| f["tag"] == tag && (kind == "rpc" || f["payload"]["type"] == name)).map(|r| r["payload"].clone()).unwrap_or_else(|| if kind == "rpc" { json!({}) } else { json!({"type":name}) });
        let mut conn = Connection::new(host.service.clone(), principal.clone());
        let frames = conn.on_message(&json!({"_tag":"Request","id":"1","tag":tag,"payload":payload,"headers":[]}).to_string());
        let refused = frames.iter().map(|f| serde_json::from_str::<Value>(f).unwrap()).any(|f| f["exit"]["_tag"] == "Failure");
        span.record("api.outcome", if refused { "refused" } else { "ok" });
    }
    exporter.shutdown();
}
