//! A thread's rows are the thread's log in the agent API as t3 folds it: turns recorded by the contract, a restart that left a run running, a
//! history read that catches the rows up, and an agent API that cannot be reached.
use super::support::*;
use agent_api_agent::testing::thread_events;
use t3_core::domain::command::Family;
use t3_core::domain::decider::{Effect, Outcome, RunStep};
use t3_core::ports::{AgentApi, AgentEventKind, AgentThread, AgentTurn, EventStore, Finish};
use t3_serve::adapters::rca::agent_event;

fn dispatch_message(rpc: &mut Rpc, command_id: &str, text: &str) {
    success(&rpc.call("orchestration.dispatchCommand", json!({"type": "message.dispatch", "createdBy": "user", "creationSource": "web", "commandId": command_id, "threadId": "t1", "messageId": format!("m-{command_id}"), "text": text})));
}

/// A run t3 recorded and no runner ever started: the state a crash right after a message leaves. The run's id.
fn crashed_run(h: &Harness, text: &str) -> String {
    let command = json!({"type": "message.dispatch", "createdBy": "user", "creationSource": "web", "commandId": "c-crash", "threadId": "t1", "messageId": "m-crash", "text": text});
    let dispatched = h.service.engine().dispatch(Family::Thread, &command).expect("a message on the seeded thread");
    match dispatched.effect {
        Some(Effect::Launch(launch)) => launch.run_id,
        other => panic!("no launch: {other:?}"),
    }
}

fn queued_run(h: &Harness, marker: &str) -> String {
    let command = json!({"type": "message.dispatch", "createdBy": "user", "creationSource": "mobile", "commandId": format!("c-{marker}"), "threadId": "t1", "messageId": format!("m-{marker}"), "text": marker, "dispatchMode": {"type": "queue_after_active"}});
    match h.service.engine().dispatch(Family::Thread, &command).unwrap().effect {
        Some(Effect::Queue(launch)) => launch.run_id,
        other => panic!("no queue: {other:?}"),
    }
}

/// What rca did for the turn while t3 was not following: the thread made and the turn started, its events in the thread's log.
fn rca_runs(h: &Harness, text: &str) -> String {
    let thread = AgentThread { thread_id: "t1".into(), instance_id: "opencode".into(), model: None, worktree: None };
    h.agent.ensure_thread(&thread).unwrap();
    let turn = AgentTurn { thread_id: "t1".into(), instance_id: "opencode".into(), model: None, prompt: text.into(), runtime_mode: "full-access".into(), interaction_mode: "default".into() , attachments: vec![], worktree: None};
    h.agent.start_turn(&turn).unwrap()
}

fn transcript(h: &Harness, bearer: &str) -> Reply {
    http(h.addr, "GET", "/api/orchestration/threads/t1/transcript", &[("Authorization", &format!("Bearer {bearer}")), ("x-t3-orchestration-protocol", "2")], "")
}

fn texts(projection: &Value) -> Vec<(String, String)> {
    projection["messages"].as_array().unwrap().iter().map(|m| (m["role"].as_str().unwrap().to_string(), m["text"].as_str().unwrap().to_string())).collect()
}

/// The status the shell shows for thread `t1`.
fn shell_status(rpc: &mut Rpc) -> String {
    let shell = rpc.send("orchestration.subscribeShell", json!({}));
    let threads = rpc.chunk(&shell)[0]["snapshot"]["threads"].clone();
    threads.as_array().unwrap().iter().find(|t| t["id"] == "t1").unwrap()["status"].as_str().unwrap().to_string()
}

fn seeded() -> (Harness, Rpc) {
    let h = Harness::start();
    let mut rpc = connect(&h);
    seed(&mut rpc);
    (h, rpc)
}

fn script() -> Vec<AgentEventKind> {
    vec![
        AgentEventKind::Text("fixed ".into()),
        AgentEventKind::ToolStarted { id: "k1".into(), name: "bash".into(), input: json!({"command": "ls"}) },
        AgentEventKind::ToolOutput { id: "k1".into(), data: "a.txt\n".into() },
        AgentEventKind::ToolFinished { id: "k1".into(), ok: true },
        AgentEventKind::Text("it".into()),
    ]
}

#[test]
fn the_recorded_turns_of_the_contract_become_the_threads_rows_a_failing_tool_a_logged_error_and_the_events_t3_shows_nothing_of() {
    // Given the events the contract recorded for two turns of a thread, read as t3 reads them
    let recorded: Vec<_> = thread_events().iter().map(|e| agent_event(e).unwrap()).collect();
    let turn_of = |turn: &str| -> Vec<AgentEventKind> { recorded.iter().filter(|e| e.turn.as_deref() == Some(turn)).map(|e| e.kind.clone()).collect() };
    // And an agent API that replays the first turn: the double starts a turn and records the user's message itself, so those are left out of the script
    let (h, mut rpc) = seeded();
    let (first, second) = (turn_of("u1"), turn_of("u2"));
    let AgentEventKind::UserText(question) = first[1].clone() else { panic!("the first turn opens with the user's message") };
    *h.agent.script.lock().unwrap() = first[2..].to_vec();
    // When the user's message is dispatched and the turn has ended
    dispatch_message(&mut rpc, "c-1", &question);
    let p = settled(&mut rpc, "t1");
    // Then the run completed and the thread shows the reasoning, the answer as one message, and the two tool calls with what each printed and whether it worked
    assert_eq!(p["runs"][0]["status"], "completed");
    let wanted = |kind: fn(&AgentEventKind) -> Option<String>| first.iter().filter_map(kind).collect::<Vec<String>>();
    let reasoning = wanted(|k| if let AgentEventKind::Reasoning(t) = k { Some(t.clone()) } else { None });
    let answer = wanted(|k| if let AgentEventKind::Text(t) = k { Some(t.clone()) } else { None }).concat();
    let item = |kind: &str| p["turnItems"].as_array().unwrap().iter().filter(|i| i["type"] == kind).cloned().collect::<Vec<Value>>();
    assert_eq!(item("reasoning")[0]["text"], reasoning.concat());
    assert_eq!(texts(&p), [("user".to_string(), question.clone()), ("assistant".to_string(), answer)]);
    let tools: Vec<(String, String, String)> = item("command_execution").iter().map(|t| (t["name"].as_str().unwrap().into(), t["status"].as_str().unwrap().into(), t["output"].as_str().unwrap().into())).collect();
    let printed = |call: &str| -> String { first.iter().filter_map(|k| if let AgentEventKind::ToolOutput { id, data } = k { (id == call).then(|| data.clone()) } else { None }).collect() };
    let ok = |call: &str| first.iter().find_map(|k| if let AgentEventKind::ToolFinished { id, ok } = k { (id == call).then_some(*ok) } else { None }).unwrap();
    let named = |call: &str| first.iter().find_map(|k| if let AgentEventKind::ToolStarted { id, name, .. } = k { (id == call).then(|| name.clone()) } else { None }).unwrap();
    let expected: Vec<(String, String, String)> = ["c1", "c2"].iter().map(|c| (named(c), if ok(c) { "completed" } else { "failed" }.to_string(), printed(c))).collect();
    assert_eq!(tools, expected);
    // And when the second turn, which only failed, is dispatched, its run failed with the error the log carries and no assistant text
    *h.agent.script.lock().unwrap() = second[1..].to_vec();
    dispatch_message(&mut rpc, "c-2", "again");
    let p = settled(&mut rpc, "t1");
    let AgentEventKind::Failure(logged) = second[1].clone() else { panic!("the second turn opens with its failure") };
    assert_eq!((p["runs"][1]["status"].as_str(), last_error(&h, "t1").as_deref()), (Some("failed"), Some(logged.as_str())));
    assert_eq!(texts(&p).len(), 3, "the user's second message and no answer");
    // And the thread is idle with both runs recorded
    assert_eq!((p["runs"].as_array().unwrap().len(), shell_status(&mut rpc).as_str()), (2, "idle"));
    assert!(matches!(second.last(), Some(AgentEventKind::TurnFinished(Finish::Failed))));
}

#[test]
fn a_history_read_catches_the_rows_up_with_a_turn_that_ended_while_t3_was_down_and_reading_again_writes_nothing() {
    // Given a run t3 recorded and a restart, while rca ran the turn to its end
    let (h, _rpc) = seeded();
    crashed_run(&h, "go");
    *h.agent.script.lock().unwrap() = script();
    rca_runs(&h, "go");
    let restarted = h.restart();
    let mut rpc = connect(&restarted);
    let bearer = bearer_of(restarted.addr, &restarted.pair(PairingRequest::default()).code);
    assert_eq!(success(&rpc.call("orchestration.getThreadProjection", json!({"threadId": "t1"})))["runs"][0]["status"], "running");
    // When the thread's transcript is read
    let reply = transcript(&restarted, &bearer);
    // Then the transcript holds what the turn did, the run is completed, and the thread is idle again
    assert_eq!(reply.status, 200, "{:?}", reply.body);
    let kinds: Vec<&str> = reply.body["items"].as_array().unwrap().iter().map(|r| r["item"]["type"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["user_message", "assistant_message", "command_execution"]);
    let p = success(&rpc.call("orchestration.getThreadProjection", json!({"threadId": "t1"}))).clone();
    assert_eq!((p["runs"][0]["status"].as_str(), shell_status(&mut rpc).as_str()), (Some("completed"), "idle"));
    assert_eq!(texts(&p), [("user".to_string(), "go".to_string()), ("assistant".to_string(), "fixed it".to_string())]);
    // And a second read, and one when the agent API sends its whole log again, ask rca and write nothing, so no text is doubled
    let written = restarted.store.last_sequence().unwrap();
    transcript(&restarted, &bearer);
    restarted.agent.resend.store(true, std::sync::atomic::Ordering::SeqCst);
    transcript(&restarted, &bearer);
    assert_eq!(restarted.store.last_sequence().unwrap(), written);
    assert_eq!(texts(success(&rpc.call("orchestration.getThreadProjection", json!({"threadId": "t1"}))))[1].1, "fixed it");
    assert_eq!(restarted.agent.calls.lock().unwrap().iter().filter(|c| **c == "thread.history").count(), 3);
}

/// Waits up to 5 s for `done`, which is then true.
fn wait_until(mut done: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !done() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(done(), "not within 5 s");
}

#[test]
fn a_run_a_restart_left_running_is_followed_to_its_end_without_taking_in_another_turns_events_or_its_own_twice() {
    // Given a run t3 recorded and a restart, while rca's turn is still running with some of its text
    let (h, _rpc) = seeded();
    crashed_run(&h, "go");
    h.agent.hold.store(true, std::sync::atomic::Ordering::SeqCst);
    *h.agent.script.lock().unwrap() = vec![AgentEventKind::Text("so far".into())];
    let turn = rca_runs(&h, "go");
    let restarted = h.restart();
    let mut rpc = connect(&restarted);
    let bearer = bearer_of(restarted.addr, &restarted.pair(PairingRequest::default()).code);
    // When t3 serve starts again and is following the turn, which says more, while an event of another turn arrives
    restarted.service.resume_runs();
    wait_until(|| h.agent.calls.lock().unwrap().contains(&"thread.subscribe"));
    h.agent.append("t1", Some(&turn), AgentEventKind::Text(", and more".into()));
    h.agent.append("t1", Some("another-turn"), AgentEventKind::Text("stray".into()));
    let streamed = |rpc: &mut Rpc| {
        let p = success(&rpc.call("orchestration.getThreadProjection", json!({"threadId": "t1"}))).clone();
        p["turnItems"].as_array().unwrap().iter().find(|i| i["type"] == "assistant_message").map(|i| i["text"].as_str().unwrap().to_string())
    };
    wait_until(|| streamed(&mut rpc).is_some_and(|text| text.ends_with("more")));
    // And a history read finds the agent API sending the whole log again while the run still runs
    h.agent.resend.store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(transcript(&restarted, &bearer).status, 200);
    h.agent.resend.store(false, std::sync::atomic::Ordering::SeqCst);
    // And the turn is interrupted
    h.agent.interrupt("t1").unwrap();
    // Then the run ended interrupted with the text of its own turn, each piece once, and the thread is idle
    let p = settled(&mut rpc, "t1");
    assert_eq!((p["runs"][0]["status"].as_str(), shell_status(&mut rpc).as_str()), (Some("interrupted"), "idle"));
    assert_eq!(texts(&p)[1].1, "so far, and more");
}

#[test]
fn after_a_restart_the_phones_queue_receives_both_automatically_dispatched_turns_in_order() {
    // Given one running and two queued rows on disk, while RCA is still running the first turn.
    let (h, _) = seeded();
    crashed_run(&h, "original");
    let queued = [queued_run(&h, "A"), queued_run(&h, "B")];
    h.agent.replies.lock().unwrap().insert("queue.list", Ok(json!({"held": false})));
    h.agent.hold.store(true, std::sync::atomic::Ordering::SeqCst);
    let turn = rca_runs(&h, "original");
    let restarted = h.restart();
    let mut rpc = connect(&restarted);

    // When T3 resumes its subscription and RCA finishes the original turn and starts each queued turn.
    restarted.service.resume_runs();
    wait_until(|| h.agent.calls.lock().unwrap().contains(&"thread.subscribe"));
    h.agent.append("t1", Some(&turn), AgentEventKind::TurnFinished(Finish::Completed));
    for (run, marker) in queued.iter().zip(["A", "B"]) {
        h.agent.append("t1", Some(run), AgentEventKind::TurnStarted);
        h.agent.append("t1", Some(run), AgentEventKind::Text(format!("reply {marker}")));
        h.agent.append("t1", Some(run), AgentEventKind::TurnFinished(Finish::Completed));
    }

    // Then both replies and completed runs appear on the phone without an explicit history read.
    let projection = |rpc: &mut Rpc| success(&rpc.call("orchestration.getThreadProjection", json!({"threadId": "t1"}))).clone();
    wait_until(|| projection(&mut rpc)["runs"][2]["status"] == "completed");
    let actual = projection(&mut rpc);
    assert!(actual["runs"].as_array().unwrap().iter().all(|run| run["status"] == "completed"));
    assert_eq!(texts(&actual).into_iter().filter(|(role, _)| role == "assistant").map(|(_, text)| text).collect::<Vec<_>>(), ["reply A", "reply B"]);
    assert_eq!(h.agent.turns.lock().unwrap().len(), 1, "recovery must not start another turn");
}

#[test]
fn after_a_restart_a_forced_stop_keeps_the_queue_waiting_until_the_phone_resumes_it() {
    // Given a running turn with a queued follow-up, the phone stops the turn and sees the hold.
    let (h, mut initial) = seeded();
    let original = crashed_run(&h, "original");
    let queued = queued_run(&h, "held");
    let projection = |rpc: &mut Rpc| success(&rpc.call("orchestration.getThreadProjection", json!({"threadId": "t1"}))).clone();
    success(&initial.call("orchestration.dispatchCommand", json!({"type": "thread.stop", "threadId": "t1", "commandId": "c-stop"})));
    assert_eq!(projection(&mut initial)["runs"][1]["queueHeld"], true);
    h.service.engine().record("t1", &original, &RunStep::Finished(Outcome::Interrupted)).unwrap();
    h.agent.replies.lock().unwrap().insert("queue.list", Ok(json!({"held": true})));
    rca_runs(&h, "original");

    // When T3 recovers, its only queue operation is to read the hold.
    let restarted = h.restart();
    let mut rpc = connect(&restarted);
    let before = h.agent.ops.lock().unwrap().len();
    restarted.service.resume_runs();
    wait_until(|| h.agent.ops.lock().unwrap().iter().any(|op| op.method() == "queue.list"));
    assert_eq!(projection(&mut rpc)["runs"][1]["status"], "queued");
    assert_eq!(projection(&mut rpc)["runs"][1]["queueHeld"], true);
    assert_eq!(h.agent.ops.lock().unwrap()[before..].iter().map(|op| op.method()).collect::<Vec<_>>(), ["queue.list"]);

    // Then the user's Resume is what allows RCA to start the turn and T3 to show the reply.
    success(&rpc.call("orchestration.dispatchCommand", json!({"type": "queue.resume", "threadId": "t1", "commandId": "c-resume"})));
    assert_eq!(projection(&mut rpc)["runs"][1]["queueHeld"], false);
    h.agent.append("t1", Some(&queued), AgentEventKind::TurnStarted);
    h.agent.append("t1", Some(&queued), AgentEventKind::Text("reply after resume".into()));
    h.agent.append("t1", Some(&queued), AgentEventKind::TurnFinished(Finish::Completed));
    wait_until(|| projection(&mut rpc)["runs"][1]["status"] == "completed");
    assert!(texts(&projection(&mut rpc)).iter().any(|(role, text)| role == "assistant" && text == "reply after resume"));
}

#[test]
fn restart_recovers_a_hold_missing_from_the_saved_queue_rows() {
    // Given RCA holds a queue that T3 saved before it projected queue holds.
    let (h, _) = seeded();
    let original = crashed_run(&h, "original");
    queued_run(&h, "held");
    h.service.engine().record("t1", &original, &RunStep::Finished(Outcome::Interrupted)).unwrap();
    h.agent.replies.lock().unwrap().insert("queue.list", Ok(json!({"held": true})));

    // When the adapter restarts, then the phone can resume the existing queued run.
    let restarted = h.restart();
    let mut rpc = connect(&restarted);
    restarted.service.resume_runs();
    wait_until(|| success(&rpc.call("orchestration.getThreadProjection", json!({"threadId": "t1"})))["runs"][1]["queueHeld"] == true);
}

#[test]
fn the_queue_stays_pending_when_its_first_subscription_fails_and_can_be_followed_again() {
    use std::sync::Arc;
    use t3_core::ports::AgentError;
    use t3_core::usecases::{engine::Engine, runner::Runner};

    // Given the queued follow-up is saved in T3 and its events are available at RCA.
    let (h, _) = seeded();
    let first = crashed_run(&h, "original");
    let queued = queued_run(&h, "follow-up");
    h.service.engine().record("t1", &first, &RunStep::Finished(Outcome::Completed)).unwrap();
    h.agent.append("t1", Some(&queued), AgentEventKind::TurnStarted);
    h.agent.append("t1", Some(&queued), AgentEventKind::Text("the existing reply".into()));
    h.agent.append("t1", Some(&queued), AgentEventKind::TurnFinished(Finish::Completed));
    let engine = Arc::new(Engine::open(h.store.clone(), h.clock.clone(), h.ids.clone()).unwrap());
    let runner = Runner::new(h.agent.clone(), engine.clone(), h.store.clone(), h.log.clone());
    let attempts = || h.agent.calls.lock().unwrap().iter().filter(|method| **method == "thread.subscribe").count();

    // When the first attempt to follow fails before reading the queued turn.
    *h.agent.follow_error_once.lock().unwrap() = Some(AgentError::Failed("connection refused".into()));
    runner.drain("t1");

    // Then T3 stops after one attempt and can follow the same turn on the next drain.
    assert_eq!(attempts(), 1);
    assert_eq!(engine.next_queued("t1").as_deref(), Some(queued.as_str()));
    runner.drain("t1");
    assert_eq!(attempts(), 2);
    assert!(engine.next_queued("t1").is_none());
    assert!(engine.read(|state| state.entities["t1"].runs[1]["status"] == "completed"));
    assert!(h.agent.turns.lock().unwrap().is_empty(), "a retry only follows the existing turn");
}

#[test]
fn after_a_restart_a_queued_run_that_already_started_keeps_receiving_its_reply() {
    // Given T3 persisted a waiting row, but RCA started its turn before T3 restarted.
    let (h, _) = seeded();
    let original = crashed_run(&h, "original");
    let queued = queued_run(&h, "queued");
    h.service.engine().record("t1", &original, &RunStep::Finished(Outcome::Completed)).unwrap();
    rca_runs(&h, "original");
    h.agent.append("t1", Some(&queued), AgentEventKind::TurnStarted);
    h.agent.append("t1", Some(&queued), AgentEventKind::Text("before restart, ".into()));

    // When T3 catches up and follows the existing turn, then RCA completes it.
    let restarted = h.restart();
    let mut rpc = connect(&restarted);
    restarted.service.resume_runs();
    wait_until(|| h.agent.calls.lock().unwrap().contains(&"thread.subscribe"));
    h.agent.append("t1", Some(&queued), AgentEventKind::Text("after restart".into()));
    h.agent.append("t1", Some(&queued), AgentEventKind::TurnFinished(Finish::Completed));

    // Then the original queued run contains both pieces of the reply exactly once.
    let p = settled(&mut rpc, "t1");
    assert_eq!(p["runs"][1]["status"], "completed");
    assert!(texts(&p).iter().any(|(role, text)| role == "assistant" && text == "before restart, after restart"));
    assert_eq!(h.agent.turns.lock().unwrap().len(), 1);
}

#[test]
fn a_run_whose_turn_was_never_started_is_failed_when_t3_serve_starts_again_and_the_agent_api_is_not_asked_to_run_it() {
    // Given a run t3 recorded and a restart, and an agent API that never heard of a turn
    let (h, _rpc) = seeded();
    crashed_run(&h, "go");
    let restarted = h.restart();
    let mut rpc = connect(&restarted);
    // When t3 serve starts again
    restarted.service.resume_runs();
    let p = settled(&mut rpc, "t1");
    // Then the run is failed saying why, and no turn was started
    assert_eq!((p["runs"][0]["status"].as_str(), last_error(&restarted, "t1").as_deref()), (Some("failed"), Some("t3 serve stopped before the turn was started")));
    assert!(h.agent.turns.lock().unwrap().is_empty());
}

#[test]
fn a_history_read_serves_the_rows_it_has_when_the_agent_api_cannot_be_reached_or_has_no_such_thread() {
    // Given a thread that ran a turn, and an agent API that fails to answer its history
    let (h, mut rpc) = seeded();
    *h.agent.script.lock().unwrap() = script();
    dispatch_message(&mut rpc, "c-1", "go");
    settled(&mut rpc, "t1");
    let bearer = bearer_of(h.addr, &h.pair(PairingRequest::default()).code);
    *h.agent.history_error.lock().unwrap() = Some(t3_core::ports::AgentError::Failed("rca is down".into()));
    // When the transcript is read, then again once the agent API says it has no such thread
    let down = transcript(&h, &bearer);
    *h.agent.history_error.lock().unwrap() = Some(t3_core::ports::AgentError::NoThread);
    let unknown = transcript(&h, &bearer);
    // Then both are the thread's rows, and only the failure is logged, with its reason
    assert_eq!((down.status, unknown.status), (200, 200));
    assert_eq!(down.body["items"], unknown.body["items"]);
    assert_eq!(down.body["items"].as_array().unwrap().len(), 3);
    let failures: Vec<String> = h.log.lines().into_iter().filter(|l| l.starts_with("run.sync.failed")).collect();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures[0].contains("error=rca is down"), "{failures:?}");
}
