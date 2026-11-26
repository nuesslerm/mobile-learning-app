//! The t3-local parts of M5 through the real router: the pairing credential a client with `access:write` mints over HTTP, the guest-share
//! commands, and the peer-link RPCs of a server that links nothing. Each refusal is proven to have reached the server by its log line.
use super::support::*;
use t3_core::ports::{EventDraft, EventStore};

fn scoped(h: &Harness, scopes: &[&str]) -> String {
    let scopes = scopes.iter().map(|s| s.to_string()).collect();
    format!("Bearer {}", bearer_of(h.addr, &h.pair(PairingRequest { scopes, ..PairingRequest::default() }).code))
}

fn mint(h: &Harness, bearer: &str, body: &str) -> Reply {
    let mut headers = vec![("Content-Type", "application/json")];
    if !bearer.is_empty() {
        headers.push(("Authorization", bearer));
    }
    http(h.addr, "POST", "/api/auth/pairing-token", &headers, body)
}

fn logged(h: &Harness, what: &str) {
    let lines = h.log.lines();
    assert!(lines.iter().any(|l| l.contains(what)), "{what} not in {lines:?}");
}

#[test]
fn a_client_with_access_write_mints_a_code_that_pairs_one_device_once_with_the_scopes_it_asked_for() {
    // Given a client that holds access:write beside the default scopes
    let h = Harness::start();
    let admin = scoped(&h, &["orchestration:read", "orchestration:operate", "filesystem:read", "access:write"]);
    // When it mints a credential for a tablet that may only read
    let minted = mint(&h, &admin, r#"{"label": "tablet", "scopes": ["orchestration:read"]}"#);
    // Then the answer names the code, the label and an expiry five minutes on
    assert_eq!(minted.status, 200, "{:?}", minted.body);
    let code = minted.body["credential"].as_str().unwrap().to_string();
    assert_eq!((minted.body["label"].clone(), minted.body["expiresAt"].clone()), (json!("tablet"), json!(t3_contracts::time::iso(START_MS + 5 * 60 * 1000))));
    assert!(minted.body["id"].is_string() && code.len() == 12);
    // And the code pairs a device whose token carries exactly the delegated scope, and a second use of it is refused
    let paired = exchange(h.addr, &code);
    assert_eq!((paired.status, paired.body["scope"].clone()), (200, json!("orchestration:read")), "{:?}", paired.body);
    assert_eq!(exchange(h.addr, &code).status, 401);
    // And the mint was logged with who made it and what it carries, never the code
    logged(&h, "pairing.credential.minted");
    assert!(!h.log.lines().iter().any(|l| l.contains(&code)));
}

#[test]
fn a_credential_with_no_scopes_named_carries_the_defaults_and_one_with_no_label_is_named_by_the_server() {
    // Given a client that holds every default scope and access:write
    let h = Harness::start();
    let mut all: Vec<&str> = t3_contracts::scopes::DEFAULT_PAIRING_SCOPES.to_vec();
    all.push("access:write");
    let admin = scoped(&h, &all);
    // When it mints with an empty body
    let minted = mint(&h, &admin, "");
    // Then the code pairs a device with the default scopes
    assert_eq!(minted.status, 200, "{:?}", minted.body);
    assert!(minted.body.get("label").is_none());
    let paired = exchange(h.addr, minted.body["credential"].as_str().unwrap());
    assert_eq!(paired.body["scope"], "orchestration:read orchestration:operate terminal:read terminal:operate filesystem:read filesystem:write");
}

#[test]
fn a_credential_is_refused_without_a_bearer_without_access_write_and_for_scopes_the_client_lacks_or_that_are_wrong() {
    // Given a device that was paired with the defaults and an admin that lacks filesystem:write
    let h = Harness::start();
    let device = scoped(&h, &t3_contracts::scopes::DEFAULT_PAIRING_SCOPES);
    let admin = scoped(&h, &["orchestration:read", "access:write"]);
    // When each asks for a credential
    let anonymous = mint(&h, "", "{}");
    let plain = mint(&h, &device, "{}");
    let beyond = mint(&h, &admin, r#"{"scopes": ["orchestration:read", "filesystem:write"]}"#);
    let unknown = mint(&h, &admin, r#"{"scopes": ["root:all"]}"#);
    let twice = mint(&h, &admin, r#"{"scopes": ["orchestration:read", "orchestration:read"]}"#);
    let none = mint(&h, &admin, r#"{"scopes": []}"#);
    let broken = mint(&h, &admin, "[1]");
    // Then each is refused with the status and the scope or reason the reference names, and nothing was minted
    assert_eq!(anonymous.status, 401);
    assert_eq!((plain.status, plain.body["requiredScope"].clone()), (403, json!("access:write")));
    assert_eq!((beyond.status, beyond.body["requiredScope"].clone(), beyond.body["code"].clone()), (403, json!("filesystem:write"), json!("insufficient_scope")));
    for refused in [&unknown, &twice, &none, &broken] {
        assert_eq!((refused.status, refused.body.clone()), (400, Value::Null));
        assert!(!refused.head.contains("content-type") && !refused.head.contains("cache-control"), "{}", refused.head);
    }
    assert_eq!(h.log.lines().iter().filter(|l| l.contains("pairing.credential.minted")).count(), 0);
    logged(&h, "access.scope.refused");
}

/// A thread t1 whose row carries a guest share, and the server started again over its log (a share exists only as a row that carries one).
fn shared_thread() -> (Harness, Rpc) {
    let first = Harness::start();
    let mut rpc = connect(&first);
    seed(&mut rpc);
    let share = json!({"id": "s1", "host": "owner", "createdAt": "2026-11-01T00:00:00.000Z", "expiresAt": "2026-11-02T00:00:00.000Z", "guestName": "Ada",
        "messages": [{"id": "g1", "text": "hi", "status": "pending", "submittedAt": "2026-11-01T00:01:00.000Z"}]});
    let draft = EventDraft { event_id: "e-share".into(), kind: "thread.guest-share-opened".into(), thread_id: Some("t1".into()), project_id: Some("p1".into()), occurred_at: START_MS, payload: json!({"guestShare": share}) };
    first.store.append(vec![draft], None).unwrap();
    let h = first.restart();
    let rpc = connect(&h);
    (h, rpc)
}

/// The share on the row of t1 as the phone's HTTP shell snapshot shows it.
fn guest(h: &Harness) -> Value {
    let bearer = scoped(h, &["orchestration:read"]);
    let reply = http(h.addr, "GET", "/api/orchestration/shell", &[("Authorization", &bearer), ("x-t3-orchestration-protocol", "2")], "");
    assert_eq!(reply.status, 200, "{:?}", reply.body);
    reply.body["threads"].as_array().unwrap().iter().find(|t| t["id"] == "t1").expect("the thread t1")["guestShare"].clone()
}

#[test]
fn a_pending_guest_message_is_dismissed_and_closing_the_share_removes_it_from_the_row_the_phone_reads() {
    // Given a thread shared with a guest whose message waits
    let (h, mut rpc) = shared_thread();
    // When the owner dismisses the message
    let dismissed = rpc.call("orchestration.dispatchCommand", json!({"type": "thread.guest-message.dismiss", "commandId": "c-d", "threadId": "t1", "guestMessageId": "g1"}));
    // Then the projection shows it dismissed and the share still open
    assert_eq!(dismissed["_tag"], "Success", "{dismissed}");
    let share = guest(&h);
    assert_eq!((share["messages"][0]["status"].clone(), share["id"].clone()), (json!("dismissed"), json!("s1")));
    // And dismissing it again is refused with the reference's message, changing nothing
    let again = rpc.call("orchestration.dispatchCommand", json!({"type": "thread.guest-message.dismiss", "commandId": "c-d2", "threadId": "t1", "guestMessageId": "g1"}));
    assert_eq!(failure(&again)["message"], "Guest message g1 is not pending.");
    assert_eq!(guest(&h), share);
    // When the share is closed, Then the row has none, and closing again succeeds
    let closed = rpc.call("orchestration.dispatchCommand", json!({"type": "thread.guest-share.close", "commandId": "c-c", "threadId": "t1"}));
    assert_eq!(closed["_tag"], "Success", "{closed}");
    assert_eq!(guest(&h), Value::Null);
    let repeat = rpc.call("orchestration.dispatchCommand", json!({"type": "thread.guest-share.close", "commandId": "c-c2", "threadId": "t1"}));
    assert_eq!(repeat["_tag"], "Success", "{repeat}");
}

#[test]
fn peer_links_are_listed_empty_unlinked_to_nothing_and_a_link_or_an_answer_is_refused_in_the_shapes_the_rpcs_declare() {
    // Given a client allowed to operate
    let h = Harness::start();
    let mut rpc = connect(&h);
    // When it lists, unlinks, links and answers
    let list = rpc.call("peerLinks.list", json!({}));
    let unlink = rpc.call("peerLinks.unlink", json!({"environmentId": "e1"}));
    let link = rpc.call("peerLinks.link", json!({"url": "https://peer.example", "pairingCode": "ABCDEFGH2345", "access": "read-only"}));
    let stranger = rpc.call("peerLinks.answerRequest", json!({"threadId": "t1", "turnItemId": "i1", "answer": {"type": "decline"}}));
    seed(&mut rpc);
    let answer = rpc.call("peerLinks.answerRequest", json!({"threadId": "t1", "turnItemId": "i1", "answer": {"type": "decline"}}));
    // Then the list is empty, nothing was removed, the link is disabled, a thread with no projection fails to load and the request of a thread is unknown
    assert_eq!(success(&list), &json!({"links": []}));
    assert_eq!(success(&unlink), &json!({"removed": false}));
    assert_eq!((failure(&link)["_tag"].clone(), failure(&link)["reason"].clone()), (json!("PeerLinkError"), json!("disabled")));
    assert_eq!((failure(&stranger)["_tag"].clone(), failure(&stranger)["reason"].clone(), failure(&stranger)["cause"]["cause"]["name"].clone()), (json!("PeerLinkRequestError"), json!("load_failed"), json!("ProjectionStoreThreadNotFoundError")));
    assert_eq!((failure(&answer)["_tag"].clone(), failure(&answer)["reason"].clone()), (json!("PeerLinkRequestError"), json!("not_found")));
    // And a link with no code, a link with a numeric url, an answer with no thread and an answer of no known type are decoder defects
    let defects = [
        (rpc.call("peerLinks.link", json!({"url": "https://peer.example", "access": "read-only"})), "Missing key\n  at [\"pairingCode\"]"),
        (rpc.call("peerLinks.link", json!({"url": 7, "pairingCode": "a", "access": "read-only"})), "Expected string\n  at [\"url\"]"),
        (rpc.call("peerLinks.answerRequest", json!({"turnItemId": "i1", "answer": {"type": "decline"}})), "Missing key\n  at [\"threadId\"]"),
    ];
    for (exit, text) in defects {
        assert_eq!((exit["cause"][0]["_tag"].clone(), exit["cause"][0]["defect"].clone()), (json!("Die"), json!(text)), "{exit}");
    }
    let shrug = rpc.call("peerLinks.answerRequest", json!({"threadId": "t1", "turnItemId": "i1", "answer": {"type": "shrug"}}));
    assert_eq!(shrug["cause"][0]["_tag"], "Die", "{shrug}");
    logged(&h, "peer.link.disabled");
}

#[test]
fn health_answers_without_a_credential_and_the_healthcheck_of_the_binary_is_green_only_where_a_server_listens() {
    // Given a running server and a port nothing listens on
    let h = Harness::start();
    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    // When /health is read with no credential
    let health = http(h.addr, "GET", "/health", &[], "");
    // Then it answers ok with the service name and a commit
    assert_eq!((health.status, health.body["ok"].clone(), health.body["service"].clone()), (200, json!(true), json!("t3-serve")));
    assert!(health.body["commit"].is_string());
    // And the binary's healthcheck exits 0 against the server and 1 against the free port
    let check = |port: u16| std::process::Command::new(env!("CARGO_BIN_EXE_t3-serve")).args(["t3", "serve", "--healthcheck", "--bind", "127.0.0.1", "--port", &port.to_string()]).output().unwrap();
    let (up, down) = (check(h.addr.port()), check(free));
    assert_eq!((up.status.code(), down.status.code()), (Some(0), Some(1)), "{}", String::from_utf8_lossy(&down.stderr));
    assert!(String::from_utf8_lossy(&down.stderr).contains("no healthy server"));
}

#[test]
fn session_context_shows_only_openable_threads_in_the_same_rca_workspace() {
    // Given three T3 threads and an RCA session outside the T3 project
    let h = Harness::start();
    let mut rpc = connect(&h);
    seed(&mut rpc);
    for thread in ["other", "sibling"] {
        success(&rpc.call("orchestration.dispatchCommand", thread_create(thread, thread, "p1")));
    }
    *h.agent.responder.lock().unwrap() = Some(Box::new(|op| match op {
        t3_core::ports::Op::ThreadList => Some(Ok(json!({"threads": [
            {"id": "unprojected", "workspace": "worktree"}, {"id": "sibling", "workspace": "worktree"},
            {"id": "other", "workspace": "elsewhere"}, {"id": "t1", "workspace": "worktree"}
        ]}))),
        _ => None,
    }));
    let reader = scoped(&h, &["orchestration:read"]);
    let get = |thread: &str, token: &str| http(h.addr, "GET", &format!("/api/orchestration/threads/{thread}/session-context"),
        &[("Authorization", token), ("x-t3-orchestration-protocol", "2")], "");
    // When the session picker opens, then the server preserves workspace activity order
    let reply = get("t1", &reader);
    assert_eq!(reply.status, 200);
    assert_eq!(reply.body, json!({"workspace":"worktree","sessions":[{"id":"sibling","title":"Thread sibling"},{"id":"t1","title":"Thread t1"}]}));
    assert_eq!(get("unprojected", &reader).status, 404);
    assert_eq!(get("t1", "").status, 401);
    assert_eq!(get("t1", &scoped(&h, &["relay:read"])).status, 403);
}

#[test]
fn session_context_opens_a_saved_chat_once_and_rejects_another_workspace() {
    use t3_core::ports::Op;
    // Given a saved tab that has no T3 projection yet
    let h = Harness::start();
    let mut rpc = connect(&h);
    seed(&mut rpc);
    *h.agent.responder.lock().unwrap() = Some(Box::new(|op| match op {
        Op::ThreadList => Some(Ok(json!({"threads":[
            {"id":"t1","workspace":"desk"},
            {"id":"saved","workspace":"desk","driver":"opencode","model":"gpt-5"},
            {"id":"outside","workspace":"other"}
        ]}))),
        Op::StateGet {scope,key} if scope == "orca" && key == "tabs.extra.desk" =>
            Some(Ok(json!({"value":[
                {"kind":"agent-session","sessionId":"saved","title":"Second"},
                {"kind":"agent-session","sessionId":"outside","title":"Elsewhere"},
                {"kind":"agent-session","sessionId":"t1","title":"First"}
            ]}))),
        _ => None,
    }));
    let token = scoped(&h, &["orchestration:read"]);
    let context = || http(h.addr,"GET","/api/orchestration/threads/t1/session-context?savedTabs=1",
        &[("Authorization",&token),("x-t3-orchestration-protocol","2")],"").body;
    // When the picker opens, then it preserves the saved order within the workspace
    let mut expected = json!([{"id":"saved","title":"Second","agent":"opencode","needsProjection":true},{"id":"t1","title":"First"}]);
    assert_eq!(context()["sessions"], expected);
    // When a selection is retried, then the same projected chat remains openable
    for _ in 0..2 {
        assert_eq!(success(&rpc.call("mobile.session.openTab",json!({"threadId":"t1","tabId":"saved"}))), &json!({"threadId":"saved"}));
    }
    expected[0].as_object_mut().unwrap().remove("needsProjection");
    assert_eq!(context()["sessions"], expected);
    failure(&rpc.call("mobile.session.openTab",json!({"threadId":"t1","tabId":"outside"})));
}

#[test]
fn mobile_checkouts_open_the_saved_projected_chat_only_inside_the_selected_workspace() {
    use t3_core::ports::Op;
    // Given a detached checkout with a saved RCA chat and another executor with a projected thread
    let h = Harness::start();
    let mut rpc = connect(&h);
    seed(&mut rpc);
    success(&rpc.call("orchestration.dispatchCommand", thread_create("outside", "outside", "p1")));
    *h.agent.responder.lock().unwrap() = Some(Box::new(|op| match op {
        Op::MobileWorktrees => Some(Ok(json!({"repo":"desk","checkouts":[
            {"id":"desk::/fixture", "name":"fixture","automaticName":"fixture","path":"/fixture","workspace":"desk","branch":null,"main":false}
        ]}))),
        Op::ThreadList => Some(Ok(json!({"threads":[{"id":"t1","workspace":"desk"},{"id":"outside","workspace":"another"}]}))),
        Op::StateGet {scope,key} if scope == "orca" && key == "tabs.active.desk" => Some(Ok(json!({"value":"t1"}))),
        _ => None,
    }));
    let reader = scoped(&h, &["orchestration:read"]);
    let get = |path, auth: &str| http(h.addr, "GET", path, &[("Authorization", auth)], "");
    // When the phone reads the catalog and opens the checkout, then only its projected chat is eligible
    let list = get("/api/mobile/workspaces", &reader);
    assert_eq!(list.status, 200);
    assert_eq!(list.body["checkouts"], json!([{"id":"desk::/fixture","name":"fixture","automaticName":"fixture","pinned":false,"branch":null,"main":false}]));
    let result = get("/api/mobile/workspaces?checkout=desk%3A%3A%2Ffixture", &reader);
    assert_eq!(result.body["active"], "t1");
    assert_eq!(result.body["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(get("/api/mobile/workspaces?checkout=outside", &reader).status, 404);
    assert_eq!(get("/api/mobile/workspaces", "").status, 401);
    assert_eq!(get("/api/mobile/workspaces", &scoped(&h, &["relay:read"])).status, 403);
}

#[test]
fn mobile_workspace_edits_preserve_other_metadata_and_require_operate_scope() {
    use t3_core::ports::Op;
    // Given a checkout whose native Orca settings already contain notes
    let h = Harness::start();
    let state = std::sync::Arc::new(std::sync::Mutex::new(json!({"comment":"kept"})));
    let saved = state.clone();
    *h.agent.responder.lock().unwrap() = Some(Box::new(move |op| match op {
        Op::MobileWorktrees => Some(Ok(json!({"repo":"desk","checkouts":[
            {"id":"desk::/one","name":"main","automaticName":"main","branch":"main","main":true}
        ]}))),
        Op::StateGet {scope,key} if scope == "orca" && key == "orca.worktree.desk::/one" => Some(Ok(json!({"value":saved.lock().unwrap().clone()}))),
        Op::StateSet {scope,key,value} if scope == "orca" && key == "orca.worktree.desk::/one" => {
            *saved.lock().unwrap() = value.clone(); Some(Ok(json!({})))
        }
        _ => None,
    }));
    let writer = scoped(&h, &["orchestration:operate"]);
    let reader = scoped(&h, &["orchestration:read"]);
    let edit = |token:&str, body:Value| http(h.addr,"POST","/api/mobile/workspaces",&[("Authorization",token),("Content-Type","application/json")],&body.to_string());
    // When the phone pins and renames a checkout, then the updates share the native record and can be retried
    let pin = json!({"checkout":"desk::/one","isPinned":true});
    assert_eq!(edit(&reader,pin.clone()).status,403);
    for _ in 0..2 { assert_eq!(edit(&writer,pin.clone()).body["pinned"],true); }
    let renamed = edit(&writer,json!({"checkout":"desk::/one","displayName":"Preview"}));
    assert_eq!(renamed.body["name"],"Preview");
    assert_eq!(*state.lock().unwrap(), json!({"comment":"kept","isPinned":true,"displayName":"Preview"}));
    // When a name is cleared, then the branch is visible and unrelated metadata survives a rejected edit
    assert_eq!(edit(&writer,json!({"checkout":"desk::/one","displayName":""})).body["name"],"main");
    assert_eq!(edit(&writer,json!({"checkout":"other::/one","displayName":"wrong"})).status,404);
    assert_eq!(edit(&writer,json!({"checkout":"desk::/one","isPinned":"yes"})).status,422);
    assert_eq!(*state.lock().unwrap(),json!({"comment":"kept","isPinned":true}));
}
