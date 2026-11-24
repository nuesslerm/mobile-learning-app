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
    json!({"type": "message.dispatch", "createdBy": "user", "creationSource": "web", "commandId": command_id, "threadId": "t1", "messageId": format!("m-{command_id}"), "text": text})
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
        json!({"type": "thread.visit", "commandId": "c2", "threadId": "t1", "visitedAt": "2026-01-01T00:00:00Z"}),
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
    let streamed = events.iter().find(|(kind, row)| kind == "turn-item.updated" && row["type"] == "assistant_message").unwrap();
    assert_eq!((&streamed.1["status"], &streamed.1["streaming"]), (&json!("running"), &json!(true)));
    // And the projection holds both messages, the three items and a completed run
    let p = projection(&mut rpc);
    let texts: Vec<(&str, &str)> = p["messages"].as_array().unwrap().iter().map(|m| (m["role"].as_str().unwrap(), m["text"].as_str().unwrap())).collect();
    assert_eq!(texts, [("user", "say hello"), ("assistant", "Hello")]);
    let items: Vec<(&str, &str)> = p["turnItems"].as_array().unwrap().iter().map(|i| (i["type"].as_str().unwrap(), i["status"].as_str().unwrap())).collect();
    assert_eq!(items, [("user_message", "completed"), ("assistant_message", "completed"), ("command_execution", "completed")]);
    let tool = p["turnItems"].as_array().unwrap().iter().find(|i| i["type"] == "command_execution").unwrap();
    assert_eq!((tool["name"].as_str(), tool["output"].as_str()), (Some("bash"), Some("a.txt\nb.txt\n")));
    assert_eq!(tool["input"], "ls");
    assert_eq!(tool["nodeId"], p["messages"][1]["nodeId"]);
    assert_eq!(p["messages"][1]["streaming"], false);
    assert_eq!(p["messages"][1]["attachments"], json!([]));
    assert_eq!(p["messages"][1]["createdBy"], "agent");
    assert_eq!(p["messages"][1]["creationSource"], "provider");
    let assistant = events.iter().find(|(kind, row)| kind == "message.updated" && row["role"] == "assistant").unwrap();
    assert_eq!(assistant.1["createdBy"], "agent");
    assert_eq!(assistant.1["creationSource"], "provider");
    assert_eq!((p["runs"][0]["status"].as_str(), p["thread"]["archivedAt"].clone()), (Some("completed"), Value::Null));
    // And the shell row shows an idle thread whose latest run completed
    let shell = rpc.send("orchestration.subscribeShell", json!({}));
    let row = rpc.chunk(&shell)[0]["snapshot"]["threads"][0].clone();
    assert_eq!((row["status"].as_str(), row["activeRunId"].clone(), row["itemCount"].as_i64()), (Some("idle"), Value::Null, Some(3)));
    assert_eq!(row["latestVisibleMessage"], json!({
        "id": p["messages"][1]["id"],
        "role": "assistant",
        "text": "Hello",
        "updatedAt": p["messages"][1]["updatedAt"],
    }));
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
    success(&dispatch(&mut rpc, json!({"type": "run.interrupt", "commandId": "c-3", "threadId": "t1", "runId": "run:thread:t1:ordinal:1"})));
    drain(&mut rpc);
    // Then the second is a dispatch error naming the active run, the agent API was told to stop the turn, and the run is interrupted
    assert!(failure(&second)["message"].as_str().unwrap().contains("active run"));
    assert_eq!(h.agent.interrupted.lock().unwrap().as_slice(), ["t1"]);
    let p = projection(&mut rpc);
    assert_eq!((p["runs"][0]["status"].as_str(), h.service.engine().read(|s| s.threads["t1"]["latestRunStopRequested"].clone())), (Some("interrupted"), json!(true)));
    // And an interrupt with nothing running is refused, while a stop of an idle thread is a no-op the pin accepts
    assert!(failure(&dispatch(&mut rpc, json!({"type": "run.interrupt", "commandId": "c-4", "threadId": "t1", "runId": "r1"})))["message"].as_str().unwrap().contains("no active run"));
    success(&dispatch(&mut rpc, json!({"type": "thread.stop", "commandId": "c-5", "threadId": "t1"})));
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
    assert_eq!((p["runs"][0]["status"].as_str(), last_error(&h, "t1").as_deref()), (Some("failed"), Some("provider opencode is not reachable")));
    assert!(p["turnItems"].as_array().unwrap().iter().any(|i| i["type"] == "error" && i["message"] == "provider opencode is not reachable"));
    success(&dispatch(&mut rpc, message("c-g", "retry")));
    drain(&mut rpc);
}

#[test]
fn a_provider_that_cannot_open_ends_the_run_unstarted_with_the_open_failure_row_and_a_stop_after_it_adds_only_its_request() {
    // Given an agent API whose provider cannot be opened
    let (h, mut rpc, _thread) = started();
    *h.agent.start_error.lock().unwrap() = Some(t3_core::ports::AgentError::Failed("provider opencode is not reachable".into()));
    // When a message is dispatched, and then the thread is stopped and a queued message is sent now
    success(&dispatch(&mut rpc, message("c-o", "go")));
    drain(&mut rpc);
    let failed = projection(&mut rpc);
    success(&dispatch(&mut rpc, json!({"type": "thread.stop", "commandId": "c-stop", "threadId": "t1"})));
    let sent_now = dispatch(&mut rpc, json!({"type": "queued-run.start", "commandId": "c-qs", "threadId": "t1", "runId": "run:thread:t1:ordinal:2"}));
    drain(&mut rpc);
    let stopped = projection(&mut rpc);
    // Then the dispatch answered after the open failed: the run is failed with no start, its provider thread is not loaded under the shared session and no session is open
    let (run, thread) = (&failed["runs"][0], &failed["providerThreads"][0]);
    assert_eq!((run["status"].as_str(), run["startedAt"].is_null(), failed["attempts"][0]["status"].as_str(), failed["providerSessions"].as_array().map(Vec::len)), (Some("failed"), true, Some("failed"), Some(0)));
    assert_eq!((thread["status"].as_str(), thread["nativeThreadRef"].is_null(), thread["providerSessionId"].as_str()), (Some("not_loaded"), true, Some("provider-session:provider-instance:opencode:shared")));
    assert!(failed["thread"]["activeProviderThreadId"].is_string());
    assert_eq!(last_error(&h, "t1").as_deref(), Some("provider opencode is not reachable"));
    // And the failure row names the pin's failure, the stop only adds its request row to the failed run, and a queued message is not sent now after a failure
    let rows = |p: &Value| p["turnItems"].as_array().unwrap().iter().map(|i| (i["type"].as_str().unwrap().to_string(), i["status"].as_str().unwrap().to_string())).collect::<Vec<_>>();
    assert_eq!(failed["turnItems"][1]["failure"]["class"], "provider_error");
    assert_eq!(rows(&stopped), [("user_message".to_string(), "completed".to_string()), ("error".into(), "failed".into()), ("run_interrupt_request".into(), "completed".into())]);
    assert_eq!(stopped["runs"][0]["status"], "failed");
    assert_eq!(failure(&sent_now)["message"], "A queued user message can only be sent now while the thread is stopped.");
}

#[test]
fn a_start_deferred_message_waits_preparing_with_its_workspace_row_and_the_commands_of_a_run_that_is_not_preparing_are_refused_in_t3s_words() {
    // Given a thread whose first run failed
    let (h, mut rpc, _thread) = started();
    *h.agent.start_error.lock().unwrap() = Some(t3_core::ports::AgentError::Failed("provider opencode is not reachable".into()));
    success(&dispatch(&mut rpc, message("c-o", "go")));
    // When a message is dispatched with its start deferred and a prepared-run command names the failed run
    let mut deferred = message("c-d", "later");
    deferred["dispatchMode"] = json!({"type": "defer_start"});
    success(&dispatch(&mut rpc, deferred));
    drain(&mut rpc);
    let release = dispatch(&mut rpc, json!({"type": "prepared-run.release", "commandId": "c-r", "threadId": "t1", "runId": "run:thread:t1:ordinal:1"}));
    drain(&mut rpc);
    let p = projection(&mut rpc);
    // Then the second run is preparing with a running workspace row and no turn was started for it, and the refusal is T3's
    let (run, node) = (&p["runs"][1], &p["nodes"][1]);
    assert_eq!((run["status"].as_str(), run["startedAt"].is_null(), node["status"].as_str(), node["checkpointScopeId"].is_null()), (Some("preparing"), true, Some("pending"), true));
    let workspace = p["turnItems"].as_array().unwrap().iter().find(|i| i["type"] == "command_execution").unwrap();
    assert_eq!((workspace["title"].as_str(), workspace["status"].as_str(), workspace["runId"].as_str()), (Some("Preparing workspace"), Some("running"), Some("run:thread:t1:ordinal:2")));
    assert_eq!(h.agent.turns.lock().unwrap().len(), 1);
    assert_eq!(failure(&release)["message"], "Run run:thread:t1:ordinal:1 is not awaiting workspace preparation.");
}

#[test]
fn an_edit_of_a_run_that_is_not_queued_is_rejected_before_commit_a_notification_is_accepted_and_a_subagent_that_cannot_be_stopped_says_so() {
    // Given a thread with one failed run
    let (h, mut rpc, _thread) = started();
    *h.agent.start_error.lock().unwrap() = Some(t3_core::ports::AgentError::Failed("provider opencode is not reachable".into()));
    success(&dispatch(&mut rpc, message("c-o", "go")));
    h.agent.replies.lock().unwrap().insert("queue.edit", Err(t3_core::ports::AgentError::Failed("Run run:thread:t1:ordinal:1 is not queued.".into())));
    h.agent.replies.lock().unwrap().insert("workflow.stop", Err(t3_core::ports::AgentError::Failed("thread t1 is not running turn x".into())));
    // When the run is edited, a delivery is accepted and a subagent is stopped
    let edit = dispatch(&mut rpc, json!({"type": "queued-run.edit", "commandId": "c-e", "threadId": "t1", "runId": "run:thread:t1:ordinal:1", "text": "a"}));
    let accepted = dispatch(&mut rpc, json!({"type": "notification.delivery.accept", "commandId": "c-n", "threadId": "t1", "messageId": "m-c-o"}));
    let stop = dispatch(&mut rpc, json!({"type": "subagent.stop", "commandId": "c-s", "threadId": "t1", "runId": "run:thread:t1:ordinal:1", "subagentId": "a"}));
    // Then the edit's cause is the rejection, the delivery has a sequence, and the subagent refusal is T3's
    assert_eq!((failure(&edit)["cause"]["name"].as_str(), failure(&edit)["cause"]["message"].as_str(), failure(&edit)["detail"].as_str()), (Some("OrchestratorCommandRejectedError"), Some("Orchestration command queued-run.edit (c-e) was rejected before commit."), Some("Run run:thread:t1:ordinal:1 is not queued.")));
    assert!(success(&accepted)["sequence"].is_number());
    assert_eq!(failure(&stop)["message"], "The requested native subagent cannot be stopped on this run.");
}

fn queued(command_id: &str, text: &str) -> Value {
    json!({"type": "message.dispatch", "createdBy": "user", "creationSource": "web", "commandId": command_id, "threadId": "t1", "messageId": format!("m-{command_id}"), "text": text, "dispatchMode": {"type": "queue_after_active"}})
}

fn run_rows(rpc: &mut Rpc) -> Vec<(String, String, Value)> {
    projection(rpc)["runs"].as_array().unwrap().iter().map(|r| (r["id"].as_str().unwrap().to_string(), r["status"].as_str().unwrap().to_string(), r["queuePosition"].clone())).collect()
}

#[test]
fn a_message_sent_behind_a_running_turn_is_a_queued_run_in_the_agent_apis_queue_and_one_the_agent_api_refuses_is_cancelled() {
    // Given a turn the agent API keeps running
    let (h, mut rpc, _thread) = started();
    h.agent.hold.store(true, std::sync::atomic::Ordering::SeqCst);
    success(&dispatch(&mut rpc, message("c-1", "long")));
    drain(&mut rpc);
    // When one message is queued behind it, then another while the agent API refuses to queue
    success(&dispatch(&mut rpc, json!({"type": "thread.model-selection.set", "commandId": "c-model", "threadId": "t1", "modelSelection": {"instanceId": "opencode", "model": "gpt-test"}})));
    success(&dispatch(&mut rpc, queued("c-2", "later")));
    let asked = |h: &Harness| h.agent.ops.lock().unwrap().iter().filter(|o| matches!(o, t3_core::ports::Op::QueueAdd { .. })).count();
    while asked(&h) < 1 {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(h.agent.ops.lock().unwrap().iter().any(|op| {
        matches!(op, t3_core::ports::Op::QueueAdd { prompt, model: Some(model), .. } if prompt == "later" && model == "gpt-test")
    }), "the queued turn must keep the selected model");
    h.agent.replies.lock().unwrap().insert("queue.add", Err(t3_core::ports::AgentError::Failed("the queue is full".into())));
    success(&dispatch(&mut rpc, queued("c-3", "never")));
    drain(&mut rpc);
    // Then the agent API was asked to queue both prompts under their run ids, and the first waits first in line while the second is cancelled
    let adds: Vec<(String, String)> = h.agent.ops.lock().unwrap().iter().filter_map(|o| if let t3_core::ports::Op::QueueAdd { item, prompt, .. } = o { Some((item.clone(), prompt.clone())) } else { None }).collect();
    assert_eq!(adds, [("run:thread:t1:ordinal:2".to_string(), "later".to_string()), ("run:thread:t1:ordinal:3".to_string(), "never".to_string())]);
    assert_eq!(run_rows(&mut rpc), [("run:thread:t1:ordinal:1".to_string(), "running".to_string(), Value::Null), ("run:thread:t1:ordinal:2".to_string(), "queued".to_string(), json!(1)), ("run:thread:t1:ordinal:3".to_string(), "cancelled".to_string(), Value::Null)]);
    // And the cancelled run's message is taken back while the queued one stays, and neither has a row of the timeline yet
    let p = projection(&mut rpc);
    let texts: Vec<&str> = p["messages"].as_array().unwrap().iter().map(|m| m["text"].as_str().unwrap()).collect();
    assert_eq!((texts, p["turnItems"].as_array().unwrap().len()), (vec!["long", "later"], 1));
}

#[test]
fn a_thread_stop_forces_the_agent_api_to_stop_and_hold_its_queue_and_a_turn_the_provider_has_not_begun_ends_at_once_with_the_interrupt_rows() {
    // Given a turn the agent API keeps running and a message queued behind it
    let (h, mut rpc, _thread) = started();
    h.agent.hold.store(true, std::sync::atomic::Ordering::SeqCst);
    success(&dispatch(&mut rpc, message("c-1", "long")));
    success(&dispatch(&mut rpc, queued("c-2", "later")));
    drain(&mut rpc);
    // When the thread is stopped with a reason
    success(&dispatch(&mut rpc, json!({"type": "thread.stop", "commandId": "c-3", "threadId": "t1", "reason": "user"})));
    drain(&mut rpc);
    // Then the agent API was told to force the stop, the run is interrupted with the two rows that say why, and the queued run still waits
    assert!(h.agent.ops.lock().unwrap().iter().any(|o| matches!(o, t3_core::ports::Op::ThreadForceStop { thread, .. } if thread == "t1")));
    let p = projection(&mut rpc);
    let rows: Vec<(&str, &str)> = p["turnItems"].as_array().unwrap().iter().map(|i| (i["type"].as_str().unwrap(), i["message"].as_str().unwrap_or(""))).collect();
    assert_eq!(rows, [("user_message", ""), ("run_interrupt_request", "user"), ("run_interrupt_result", "Run interrupted before provider start")]);
    assert_eq!(run_rows(&mut rpc).into_iter().map(|(_, status, position)| (status, position)).collect::<Vec<_>>(), [("interrupted".to_string(), Value::Null), ("queued".to_string(), json!(1))]);
}

#[test]
fn a_queued_message_is_steered_into_a_turn_only_once_the_provider_has_begun_it() {
    // Given a turn the agent API keeps running, with a message queued behind it
    let (h, mut rpc, _thread) = started();
    h.agent.hold.store(true, std::sync::atomic::Ordering::SeqCst);
    success(&dispatch(&mut rpc, message("c-1", "long")));
    success(&dispatch(&mut rpc, queued("c-2", "later")));
    drain(&mut rpc);
    let steer = |id: &str| json!({"type": "queued-message.promote-to-steer", "commandId": id, "threadId": "t1", "queuedRunId": "run:thread:t1:ordinal:2", "targetRunId": "run:thread:t1:ordinal:1"});
    // When the queued message is promoted before the provider shows anything
    let early = dispatch(&mut rpc, steer("c-3"));
    // Then T3 refuses in its words and the agent API is not asked
    assert_eq!(failure(&early)["message"], "No running provider turn found for active run run:thread:t1:ordinal:1.");
    assert!(!h.agent.ops.lock().unwrap().iter().any(|o| matches!(o, t3_core::ports::Op::TurnSteer { .. })));
    // And once the turn has shown text, the promotion is asked of the agent API
    h.agent.append("t1", Some("u1"), AgentEventKind::Text("working".into()));
    std::thread::sleep(std::time::Duration::from_millis(300));
    success(&dispatch(&mut rpc, steer("c-4")));
    assert!(h.agent.ops.lock().unwrap().iter().any(|o| matches!(o, t3_core::ports::Op::TurnSteer { .. })));
}

#[test]
fn undecodable_commands_are_defects_and_commands_the_server_does_not_build_are_typed_refusals() {
    let (_h, mut rpc, _thread) = started();
    // When a command misses its id, names an unknown type, and one is a real type this server does not build; and a dispatch mode it does not offer
    let missing = dispatch(&mut rpc, json!({"type": "message.dispatch"}));
    let unknown = dispatch(&mut rpc, json!({"type": "conformance.noSuch", "commandId": "x"}));
    let settle = dispatch(&mut rpc, json!({"type": "thread.pull-request.sync", "commandId": "c-s", "threadId": "t1"}));
    let queued = dispatch(&mut rpc, json!({"type": "message.dispatch", "createdBy": "user", "creationSource": "web", "commandId": "c-q", "threadId": "t1", "dispatchMode": "run_later", "messageId": "m-q", "text": "x"}));
    let empty = dispatch(&mut rpc, json!({"type": "message.dispatch", "createdBy": "user", "creationSource": "web", "commandId": "c-e", "threadId": "t1", "messageId": "m-e", "text": ""}));
    // Then the first two are decoder defects, the others dispatch errors that say what is unsupported or missing
    assert_eq!(missing["cause"][0]["defect"], "Missing key\n  at [\"createdBy\"]");
    assert!(unknown["cause"][0]["defect"].as_str().unwrap().starts_with("Expected { readonly \"type\": \"thread.stop\""));
    // And a command that is no object at all is the same union defect
    let wrong = dispatch(&mut rpc, json!("wrong"));
    assert_eq!(wrong["cause"][0]["defect"], unknown["cause"][0]["defect"]);
    assert!(failure(&settle)["message"].as_str().unwrap().contains("thread.pull-request.sync is not supported"));
    assert!(failure(&queued)["message"].as_str().unwrap().contains("run_later"));
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
    let root = rpc.call("projects.listEntries", json!({"cwd": "/workspace/demo"}));
    let src = rpc.call("projects.listEntries", json!({"cwd": "/workspace/demo", "directoryPath": "src"}));
    let read = rpc.call("projects.readFile", json!({"cwd": "/workspace/demo", "relativePath": "README.md"}));
    let missing = rpc.call("projects.readFile", json!({"cwd": "/workspace/demo", "relativePath": "nope"}));
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
    // When each is called with the smallest payload that decodes (an empty one where no schema is declared)
    for tag in unsupported.iter().copied() {
        let exit = rpc.call(tag, t3_contracts::payload::minimal(tag).unwrap_or_else(|| json!({})));
        // Then it fails with the typed error and the tag, never a closed socket
        let error = failure(&exit);
        assert_eq!((error["_tag"].as_str(), error["method"].as_str()), (Some("UnsupportedOperationError"), Some(tag)), "{tag}");
    }
    assert_eq!(success(&rpc.call("server.probe", json!({}))), &json!({}));
}

#[test]
fn an_update_that_changes_nothing_is_accepted_for_a_thread_and_for_a_project_as_the_pin_answers_it() {
    // Given a project with a thread
    let h = Harness::start();
    let mut rpc = connect(&h);
    rpc.call("projects.mutate", project_create("c-p", "p1"));
    rpc.call("orchestration.dispatchCommand", thread_create("c-t", "t1", "p1"));

    // When each is updated with no field to change
    let thread = rpc.call("orchestration.dispatchCommand", json!({"type": "thread.metadata.update", "commandId": "c-m", "threadId": "t1"}));
    let project = rpc.call("projects.mutate", json!({"type": "project.update", "commandId": "c-u", "projectId": "p1"}));

    // Then both succeed instead of being refused
    success(&thread);
    success(&project);
}
