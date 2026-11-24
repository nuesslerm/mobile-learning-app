//! The desktop's thread reads through the socket: the in-thread find and its stream, and the tag a person sets on a thread.
use super::support::*;
use t3_core::ports::{AgentEventKind, Op};

fn started() -> (Harness, Rpc) {
    let h = Harness::start();
    let mut rpc = connect(&h);
    seed(&mut rpc);
    forget_seeding(&h);
    (h, rpc)
}

fn say(rpc: &mut Rpc, command: &str, text: &str) {
    let message = json!({"type": "message.dispatch", "createdBy": "user", "creationSource": "web", "commandId": command, "threadId": "t1", "messageId": format!("m-{command}"), "text": text});
    success(&rpc.call("orchestration.dispatchCommand", message));
    settled(rpc, "t1");
}

#[test]
fn a_thread_without_the_query_answers_the_empty_result_the_pinned_server_recorded() {
    // Given a thread that holds no message
    let (_h, mut rpc) = started();
    // When the desktop searches it
    let found = success(&rpc.call("orchestration.searchThread", json!({"threadId": "t1", "query": "a"}))).clone();
    // Then no match is found, and the result has the recorded keys
    assert_eq!((found["totalMatches"].clone(), found["activeIndex"].clone(), found["match"].clone(), found["navigation"].clone()), (json!(0), json!(0), Value::Null, json!([])));
    assert!(found["snapshotSequence"].is_u64(), "{found}");
    assert_eq!(found.as_object().unwrap().len(), 5);
}

#[test]
fn a_thread_the_server_does_not_hold_fails_with_the_search_error_and_the_missing_projection() {
    // Given a socket with no thread
    let (_h, mut rpc) = started();
    // When it searches a thread that was never made
    let exit = rpc.call("orchestration.searchThread", json!({"threadId": "nope", "query": "a"}));
    // Then the declared search error names the missing projection
    let error = failure(&exit);
    assert_eq!(error["_tag"], "OrchestrationV2SearchThreadError");
    assert_eq!(error["cause"]["cause"]["name"], "ProjectionStoreThreadNotFoundError");
    assert_eq!(error["cause"]["cause"]["message"], "No orchestration projection exists for thread nope.");
}

#[test]
fn the_find_counts_every_occurrence_in_the_messages_and_walks_them_by_index_and_offset() {
    // Given a thread whose user message holds the word twice and whose reply holds it once, in another case
    let (h, mut rpc) = started();
    *h.agent.script.lock().unwrap() = vec![AgentEventKind::Text("A NEEDLE for you".into())];
    say(&mut rpc, "c-1", "needle here, a needle there");
    // When the desktop searches the word
    let found = success(&rpc.call("orchestration.searchThread", json!({"threadId": "t1", "query": " needle "}))).clone();
    // Then three occurrences are found, the first one is active, and the navigation lists the two messages with where their counts start
    assert_eq!((found["totalMatches"].clone(), found["activeIndex"].clone()), (json!(3), json!(0)));
    assert_eq!(found["match"], json!({"entryId": "m-c-1", "runId": found["match"]["runId"], "occurrence": 0}));
    let navigation = found["navigation"].as_array().unwrap();
    assert_eq!(navigation.iter().map(|n| (n["startIndex"].as_u64().unwrap(), n["count"].as_u64().unwrap())).collect::<Vec<_>>(), [(0, 2), (2, 1)]);
    // And an index inside the reply picks it, an offset wraps around the end, and an index past the end stops at the last one
    let third = success(&rpc.call("orchestration.searchThread", json!({"threadId": "t1", "query": "needle", "index": 2}))).clone();
    assert_eq!((third["activeIndex"].clone(), third["match"]["entryId"] != "m-c-1", third["match"]["occurrence"].clone()), (json!(2), true, json!(0)));
    let wrapped = success(&rpc.call("orchestration.searchThread", json!({"threadId": "t1", "query": "needle", "offset": -1}))).clone();
    assert_eq!(wrapped["activeIndex"], json!(2));
    let past = success(&rpc.call("orchestration.searchThread", json!({"threadId": "t1", "query": "needle", "index": 99}))).clone();
    assert_eq!(past["activeIndex"], json!(2));
    // And a start inside the first message continues from its occurrence
    let from_start = success(&rpc.call("orchestration.searchThread", json!({"threadId": "t1", "query": "needle", "start": {"entryId": "m-c-1", "occurrence": 1}}))).clone();
    assert_eq!((from_start["activeIndex"].clone(), from_start["match"]["occurrence"].clone()), (json!(1), json!(1)));
}

#[test]
fn the_find_stream_sends_the_result_as_one_chunk_and_ends_once_it_is_acknowledged() {
    // Given a thread with one message
    let (_h, mut rpc) = started();
    say(&mut rpc, "c-1", "hello");
    // When the desktop opens the find stream
    let stream = rpc.send("orchestration.searchThreadStream", json!({"threadId": "t1", "query": "hello"}));
    // Then the one chunk holds the result, and the stream exits with no value after the ack
    let values = rpc.chunk(&stream);
    assert_eq!((values.len(), values[0]["totalMatches"].clone()), (1, json!(1)));
    let end = rpc.recv();
    assert_eq!((end["_tag"].as_str(), end["requestId"].clone(), end["exit"]["_tag"].as_str(), end["exit"]["value"].clone()), (Some("Exit"), stream, Some("Success"), Value::Null));
}

#[test]
fn a_query_longer_than_the_contract_allows_is_a_decode_defect() {
    // Given a socket
    let (_h, mut rpc) = started();
    // When the query is 201 characters
    let exit = rpc.call("orchestration.searchThread", json!({"threadId": "t1", "query": "a".repeat(201)}));
    // Then the call dies as an undecodable payload and no error is declared
    assert_eq!((exit["_tag"].as_str(), exit["cause"][0]["_tag"].as_str()), (Some("Failure"), Some("Die")));
}

#[test]
fn a_tag_set_on_a_thread_is_accepted_and_kept_in_the_agent_api_state() {
    // Given a thread
    let (h, mut rpc) = started();
    // When a tag is set on it, then taken off
    let on = rpc.call("orchestration.dispatchCommand", json!({"type": "thread.tag.set", "commandId": "c-tag", "threadId": "t1", "tag": "urgent", "tagged": true}));
    let off = rpc.call("orchestration.dispatchCommand", json!({"type": "thread.tag.set", "commandId": "c-untag", "threadId": "t1", "tag": "urgent", "tagged": false}));
    // Then both answer the sequence the command made, and the state holds the tag as it was last said
    assert!(success(&on)["sequence"].is_u64() && success(&off)["sequence"].as_u64() > success(&on)["sequence"].as_u64());
    let kept: Vec<(String, Value)> = h.agent.ops.lock().unwrap().iter().filter_map(|op| if let Op::StateSet { key, value, .. } = op { Some((key.clone(), value.clone())) } else { None }).collect();
    assert_eq!(kept, [("thread.tag:t1:urgent".to_string(), json!({"tagged": true})), ("thread.tag:t1:urgent".to_string(), json!({"tagged": false}))]);
}

#[test]
fn a_tag_set_on_a_thread_that_does_not_exist_is_refused() {
    // Given a socket with no thread "ghost"
    let (_h, mut rpc) = started();
    // When a tag is set on it
    let exit = rpc.call("orchestration.dispatchCommand", json!({"type": "thread.tag.set", "commandId": "c-ghost", "threadId": "ghost", "tag": "urgent", "tagged": true}));
    // Then the dispatch fails with the declared dispatch error
    assert_eq!(failure(&exit)["_tag"], "OrchestrationV2DispatchCommandError");
}

#[test]
fn a_tag_that_is_empty_longer_than_64_characters_or_holds_a_colon_is_refused_and_nothing_is_kept() {
    // Given a thread
    let (h, mut rpc) = started();
    // When a tag is set that is empty, 65 characters long, or holds a colon
    let exits: Vec<Value> = ["".to_string(), "a".repeat(65), "x:y".to_string()]
        .into_iter()
        .enumerate()
        .map(|(i, tag)| rpc.call("orchestration.dispatchCommand", json!({"type": "thread.tag.set", "commandId": format!("c-bad-{i}"), "threadId": "t1", "tag": tag, "tagged": true})))
        .collect();
    // Then each dispatch fails with the declared dispatch error and the state holds no tag
    assert!(exits.iter().all(|exit| failure(exit)["_tag"] == "OrchestrationV2DispatchCommandError"));
    assert!(!h.agent.ops.lock().unwrap().iter().any(|op| matches!(op, Op::StateSet { .. })));
}

#[test]
fn the_session_scan_groups_the_executors_cli_sessions_by_directory_and_marks_the_project_t3_holds() {
    // Given an executor whose home holds two claude sessions and a codex one in a held project's folder (one written with a trailing slash), a claude one elsewhere and one with no directory
    let (h, mut rpc) = started();
    *h.agent.responder.lock().unwrap() = Some(Box::new(|op| {
        let Op::Catalog { method: "sessions.list", .. } = op else { return None };
        Some(Ok(json!({"sessions": [
            {"agent": "claude", "session_id": "a", "cwd": "/workspace/demo", "modified_ms": 1_700_000_000_000u64},
            {"agent": "codex", "session_id": "b", "cwd": "/workspace/demo/", "modified_ms": 1_700_000_100_000u64},
            {"agent": "claude", "session_id": "c", "cwd": "/home/me/notes", "modified_ms": 1_800_000_000_000u64},
            {"agent": "claude", "session_id": "d", "cwd": "/workspace/demo", "modified_ms": 1_600_000_000_000u64},
            {"agent": "claude", "session_id": "e", "modified_ms": 1_800_000_000_000u64},
        ]})))
    }));
    // When the desktop scans
    let found = success(&rpc.call("agentSessions.scan", json!({}))).clone();
    // Then the newest directory comes first as a candidate to import, and the held project's folder is marked imported with its id
    let candidates = found["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 2, "{found}");
    assert_eq!((candidates[0]["path"].as_str(), candidates[0]["title"].as_str(), candidates[0]["alreadyImported"].clone()), (Some("/home/me/notes"), Some("notes"), json!(false)));
    assert!(candidates[0].get("projectId").is_none());
    assert_eq!((candidates[1]["path"].as_str(), candidates[1]["threadCount"].clone(), candidates[1]["sources"].clone()), (Some("/workspace/demo"), json!(3), json!(["claudeAgent", "codex"])));
    assert_eq!((candidates[1]["alreadyImported"].clone(), candidates[1]["projectId"].clone()), (json!(true), json!("p1")));
    assert!(found["scannedAt"].is_string());
}

#[test]
fn the_session_scan_of_a_host_with_no_session_is_the_recorded_empty_answer() {
    // Given an executor that lists no session
    let (_h, mut rpc) = started();
    // When the desktop scans
    let found = success(&rpc.call("agentSessions.scan", json!({}))).clone();
    // Then there is no candidate
    assert_eq!(found["candidates"], json!([]));
}
