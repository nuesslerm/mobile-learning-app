//! Pairing and the socket gate: the happy path and every refusal, each refusal proven to have reached the server by its log line.
use super::support::*;

const FIVE_MINUTES_MS: i64 = 5 * 60 * 1000;

fn refusal_logged(h: &Harness, line: &str) {
    let lines = h.log.lines();
    assert!(lines.iter().any(|l| l == line), "{line} not in {lines:?}");
}

#[test]
fn the_descriptor_names_protocol_2_and_the_environment_a_client_will_compare() {
    // Given a running server
    let h = Harness::start();
    // When a client reads the descriptor
    let reply = http(h.addr, "GET", "/.well-known/t3/environment", &[], "");
    // Then it advertises orchestration protocol 2, the environment id, a version and only the probe capability
    assert_eq!(reply.status, 200);
    let d = &reply.body;
    assert_eq!(d["orchestrationProtocolVersion"], 2);
    assert_eq!(d["environmentId"], ENVIRONMENT_ID);
    assert_eq!((d["capabilities"]["connectionProbe"].clone(), d["capabilities"]["repositoryIdentity"].clone()), (json!(true), json!(false)));
    assert!(d["serverVersion"].is_string() && d["label"].is_string() && d["platform"]["os"].is_string());
}

#[test]
fn a_pairing_code_becomes_a_30_day_bearer_a_5_minute_ticket_and_an_open_socket() {
    // Given a freshly minted code with the default scopes
    let h = Harness::start();
    let pairing = h.pair(PairingRequest::default());
    assert_eq!(pairing.code.len(), 12);

    // When it is exchanged at /oauth/token
    let reply = exchange(h.addr, &pairing.code);

    // Then the answer is a bearer token with 30 days, the granted permissions and the legacy subset
    assert_eq!(reply.status, 200, "{:?}", reply.body);
    let grant = &reply.body;
    assert_eq!((grant["token_type"].as_str(), grant["expires_in"].as_i64()), (Some("Bearer"), Some(30 * 24 * 3600 - 1)));
    assert_eq!(grant["scope"], "orchestration:read orchestration:operate terminal:read terminal:operate filesystem:read filesystem:write");

    // And the bearer opens a session that names its permissions
    let token = grant["access_token"].as_str().unwrap();
    let session = http(h.addr, "GET", "/api/auth/session", &[("Authorization", &format!("Bearer {token}"))], "");
    let permissions: Vec<&str> = session.body["permissions"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert_eq!((session.body["authenticated"].clone(), permissions.join(" ")), (json!(true), grant["scope"].as_str().unwrap().to_string()));
    let legacy: Vec<&str> = session.body["scopes"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(legacy.iter().all(|s| permissions.contains(s)) && !legacy.contains(&"filesystem:write"), "{legacy:?}");

    // And the bearer buys a ticket that expires in 5 minutes, which opens the socket
    let t = ticket(h.addr, token);
    assert_eq!(t.status, 200, "{:?}", t.body);
    assert_eq!(t.body["expiresAt"], t3_contracts::time::iso(START_MS + FIVE_MINUTES_MS));
    let mut rpc = open_socket(h.addr, &format!("orchestrationProtocol=2&wsTicket={}", t.body["ticket"].as_str().unwrap())).expect("open");
    assert_eq!(success(&rpc.call("server.probe", json!({}))), &json!({}));
}

#[test]
fn a_code_that_is_wrong_expired_or_already_used_is_refused_the_same_way_and_logged_with_its_reason() {
    // Given a code, a second code left to expire, and a third used once
    let h = Harness::start();
    let expiring = h.pair(PairingRequest::default());
    h.clock.advance_ms(FIVE_MINUTES_MS + 1);
    let live = h.pair(PairingRequest::default());
    assert_eq!(exchange(h.addr, &live.code).status, 200);

    // When each is exchanged: a code nobody minted, the expired one, the used one again
    let wrong = exchange(h.addr, "AAAAAAAAAAAA");
    let expired = exchange(h.addr, &expiring.code);
    let reused = exchange(h.addr, &live.code);

    // Then all three are 401 auth_invalid with one body, and the log tells the three reasons apart
    for reply in [&wrong, &expired, &reused] {
        assert_eq!((reply.status, reply.body["code"].as_str()), (401, Some("auth_invalid")), "{:?}", reply.body);
    }
    let untraced = |r: &Reply| { let mut b = r.body.clone(); b.as_object_mut().unwrap().remove("traceId"); b };
    assert!(untraced(&wrong) == untraced(&expired) && untraced(&expired) == untraced(&reused));
    for reason in ["unknown", "expired", "used"] {
        refusal_logged(&h, &format!("oauth.exchange.refused reason={reason}"));
    }
}

#[test]
fn a_malformed_exchange_is_a_400_and_a_code_survives_it() {
    // Given a code
    let h = Harness::start();
    let pairing = h.pair(PairingRequest::default());
    // When the exchange names another grant type, or leaves the token out
    let grant = http(h.addr, "POST", "/oauth/token", &[], &format!("grant_type=password&subject_token={}", pairing.code));
    let missing = http(h.addr, "POST", "/oauth/token", &[], &format!("grant_type={}", GRANT.replace(':', "%3A")));
    // Then both are 400, and the code still works afterwards
    assert_eq!((grant.status, missing.status), (400, 400));
    assert_eq!(exchange(h.addr, &pairing.code).status, 200);
}

#[test]
fn a_socket_without_protocol_2_is_426_before_any_ticket_is_looked_at() {
    // Given a valid ticket
    let h = Harness::start();
    let token = bearer_of(h.addr, &h.pair(PairingRequest::default()).code);
    let t = ticket(h.addr, &token).body["ticket"].as_str().unwrap().to_string();

    // When the upgrade names no protocol, protocol 1 or protocol 3
    for query in [format!("wsTicket={t}"), format!("orchestrationProtocol=1&wsTicket={t}"), format!("orchestrationProtocol=3&wsTicket={t}")] {
        // Then it is 426 (a plain request shows the body: the gate answers before the upgrade headers are read)
        let reply = http(h.addr, "GET", &format!("/ws?{query}"), &[], "");
        assert_eq!(reply.status, 426, "{query}");
        assert_eq!((reply.body["code"].as_str(), reply.body["orchestrationProtocolVersion"].as_i64()), (Some("orchestration_protocol_incompatible"), Some(2)));
        assert_eq!(open_socket(h.addr, &query).err(), Some(426));
    }
    refusal_logged(&h, "ws.upgrade.refused reason=protocol");

    // And the refused attempts did not spend the ticket
    assert!(open_socket(h.addr, &format!("orchestrationProtocol=2&wsTicket={t}")).is_ok());
}

#[test]
fn a_ticket_that_is_bad_spent_expired_or_for_media_is_401() {
    // Given a bearer and four tickets: one spent, one left to expire, one for media, and a made-up one
    let h = Harness::start();
    let token = bearer_of(h.addr, &h.pair(PairingRequest::default()).code);
    let fresh = |use_: &str| http(h.addr, "POST", &format!("/api/auth/websocket-ticket/{use_}"), &[("Authorization", &format!("Bearer {token}"))], "").body["ticket"].as_str().unwrap().to_string();
    let spent = fresh("rpc");
    assert!(open_socket(h.addr, &format!("orchestrationProtocol=2&wsTicket={spent}")).is_ok());
    let stale = fresh("rpc");
    let media = fresh("media");
    h.clock.advance_ms(FIVE_MINUTES_MS);

    // When each is presented on a protocol 2 upgrade
    let statuses: Vec<_> = [spent, stale, media, "not-a-ticket".to_string()].iter().map(|t| open_socket(h.addr, &format!("orchestrationProtocol=2&wsTicket={t}")).err()).collect();

    // Then every one is 401, as is a missing ticket
    assert_eq!(statuses, [Some(401); 4]);
    assert_eq!(open_socket(h.addr, "orchestrationProtocol=2").err(), Some(401));
    refusal_logged(&h, "ws.upgrade.refused reason=bad_ticket");
    refusal_logged(&h, "ws.upgrade.refused reason=no_ticket");
}

#[test]
fn a_bearer_that_is_missing_unknown_or_past_30_days_buys_no_ticket() {
    // Given a bearer
    let h = Harness::start();
    let token = bearer_of(h.addr, &h.pair(PairingRequest::default()).code);
    // When tickets are asked for without it, with a made-up one, and after 30 days
    let none = http(h.addr, "POST", "/api/auth/websocket-ticket", &[], "");
    let unknown = ticket(h.addr, "made-up");
    h.clock.advance_ms(30 * 24 * 3600 * 1000);
    let expired = ticket(h.addr, &token);
    // Then each is 401
    assert_eq!((none.status, unknown.status, expired.status), (401, 401, 401));
    refusal_logged(&h, "auth.bearer.refused reason=expired");
}

#[test]
fn no_code_token_or_ticket_ever_reaches_a_log_line() {
    // Given a full pairing flow with a refused attempt before and after it
    let h = Harness::start();
    let pairing = h.pair(PairingRequest::default());
    let _ = exchange(h.addr, "WRONGCODE234");
    let reply = exchange(h.addr, &pairing.code);
    let token = reply.body["access_token"].as_str().unwrap().to_string();
    let t = ticket(h.addr, &token).body["ticket"].as_str().unwrap().to_string();
    let _ = open_socket(h.addr, "wsTicket=x");
    let _ = open_socket(h.addr, &format!("orchestrationProtocol=2&wsTicket={t}"));
    let _ = exchange(h.addr, &pairing.code);

    // When the log is read
    let lines = h.log.lines();

    // Then the requests demonstrably arrived (the log has their events) and none of the secrets is in any line
    assert!(lines.iter().any(|l| l.starts_with("oauth.exchange.ok")) && lines.iter().any(|l| l.starts_with("oauth.exchange.refused")), "{lines:?}");
    for secret in [pairing.code.as_str(), "WRONGCODE234", token.as_str(), t.as_str()] {
        assert!(!lines.iter().any(|l| l.contains(secret)), "{secret} leaked: {lines:?}");
    }
}
