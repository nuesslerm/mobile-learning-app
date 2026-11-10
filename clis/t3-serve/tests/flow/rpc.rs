//! The RPC session over a real socket: connect order, streams and their pacing, resume, idempotency, scopes and the refusals
//! that must leave the socket alive.
use super::support::*;
use std::time::Duration;

const QUIET: Duration = Duration::from_millis(300);

fn tag_of(error: &Value) -> &str {
    error["_tag"].as_str().unwrap_or("")
}

#[test]
fn subscribe_server_config_comes_first_with_the_descriptors_environment_and_the_catalog_then_probe_answers() {
    // Given a paired session
    let h = Harness::start();
    let mut rpc = connect(&h);
    let descriptor = http(h.addr, "GET", "/.well-known/t3/environment", &[], "").body;

    // When the client opens subscribeServerConfig and then probes
    let id = rpc.send("subscribeServerConfig", json!({}));
    let first = rpc.chunk(&id);

    // Then the first value is a snapshot whose environment id equals the descriptor's, with the one rca provider and its models
    assert_eq!(first[0]["type"], "snapshot");
    let config = &first[0]["config"];
    assert_eq!(config["environment"]["environmentId"], descriptor["environmentId"]);
    assert_eq!((config["providers"][0]["instanceId"].as_str(), config["providers"][0]["models"][0]["slug"].as_str()), (Some("opencode"), Some("rca-default")));
    // And the probe answers on the same socket while the stream stays open
    assert_eq!(success(&rpc.call("server.probe", json!({}))), &json!({}));
    assert_eq!(success(&rpc.call("server.getConfig", json!({})))["environment"]["environmentId"], descriptor["environmentId"]);
}

#[test]
fn the_lifecycle_stream_says_welcome_then_ready_in_sequence() {
    let h = Harness::start();
    let mut rpc = connect(&h);
    let id = rpc.send("subscribeServerLifecycle", json!({}));
    let frames = rpc.chunk(&id);
    let order: Vec<(&str, i64)> = frames.iter().map(|f| (f["type"].as_str().unwrap(), f["sequence"].as_i64().unwrap())).collect();
    assert_eq!(order, [("welcome", 1), ("ready", 2)]);
    assert!(frames.iter().all(|f| f["version"] == 1));
}

#[test]
fn a_created_project_and_thread_appear_in_the_shell_snapshot_and_the_thread_snapshot() {
    // Given a project and a thread created through the RPCs
    let h = Harness::start();
    let mut rpc = connect(&h);
    seed(&mut rpc);

    // When a client subscribes to the shell and to the thread
    let shell = rpc.send("orchestration.subscribeShell", json!({}));
    let first = rpc.chunk(&shell);
    let thread = rpc.send("orchestration.subscribeThread", json!({"threadId": "t1"}));
    let snap = rpc.chunk(&thread);

    // Then the shell snapshot holds both at sequence 2, and the thread snapshot is its projection
    assert_eq!(first[0]["kind"], "snapshot");
    let s = &first[0]["snapshot"];
    assert_eq!((s["snapshotSequence"].as_i64(), s["projects"][0]["id"].as_str(), s["threads"][0]["id"].as_str(), s["threads"][0]["projectId"].as_str()), (Some(2), Some("p1"), Some("t1"), Some("p1")));
    assert_eq!(snap[0]["kind"], "snapshot");
    assert_eq!((snap[0]["snapshotSequence"].as_i64(), snap[0]["projection"]["thread"]["title"].as_str()), (Some(2), Some("Thread t1")));
    assert_eq!(snap[0]["projection"]["messages"], json!([]));
}

#[test]
fn an_event_after_the_snapshot_reaches_both_open_streams_live() {
    // Given a client with the shell and the thread open after the snapshot
    let h = Harness::start();
    let mut watcher = connect(&h);
    seed(&mut watcher);
    let shell = watcher.send("orchestration.subscribeShell", json!({}));
    watcher.chunk(&shell);
    let thread = watcher.send("orchestration.subscribeThread", json!({"threadId": "t1"}));
    watcher.chunk(&thread);

    // When another client renames the thread
    let mut writer = connect(&h);
    let result = writer.call("orchestration.dispatchCommand", json!({"type": "thread.metadata.update", "commandId": "c-rename", "threadId": "t1", "title": "Renamed"}));
    assert_eq!(success(&result)["sequence"], 3);

    // Then the watcher gets the event on the thread stream and the updated thread on the shell stream, both at sequence 3
    let (mut event, mut update) = (vec![], vec![]);
    for _ in 0..2 {
        let frame = watcher.recv();
        watcher.ack(&frame["requestId"]);
        let values = frame["values"].as_array().unwrap().clone();
        if frame["requestId"] == thread { event = values } else { update = values }
    }
    assert_eq!((event[0]["kind"].as_str(), event[0]["sequence"].as_i64(), event[0]["event"]["type"].as_str()), (Some("event"), Some(3), Some("thread.metadata-updated")));
    assert_eq!((update[0]["kind"].as_str(), update[0]["sequence"].as_i64(), update[0]["thread"]["title"].as_str(), update[0]["location"].as_str()), (Some("thread.updated"), Some(3), Some("Renamed"), Some("active")));
}

#[test]
fn a_stream_sends_nothing_more_until_the_ack_and_then_batches_what_waited() {
    // Given a shell stream whose first chunk is not acknowledged
    let h = Harness::start();
    let mut rpc = connect(&h);
    seed(&mut rpc);
    let shell = rpc.send("orchestration.subscribeShell", json!({}));
    rpc.chunk_unacked(&shell);

    // When two events happen meanwhile
    for (i, thread) in ["t2", "t3"].iter().enumerate() {
        assert_eq!(success(&rpc.call("orchestration.dispatchCommand", thread_create(&format!("c-{thread}"), thread, "p1")))["sequence"], 3 + i as i64);
    }

    // Then nothing arrives until the ack, and after it one chunk carries both
    assert!(rpc.try_recv(QUIET).is_none());
    rpc.ack(&shell);
    let batch = rpc.chunk(&shell);
    assert_eq!(batch.iter().map(|f| f["sequence"].as_i64().unwrap()).collect::<Vec<_>>(), [3, 4]);
}

#[test]
fn resuming_after_a_sequence_replays_only_what_was_missed_and_can_mark_the_end() {
    // Given three events: a project, a thread, a rename
    let h = Harness::start();
    let mut rpc = connect(&h);
    seed(&mut rpc);
    rpc.call("orchestration.dispatchCommand", json!({"type": "thread.metadata.update", "commandId": "c-r", "threadId": "t1", "title": "Renamed"}));

    // When the shell resumes after sequence 1 with a completion marker, and the thread after sequence 2
    let shell = rpc.send("orchestration.subscribeShell", json!({"afterSequence": 1, "requestCompletionMarker": true}));
    let frames = rpc.chunk(&shell);
    let thread = rpc.send("orchestration.subscribeThread", json!({"threadId": "t1", "afterSequence": 2}));
    let events = rpc.chunk(&thread);

    // Then the shell replays sequences 2 and 3 (no snapshot) and ends with the marker, and the thread replays only sequence 3
    let kinds: Vec<(&str, Option<i64>)> = frames.iter().map(|f| (f["kind"].as_str().unwrap(), f["sequence"].as_i64())).collect();
    assert_eq!(kinds, [("thread.updated", Some(2)), ("thread.updated", Some(3)), ("synchronized", None)]);
    assert_eq!(events.iter().map(|f| (f["kind"].as_str().unwrap(), f["sequence"].as_i64().unwrap())).collect::<Vec<_>>(), [("event", 3)]);
}

#[test]
fn a_client_more_than_1000_events_behind_or_ahead_of_the_log_gets_a_fresh_snapshot() {
    // Given a log of 1002 events (project, thread, 1000 renames)
    let h = Harness::start();
    let mut rpc = connect(&h);
    seed(&mut rpc);
    for i in 0..1000 {
        let exit = rpc.call("orchestration.dispatchCommand", json!({"type": "thread.metadata.update", "commandId": format!("c-{i}"), "threadId": "t1", "title": format!("Title {i}")}));
        assert_eq!(success(&exit)["sequence"], 3 + i);
    }

    // When shells resume from sequence 1 (1001 behind), from 1 + 1000... the oldest inside the gap, and from beyond the log
    let far = rpc.send("orchestration.subscribeShell", json!({"afterSequence": 1}));
    let far_frames = rpc.chunk(&far);
    let near = rpc.send("orchestration.subscribeShell", json!({"afterSequence": 1002 - 3}));
    let near_frames = rpc.chunk(&near);
    let ahead = rpc.send("orchestration.subscribeShell", json!({"afterSequence": 5000}));
    let ahead_frames = rpc.chunk(&ahead);

    // Then the far and the ahead clients are sent snapshots at the log's end, and the near one only the last three updates
    for frames in [&far_frames, &ahead_frames] {
        assert_eq!((frames.len(), frames[0]["kind"].as_str(), frames[0]["snapshot"]["snapshotSequence"].as_i64()), (1, Some("snapshot"), Some(1002)));
    }
    assert_eq!(near_frames.iter().map(|f| f["sequence"].as_i64().unwrap()).collect::<Vec<_>>(), [1000, 1001, 1002]);
}

#[test]
fn a_command_id_dispatched_twice_acts_once_and_answers_the_same_sequence() {
    // Given a thread created with a command id
    let h = Harness::start();
    let mut rpc = connect(&h);
    seed(&mut rpc);

    // When the same command is sent again, and a different command id tries to create the same thread
    let again = rpc.call("orchestration.dispatchCommand", thread_create("c-t1", "t1", "p1"));
    let clash = rpc.call("orchestration.dispatchCommand", thread_create("c-other", "t1", "p1"));

    // Then the repeat answers the original sequence 2 without a new event, and the clash is a declared dispatch error
    assert_eq!(success(&again)["sequence"], 2);
    let error = failure(&clash);
    assert_eq!((tag_of(error), error["commandId"].as_str(), error["commandType"].as_str()), ("OrchestrationV2DispatchCommandError", Some("c-other"), Some("thread.create")));
    let shell = rpc.send("orchestration.subscribeShell", json!({}));
    let snapshot = &rpc.chunk(&shell)[0]["snapshot"];
    assert_eq!((snapshot["snapshotSequence"].as_i64(), snapshot["threads"].as_array().unwrap().len()), (Some(2), 1));
}

#[test]
fn archive_unarchive_and_delete_move_the_thread_between_the_shell_lists_and_out() {
    // Given a thread
    let h = Harness::start();
    let mut rpc = connect(&h);
    seed(&mut rpc);
    let shell = rpc.send("orchestration.subscribeShell", json!({}));
    rpc.chunk(&shell);

    // When it is archived, unarchived and deleted
    let mut frames = Vec::new();
    for (i, kind) in ["thread.archive", "thread.unarchive", "thread.delete"].iter().enumerate() {
        let exit = rpc.call("orchestration.dispatchCommand", json!({"type": kind, "commandId": format!("c-{i}"), "threadId": "t1"}));
        assert_eq!(success(&exit)["sequence"], 3 + i as i64);
        frames.extend(rpc.chunk(&shell));
    }

    // Then the shell saw it in the archive, back active, then removed
    let seen: Vec<(&str, Option<&str>)> = frames.iter().map(|f| (f["kind"].as_str().unwrap(), f["location"].as_str())).collect();
    assert_eq!(seen, [("thread.removed", Some("active")), ("thread.updated", Some("active")), ("thread.removed", Some("active"))]);
    // And its stream is a declared not-found now
    let thread = rpc.call("orchestration.subscribeThread", json!({"threadId": "t1"}));
    assert_eq!(tag_of(failure(&thread)), "NotFoundError");
    let again = rpc.call("orchestration.dispatchCommand", json!({"type": "thread.archive", "commandId": "c-9", "threadId": "t1"}));
    assert_eq!(tag_of(failure(&again)), "OrchestrationV2DispatchCommandError");
}

#[test]
fn a_project_can_be_created_updated_and_archived_and_a_thread_needs_a_live_project() {
    let h = Harness::start();
    let mut rpc = connect(&h);
    // Given a thread for a project that does not exist
    let orphan = rpc.call("orchestration.dispatchCommand", thread_create("c-o", "t9", "nope"));
    assert_eq!(tag_of(failure(&orphan)), "OrchestrationV2DispatchCommandError");

    // When a project is created, renamed and archived
    assert_eq!(success(&rpc.call("projects.mutate", project_create("c-1", "p1")))["id"], "p1");
    let shell = rpc.send("orchestration.subscribeShell", json!({}));
    rpc.chunk(&shell);
    rpc.call("projects.mutate", json!({"type": "project.update", "commandId": "c-2", "projectId": "p1", "title": "Renamed"}));
    let renamed = rpc.chunk(&shell);
    rpc.call("projects.mutate", json!({"type": "project.archive", "commandId": "c-3", "projectId": "p1"}));
    let archived = rpc.chunk(&shell);

    // Then the shell sees the rename as a project update and the archive as a removal
    assert_eq!((renamed[0]["kind"].as_str(), renamed[0]["project"]["title"].as_str()), (Some("project.updated"), Some("Renamed")));
    assert_eq!((archived[0]["kind"].as_str(), archived[0]["projectId"].as_str()), (Some("project.removed"), Some("p1")));
}

#[test]
fn a_project_with_threads_is_deleted_only_with_force_and_then_its_threads_go_with_it() {
    // Given a project with an active and an archived thread, an empty project, and the shell open
    let h = Harness::start();
    let mut rpc = connect(&h);
    seed(&mut rpc);
    rpc.call("orchestration.dispatchCommand", thread_create("c-t2", "t2", "p1"));
    rpc.call("orchestration.dispatchCommand", json!({"type": "thread.archive", "commandId": "c-a2", "threadId": "t2"}));
    success(&rpc.call("projects.mutate", project_create("c-p2", "p2")));
    let shell = rpc.send("orchestration.subscribeShell", json!({}));
    rpc.chunk(&shell);
    let delete = |command_id: &str, project: &str, force: Option<bool>| {
        let mut command = json!({"type": "project.delete", "commandId": command_id, "projectId": project});
        if let Some(force) = force {
            command["force"] = json!(force);
        }
        command
    };

    // When the busy project is deleted without force, then with it
    let refused = rpc.call("projects.mutate", delete("c-d1", "p1", None));
    let deleted = rpc.call("projects.mutate", delete("c-d2", "p1", Some(true)));
    let removed = rpc.chunk(&shell);

    // Then the first is the pin's not-empty error and wrote nothing, and the second answers the row with its deletion time
    let error = failure(&refused);
    assert_eq!((tag_of(error), error["commandId"].as_str(), error["message"].as_str()), ("ProjectMutationError", Some("c-d1"), Some("Project p1 is not empty.")));
    let row = success(&deleted);
    assert_eq!((row["id"].as_str(), row["deletedAt"].is_string()), (Some("p1"), true));
    // And the shell lost both threads, each from its list, and then the project
    let seen: Vec<(&str, &str)> = removed.iter().map(|f| (f["kind"].as_str().unwrap(), f["threadId"].as_str().or(f["projectId"].as_str()).unwrap())).collect();
    assert_eq!(seen, [("thread.removed", "t1"), ("thread.removed", "t2"), ("project.removed", "p1")]);
    assert_eq!(removed.iter().filter_map(|f| f["location"].as_str()).collect::<Vec<_>>(), ["active", "archive"]);
    // And a retry answers the same row, another delete of it is the generic mutation error, and no thread can join it
    assert_eq!(success(&rpc.call("projects.mutate", delete("c-d2", "p1", Some(true)))), row);
    let again = rpc.call("projects.mutate", delete("c-d3", "p1", Some(true)));
    assert_eq!((tag_of(failure(&again)), failure(&again)["message"].as_str()), ("ProjectMutationError", Some("Failed to mutate project.")));
    assert_eq!(tag_of(failure(&rpc.call("orchestration.dispatchCommand", thread_create("c-t3", "t3", "p1")))), "OrchestrationV2DispatchCommandError");
    // And the empty project needs no force
    assert_eq!(success(&rpc.call("projects.mutate", delete("c-d4", "p2", None)))["id"], "p2");
}

#[test]
fn an_rpc_the_server_does_not_build_answers_a_declared_failure_and_the_socket_lives_on() {
    // Given a session with the default scopes
    let h = Harness::start();
    let mut rpc = connect(&h);

    // When it calls an RPC of the mcp apps family, a stream RPC, unknown tags (one under a known family's prefix) and an unsupported command type
    let terminal = rpc.call("mcpApps.callTool", json!({}));
    let stream = rpc.call("mcpApps.readResource", json!({}));
    let nonsense = rpc.call("nonsense.method", json!({}));
    assert_eq!(nonsense["cause"][0]["defect"], "Unknown request tag: nonsense.method");
    let pull_request = rpc.call("pullRequests.noSuchMethod", json!({}));
    assert_eq!(pull_request["cause"][0]["defect"], "Unknown request tag: pullRequests.noSuchMethod");
    let settle = rpc.call("orchestration.dispatchCommand", json!({"type": "thread.pull-request.sync", "commandId": "c-s", "threadId": "t1"}));

    // Then each is a well-formed declared failure (not a dropped socket), and the session still answers
    assert_eq!((tag_of(failure(&terminal)), failure(&terminal)["method"].as_str()), ("UnsupportedOperationError", Some("mcpApps.callTool")));
    assert_eq!(tag_of(failure(&stream)), "UnsupportedOperationError");
    assert!(failure(&settle)["message"].as_str().unwrap().contains("thread.pull-request.sync"));
    assert_eq!(success(&rpc.call("server.probe", json!({}))), &json!({}));
    let log = h.log.lines();
    assert!(log.iter().any(|l| l == "rpc.unsupported tag=mcpApps.callTool known=true") && log.iter().any(|l| l == "rpc.unsupported tag=nonsense.method known=false"), "{log:?}");
}

#[test]
fn a_session_without_the_scope_of_an_rpc_is_denied_with_the_permission_it_lacks() {
    // Given a default session (no settings:write) and a read-only session
    let h = Harness::start();
    let mut default = connect(&h);
    let mut reader = connect_with(&h, PairingRequest { scopes: vec!["orchestration:read".into()], ..PairingRequest::default() });

    // When the default one updates settings and the read-only one dispatches a command and then reads the shell
    let settings = default.call("server.updateSettings", json!({"patch": {}}));
    let dispatch = reader.call("orchestration.dispatchCommand", project_create("c-1", "p1"));
    let project = reader.call("projects.mutate", project_create("c-1", "p1"));
    let shell = reader.send("orchestration.subscribeShell", json!({}));

    // Then each denial is an EnvironmentAuthorizationError naming the permission and its legacy parent, nothing was written, and the read works
    let denied = failure(&settings);
    assert_eq!((tag_of(denied), denied["requiredPermission"].as_str(), denied["requiredScope"].as_str()), ("EnvironmentAuthorizationError", Some("settings:write"), Some("orchestration:operate")));
    for exit in [&dispatch, &project] {
        assert_eq!((tag_of(failure(exit)), failure(exit)["requiredPermission"].as_str()), ("EnvironmentAuthorizationError", Some("orchestration:operate")));
    }
    assert_eq!(reader.chunk(&shell)[0]["snapshot"]["projects"], json!([]));
    assert!(h.log.lines().iter().any(|l| l.starts_with("rpc.denied tag=server.updateSettings scope=settings:write")));
}

#[test]
fn each_pull_request_rpc_the_phone_sends_is_known_and_needs_the_scope_the_pinned_server_requires() {
    // Given the phone's pull-request RPCs and their scopes, from the reference scan's checked-in list
    let list = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/plans/rca-parity/coverage/t3-methods.json")).unwrap();
    let list: Value = serde_json::from_str(&list).unwrap();
    let phone: std::collections::BTreeMap<&str, &str> = list["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["kind"] == "rpc" && r["phone"] == true && r["name"].as_str().is_some_and(|n| n.starts_with("pullRequests.")))
        .map(|r| (r["name"].as_str().unwrap(), r["scope"].as_str().unwrap_or("no scope recorded")))
        .collect();
    assert!(!phone.is_empty());
    // Then the scope table knows exactly those tags
    let mut known: Vec<&str> = t3_contracts::scopes::known_tags().into_iter().filter(|t| t.starts_with("pullRequests.")).collect();
    known.sort();
    assert_eq!(known, phone.keys().copied().collect::<Vec<_>>());

    // When each is sent by a session holding every scope but its own, and by one holding them all
    let h = Harness::start();
    let all: Vec<String> = t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect();
    let mut full = connect_with(&h, PairingRequest { scopes: all.clone(), ..PairingRequest::default() });
    for (tag, scope) in &phone {
        let mut lacking = connect_with(&h, PairingRequest { scopes: all.iter().filter(|s| s != scope).cloned().collect(), ..PairingRequest::default() });
        let denied = lacking.call(tag, json!({}));
        // Then the first is denied naming that scope, and the second is the typed unsupported error of a known tag
        assert_eq!((tag_of(failure(&denied)), failure(&denied)["requiredPermission"].as_str()), ("EnvironmentAuthorizationError", Some(*scope)), "{tag}");
        if !t3_contracts::rpc::IMPLEMENTED_RPCS.contains(tag) {
            let answered = full.call(tag, json!({}));
            assert_eq!((tag_of(failure(&answered)), failure(&answered)["method"].as_str()), ("UnsupportedOperationError", Some(*tag)), "{tag}");
        }
    }
}

#[test]
fn a_malformed_frame_a_ping_and_an_interrupt_are_each_answered_without_closing_the_socket() {
    // Given a session with the shell open
    let h = Harness::start();
    let mut rpc = connect(&h);
    seed(&mut rpc);
    let shell = rpc.send("orchestration.subscribeShell", json!({}));
    rpc.chunk(&shell);

    // When it sends garbage and a Ping
    rpc.send_raw("garbage");
    let protocol = rpc.recv();
    rpc.send_raw(r#"{"_tag":"Ping"}"#);
    let pong = rpc.recv();

    // Then it gets a ClientProtocolError and a Pong
    assert_eq!((protocol["_tag"].as_str(), pong["_tag"].as_str()), (Some("ClientProtocolError"), Some("Pong")));

    // And after an Interrupt of the shell stream a new event is not delivered on it
    rpc.send_raw(&json!({"_tag": "Interrupt", "requestId": shell}).to_string());
    let ended = rpc.recv();
    assert_eq!((ended["requestId"].clone(), ended["exit"]["cause"][0]["_tag"].clone()), (shell.clone(), json!("Interrupt")));
    rpc.call("orchestration.dispatchCommand", thread_create("c-t2", "t2", "p1"));
    assert!(rpc.try_recv(QUIET).is_none());
}
