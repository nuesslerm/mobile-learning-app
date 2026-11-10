//! Orchestration V2 through the socket: thread commands and their events, a turn from message to finished run on a scripted
//! agent API double, interrupts and failures, the file RPCs, and the declared-unsupported table.
use super::support::*;
use std::time::Duration;
use t3_core::ports::AgentEventKind;

const QUIET: Duration = Duration::from_millis(300);

thread_local! {
    /// Chunks that arrived while a command waited for its answer.
    static KEPT: std::cell::RefCell<Vec<Value>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Every frame that arrives until the socket is quiet, acknowledged (and the chunks a command call set aside).
fn drain(rpc: &mut Rpc) -> Vec<Value> {
    let mut frames = KEPT.with(|k| std::mem::take(&mut *k.borrow_mut()));
    while let Some(frame) = rpc.try_recv(QUIET) {
        if frame["_tag"] == "Chunk" {
            rpc.ack(&frame["requestId"]);
        }
        frames.push(frame);
    }
    frames
}

fn values_of(frames: &[Value], stream: &Value) -> Vec<Value> {
    frames.iter().filter(|f| f["_tag"] == "Chunk" && &f["requestId"] == stream).flat_map(|f| f["values"].as_array().unwrap().clone()).collect()
}

fn events_of(values: &[Value]) -> Vec<(String, Value)> {
    values.iter().filter(|v| v["kind"] == "event").map(|v| (v["event"]["type"].as_str().unwrap().to_string(), v["event"]["payload"].clone())).collect()
}

fn dispatch(rpc: &mut Rpc, command: Value) -> Value {
    let id = rpc.send("orchestration.dispatchCommand", command);
    loop {
        let frame = rpc.recv();
        if frame["_tag"] == "Chunk" {
            rpc.ack(&frame["requestId"]);
            KEPT.with(|k| k.borrow_mut().push(frame));
        } else if frame["requestId"] == id {
            return frame["exit"].clone();
        }
    }
}

fn message(command_id: &str, text: &str) -> Value {
    json!({"type": "message.dispatch", "commandId": command_id, "threadId": "t1", "message": {"messageId": format!("m-{command_id}"), "text": text}})
}

fn started() -> (Harness, Rpc, Value) {
    let h = Harness::start();
    let mut rpc = connect(&h);
    seed(&mut rpc);
    forget_seeding(&h);
    let thread = rpc.send("orchestration.subscribeThread", json!({"threadId": "t1"}));
    rpc.chunk(&thread);
    (h, rpc, thread)
}

fn projection(rpc: &mut Rpc) -> Value {
    success(&rpc.call("orchestration.getThreadProjection", json!({"threadId": "t1"}))).clone()
}

#[test]
fn a_thread_can_be_pinned_visited_and_have_its_modes_and_model_changed_each_as_an_event_with_the_new_row() {
    // Given a thread with its stream open
    let (_h, mut rpc, thread) = started();
    let commands = [
        json!({"type": "thread.pin", "commandId": "c1", "threadId": "t1"}),
        json!({"type": "thread.visit", "commandId": "c2", "threadId": "t1"}),
        json!({"type": "thread.runtime-mode.set", "commandId": "c3", "threadId": "t1", "runtimeMode": "approval-required"}),
        json!({"type": "thread.interaction-mode.set", "commandId": "c4", "threadId": "t1", "interactionMode": "plan"}),
        json!({"type": "thread.model-selection.set", "commandId": "c5", "threadId": "t1", "modelSelection": {"instanceId": "claudeAgent", "model": "opus"}}),
        json!({"type": "thread.unpin", "commandId": "c6", "threadId": "t1"}),
    ];
    // When each is dispatched
    for c in commands {
        success(&dispatch(&mut rpc, c));
    }
    // Then the thread stream carried one event per command, each holding the row after the change
    let events = events_of(&values_of(&drain(&mut rpc), &thread));
    let kinds: Vec<&str> = events.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(kinds, ["thread.pinned", "thread.visited", "thread.runtime-mode-updated", "thread.interaction-mode-updated", "thread.model-selection-updated", "thread.unpinned"]);
    assert!(events[0].1["pinnedAt"].is_string() && events[1].1["lastVisitedAt"].is_string() && events[5].1["pinnedAt"].is_null());
    assert_eq!((events[2].1["runtimeMode"].as_str(), events[3].1["interactionMode"].as_str(), events[4].1["providerInstanceId"].as_str()), (Some("approval-required"), Some("plan"), Some("claudeAgent")));
}

#[test]
fn a_message_starts_a_turn_in_the_agent_api_and_the_thread_shows_the_streamed_text_a_tool_row_and_a_finished_run() {
    // Given an agent API whose turn answers with two text deltas around one tool call that prints twice
    let (h, mut rpc, thread) = started();
    *h.agent.script.lock().unwrap() = vec![
        AgentEventKind::Text("Hel".into()),
        AgentEventKind::ToolStarted { id: "k1".into(), name: "bash".into(), input: json!({"command": "ls"}) },
        AgentEventKind::ToolOutput { id: "k1".into(), data: "a.txt\n".into() },
        AgentEventKind::ToolOutput { id: "k1".into(), data: "b.txt\n".into() },
        AgentEventKind::ToolFinished { id: "k1".into(), ok: true },
        AgentEventKind::Text("lo".into()),
    ];
    // When a message is dispatched
    success(&dispatch(&mut rpc, message("c-m", "say hello")));
    let frames = drain(&mut rpc);
    // Then the thread was made in the agent API and its turn started with the prompt and the thread's instance, in that order
    let turn = h.agent.turns.lock().unwrap()[0].clone();
    assert_eq!((turn.thread_id.as_str(), turn.prompt.as_str(), turn.instance_id.as_str()), ("t1", "say hello", "opencode"));
    assert_eq!(h.agent.calls.lock().unwrap().as_slice(), ["thread.create", "turn.start", "thread.subscribe"]);
    // And the stream carried the user message, the run, the text, the tool and the end
    let events = events_of(&values_of(&frames, &thread));
    let kinds: Vec<&str> = events.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(kinds.first(), Some(&"message.updated"));
    assert!(kinds.contains(&"run.created") && kinds.last() == Some(&"run.updated"), "{kinds:?}");
    // And the projection holds both messages, the three items and a completed run
    let p = projection(&mut rpc);
    let texts: Vec<(&str, &str)> = p["messages"].as_array().unwrap().iter().map(|m| (m["role"].as_str().unwrap(), m["text"].as_str().unwrap())).collect();
    assert_eq!(texts, [("user", "say hello"), ("assistant", "Hello")]);
    let items: Vec<(&str, &str)> = p["turnItems"].as_array().unwrap().iter().map(|i| (i["type"].as_str().unwrap(), i["status"].as_str().unwrap())).collect();
    assert_eq!(items, [("user_message", "completed"), ("assistant_message", "completed"), ("command_execution", "completed")]);
    let tool = p["turnItems"].as_array().unwrap().iter().find(|i| i["type"] == "command_execution").unwrap();
    assert_eq!((tool["name"].as_str(), tool["output"].as_str()), (Some("bash"), Some("a.txt\nb.txt\n")));
    assert_eq!((p["runs"][0]["status"].as_str(), p["thread"]["archivedAt"].clone()), (Some("completed"), Value::Null));
    // And the shell row shows an idle thread whose latest run completed
    let shell = rpc.send("orchestration.subscribeShell", json!({}));
    let row = rpc.chunk(&shell)[0]["snapshot"]["threads"][0].clone();
    assert_eq!((row["status"].as_str(), row["activeRunId"].clone(), row["itemCount"].as_i64()), (Some("idle"), Value::Null, Some(3)));
}

#[test]
fn a_second_message_is_refused_while_a_run_is_active_and_an_interrupt_ends_the_run_and_the_turn_in_the_agent_api() {
    // Given a turn the agent API keeps running
    let (h, mut rpc, _thread) = started();
    h.agent.hold.store(true, std::sync::atomic::Ordering::SeqCst);
    success(&dispatch(&mut rpc, message("c-1", "long")));
    drain(&mut rpc);
    // When another message arrives, then the run is interrupted
    let second = dispatch(&mut rpc, message("c-2", "again"));
    success(&dispatch(&mut rpc, json!({"type": "run.interrupt", "commandId": "c-3", "threadId": "t1"})));
    drain(&mut rpc);
    // Then the second is a dispatch error naming the active run, the agent API was told to stop the turn, and the run is interrupted
    assert!(failure(&second)["message"].as_str().unwrap().contains("active run"));
    assert_eq!(h.agent.interrupted.lock().unwrap().as_slice(), ["t1"]);
    let p = projection(&mut rpc);
    assert_eq!((p["runs"][0]["status"].as_str(), p["runs"][0]["stopRequested"].clone()), (Some("interrupted"), json!(true)));
    // And an interrupt with nothing running is refused
    assert!(failure(&dispatch(&mut rpc, json!({"type": "thread.stop", "commandId": "c-4", "threadId": "t1"})))["message"].as_str().unwrap().contains("no active run"));
}

#[test]
fn a_provider_that_fails_leaves_a_failed_run_with_the_error_in_the_thread() {
    // Given an agent API whose turn fails after some text
    let (h, mut rpc, _thread) = started();
    *h.agent.script.lock().unwrap() = vec![AgentEventKind::Text("partial".into())];
    *h.agent.fail.lock().unwrap() = Some("provider opencode is not reachable".into());
    // When a message is dispatched
    success(&dispatch(&mut rpc, message("c-f", "go")));
    drain(&mut rpc);
    // Then the run is failed with the message, an error item exists, and the thread is idle again
    let p = projection(&mut rpc);
    assert_eq!((p["runs"][0]["status"].as_str(), p["runs"][0]["error"].as_str()), (Some("failed"), Some("provider opencode is not reachable")));
    assert!(p["turnItems"].as_array().unwrap().iter().any(|i| i["type"] == "error" && i["message"] == "provider opencode is not reachable"));
    success(&dispatch(&mut rpc, message("c-g", "retry")));
    drain(&mut rpc);
}

#[test]
fn undecodable_commands_are_defects_and_commands_the_server_does_not_build_are_typed_refusals() {
    let (_h, mut rpc, _thread) = started();
    // When a command misses its id, names an unknown type, and one is a real type this server does not build; and a queue mode
    let missing = dispatch(&mut rpc, json!({"type": "message.dispatch"}));
    let unknown = dispatch(&mut rpc, json!({"type": "conformance.noSuch", "commandId": "x"}));
    let settle = dispatch(&mut rpc, json!({"type": "thread.pull-request.sync", "commandId": "c-s", "threadId": "t1"}));
    let queued = dispatch(&mut rpc, json!({"type": "message.dispatch", "commandId": "c-q", "threadId": "t1", "dispatchMode": "queue_after_active", "message": {"text": "x"}}));
    let empty = dispatch(&mut rpc, json!({"type": "message.dispatch", "commandId": "c-e", "threadId": "t1", "message": {"text": ""}}));
    // Then the first two are decoder defects, the others dispatch errors that say what is unsupported or missing
    assert_eq!(missing["cause"][0]["defect"], "Missing key\n  at [\"commandId\"]");
    assert!(unknown["cause"][0]["defect"].as_str().unwrap().starts_with("Expected { readonly \"type\": \"thread.stop\""));
    assert!(failure(&settle)["message"].as_str().unwrap().contains("thread.pull-request.sync is not supported"));
    assert!(failure(&queued)["message"].as_str().unwrap().contains("queue_after_active"));
    assert!(failure(&empty)["message"].as_str().unwrap().contains("text"));
}

#[test]
fn the_file_rpcs_list_and_read_through_the_agent_api_under_the_project_and_a_file_that_is_not_there_is_a_typed_error() {
    // Given an agent API with two files under the project's directory
    let h = Harness::start();
    let mut rpc = connect(&h);
    seed(&mut rpc);
    h.agent.files.lock().unwrap().extend([("/workspace/demo/README.md".to_string(), "# demo".to_string()), ("/workspace/demo/src/main.rs".to_string(), "fn main() {}".to_string())]);
    // When a client lists the root and src, and reads a file and one that is not there
    let root = rpc.call("projects.listEntries", json!({"projectId": "p1"}));
    let src = rpc.call("projects.listEntries", json!({"projectId": "p1", "relativePath": "src"}));
    let read = rpc.call("projects.readFile", json!({"projectId": "p1", "relativePath": "README.md"}));
    let missing = rpc.call("projects.readFile", json!({"projectId": "p1", "relativePath": "nope"}));
    // Then the entries are paths with kinds, the read returns the contents, and a miss is a typed error
    let paths: Vec<(&str, &str)> = success(&root)["entries"].as_array().unwrap().iter().map(|e| (e["path"].as_str().unwrap(), e["kind"].as_str().unwrap())).collect();
    assert_eq!(paths, [("README.md", "file"), ("src", "directory")]);
    assert_eq!(success(&src)["entries"][0]["path"], "src/main.rs");
    assert_eq!(success(&read)["contents"], "# demo");
    assert_eq!(failure(&missing)["_tag"], "ExecutionHostError");
}

#[test]
fn every_rpc_of_the_scope_table_this_server_does_not_build_is_a_typed_unsupported_error_naming_its_tag() {
    // Given the RPC tags of the scope table, minus the ones implemented (derived, not copied)
    let h = Harness::start();
    let mut rpc = connect_with(&h, PairingRequest { scopes: t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect(), ..PairingRequest::default() });
    let unsupported: Vec<&str> = t3_contracts::scopes::known_tags().into_iter().filter(|t| !t3_contracts::rpc::IMPLEMENTED_RPCS.contains(t)).collect();
    assert!(unsupported.len() > 30);
    // When each is called with an empty payload
    for tag in unsupported.iter().copied() {
        let exit = rpc.call(tag, json!({}));
        // Then it fails with the typed error and the tag, never a closed socket
        let error = failure(&exit);
        assert_eq!((error["_tag"].as_str(), error["method"].as_str()), (Some("UnsupportedOperationError"), Some(tag)), "{tag}");
    }
    assert_eq!(success(&rpc.call("server.probe", json!({}))), &json!({}));
}
