//! The orchestration HTTP routes a phone reads a thread's past through: the transcript, history pages walked older-first from
//! a cursor, the conversation view, and the gate (protocol header, bearer, scope) with the environment errors the pin declares.
use super::support::*;
use std::net::SocketAddr;
use t3_contracts::history::Cursor;
use t3_core::ports::AgentEventKind;

fn get(addr: SocketAddr, path: &str, bearer: Option<&str>, protocol: Option<&str>) -> Reply {
    let auth = bearer.map(|b| format!("Bearer {b}"));
    let mut headers = Vec::new();
    if let Some(auth) = &auth {
        headers.push(("Authorization", auth.as_str()));
    }
    if let Some(protocol) = protocol {
        headers.push(("x-t3-orchestration-protocol", protocol));
    }
    http(addr, "GET", path, &headers, "")
}

/// A server whose thread `t1` ran one turn of `tools` tool calls and a reply, and a bearer with the default scopes.
fn with_history(tools: usize) -> (Harness, Rpc, String) {
    let h = Harness::start();
    let mut rpc = connect(&h);
    seed(&mut rpc);
    let mut script: Vec<AgentEventKind> = (0..tools)
        .flat_map(|i| {
            let id = format!("k{i}");
            [AgentEventKind::ToolStarted { id: id.clone(), name: "bash".into(), input: json!({"command": "ls"}) }, AgentEventKind::ToolOutput { id: id.clone(), data: "ok".into() }, AgentEventKind::ToolFinished { id, ok: true }]
        })
        .collect();
    script.push(AgentEventKind::Text("done".into()));
    *h.agent.script.lock().unwrap() = script;
    success(&rpc.call("orchestration.dispatchCommand", json!({"type": "message.dispatch", "commandId": "c-m", "threadId": "t1", "message": {"messageId": "m1", "text": "go"}})));
    settled(&mut rpc, "t1");
    let bearer = bearer_of(h.addr, &h.pair(PairingRequest::default()).code);
    (h, rpc, bearer)
}

fn item_ids(rows: &Value) -> Vec<String> {
    rows.as_array().unwrap().iter().map(|r| r["item"]["id"].as_str().unwrap().to_string()).collect()
}

#[test]
fn the_transcript_is_every_turn_item_of_the_thread_as_projected_rows_in_order() {
    // Given a thread that ran a turn with two tool calls
    let (h, mut rpc, bearer) = with_history(2);
    let projection = settled(&mut rpc, "t1");
    // When its transcript is read
    let reply = get(h.addr, "/api/orchestration/threads/t1/transcript", Some(&bearer), Some("2"));
    // Then it names the thread and holds the projection's turn items in order, each a local row of this thread at its position
    assert_eq!(reply.status, 200, "{:?}", reply.body);
    assert_eq!((reply.body["threadId"].as_str(), reply.body["title"].as_str()), (Some("t1"), projection["thread"]["title"].as_str()));
    let ids: Vec<String> = projection["turnItems"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(item_ids(&reply.body["items"]), ids);
    for (position, row) in reply.body["items"].as_array().unwrap().iter().enumerate() {
        assert_eq!((row["position"].as_u64(), row["visibility"].as_str(), row["sourceThreadId"].as_str(), &row["sourceItemId"]), (Some(position as u64), Some("local"), Some("t1"), &row["item"]["id"]));
    }
    // And the projection's visible items are the same rows
    assert_eq!(projection["visibleTurnItems"], reply.body["items"]);
}

#[test]
fn history_pages_walk_the_timeline_older_first_from_a_cursor_until_none_is_left() {
    // Given a thread longer than one page, and a cursor that ends at (and takes in) its newest row
    let (h, _rpc, bearer) = with_history(100);
    let all = get(h.addr, "/api/orchestration/threads/t1/transcript", Some(&bearer), Some("2")).body["items"].clone();
    let newest = all.as_array().unwrap().last().unwrap();
    let start = Cursor { snapshot_sequence: 0, source_thread_id: "t1".into(), source_item_id: newest["sourceItemId"].as_str().unwrap().into(), position: newest["position"].as_u64().unwrap(), inclusive: true };
    // When the pages are followed by their next cursors until none is left
    let (mut cursor, mut pages) = (Some(start.encode()), Vec::new());
    while let Some(c) = cursor {
        let reply = get(h.addr, &format!("/api/orchestration/threads/t1/history?cursor={c}"), Some(&bearer), Some("2"));
        assert_eq!(reply.status, 200, "{:?}", reply.body);
        assert_eq!(reply.body["hasMoreHistory"].as_bool(), Some(reply.body["nextCursor"].is_string()));
        cursor = reply.body["nextCursor"].as_str().map(str::to_string);
        pages.push(reply.body);
    }
    // Then they hold every row once, newest page first, each page full but the last, each numbered from 0
    let walked: Vec<String> = pages.iter().rev().flat_map(|p| item_ids(&p["items"])).collect();
    assert_eq!(walked, item_ids(&all));
    let sizes: Vec<usize> = pages.iter().map(|p| p["items"].as_array().unwrap().len()).collect();
    assert!(sizes.len() > 1 && sizes[..sizes.len() - 1].iter().all(|n| *n == t3_contracts::history::PAGE_MAX_ITEMS), "{sizes:?}");
    assert!(pages.iter().all(|p| p["items"].as_array().unwrap().iter().enumerate().all(|(i, r)| r["position"].as_u64() == Some(i as u64))));
    // And the conversation view of the first page leaves the tool calls out
    let conversation = get(h.addr, &format!("/api/orchestration/threads/t1/history?cursor={}&view=conversation", start.encode()), Some(&bearer), Some("2")).body;
    let kinds: Vec<&str> = conversation["items"].as_array().unwrap().iter().map(|r| r["item"]["type"].as_str().unwrap()).collect();
    assert!(!kinds.is_empty() && !kinds.contains(&"command_execution"), "{kinds:?}");
}

#[test]
fn the_routes_refuse_without_the_protocol_a_bearer_or_the_scope_and_answer_an_unknown_thread_and_a_bad_cursor_with_the_pins_errors() {
    // Given a thread, a default bearer, and a bearer without orchestration:read
    let (h, _rpc, bearer) = with_history(0);
    let narrow = bearer_of(h.addr, &h.pair(PairingRequest { scopes: vec!["terminal:read".into()], ..PairingRequest::default() }).code);
    let history = "/api/orchestration/threads/t1/history?cursor=";
    let cursor = Cursor { snapshot_sequence: 0, source_thread_id: "t1".into(), source_item_id: "x".into(), position: 0, inclusive: false }.encode();
    let cases = [
        ("/api/orchestration/threads/t1/transcript", Some(bearer.as_str()), None, 426, "", "orchestration_protocol_incompatible"),
        ("/api/orchestration/threads/t1/transcript", None, Some("2"), 401, "EnvironmentAuthInvalidError", "missing_credential"),
        ("/api/orchestration/threads/t1/transcript", Some("not-a-token"), Some("2"), 401, "EnvironmentAuthInvalidError", "invalid_credential"),
        ("/api/orchestration/threads/t1/transcript", Some(narrow.as_str()), Some("2"), 403, "EnvironmentScopeRequiredError", "insufficient_scope"),
        ("/api/orchestration/threads/nope/transcript", Some(bearer.as_str()), Some("2"), 404, "EnvironmentResourceNotFoundError", "thread_not_found"),
        (history, Some(bearer.as_str()), Some("2"), 400, "EnvironmentRequestInvalidError", "invalid_history_cursor"),
        ("/api/orchestration/threads/t1/history?cursor=%21%21", Some(bearer.as_str()), Some("2"), 400, "EnvironmentRequestInvalidError", "invalid_history_cursor"),
    ];
    for (path, auth, protocol, status, tag, reason) in cases {
        // When each request is made
        let reply = get(h.addr, path, auth, protocol);
        // Then the status and the error name the cause, and a traced refusal leaves its trace id in the log
        assert_eq!(reply.status, status, "{path} {:?}", reply.body);
        if tag.is_empty() {
            assert_eq!(reply.body["code"].as_str(), Some(reason));
            continue;
        }
        assert_eq!(reply.body["_tag"].as_str(), Some(tag), "{path}");
        assert!(reply.body["reason"].as_str() == Some(reason) || reply.body["code"].as_str() == Some(reason), "{:?}", reply.body);
        let trace = reply.body["traceId"].as_str().unwrap();
        assert!(h.log.lines().iter().any(|l| l.starts_with("orchestration.http.refused") && l.contains(trace)), "{trace}");
    }
    // And the scope error names the permission, while a good cursor of the same thread is served
    let scoped = get(h.addr, "/api/orchestration/threads/t1/history?cursor=x", Some(&narrow), Some("2")).body;
    assert_eq!(scoped["requiredPermission"].as_str(), Some("orchestration:read"));
    assert_eq!(get(h.addr, &format!("{history}{cursor}"), Some(&bearer), Some("2")).status, 200);
}
