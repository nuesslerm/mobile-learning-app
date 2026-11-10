//! The M4 commands and reads of the thread surface through the socket, over an agent API double: each command asks the agent API for the
//! method the mapping names and only an accepted one is recorded; a refusal is the agent's own message and changes nothing; device-state
//! commands are kept in `state`; and the RPCs of M4 answer from the agent API's contract shapes or from the rows t3 keeps.
use super::support::*;
use agent_api_agent::lifecycle::{FamilyResult, HitSource, SearchHit, SearchResult, Shelf, ShelfResult, ThreadState};
use agent_api_agent::{lifecycle, queue, ThreadView};
use t3_core::ports::{AgentError, AgentEventKind, MetaStore, Op};

fn dispatch(rpc: &mut Rpc, command: Value) -> Value {
    let id = rpc.send("orchestration.dispatchCommand", command);
    loop {
        let frame = rpc.recv();
        if frame["_tag"] == "Chunk" {
            rpc.ack(&frame["requestId"]);
        } else if frame["requestId"] == id {
            return frame["exit"].clone();
        }
    }
}

fn started() -> (Harness, Rpc) {
    let h = Harness::start();
    let mut rpc = connect(&h);
    seed(&mut rpc);
    forget_seeding(&h);
    (h, rpc)
}

fn cmd(kind: &str, extra: Value) -> Value {
    let mut c = json!({"type": kind, "commandId": format!("c-{kind}-{extra}"), "threadId": "t1"});
    c.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap_or_default());
    c
}

fn row(rpc: &mut Rpc, thread: &str) -> Value {
    success(&rpc.call("orchestration.getThreadProjection", json!({"threadId": thread})))["thread"].clone()
}

fn ops(h: &Harness) -> Vec<Op> {
    h.agent.ops.lock().unwrap().iter().filter(|o| !matches!(o, Op::StateSet { .. })).cloned().collect()
}

fn thread_view(id: &str) -> ThreadView {
    serde_json::from_value(json!({"id": id, "title": id, "workspace": "w1", "driver": "opencode", "status": "idle", "last_seq": 1, "created_at": "2026-11-04T10:00:00Z", "updated_at": "2026-11-04T10:00:00Z"})).unwrap()
}

fn state_of(id: &str) -> ThreadState {
    serde_json::from_value(json!({"thread": thread_view(id)})).unwrap()
}

#[test]
fn every_forwarded_command_asks_the_agent_api_for_its_method_and_an_accepted_one_leaves_its_mark_on_the_rows() {
    // Given a thread, and an agent API that accepts everything
    let (h, mut rpc) = started();
    let until = "2030-01-01T00:00:00.000Z";
    let t = || "t1".to_string();
    let table: Vec<(Value, Op)> = vec![
        (cmd("thread.settle", json!({})), Op::ThreadSettle { thread: t(), state: "settled", until_ms: None, at_ms: None }),
        (cmd("thread.snooze", json!({"snoozedUntil": until})), Op::ThreadSettle { thread: t(), state: "snoozed", until_ms: Some(1_893_456_000_000), at_ms: None }),
        (cmd("thread.unsnooze", json!({"reason": "user"})), Op::ThreadSettle { thread: t(), state: "awake", until_ms: None, at_ms: None }),
        (cmd("thread.unsettle", json!({"reason": "user"})), Op::ThreadSettle { thread: t(), state: "active", until_ms: None, at_ms: None }),
        (cmd("thread.archive", json!({})), Op::ThreadArchive { thread: t(), archived: true }),
        (cmd("thread.unarchive", json!({})), Op::ThreadArchive { thread: t(), archived: false }),
        (cmd("queue.resume", json!({})), Op::QueueResume { thread: t(), item: None }),
        (cmd("queued-run.start", json!({"runId": "q1"})), Op::QueueResume { thread: t(), item: Some("q1".into()) }),
        (cmd("queued-run.reorder", json!({"runId": "q1", "beforeRunId": "q2"})), Op::QueueMove { thread: t(), item: "q1".into(), before: Some("q2".into()) }),
        (cmd("queued-run.edit", json!({"runId": "q1", "text": "sooner"})), Op::QueueEdit { thread: t(), item: "q1".into(), prompt: "sooner".into() }),
        (cmd("queued-run.cancel", json!({"runId": "q1"})), Op::QueueDelete { thread: t(), item: "q1".into() }),
        (cmd("queued-message.promote-to-steer", json!({"queuedRunId": "q1", "targetRunId": "r9"})), Op::TurnSteer { thread: t(), turn: Some("r9".into()), item: Some("q1".into()) }),
        (cmd("runtime-request.respond", json!({"requestId": "a1", "decision": "acceptForSession"})), Op::ApprovalAnswer { thread: t(), approval: "a1".into(), decision: "accept_for_session".into() }),
        (cmd("runtime-request.respond", json!({"requestId": "i1", "answers": {"q1": "yes", "q2": ["a", "b"]}})), Op::InputAnswer { thread: t(), request: "i1".into(), answer: "yes\na, b".into() }),
        (cmd("thread.user-input.dismiss", json!({"requestId": "i1"})), Op::InputDismiss { thread: t(), request: "i1".into() }),
        (cmd("checkpoint.rollback", json!({"checkpointId": "r1", "scopeId": "s", "restoreFiles": false})), Op::ThreadRollback { thread: t(), turn: "r1".into(), restore_files: false }),
        (cmd("provider-session.detach", json!({"providerSessionId": "s1"})), Op::ThreadDetach { thread: t(), session: "s1".into() }),
        (cmd("subagent.stop", json!({"runId": "r1", "subagentId": "n1"})), Op::WorkflowStop { thread: t(), turn: "r1".into(), workflow: Some("n1".into()) }),
        (json!({"type": "thread.fork", "commandId": "c-fork", "sourceThreadId": "t1", "targetThreadId": "f1", "sourcePoint": {}}), Op::ThreadFork { thread: t(), fork: "f1".into(), turn: None, title: None }),
        (json!({"type": "thread.merge_back", "commandId": "c-merge", "sourceThreadId": "f1", "targetThreadId": "t1", "sourcePoint": {}}), Op::ThreadMergeBack { thread: "f1".into(), into: t(), turn: None }),
        (cmd("thread.delete", json!({"onlyIfUnused": true})), Op::ThreadDelete { thread: t(), only_if_unused: true }),
    ];
    for (command, expected) in table {
        let before = ops(&h).len();
        // When the command is dispatched
        let kind = command["type"].as_str().unwrap().to_string();
        success(&dispatch(&mut rpc, command));
        // Then the agent API was asked for exactly the request the mapping names, after making sure it has the thread
        let asked: Vec<Op> = ops(&h)[before..].to_vec();
        assert_eq!(asked.last(), Some(&expected), "{kind}");
    }
    // And the rows carry what was accepted: the fork is a thread of the family, the archive was undone and the delete removed the thread
    let fork = row(&mut rpc, "f1");
    assert_eq!((fork["lineage"]["parentThreadId"].as_str(), fork["lineage"]["relationshipToParent"].as_str()), (Some("t1"), Some("fork")));
    let gone = rpc.call("orchestration.getThreadProjection", json!({"threadId": "t1"}));
    assert_eq!(failure(&gone)["_tag"], "NotFoundError");
}

#[test]
fn a_settle_that_was_accepted_marks_the_row_and_one_the_agent_api_refused_is_its_own_message_and_changes_nothing() {
    // Given the contract's recorded refusal of an archive of an archived thread, and a thread
    let (h, mut rpc) = started();
    let recorded = agent_api_agent::testing::group_fixtures("lifecycle");
    let refusal = recorded.iter().find(|f| f.method == "thread.archive").and_then(|f| f.exchanges.iter().find(|e| !e.succeeds())).unwrap();
    let message = refusal.response["error"]["message"].as_str().unwrap().to_string();
    // When the thread is settled, then archived while the agent API refuses with that message
    success(&dispatch(&mut rpc, cmd("thread.settle", json!({}))));
    assert_eq!(row(&mut rpc, "t1")["settledOverride"], "settled");
    h.agent.replies.lock().unwrap().insert("thread.archive", Err(AgentError::Failed(message.clone())));
    let sequence = h.service.engine().read(|s| s.last_sequence);
    let refused = dispatch(&mut rpc, cmd("thread.archive", json!({})));
    // Then T3 answers a dispatch error with that message, and no event was recorded
    assert_eq!(failure(&refused)["message"], message.as_str());
    assert_eq!((h.service.engine().read(|s| s.last_sequence), row(&mut rpc, "t1")["archivedAt"].is_null()), (sequence, true));
}

#[test]
fn a_queued_message_sent_while_a_turn_runs_is_refused_with_the_contracts_words_and_a_retry_of_a_prepared_run_never_reaches_the_agent_api() {
    // Given a thread whose agent API refuses to start a queued message now, as the queue contract words it
    let (h, mut rpc) = started();
    h.agent.replies.lock().unwrap().insert("queue.resume", Err(AgentError::Failed(queue::message::NOT_STOPPED.into())));
    // When the queued run is started and a prepared run is retried
    let start = dispatch(&mut rpc, cmd("queued-run.start", json!({"runId": "q1"})));
    let before = ops(&h).len();
    let retry = dispatch(&mut rpc, cmd("prepared-run.retry", json!({"runId": "r7"})));
    // Then the first is T3's refusal in the contract's words and the second T3's own, for which the agent API was not asked
    assert_eq!(failure(&start)["message"], queue::message::NOT_STOPPED);
    assert_eq!(failure(&retry)["message"], "Run r7 has no failed workspace preparation to retry.");
    assert_eq!(ops(&h).len(), before);
}

#[test]
fn a_delete_the_agent_api_kept_because_the_thread_was_used_leaves_the_thread_in_place() {
    // Given a thread the agent API says was used
    let (h, mut rpc) = started();
    h.agent.replies.lock().unwrap().insert("thread.delete", Ok(json!({"thread": "t1", "deleted": false})));
    // When it is deleted only if unused
    let sequence = success(&dispatch(&mut rpc, cmd("thread.delete", json!({"onlyIfUnused": true}))))["sequence"].as_u64().unwrap();
    // Then the command succeeded at the current sequence and the thread is still there
    assert_eq!(sequence, h.service.engine().read(|s| s.last_sequence));
    assert_eq!(row(&mut rpc, "t1")["id"], "t1");
}

#[test]
fn the_device_state_commands_are_kept_in_the_agent_apis_state_by_what_changed() {
    // Given a thread
    let (h, mut rpc) = started();
    let cases = [
        (cmd("thread.pin", json!({"orderKey": "k1"})), "thread.pins:t1", "pinOrderKey", json!("k1")),
        (cmd("thread.pin.reorder", json!({"orderKey": "k2"})), "thread.pins:t1", "pinOrderKey", json!("k2")),
        (cmd("thread.active.reorder", json!({"orderKey": "k3"})), "thread.activeOrder:t1", "orderKey", json!("k3")),
        (cmd("thread.auto-settle.set", json!({"enabled": true})), "thread.autoSettle:t1", "autoSettleDisabledAt", Value::Null),
        (cmd("thread.metadata.update", json!({"title": "Renamed"})), "thread.settings:t1", "title", json!("Renamed")),
        (cmd("thread.runtime-mode.set", json!({"runtimeMode": "approval-required"})), "thread.settings:t1", "runtimeMode", json!("approval-required")),
        (cmd("thread.interaction-mode.set", json!({"interactionMode": "plan"})), "thread.settings:t1", "interactionMode", json!("plan")),
        (cmd("thread.model-selection.set", json!({"modelSelection": {"instanceId": "opencode", "model": "m2"}})), "thread.settings:t1", "modelSelection", json!({"instanceId": "opencode", "model": "m2"})),
        (cmd("thread.mark-unread", json!({})), "thread.visits:t1", "lastVisitedAt", Value::Null),
    ];
    for (command, key, field, value) in cases {
        let kind = command["type"].as_str().unwrap().to_string();
        // When the command is dispatched
        success(&dispatch(&mut rpc, command));
        // Then the newest state the agent API was given is under the key of its kind, scoped to t3, and holds the value the row now has
        let given = h.agent.ops.lock().unwrap().iter().rev().find_map(|o| match o {
            Op::StateSet { scope, key: k, value } if k == key => Some((scope.clone(), value.clone())),
            _ => None,
        });
        let (scope, kept) = given.unwrap_or_else(|| panic!("{kind} kept nothing under {key}"));
        assert_eq!((scope.as_str(), &kept[field]), ("t3", &value), "{kind}");
    }
}

#[test]
fn search_family_and_the_shelf_answer_t3s_shapes_from_what_the_agent_api_found() {
    // Given threads t1 and a fork f1, and an agent API that finds t1 by title and by a message, knows the family and holds both on the shelf
    let (h, mut rpc) = started();
    success(&dispatch(&mut rpc, json!({"type": "thread.fork", "commandId": "c-fork", "sourceThreadId": "t1", "targetThreadId": "f1", "sourcePoint": {}})));
    let hits = SearchResult {
        matches: vec![
            SearchHit { thread: "t1".into(), title: "Thread t1".into(), source: HitSource::Title, snippet: None, at_ms: None },
            SearchHit { thread: "f1".into(), title: "Thread t1 fork".into(), source: HitSource::Assistant, snippet: Some("the needle".into()), at_ms: Some(1_793_491_200_000) },
            SearchHit { thread: "gone".into(), title: "unknown to t3".into(), source: HitSource::User, snippet: None, at_ms: None },
        ],
    };
    let family = FamilyResult { root: Some("t1".into()), threads: vec![state_of("t1"), state_of("f1")] };
    h.agent.replies.lock().unwrap().insert("thread.search", Ok(serde_json::to_value(hits).unwrap()));
    h.agent.replies.lock().unwrap().insert("thread.family", Ok(serde_json::to_value(family).unwrap()));
    // When the client searches and asks for the family
    let found = rpc.call("orchestration.searchThreads", json!({"query": "needle", "limit": 5}));
    let fam = rpc.call("orchestration.getThreadFamily", json!({"threadId": "f1"}));
    // Then the matches carry the project of each thread known to t3 (a title match as a user match on the title), and the family is their shell rows
    let matches = success(&found)["matches"].as_array().unwrap();
    assert_eq!(matches.iter().map(|m| (m["threadId"].as_str().unwrap(), m["projectId"].as_str().unwrap(), m["source"].as_str().unwrap(), m["snippet"].as_str().unwrap())).collect::<Vec<_>>(), [("t1", "p1", "user", "Thread t1"), ("f1", "p1", "assistant", "the needle")]);
    assert_eq!(matches[1]["messageCreatedAt"], "2026-11-01T00:00:00.000Z");
    assert!(ops(&h).contains(&Op::ThreadSearch { query: "needle".into(), limit: 5 }));
    assert_eq!(success(&fam)["threads"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect::<Vec<_>>(), ["t1", "f1"]);
    // And an unknown thread has an empty family without the agent API being asked, and a one letter query is a decode defect
    let before = ops(&h).len();
    assert_eq!(success(&rpc.call("orchestration.getThreadFamily", json!({"threadId": "nobody"})))["threads"], json!([]));
    assert_eq!(ops(&h).len(), before);
    assert!(rpc.call("orchestration.searchThreads", json!({"query": "x"}))["cause"][0]["defect"].is_string());
}

#[test]
fn older_settled_threads_keep_paging_the_shelf_until_the_limit_of_the_asked_projects_is_met_and_a_limit_of_zero_only_counts() {
    // Given 56 settled threads in two projects, and a shelf that answers 7 of them at a time, newest settle first
    let (h, mut rpc) = started();
    success(&rpc.call("projects.mutate", project_create("c-p2", "p2")));
    let ids: Vec<(String, u64)> = (0..56)
        .map(|i| {
            let id = format!("s{i:02}");
            let project = if i % 2 == 0 { "p1" } else { "p2" };
            success(&dispatch(&mut rpc, thread_create(&format!("c-{id}"), &id, project)));
            (id, 1_700_000_000_000 + (i as u64) * 1000)
        })
        .collect();
    let mut shelf = ids.clone();
    shelf.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let pages = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = pages.clone();
    *h.agent.responder.lock().unwrap() = Some(Box::new(move |op| {
        let Op::ThreadShelf { shelf: name, before, limit } = op else { return None };
        assert_eq!(*name, "settled");
        counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let after: Vec<&(String, u64)> = shelf.iter().filter(|(id, at)| before.as_ref().is_none_or(|(b, who)| at < b || (at == b && who.as_ref().is_some_and(|w| id > w)))).collect();
        let take = (*limit).min(7) as usize;
        let page: Vec<ThreadState> = after.iter().take(take).map(|(id, _)| state_of(id)).collect();
        let next = (after.len() > take).then(|| after[take - 1]).map(|(id, at)| lifecycle::Cursor { at_ms: *at, thread: Some(id.clone()) });
        Some(Ok(serde_json::to_value(ShelfResult { shelf: Shelf::Settled, threads: page, next, total: after.len() as u32 }).unwrap()))
    }));
    let before = json!({"settledAt": "2030-01-01T00:00:00.000Z"});
    // When 20 older settled threads of project p1 are asked for, and then only the counts
    let page = rpc.call("orchestration.getOlderSettledThreads", json!({"before": before, "projectIds": ["p1"], "limit": 20}));
    let used = pages.load(std::sync::atomic::Ordering::SeqCst);
    let counts = rpc.call("orchestration.getOlderSettledThreads", json!({"before": before, "limit": 0}));
    // Then exactly 20 rows of p1 came back, newest settle first, found across several pages, and the counts are the 28 of each project
    let rows: Vec<&str> = success(&page)["threads"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect();
    let expected: Vec<&str> = shelf_ids(&ids).into_iter().filter(|id| ids.iter().position(|(i, _)| i == id).unwrap() % 2 == 0).take(20).collect();
    assert_eq!(rows, expected);
    assert!(used > 3, "{used} pages");
    let mut counted: Vec<(&str, u64)> = success(&counts)["counts"].as_array().unwrap().iter().map(|c| (c["projectId"].as_str().unwrap(), c["count"].as_u64().unwrap())).collect();
    counted.sort_unstable();
    assert_eq!(counted, [("p1", 28), ("p2", 28)]);
}

fn shelf_ids(ids: &[(String, u64)]) -> Vec<&str> {
    let mut sorted: Vec<&(String, u64)> = ids.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    sorted.into_iter().map(|(id, _)| id.as_str()).collect()
}

/// A thread whose one turn ran a command and said something.
fn with_turn() -> (Harness, Rpc, Value) {
    let (h, mut rpc) = started();
    *h.agent.script.lock().unwrap() = vec![
        AgentEventKind::ToolStarted { id: "k".into(), name: "bash".into(), input: json!({"command": "ls"}) },
        AgentEventKind::ToolOutput { id: "k".into(), data: "a\nb\n".into() },
        AgentEventKind::ToolFinished { id: "k".into(), ok: true },
        AgentEventKind::Text("done".into()),
    ];
    success(&dispatch(&mut rpc, json!({"type": "message.dispatch", "commandId": "c-m", "threadId": "t1", "message": {"messageId": "m1", "text": "go"}})));
    let projection = settled(&mut rpc, "t1");
    (h, rpc, projection)
}

#[test]
fn a_turn_item_is_read_from_the_rows_and_a_commands_output_comes_as_one_finished_replace_frame() {
    // Given a thread that ran a command
    let (_h, mut rpc, projection) = with_turn();
    let tool = projection["turnItems"].as_array().unwrap().iter().find(|i| i["type"] == "command_execution").unwrap().clone();
    let id = tool["id"].as_str().unwrap();
    // When the item is fetched, a missing one is fetched, and the command's output is subscribed to
    let fetched = rpc.call("orchestration.getTurnItem", json!({"threadId": "t1", "itemId": id}));
    let missing = rpc.call("orchestration.getTurnItem", json!({"threadId": "t1", "itemId": "nope"}));
    let stream = rpc.send("orchestration.subscribeCommandOutput", json!({"threadId": "t1", "itemId": id}));
    let frames = rpc.chunk(&stream);
    let unknown = rpc.call("orchestration.subscribeCommandOutput", json!({"threadId": "t1", "itemId": "nope"}));
    // Then the item is the row, the missing one is null, the output is exactly what the command printed with running false, and an unknown command fails
    assert_eq!(success(&fetched)["item"], tool);
    assert_eq!(success(&missing)["item"], Value::Null);
    assert_eq!(frames, [json!({"kind": "replace", "text": "a\nb\n", "truncated": false, "running": false})]);
    assert_eq!(failure(&unknown)["_tag"], "OrchestrationV2CommandOutputError");
}

#[test]
fn a_force_stop_and_a_workflow_stop_name_the_turn_of_the_run_and_a_workflow_that_is_not_running_is_not_asked_for() {
    // Given a thread with a turn that runs until it is stopped
    let (h, mut rpc) = started();
    h.agent.hold.store(true, std::sync::atomic::Ordering::SeqCst);
    let quiet = rpc.call("orchestration.stopWorkflow", json!({"threadId": "t1", "subagentId": "n1"}));
    assert_eq!(failure(&quiet)["reason"], "not-running");
    assert!(ops(&h).is_empty());
    success(&dispatch(&mut rpc, json!({"type": "message.dispatch", "commandId": "c-m", "threadId": "t1", "message": {"messageId": "m1", "text": "go"}})));
    let run = h.service.engine().active_run("t1").unwrap();
    // The turn the run is gets recorded once the agent has started it, so wait for that before naming it
    let recorded = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < recorded && !h.store.get(&format!("agent-run:{run}")).unwrap().is_some_and(|t| t == "u1") {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    h.agent.replies.lock().unwrap().insert("thread.forceStop", Ok(json!({"thread": "t1", "turn": "u1", "outcome": "shared", "shared_with": ["t2"], "seq": 4})));
    // When the workflow is stopped and the thread is force stopped for its run
    success(&rpc.call("orchestration.stopWorkflow", json!({"threadId": "t1", "subagentId": "n1"})));
    let stopped = rpc.call("orchestration.forceStopThread", json!({"threadId": "t1", "runId": run}));
    // Then the agent API was asked about the turn the run is, and the result is T3's
    assert_eq!(ops(&h), [Op::WorkflowStop { thread: "t1".into(), turn: "u1".into(), workflow: Some("n1".into()) }, Op::ThreadForceStop { thread: "t1".into(), turn: Some("u1".into()) }]);
    assert_eq!(success(&stopped), &json!({"outcome": "shared", "sharedWithThreadIds": ["t2"]}));
}

#[test]
fn a_workflow_script_is_never_read_and_a_secret_answer_finds_no_request_and_is_sent_nowhere() {
    // Given a thread
    let (h, mut rpc) = started();
    // When scripts of each kind of path are asked for, and secrets are answered, one malformed
    let absolute = rpc.call("orchestration.getWorkflowScript", json!({"threadId": "t1", "scriptPath": "/home/u/.claude/projects/x/wf.js"}));
    let relative = rpc.call("orchestration.getWorkflowScript", json!({"threadId": "t1", "scriptPath": "wf.js"}));
    let wrong = rpc.call("orchestration.getWorkflowScript", json!({"threadId": "t1", "scriptPath": "/etc/passwd"}));
    let saved = rpc.call("secrets.answerRequest", json!({"threadId": "t1", "turnItemId": "i1", "answer": {"type": "save", "secret": "hunter2"}}));
    let declined = rpc.call("secrets.answerRequest", json!({"threadId": "t1", "turnItemId": "i1", "answer": {"type": "decline"}}));
    let malformed = rpc.call("secrets.answerRequest", json!({"threadId": "t1", "turnItemId": "i1", "answer": {"type": "save"}}));
    // Then a .js path has no root, anything else is an invalid path, the secret has no request, a malformed answer is a defect, and nothing reached the agent API
    assert_eq!([&absolute, &relative, &wrong].map(|e| failure(e)["reason"].as_str().unwrap().to_string()), ["root-unavailable", "invalid-path", "invalid-path"]);
    assert_eq!([&saved, &declined].map(|e| failure(e)["reason"].as_str().unwrap().to_string()), ["not_found", "not_found"]);
    assert!(malformed["cause"][0]["defect"].is_string());
    assert!(ops(&h).is_empty());
    assert!(!h.log.lines().iter().any(|l| l.contains("hunter2")));
}

#[test]
fn the_archived_shell_and_the_two_snapshots_a_phone_fetches_over_http_read_the_rows() {
    // Given an archived thread t1 and a live one t2
    let (h, mut rpc) = started();
    success(&dispatch(&mut rpc, thread_create("c-t2", "t2", "p1")));
    success(&dispatch(&mut rpc, cmd("thread.archive", json!({}))));
    let bearer = bearer_of(h.addr, &h.pair(PairingRequest::default()).code);
    let auth = format!("Bearer {bearer}");
    let get = |path: &str| http(h.addr, "GET", path, &[("Authorization", auth.as_str()), ("x-t3-orchestration-protocol", "2")], "");
    // When the archived snapshot, the shell snapshot and a bounded thread snapshot are read
    let archived = rpc.call("orchestration.getArchivedShellSnapshot", json!({}));
    let shell = get("/api/orchestration/shell");
    let bounded = get("/api/orchestration/threads/t2/bounded");
    let missing = get("/api/orchestration/threads/nobody/bounded");
    let ungated = http(h.addr, "GET", "/api/orchestration/shell", &[("x-t3-orchestration-protocol", "2")], "");
    // Then the archived snapshot holds only t1, the shell lists t2 live and t1 archived at the same sequence, the bounded snapshot is t2's whole projection, and the gate holds
    let ids = |v: &Value| v.as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!(ids(&success(&archived)["threads"]), ["t1"]);
    assert_eq!((ids(&shell.body["threads"]), ids(&shell.body["archivedThreads"])), (vec!["t2".to_string()], vec!["t1".to_string()]));
    assert_eq!(shell.body["snapshotSequence"], success(&archived)["snapshotSequence"]);
    assert_eq!((bounded.status, bounded.body["projection"]["thread"]["id"].as_str(), bounded.body["hasMoreHistory"].as_bool()), (200, Some("t2"), Some(false)));
    assert_eq!((missing.status, ungated.status), (404, 401));
}
