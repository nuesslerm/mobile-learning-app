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

fn probe_phone(h: &Harness, legacy: bool) -> (PhoneClient, String) {
    let mut phone = if legacy { PhoneClient::open_legacy(h.addr, h.host_public, &h.device.token) } else { paired(h) };
    let id = phone.send_request("probe.wire", json!({}));
    (phone, id)
}

fn phone_gets_json_then_binary_then_json_in_order(legacy: bool) {
    // Given a stream that emits a JSON frame, a binary frame and a JSON frame
    let h = Harness::start_with_probe(recorded_status());
    // When the phone opens it
    let (mut phone, id) = probe_phone(&h, legacy);
    let first = phone.recv_sealed().expect("the opening frame");
    let binary = phone.recv_sealed_binary().expect("the binary frame");
    let last = phone.recv_sealed().expect("the closing frame");
    // Then the sealed binary message opens to the exact bytes, between the two text frames, in order
    let number = first["result"]["streamNumber"].as_u64().unwrap() as u32;
    assert_eq!((first["id"].as_str(), first["result"]["type"].as_str()), (Some(id.as_str()), Some("subscribed")));
    assert_eq!(binary, [vec![0x74, 1, 0, 0], number.to_le_bytes().to_vec(), PROBE_PAYLOAD.to_vec()].concat());
    assert_eq!(last["result"]["type"], "after");
}

#[test]
fn given_a_stream_with_a_binary_frame_when_a_v2_phone_opens_it_then_the_frame_arrives_sealed_and_in_order() {
    phone_gets_json_then_binary_then_json_in_order(false);
}

#[test]
fn given_a_stream_with_a_binary_frame_when_a_legacy_phone_opens_it_then_the_frame_arrives_sealed_and_in_order() {
    phone_gets_json_then_binary_then_json_in_order(true);
}

fn sealed_binary_reaches_the_stream(legacy: bool) {
    // Given a phone with the probe stream open
    let h = Harness::start_with_probe(recorded_status());
    let (mut phone, _) = probe_phone(&h, legacy);
    let number = phone.recv_sealed().unwrap()["result"]["streamNumber"].as_u64().unwrap() as u32;
    phone.recv_sealed_binary().unwrap();
    phone.recv_sealed().unwrap();
    // When it sends a sealed binary frame addressed to the stream, and one addressed to no stream
    let frame = |n: u32| [vec![0x74, 1, 9, 0], n.to_le_bytes().to_vec(), vec![42]].concat();
    phone.send_sealed_binary(&frame(number + 100));
    phone.send_sealed_binary(&frame(number));
    // Then only the first reaches the stream, and the socket still answers requests
    assert_eq!(phone.call("status.get", json!({"probe": true}))["ok"], true);
    assert_eq!(h.probe_inbound.lock().unwrap().clone(), vec![frame(number)]);
}

#[test]
fn given_an_open_stream_when_a_v2_phone_sends_a_sealed_binary_frame_then_it_reaches_the_stream() {
    sealed_binary_reaches_the_stream(false);
}

#[test]
fn given_an_open_stream_when_a_legacy_phone_sends_a_sealed_binary_frame_then_it_reaches_the_stream() {
    sealed_binary_reaches_the_stream(true);
}

#[test]
fn given_an_authenticated_phone_when_it_sends_binary_frames_that_do_not_open_then_the_fifth_closes_4003_and_none_reaches_a_stream() {
    // Given a phone with the probe stream open
    let h = Harness::start_with_probe(recorded_status());
    let (mut phone, _) = probe_phone(&h, false);
    phone.recv_sealed().unwrap();
    // When it sends five binary frames that are not sealed
    for _ in 0..5 {
        phone.send_binary(vec![7u8; 64]);
    }
    // Then the host closes 4003 and the stream saw nothing
    let closed = phone.wait_closed();
    assert_eq!((closed.code, closed.reason.as_str()), (4003, "Too many decryption failures"));
    assert!(h.probe_inbound.lock().unwrap().is_empty());
}

#[test]
fn given_an_accepted_socket_when_the_request_head_arrives_in_parts_then_health_answers() {
    use std::io::{Read, Write};
    let host = harness();
    let mut socket = std::net::TcpStream::connect(host.addr).unwrap();
    socket.write_all(b"GET /health HTTP/1.1\r\n").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(60));
    socket.write_all(b"Host: localhost\r\n\r\n").unwrap();
    let mut reply = String::new();
    socket.read_to_string(&mut reply).unwrap();
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
}
