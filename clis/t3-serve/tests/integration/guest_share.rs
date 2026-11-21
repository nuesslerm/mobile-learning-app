//! Guest links through the chain: sharing is off until the owner turns the setting on (kept by the real rca-service), the owner makes a link, a
//! guest joins and writes over the two routes a page calls, and the owner dismisses the message and closes the link. A call counts for its row
//! only when it was answered.
use crate::rig::*;
use crate::support::*;
use std::io::{Read, Write};

fn all_scopes() -> Vec<String> {
    t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect()
}

/// One POST with a JSON body: its status, head and parsed body.
fn post(chain: &Chain, path: &str, cookie: Option<&str>, body: &Value) -> (u16, String, Value) {
    let text = body.to_string();
    let jar = cookie.map(|c| format!("Cookie: {c}\r\n")).unwrap_or_default();
    let mut stream = std::net::TcpStream::connect(chain.addr).unwrap();
    stream.set_read_timeout(Some(std::time::Duration::from_secs(60))).unwrap();
    stream.write_all(format!("POST {path} HTTP/1.1\r\nHost: t3\r\nConnection: close\r\nContent-Type: application/json\r\n{jar}Content-Length: {}\r\n\r\n{text}", text.len()).as_bytes()).unwrap();
    let mut all = String::new();
    stream.read_to_string(&mut all).unwrap();
    let (head, body) = all.split_once("\r\n\r\n").unwrap_or((&all, ""));
    (head.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap(), head.to_string(), serde_json::from_str(body).unwrap_or(Value::Null))
}

fn shared_row(phone: &Phone, case: &mut exec_testkit::Case, chain: &Chain) -> Value {
    let shell = phone.get(case, chain, &["http:orchestration.shellSnapshot"], "/api/orchestration/shell");
    shell.body["threads"].as_array().unwrap().iter().find(|t| t["id"] == "t1").unwrap()["guestShare"].clone()
}

#[test]
fn the_owner_turns_sharing_on_makes_a_link_a_guest_writes_through_it_and_the_owner_dismisses_the_message_and_closes_the_link() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain with a paired owner, a thread, and the default setting
        let chain = Chain::start();
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        phone.seed(case, &chain);
        // When the owner asks for a link before turning sharing on
        let off = phone.calls.call("guestShare.create", json!({"threadId": "t1"}));
        // Then sharing is off, as recorded
        assert_eq!(off["_tag"], "Failure", "{off}");
        assert!(off.to_string().contains("disabled"), "{off}");
        // When the owner turns it on and asks again
        let on = phone.call(case, &chain, &["rpc:server.updateSettings"], "server.updateSettings", json!({"patch": {"guestSharingEnabled": true}}));
        assert_eq!(on["_tag"], "Success", "{on}");
        let made = phone.call(case, &chain, &["rpc:guestShare.create"], "guestShare.create", json!({"threadId": "t1"}));
        let url = success(&made)["url"].as_str().unwrap().to_string();
        let share = success(&made)["share"]["id"].as_str().unwrap().to_string();
        assert!(url.starts_with(&format!("https://{GUEST_HOST}/guest#")), "{url}");
        // And a guest redeems the invite and writes
        let invite = url.rsplit('#').next().unwrap().to_string();
        let (status, head, _) = case.call(&chain.tap, &[], || {
            let r = post(&chain, "/api/guest/redeem", None, &json!({"credential": invite, "name": "Ada"}));
            (r.0 == 204, r)
        });
        assert_eq!(status, 204, "{head}");
        let cookie = head.split("t3_guest_").nth(1).map(|c| format!("t3_guest_{}", c.split(';').next().unwrap())).expect("a session cookie");
        let sent = case.call(&chain.tap, &[], || {
            let r = post(&chain, &format!("/api/guest/messages?share={share}"), Some(&cookie), &json!({"shareId": share, "text": "hello owner"}));
            (r.0 == 204, r)
        });
        assert_eq!(sent.0, 204, "{}", sent.1);
        // Then the owner's row shows the guest and the waiting message
        let row = shared_row(&phone, case, &chain);
        assert_eq!((row["guestName"].clone(), row["messages"][0]["text"].clone(), row["messages"][0]["status"].clone()), (json!("Ada"), json!("hello owner"), json!("pending")));
        // When the owner dismisses the message
        let id = row["messages"][0]["id"].as_str().unwrap().to_string();
        let dismissed = phone.command(case, &chain, json!({"type": "thread.guest-message.dismiss", "commandId": "c-dismiss", "threadId": "t1", "guestMessageId": id}));
        // Then the message is dismissed
        assert_eq!(dismissed["_tag"], "Success", "{dismissed}");
        assert_eq!(shared_row(&phone, case, &chain)["messages"][0]["status"], "dismissed");
        // When the owner closes the link
        let closed = phone.command(case, &chain, json!({"type": "thread.guest-share.close", "commandId": "c-close", "threadId": "t1"}));
        // Then the thread holds no share and the guest's session is refused
        assert_eq!(closed["_tag"], "Success", "{closed}");
        assert_eq!(shared_row(&phone, case, &chain), Value::Null);
        let after = post(&chain, &format!("/api/guest/messages?share={share}"), Some(&cookie), &json!({"shareId": share, "text": "still there?"}));
        assert_eq!((after.0, after.2["reason"].clone()), (403, json!("inactive")));
    });
}
