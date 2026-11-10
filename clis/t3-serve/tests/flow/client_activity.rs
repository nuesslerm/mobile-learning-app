//! `server.reportClientActivity` through the socket, over an agent API double: the report goes to the agent API as `service.clientActivity`
//! in the contract's words, a report that does not decode is a defect that asks nothing, and a client without `diagnostics:read` is refused.
use super::support::*;
use t3_core::ports::{AgentError, Op};

fn report() -> Value {
    json!({
        "clientId": "phone-1", "clientKind": "mobile", "visible": true, "focused": false, "recentlyInteracted": true, "appState": "active", "lowPowerMode": "false",
        "scopes": [{"type": "thread", "threadId": "t1"}, {"type": "server-config"}, {"type": "vcs-status", "cwd": "/work/p"}, {"type": "provider-status", "instanceId": "rca"}],
        "ttlMs": 30000, "observedAt": "2026-11-01T00:00:05.250Z"
    })
}

fn with_diagnostics(h: &Harness) -> Rpc {
    let mut scopes: Vec<String> = t3_contracts::scopes::DEFAULT_PAIRING_SCOPES.iter().map(|s| s.to_string()).collect();
    scopes.push("diagnostics:read".into());
    connect_with(h, PairingRequest { scopes, ..PairingRequest::default() })
}

fn activity_ops(h: &Harness) -> Vec<Op> {
    h.agent.ops.lock().unwrap().iter().filter(|o| matches!(o, Op::ClientActivity(_))).cloned().collect()
}

#[test]
fn a_report_reaches_the_agent_api_as_the_lease_the_client_described_and_answers_with_no_value() {
    // Given a client allowed to report, and an agent API that keeps leases
    let h = Harness::start();
    let mut rpc = with_diagnostics(&h);
    // When the client reports itself visible with four scopes and a ttl
    let exit = rpc.call("server.reportClientActivity", report());
    // Then the RPC succeeds with no value, and the agent API was asked once, with the client, its states, the scopes as strings and the time in ms
    assert_eq!(exit["_tag"], "Success", "{exit}");
    let sent = activity_ops(&h);
    let [Op::ClientActivity(a)] = sent.as_slice() else { panic!("one service.clientActivity expected") };
    assert_eq!((a.client.as_str(), a.kind.as_str(), a.visible, a.focused, a.recently_interacted), ("phone-1", "mobile", true, false, true));
    assert_eq!((a.app_state.as_deref(), a.low_power_mode.as_deref(), a.ttl_ms), (Some("active"), Some("false"), Some(30_000)));
    assert_eq!(a.scopes, ["thread:t1", "server-config", "vcs-status:/work/p", "provider-status:rca"]);
    assert_eq!(a.observed_at_ms as i64, START_MS + 5_250);
}

#[test]
fn a_report_that_does_not_decode_is_a_defect_and_the_agent_api_is_not_asked() {
    // Given a client allowed to report
    let h = Harness::start();
    let mut rpc = with_diagnostics(&h);
    for (what, patch) in [("a kind the contract lacks", json!({"clientKind": "tv"})), ("a scope with no cwd", json!({"scopes": [{"type": "vcs-status"}]})), ("no visibility", json!({"visible": "yes"})), ("a date that is none", json!({"observedAt": "yesterday"}))] {
        // When the report is wrong in one way
        let mut payload = report();
        payload.as_object_mut().unwrap().extend(patch.as_object().unwrap().clone());
        let exit = rpc.call("server.reportClientActivity", payload);
        // Then the exit is a defect, not a declared error
        assert_eq!((exit["_tag"].as_str(), exit["cause"][0]["_tag"].as_str()), (Some("Failure"), Some("Die")), "{what}: {exit}");
    }
    assert!(activity_ops(&h).is_empty());
}

#[test]
fn a_client_without_diagnostics_scope_is_refused_and_an_agent_api_that_refuses_the_lease_is_told_to_the_client_in_its_words() {
    // Given a client with the default scopes
    let h = Harness::start();
    let mut plain = connect(&h);
    // When it reports, Then it is refused for the scope it lacks and the agent API is not asked
    let refused = plain.call("server.reportClientActivity", report());
    assert!(failure(&refused).to_string().contains("diagnostics:read"), "{refused}");
    assert!(activity_ops(&h).is_empty());
    // And when the agent API refuses a report from a client that may make one, the client gets its message
    *h.agent.responder.lock().unwrap() = Some(Box::new(|op| matches!(op, Op::ClientActivity(_)).then(|| Err(AgentError::Failed("the lease store is full".into())))));
    let mut rpc = with_diagnostics(&h);
    let exit = rpc.call("server.reportClientActivity", report());
    assert!(failure(&exit).to_string().contains("the lease store is full"), "{exit}");
}
