use crate::support::*;

#[test]
fn given_a_paired_phone_when_it_runs_the_v2_handshake_then_it_is_authenticated_and_requests_get_answers_stamped_with_the_runtime_id() {
    // Given a harness and a phone paired with it
    let h = harness();
    let mut phone = PhoneClient::connect(h.addr, h.host_public);
    // When it sends the v2 hello and its token
    let ready = phone.hello_v2().expect("e2ee_ready");
    let answer = phone.authenticate(&h.device.token).expect("an answer");
    // Then the host answered ready for v2, authenticated the phone against the same transcript, and serves requests
    assert_eq!((ready["type"].as_str(), ready["v"].as_u64()), (Some("e2ee_ready"), Some(2)));
    assert_eq!(answer["type"], "e2ee_authenticated");
    assert!(answer["transcriptHashB64"].as_str().is_some_and(|t| !t.is_empty()));
    let reply = phone.call("status.get", json!({"probe": true}));
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["_meta"]["runtimeId"], RUNTIME_ID);
}

#[test]
fn given_a_paired_phone_when_it_uses_the_legacy_framing_then_it_is_authenticated_and_requests_get_answers() {
    // Given a phone that speaks the legacy NaCl box framing
    let h = harness();
    let mut phone = PhoneClient::connect(h.addr, h.host_public);
    // When it says hello and authenticates with its token
    let ready = phone.hello_legacy().expect("e2ee_ready");
    let answer = phone.authenticate(&h.device.token).expect("an answer");
    // Then the host answers with a bare ready, authenticates it, and serves a request
    assert_eq!(ready, json!({"type": "e2ee_ready"}));
    assert_eq!(answer, json!({"type": "e2ee_authenticated"}));
    assert_eq!(phone.call("host.platform", json!({}))["ok"], true);
}

#[test]
fn given_a_wrong_token_when_the_phone_authenticates_then_it_gets_a_sealed_unauthorized_and_a_closed_socket_and_nothing_is_served() {
    // Given a phone that holds a token no device was paired with
    let h = harness();
    let mut phone = PhoneClient::connect(h.addr, h.host_public);
    phone.hello_v2().expect("e2ee_ready");
    // When it authenticates and then tries a request
    let answer = phone.authenticate("not-a-paired-token").expect("a sealed answer");
    phone.send_request("status.get", json!({}));
    let closed = phone.wait_closed();
    // Then the answer is the sealed unauthorized error, the socket closes 4001, and rca was never asked
    assert_eq!(answer, json!({"type": "e2ee_error", "error": {"code": "unauthorized"}}));
    assert_eq!((closed.code, closed.reason.as_str()), (4001, "Unauthorized"));
    assert!(h.agent.calls().is_empty(), "an unauthenticated socket reached the agent API: {:?}", h.agent.calls());
}

#[test]
fn given_an_auth_message_of_the_wrong_shape_when_sent_then_it_is_refused_as_bad_auth() {
    // Given a v2 phone whose auth carries a key the exact shape does not have
    let h = harness();
    let mut phone = PhoneClient::connect(h.addr, h.host_public);
    phone.hello_v2().expect("e2ee_ready");
    // When it seals that auth, even with a valid token
    phone.send_sealed(&json!({"type": "e2ee_auth", "v": 2, "deviceToken": h.device.token, "extra": true}).to_string());
    let answer = phone.recv_sealed().expect("a sealed answer");
    // Then it is refused as bad_auth and the socket closes
    assert_eq!(answer, json!({"type": "e2ee_error", "error": {"code": "bad_auth"}}));
    assert_eq!(phone.wait_closed().code, 4001);
}

#[test]
fn given_frames_that_are_not_a_hello_or_do_not_open_when_sent_then_the_socket_closes_4001() {
    // Given a socket and a plain request instead of a hello
    let h = harness();
    let mut plain = PhoneClient::connect(h.addr, h.host_public);
    plain.send_raw(&json!({"id": "1", "method": "status.get"}).to_string());
    assert_eq!(plain.wait_closed().reason, "Invalid e2ee_hello");
    // And a socket whose first frame after the ready is garbage
    let mut garbage = PhoneClient::connect(h.addr, h.host_public);
    garbage.hello_v2().expect("e2ee_ready");
    garbage.send_raw("AAAA");
    // Then each is closed 4001 and none reaches rca
    let closed = garbage.wait_closed();
    assert_eq!((closed.code, closed.reason.as_str()), (4001, "Unauthorized"));
    assert!(h.agent.calls().is_empty());
}

#[test]
fn given_a_request_that_names_another_token_when_sent_then_it_is_refused_and_a_revoked_device_is_refused_at_once() {
    // Given an authenticated phone
    let h = harness();
    let mut phone = paired(&h);
    // When a request carries a token other than the one the socket proved
    let mismatch = phone.call("status.get", json!({"probe": true}));
    assert_eq!(mismatch["ok"], true);
    phone.send_sealed(&json!({"id": "x", "method": "status.get", "deviceToken": "someone-else"}).to_string());
    let refused = phone.recv_sealed().unwrap();
    // Then it is unauthorized, and so is the next request once the device is revoked, with the socket still open
    assert_eq!((refused["ok"].clone(), refused["error"]["code"].clone()), (json!(false), json!("unauthorized")));
    h.principals.revoke(&h.device.id);
    let after = phone.call("status.get", json!({"probe": true}));
    assert_eq!((after["ok"].clone(), after["error"]["message"].clone()), (json!(false), json!("Invalid device token")));
}

#[test]
fn given_malformed_requests_and_unknown_methods_when_sent_then_each_gets_the_references_error() {
    // Given an authenticated phone
    let h = harness();
    let mut phone = paired(&h);
    // When it sends a non-JSON frame, a frame with no id, no method, and an unknown method
    let mut answers = Vec::new();
    for raw in ["not json", r#"{"method":"status.get"}"#, r#"{"id":"m1"}"#] {
        phone.send_sealed(raw);
        answers.push(phone.recv_sealed().unwrap());
    }
    answers.push(phone.call("no.such.method", json!({})));
    // Then each carries the reference's code, message and the id it could be addressed to
    let seen: Vec<(String, String, String)> = answers.iter().map(|a| (a["id"].as_str().unwrap().into(), a["error"]["code"].as_str().unwrap().into(), a["error"]["message"].as_str().unwrap().into())).collect();
    assert_eq!(seen[0], ("unknown".into(), "bad_request".into(), "Invalid JSON request".into()));
    assert_eq!(seen[1], ("unknown".into(), "bad_request".into(), "Missing request id".into()));
    assert_eq!(seen[2], ("m1".into(), "bad_request".into(), "Missing RPC method".into()));
    assert_eq!((seen[3].1.as_str(), seen[3].2.as_str()), ("method_not_found", "Unknown method: no.such.method"));
}

#[test]
fn given_a_closed_socket_when_a_stream_was_open_then_the_host_ends_it_and_stays_up_for_the_next_phone() {
    // Given a phone with an open client-events stream
    let h = harness();
    let mut first = paired(&h);
    let id = first.send_request("runtime.clientEvents.subscribe", json!({}));
    assert_eq!(first.next_stream_frame(&id)["result"]["type"], "ready");
    // When it drops the socket without unsubscribing
    drop(first);
    // Then a second phone is served as before
    let mut second = paired(&h);
    assert_eq!(second.call("status.get", json!({"probe": true}))["ok"], true);
}

#[test]
fn given_a_binary_frame_before_authentication_when_it_does_not_open_then_it_closes_unauthorized_and_when_it_opens_then_invalid_binary() {
    // Given a socket past the hello and a frame of garbage bytes
    let h = harness();
    let mut garbage = PhoneClient::connect(h.addr, h.host_public);
    garbage.hello_v2().expect("e2ee_ready");
    garbage.send_binary(vec![7u8; 64]);
    // Then it is the reference's unpaired signal, 4001 Unauthorized
    let closed = garbage.wait_closed();
    assert_eq!((closed.code, closed.reason.as_str()), (4001, "Unauthorized"));
    // And a binary frame sent before any hello closes the same way
    let mut early = PhoneClient::connect(h.addr, h.host_public);
    early.send_binary(vec![7u8; 64]);
    assert_eq!(early.wait_closed().code, 4001);
    assert!(h.agent.calls().is_empty());
}

#[test]
fn given_a_peer_that_pings_but_never_authenticates_when_the_deadline_passes_then_the_socket_closes_4002() {
    // Given a server with a short handshake deadline and a peer that only pings after the upgrade
    let h = Harness::start_with_deadline(recorded_status(), std::time::Duration::from_millis(300));
    let mut peer = PhoneClient::connect(h.addr, h.host_public);
    // When it keeps pinging faster than the read timeout
    let started = std::time::Instant::now();
    while started.elapsed() < std::time::Duration::from_millis(900) && peer.send_ping() {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // Then the host closed it 4002 and nothing reached rca
    let closed = peer.wait_closed();
    assert_eq!((closed.code, closed.reason.as_str()), (4002, "E2EE handshake timeout"));
    assert!(h.agent.calls().is_empty());
}
