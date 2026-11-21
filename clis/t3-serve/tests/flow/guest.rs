//! Guest links through the real router: the owner opens one with `guestShare.create` (off until the setting is on and the host names its
//! address), a guest joins and writes over the two HTTP routes a page calls, and the owner's rows and commands follow. The reference's
//! success shape was never recorded, so these answers are authored from its source (`docs/gaps/t3-mcp-secret-guest-telemetry-no-success-shape.md`).
use super::support::*;
use t3_core::usecases::pairing::PairingRequest;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use t3_core::ports::Op;

const HOST: &str = "guests.example";

/// An HTTP exchange whose head keeps its case: the guest's cookie is case-sensitive.
fn raw(h: &Harness, path: &str, headers: &[(&str, &str)], body: &str) -> (u16, String, Value) {
    let mut stream = TcpStream::connect(h.addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut request = format!("POST {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Length: {}\r\n", h.addr, body.len());
    for (k, v) in headers {
        request.push_str(&format!("{k}: {v}\r\n"));
    }
    stream.write_all(format!("{request}\r\n{body}").as_bytes()).unwrap();
    let mut text = String::new();
    stream.read_to_string(&mut text).unwrap();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    (head.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap_or(0), head.to_string(), serde_json::from_str(body).unwrap_or(Value::Null))
}

/// The device state of the double's agent API, where the settings the owner changes are kept (as the service's `state` group does).
fn keep_state(h: &Harness) {
    let kept: Arc<Mutex<HashMap<(String, String), Value>>> = Arc::default();
    *h.agent.responder.lock().unwrap() = Some(Box::new(move |op| match op {
        Op::StateSet { scope, key, value } => {
            kept.lock().unwrap().insert((scope.clone(), key.clone()), value.clone());
            Some(Ok(json!({})))
        }
        Op::StateGet { scope, key } => Some(Ok(json!({"value": kept.lock().unwrap().get(&(scope.clone(), key.clone())).cloned()}))),
        _ => None,
    }));
}

/// The owner's session: it may operate and change settings.
fn owner(h: &Harness) -> Rpc {
    let scopes = ["orchestration:read", "orchestration:operate", "settings:write"].iter().map(|s| s.to_string()).collect();
    connect_with(h, PairingRequest { scopes, ..PairingRequest::default() })
}

fn turn_on(rpc: &mut Rpc) {
    let on = rpc.call("server.updateSettings", json!({"patch": {"guestSharingEnabled": true}}));
    assert_eq!(on["_tag"], "Success", "{on}");
}

/// A server with a thread t1, sharing on, and the link made: the invite credential and the share id.
fn shared() -> (Harness, Rpc, String, String) {
    let h = Harness::start_with_guest_host(HOST);
    keep_state(&h);
    let mut rpc = owner(&h);
    seed(&mut rpc);
    turn_on(&mut rpc);
    let made = rpc.call("guestShare.create", json!({"threadId": "t1"}));
    let value = success(&made).clone();
    let credential = value["url"].as_str().unwrap().strip_prefix(&format!("https://{HOST}/guest#")).expect("the link's address").to_string();
    let id = value["share"]["id"].as_str().unwrap().to_string();
    (h, rpc, credential, id)
}

fn join(h: &Harness, credential: &str, name: &str) -> (u16, String, Value) {
    raw(h, "/api/guest/redeem", &[("Content-Type", "application/json")], &json!({"credential": credential, "name": name}).to_string())
}

fn cookie_of(head: &str, share: &str) -> String {
    let name = format!("t3_guest_{share}=");
    let start = head.find(&name).expect("the session cookie") + name.len();
    let value: String = head[start..].chars().take_while(|c| *c != ';' && *c != '\r').collect();
    format!("{name}{value}")
}

fn write(h: &Harness, cookie: &str, share: &str, body: Value) -> (u16, String, Value) {
    raw(h, &format!("/api/guest/messages?share={share}"), &[("Content-Type", "application/json"), ("Cookie", cookie)], &body.to_string())
}

/// The share on the row of t1 as the phone's HTTP shell snapshot shows it.
fn row_share(h: &Harness) -> Value {
    let scopes = vec!["orchestration:read".to_string()];
    let bearer = format!("Bearer {}", bearer_of(h.addr, &h.pair(PairingRequest { scopes, ..PairingRequest::default() }).code));
    let reply = http(h.addr, "GET", "/api/orchestration/shell", &[("Authorization", &bearer), ("x-t3-orchestration-protocol", "2")], "");
    assert_eq!(reply.status, 200, "{:?}", reply.body);
    reply.body["threads"].as_array().unwrap().iter().find(|t| t["id"] == "t1").expect("the thread t1")["guestShare"].clone()
}

#[test]
fn a_fresh_server_answers_disabled_and_with_sharing_on_but_no_host_answers_connect_unavailable_and_shares_nothing() {
    // Given a server with sharing at its default and a thread
    let h = Harness::start();
    keep_state(&h);
    let mut rpc = owner(&h);
    seed(&mut rpc);
    // When the owner asks for a link
    let off = rpc.call("guestShare.create", json!({"threadId": "t1"}));
    // Then the recorded answer comes back
    assert_eq!(failure(&off)["reason"], "disabled", "{off}");
    assert_eq!(failure(&off)["_tag"], "GuestShareCreateError");
    // When sharing is turned on but the host names no address
    turn_on(&mut rpc);
    let none = rpc.call("guestShare.create", json!({"threadId": "t1"}));
    // Then the reference's other refusal comes back and the thread holds no share
    assert_eq!(failure(&none)["reason"], "connect_unavailable", "{none}");
    assert_eq!(row_share(&h), Value::Null);
}

#[test]
fn with_sharing_on_and_a_host_a_link_is_made_for_a_live_thread_and_refused_for_one_that_is_not_there() {
    // Given a server with a host, sharing on and a thread
    let h = Harness::start_with_guest_host(HOST);
    keep_state(&h);
    let mut rpc = owner(&h);
    seed(&mut rpc);
    turn_on(&mut rpc);
    // When a link is made for the thread
    let made = success(&rpc.call("guestShare.create", json!({"threadId": "t1"}))).clone();
    // Then the address is the host's guest page with the credential in the fragment, and the share is the row's
    assert!(made["url"].as_str().unwrap().starts_with(&format!("https://{HOST}/guest#")), "{made}");
    let share = &made["share"];
    assert_eq!((share["host"].clone(), share["guestName"].clone(), share["messages"].clone()), (json!(HOST), Value::Null, json!([])));
    assert_eq!(*share, row_share(&h));
    let (created, expires) = (share["createdAt"].as_str().unwrap(), share["expiresAt"].as_str().unwrap());
    assert_eq!((created, expires), ("2026-11-01T00:00:00.000Z", "2026-11-02T00:00:00.000Z"));
    // And a thread that does not exist is refused as unavailable, and a request with no thread id is undecodable
    let missing = rpc.call("guestShare.create", json!({"threadId": "nope"}));
    assert_eq!(failure(&missing)["reason"], "thread_unavailable", "{missing}");
    assert_eq!(rpc.call("guestShare.create", json!({}))["_tag"], "Failure");
}

#[test]
fn a_guest_joins_once_and_writes_and_the_owner_dismisses_the_message_and_closes_the_link() {
    // Given a link the owner made
    let (h, mut rpc, credential, share) = shared();
    // When the guest redeems it with a name
    let (status, head, _) = join(&h, &credential, "Ada");
    // Then the answer sets a session cookie for the share that scripts cannot read, and the row names the guest
    assert_eq!(status, 204);
    assert!(head.contains("HttpOnly") && head.contains("Secure") && head.contains("SameSite=Strict") && head.contains("Path=/api/guest"), "{head}");
    assert_eq!(row_share(&h)["guestName"], "Ada");
    let cookie = cookie_of(&head, &share);
    // And the same link cannot be spent twice
    let (again, _, body) = join(&h, &credential, "Bob");
    assert_eq!((again, body["reason"].clone()), (409, json!("used")));
    // When the guest writes a message
    let (sent, _, _) = write(&h, &cookie, &share, json!({"shareId": share, "text": "  hello owner  "}));
    // Then it waits on the row, trimmed, for the owner
    assert_eq!(sent, 204);
    let waiting = row_share(&h)["messages"][0].clone();
    assert_eq!((waiting["text"].clone(), waiting["status"].clone()), (json!("hello owner"), json!("pending")));
    // When the owner dismisses it
    let id = waiting["id"].as_str().unwrap();
    let dismissed = rpc.call("orchestration.dispatchCommand", json!({"type": "thread.guest-message.dismiss", "commandId": "c-dismiss", "threadId": "t1", "guestMessageId": id}));
    assert_eq!(dismissed["_tag"], "Success", "{dismissed}");
    // Then the row says so, and when the owner closes the link the guest's session stops working
    assert_eq!(row_share(&h)["messages"][0]["status"], "dismissed");
    let closed = rpc.call("orchestration.dispatchCommand", json!({"type": "thread.guest-share.close", "commandId": "c-close", "threadId": "t1"}));
    assert_eq!(closed["_tag"], "Success", "{closed}");
    let (after, _, body) = write(&h, &cookie, &share, json!({"shareId": share, "text": "still there?"}));
    assert_eq!((after, body["reason"].clone()), (403, json!("inactive")));
}

#[test]
fn a_guest_message_is_refused_when_empty_stale_busy_or_from_no_session_and_a_name_that_hides_text_is_refused_at_the_door() {
    // Given a link and a guest who joined
    let (h, _rpc, credential, share) = shared();
    let (status, head, _) = join(&h, &credential, "Ada");
    assert_eq!(status, 204);
    let cookie = cookie_of(&head, &share);
    // Then an empty message, a page for another link and a request with no session are each refused with the reference's reason
    let reason = |(status, _, body): (u16, String, Value)| (status, body["reason"].clone());
    assert_eq!(reason(write(&h, &cookie, &share, json!({"shareId": share, "text": "   "}))), (400, json!("empty")));
    assert_eq!(reason(write(&h, &cookie, &share, json!({"shareId": "other", "text": "hi"}))), (409, json!("stale_page")));
    assert_eq!(reason(write(&h, "t3_guest_x=forged", &share, json!({"shareId": share, "text": "hi"}))), (403, json!("invalid")));
    assert_eq!(reason(write(&h, &cookie, &share, json!({"shareId": share, "text": "x".repeat(4001)}))), (400, json!("invalid")));
    // When three messages wait, a fourth is told to wait for the owner
    for n in 0..3 {
        assert_eq!(write(&h, &cookie, &share, json!({"shareId": share, "text": format!("m{n}")})).0, 204);
    }
    assert_eq!(reason(write(&h, &cookie, &share, json!({"shareId": share, "text": "m3"}))), (409, json!("busy")));
    // And a name with a direction override, or a forged invite, never joins anyone
    let other = Harness::start_with_guest_host(HOST);
    keep_state(&other);
    let mut rpc = owner(&other);
    seed(&mut rpc);
    turn_on(&mut rpc);
    let made = success(&rpc.call("guestShare.create", json!({"threadId": "t1"}))).clone();
    let invite = made["url"].as_str().unwrap().rsplit('#').next().unwrap().to_string();
    assert_eq!(reason(join(&other, &invite, "Ada\u{202E}evil")), (400, json!("invalid")));
    assert_eq!(reason(join(&other, "forged.credential", "Ada")), (403, json!("invalid")));
    assert_eq!(row_share(&other)["guestName"], Value::Null);
}

#[test]
fn a_link_stops_working_when_it_expires_when_sharing_is_turned_off_and_a_new_link_replaces_the_old_one() {
    // Given a link nobody has joined yet
    let (h, mut rpc, credential, share) = shared();
    // When a day has passed
    h.clock.advance_ms(24 * 60 * 60 * 1000);
    // Then the invite is no longer active
    let (status, _, body) = join(&h, &credential, "Ada");
    assert_eq!((status, body["reason"].clone()), (403, json!("inactive")));
    // When the owner makes a new link, the old credential no longer matches the thread's share
    let fresh = success(&rpc.call("guestShare.create", json!({"threadId": "t1"}))).clone();
    assert_ne!(fresh["share"]["id"], json!(share));
    let (replaced, _, body) = join(&h, &credential, "Ada");
    assert_eq!((replaced, body["reason"].clone()), (403, json!("inactive")));
    // And with the setting turned off again the new link answers nothing either
    let invite = fresh["url"].as_str().unwrap().rsplit('#').next().unwrap().to_string();
    let off = rpc.call("server.updateSettings", json!({"patch": {"guestSharingEnabled": false}}));
    assert_eq!(off["_tag"], "Success", "{off}");
    let (disabled, _, body) = join(&h, &invite, "Ada");
    assert_eq!((disabled, body["reason"].clone()), (403, json!("inactive")));
}

#[test]
fn a_paired_client_cannot_dispatch_the_server_only_guest_commands_so_no_link_or_message_appears_past_the_setting_and_host_gate() {
    // Given a server with sharing on, a host, a thread, and an owner session with every scope
    let h = Harness::start_with_guest_host(HOST);
    keep_state(&h);
    let mut rpc = owner(&h);
    seed(&mut rpc);
    turn_on(&mut rpc);
    // When the owner's client dispatches each command only the server may issue
    let open = rpc.call("orchestration.dispatchCommand", json!({"type": "thread.guest-share.open", "commandId": "c-open", "threadId": "t1", "shareId": "s-forged", "host": HOST}));
    let redeem = rpc.call("orchestration.dispatchCommand", json!({"type": "thread.guest-share.redeem", "commandId": "c-redeem", "threadId": "t1", "guestName": "Eve"}));
    let submit = rpc.call("orchestration.dispatchCommand", json!({"type": "thread.guest-message.submit", "commandId": "c-submit", "threadId": "t1", "text": "hi"}));
    // Then each is refused as a dispatch that does not decode, and the thread holds no share
    for answer in [&open, &redeem, &submit] {
        assert_ne!(answer["_tag"], "Success", "{answer}");
    }
    assert_eq!(row_share(&h), Value::Null);
}
