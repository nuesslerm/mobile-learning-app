//! The M5 rows of t3 serve through the chain: a client's activity reaches the real rca-service as `service.clientActivity` and is kept there,
//! and a pairing code minted over HTTP pairs a second device with the delegated scopes only. The refusals prove the request arrived by the
//! status and the empty window they leave, never by a missing answer.
use crate::chain::last_window;
use crate::rig::*;
use crate::support::*;

fn with(extra: &[&str]) -> Vec<String> {
    t3_contracts::scopes::DEFAULT_PAIRING_SCOPES.iter().chain(extra).map(|s| s.to_string()).collect()
}

fn report(client: &str) -> Value {
    json!({"clientId": client, "clientKind": "mobile", "visible": true, "focused": true, "recentlyInteracted": true, "appState": "active",
        "scopes": [{"type": "server-config"}, {"type": "thread", "threadId": "t1"}], "ttlMs": 60000, "observedAt": "2026-11-01T00:00:05.000Z"})
}

#[test]
fn a_clients_activity_is_kept_by_the_real_service_and_a_client_without_the_diagnostics_scope_never_reaches_it() {
    exec_testkit::case!(SUITE, |case| {
        // Given a phone paired with diagnostics:read, and one with the default scopes
        let chain = Chain::start();
        let mut reporter = Phone::connect_with(case, &chain, &with(&["diagnostics:read"]));
        let mut plain = Phone::connect(case, &chain);
        // When the reporter says it is looking at the server config and a thread
        let kept = reporter.call(case, &chain, &["rpc:server.reportClientActivity"], "server.reportClientActivity", report("phone-1"));
        // Then the call succeeded, and rca answered service.clientActivity itself, with no executor tool
        assert_eq!(kept["_tag"], "Success", "{kept}");
        let (asked, executor) = last_window(case);
        assert_eq!(asked, [("service.clientActivity".to_string(), true)], "{}", chain.evidence());
        assert_eq!(executor, json!([]));
        // And a renewal for the same client is answered too, while the one without the scope is refused before rca is asked
        assert_eq!(reporter.call(case, &chain, &["rpc:server.reportClientActivity"], "server.reportClientActivity", report("phone-1"))["_tag"], "Success");
        let refused = plain.call(case, &chain, &["rpc:server.reportClientActivity"], "server.reportClientActivity", report("phone-2"));
        assert!(failure(&refused).to_string().contains("diagnostics:read"), "{refused}");
        assert!(last_window(case).0.is_empty());
    });
}

#[test]
fn a_pairing_code_minted_over_http_pairs_a_second_device_with_only_the_delegated_scopes_and_a_device_without_access_write_is_refused() {
    exec_testkit::case!(SUITE, |case| {
        // Given a phone paired with access:write, and one paired with the defaults
        let chain = Chain::start();
        let admin = Phone::connect_with(case, &chain, &with(&["access:write"]));
        let device = Phone::connect(case, &chain);
        // When the admin mints a credential for a reader
        let headers = [("Authorization", admin.bearer.as_str())];
        let minted = http_call(case, &chain, &["http:auth.pairingCredential"], ("POST", "/api/auth/pairing-token", &headers, r#"{"label": "reader", "scopes": ["orchestration:read"]}"#), 200);
        // Then the code pairs a device that holds that one scope, once
        let code = minted.body["credential"].as_str().expect("a credential").to_string();
        let paired = exchange(chain.addr, &code);
        assert_eq!((paired.status, paired.body["scope"].clone()), (200, json!("orchestration:read")), "{:?}", paired.body);
        assert_eq!(exchange(chain.addr, &code).status, 401);
        // And the device without access:write is refused with the scope it lacks, and the chain's log shows the refusal arrived
        let own = [("Authorization", device.bearer.as_str())];
        let refused = http_call(case, &chain, &["http:auth.pairingCredential"], ("POST", "/api/auth/pairing-token", &own, "{}"), 403);
        assert_eq!(refused.body["requiredScope"], "access:write");
    });
}
