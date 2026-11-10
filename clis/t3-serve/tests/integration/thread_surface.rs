//! The thread surface of M4 through the chain, driven as the phone drives it: each test sends T3's own commands and RPCs over the paired socket and
//! each call's window shows the agent API method rca answered and the executor tool that ran. What a phone cannot read (rca's own shelves, the ids
//! rca makes, the state it keeps) is read through a side door that is not in any window. The shapes asserted are the pinned contract's
//! (`references/t3code/packages/contracts`: OrchestrationCommand, the OrchestrationRpc result schemas); the pinned server's own recording needs
//! its release binary and a provider login, so no new golden is recorded here and the recorded session of the replay test stays the only one.
use crate::chain::{hit, last_window, said, stays, writes};
use crate::rig::*;
use crate::support::*;
use exec_testkit::script::{Script, Turn};

/// Claude Code's first line, which announces the conversation the thread then keeps.
fn init(session: &str) -> String {
    json!({"type": "system", "subtype": "init", "session_id": session}).to_string()
}

/// What the call before the last asked rca, in order, and whether each was answered: the proof a refusal or a result came from rca.
fn asked(case: &exec_testkit::Case) -> Vec<(String, bool)> {
    last_window(case).0
}

fn answered(case: &exec_testkit::Case, method: &str) -> bool {
    asked(case).iter().any(|(m, ok)| m == method && *ok)
}

fn refused_by_rca(case: &exec_testkit::Case, method: &str) -> bool {
    asked(case).iter().any(|(m, ok)| m == method && !*ok)
}

/// A command with an id of its own: T3 answers a command id it has accepted with the first answer.
fn cmd(kind: &str, thread: &str, extra: Value) -> Value {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let mut c = json!({"type": kind, "commandId": format!("c-{kind}-{thread}-{n}"), "threadId": thread});
    c.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap_or_default());
    c
}

fn wait_started(chain: &Chain, program: &str, count: usize) {
    let started = std::time::Instant::now();
    while chain.cli.invocations(program).len() < count {
        assert!(started.elapsed() < std::time::Duration::from_secs(30), "the CLI never started {count} times: {}", chain.evidence());
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// The ids of the threads on one of rca's shelves.
fn shelf(rca: &mut Direct, name: &str) -> Vec<String> {
    let page = rca.call("agent.lifecycle", "thread.shelf", json!({"shelf": name, "limit": 50})).expect("rca answers the shelf");
    page["threads"].as_array().unwrap().iter().map(|t| t["thread"]["id"].as_str().unwrap().to_string()).collect()
}

/// A thread's settle and snooze state as rca keeps it.
fn settle_of(rca: &mut Direct, thread: &str) -> Value {
    let family = rca.call("agent.lifecycle", "thread.family", json!({"thread": thread})).expect("rca answers the family");
    family["threads"].as_array().unwrap().iter().find(|t| t["thread"]["id"] == thread).expect("the family holds the thread")["settle"].clone()
}

fn completed(runs: usize) -> impl Fn(&Value) -> bool {
    move |p| p["runs"].as_array().is_some_and(|r| r.len() == runs && r.iter().all(|r| r["status"] == "completed"))
}

#[test]
fn settle_snooze_archive_fork_merge_back_detach_and_delete_each_change_what_rca_holds_and_a_second_archive_is_refused() {
    exec_testkit::case!(SUITE, |case| {
        // Given a thread whose Claude Code announced a conversation and wrote a note, and rca's shelves read through the side door
        let chain = Chain::start();
        chain.cli.script("claude", &Script::of(vec![Turn::replaying(&[init("sess-1"), said("writing the note")]).then_calling("k1", "write_file", json!({"path": "notes/a.txt", "content": "hello", "create_dirs": true})).then_line(&said("done"))]));
        let mut phone = Phone::connect(case, &chain);
        let mut rca = chain.direct();
        phone.seed(case, &chain);
        phone.message(case, &chain, "t1", "c-m1", "make a note");
        // When the phone settles it, then snoozes it and wakes it, then settles and unsettles it
        let at = "2026-11-01T10:00:00.000Z";
        success(&phone.command(case, &chain, cmd("thread.settle", "t1", json!({"settledAt": at}))));
        assert!(answered(case, "thread.settle"), "{:?}", asked(case));
        // Then rca's settled shelf holds the thread, and it is off the shelf again once unsettled
        assert_eq!(shelf(&mut rca, "settled"), ["t1"]);
        success(&phone.command(case, &chain, cmd("thread.unsettle", "t1", json!({"reason": "user"}))));
        assert!(answered(case, "thread.settle") && shelf(&mut rca, "settled").is_empty());
        success(&phone.command(case, &chain, cmd("thread.snooze", "t1", json!({"snoozedUntil": "2031-01-01T00:00:00.000Z"}))));
        // Then rca holds the snooze until the given time, and not as a settle
        assert!(answered(case, "thread.settle"));
        let snoozed = settle_of(&mut rca, "t1");
        assert_eq!(snoozed["snoozed_until_ms"], 1_924_992_000_000_u64, "{snoozed}");
        assert!(shelf(&mut rca, "settled").is_empty());
        success(&phone.command(case, &chain, cmd("thread.unsnooze", "t1", json!({"reason": "user"}))));
        // And the unsnooze wakes it: no snooze left and a wake time kept
        assert!(answered(case, "thread.settle"));
        let awake = settle_of(&mut rca, "t1");
        assert!(awake.get("snoozed_until_ms").is_none() && awake["woke_at_ms"].is_u64(), "{awake}");
        // When it is archived, and archived again
        success(&phone.command(case, &chain, cmd("thread.archive", "t1", json!({}))));
        assert_eq!(shelf(&mut rca, "archived"), ["t1"]);
        let again = phone.command(case, &chain, cmd("thread.archive", "t1", json!({})));
        // Then rca itself refused the second one (its words are T3's), and the unarchive puts the thread back
        assert!(refused_by_rca(case, "thread.archive"), "the request never reached rca: {:?}", asked(case));
        assert!(failure(&again)["message"].as_str().is_some_and(|m| m.to_lowercase().contains("archived")), "{again}");
        success(&phone.command(case, &chain, cmd("thread.unarchive", "t1", json!({}))));
        assert!(answered(case, "thread.archive") && shelf(&mut rca, "archived").is_empty());
        // When the thread is forked, the fork is merged back, and the next message runs: rca carries the fork's context into it
        success(&phone.command(case, &chain, json!({"type": "thread.fork", "commandId": "c-fork", "sourceThreadId": "t1", "targetThreadId": "f1", "sourcePoint": {}})));
        assert!(answered(case, "thread.fork"), "{:?}", asked(case));
        let family = rca.call("agent.lifecycle", "thread.family", json!({"thread": "f1"})).expect("rca knows the family");
        assert_eq!(family["threads"].as_array().unwrap().iter().map(|t| t["thread"]["id"].as_str().unwrap()).collect::<Vec<_>>(), ["t1", "f1"]);
        success(&phone.command(case, &chain, json!({"type": "thread.merge_back", "commandId": "c-merge", "sourceThreadId": "f1", "targetThreadId": "t1", "sourcePoint": {}})));
        assert!(answered(case, "thread.mergeBack"), "{:?}", asked(case));
        let projection = phone.command_settled(case, &chain, json!({"type": "message.dispatch", "commandId": "c-m2", "threadId": "t1", "message": {"messageId": "m2", "text": "again"}}), "t1", completed(2));
        // Then the turn ran Claude Code resuming the conversation it announced, and the executor wrote the note again
        let (agent, executor) = last_window(case);
        assert!(agent.contains(&("turn.start".to_string(), true)) && hit(&executor, "tool", "write_file").is_some_and(|h| h["ok"] == true), "{agent:?} {executor}");
        assert!(projection["runs"].as_array().unwrap().len() == 2);
        let starts = chain.cli.invocations("claude");
        assert_eq!(starts.len(), 2);
        assert!(starts[1].argv.windows(2).any(|w| w == ["--resume", "sess-1"]), "{:?}", starts[1].argv);
        assert_eq!(rca.events("t1", "thread.merge_consumed").len(), 1);
        // When the phone detaches the provider session and sends another message
        success(&phone.command(case, &chain, cmd("provider-session.detach", "t1", json!({"providerSessionId": "sess-1"}))));
        assert!(answered(case, "thread.detach"), "{:?}", asked(case));
        phone.command_settled(case, &chain, json!({"type": "message.dispatch", "commandId": "c-m3", "threadId": "t1", "message": {"messageId": "m3", "text": "fresh"}}), "t1", completed(3));
        // Then Claude Code started a new conversation: no --resume
        let starts = chain.cli.invocations("claude");
        assert_eq!(starts.len(), 3);
        assert!(!starts[2].argv.iter().any(|a| a == "--resume"), "{:?}", starts[2].argv);
        // When the fork is deleted
        success(&phone.command(case, &chain, cmd("thread.delete", "f1", json!({}))));
        // Then rca answered the delete and the fork is gone from rca and from the shell the phone reads
        assert!(answered(case, "thread.delete"), "{:?}", asked(case));
        let listed = rca.call("agent.threads", "thread.list", json!({})).expect("rca lists its threads");
        let ids: Vec<&str> = listed["threads"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["t1"], "rca still lists the deleted fork");
        let (_, shell) = phone.stream(case, &chain, &["rpc:orchestration.subscribeShell"], "orchestration.subscribeShell", json!({}));
        assert_eq!(shell[0]["snapshot"]["threads"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect::<Vec<_>>(), ["t1"]);
    });
}

fn queued(rca: &mut Direct, thread: &str) -> Vec<(String, String)> {
    let list = rca.call("agent.queue", "queue.list", json!({"thread": thread})).expect("rca lists the queue");
    list["items"].as_array().unwrap().iter().map(|i| (i["item"].as_str().unwrap().to_string(), i["prompt"].as_str().unwrap().to_string())).collect()
}

fn message(id: &str, thread: &str, text: &str) -> Value {
    json!({"type": "message.dispatch", "commandId": format!("c-{id}"), "threadId": thread, "message": {"messageId": id, "text": text}})
}

#[test]
fn queued_runs_are_edited_cancelled_and_started_through_rca_and_a_promoted_one_steers_the_running_turn_before_the_rest_start_by_themselves() {
    exec_testkit::case!(SUITE, |case| {
        // Given a thread whose first turn stays, then a steered turn that writes steered.txt and a queued turn that writes queued.txt
        let chain = Chain::start();
        chain.cli.script("claude", &Script::of(vec![stays(), writes("steered.txt", "steered"), writes("queued.txt", "from the queue")]));
        let mut phone = Phone::connect(case, &chain);
        let mut rca = chain.direct();
        phone.seed(case, &chain);
        success(&phone.command(case, &chain, message("m1", "t1", "work")));
        wait_started(&chain, "claude", 1);
        let run = phone.run_of("t1");
        // And three messages queued behind it (a phone cannot queue one yet, so rca is asked directly)
        for (item, prompt) in [("q1", "first queued"), ("q2", "the steer text"), ("q3", "to cancel")] {
            rca.call("agent.queue", "queue.add", json!({"thread": "t1", "item": item, "prompt": prompt})).expect("rca queues it");
        }
        // When the phone edits the first, cancels the third and asks to start the first while the turn runs
        success(&phone.command(case, &chain, cmd("queued-run.edit", "t1", json!({"runId": "q1", "text": "edited queued"}))));
        assert!(answered(case, "queue.edit"), "{:?}", asked(case));
        success(&phone.command(case, &chain, cmd("queued-run.cancel", "t1", json!({"runId": "q3"}))));
        assert!(answered(case, "queue.delete"), "{:?}", asked(case));
        let early = phone.command(case, &chain, cmd("queued-run.start", "t1", json!({"runId": "q1"})));
        // Then rca holds the edit and the cancel, and refused the start itself with the words T3 shows
        assert_eq!(queued(&mut rca, "t1"), [("q1".to_string(), "edited queued".to_string()), ("q2".to_string(), "the steer text".to_string())]);
        assert!(refused_by_rca(case, "queue.resume"), "the request never reached rca: {:?}", asked(case));
        assert_eq!(failure(&early)["message"], "A queued user message can only be sent now while the thread is stopped.");
        // When the phone promotes the second to steer the running turn
        let steer = cmd("queued-message.promote-to-steer", "t1", json!({"queuedRunId": "q2", "targetRunId": run}));
        let projection = case.call(&chain.tap, &rows(&["rpc:orchestration.dispatchCommand", "command:queued-message.promote-to-steer"]), || {
            let exit = phone.calls.call("orchestration.dispatchCommand", steer);
            wait_started(&chain, "claude", 3);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            let mut projection = phone.settled("t1");
            while !(chain.repo.path().join("queued.txt").exists() && completed(1)(&projection)) && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(100));
                projection = phone.settled("t1");
            }
            (exit["_tag"] == "Success" && completed(1)(&projection), projection)
        });
        // Then rca steered the turn, and the executor wrote the steered file and, when the turn ended, the next queued item's file
        let (agent, executor) = last_window(case);
        assert!(agent.contains(&("turn.steer".to_string(), true)), "{agent:?}");
        assert_eq!(executor.as_array().unwrap().iter().filter(|h| h["tool"] == "write_file" && h["ok"] == true).count(), 2, "{executor}");
        assert_eq!(std::fs::read_to_string(chain.repo.path().join("steered.txt")).unwrap(), "steered");
        assert_eq!(std::fs::read_to_string(chain.repo.path().join("queued.txt")).unwrap(), "from the queue");
        // And the CLI was started a second time with the steer text and a third time for the queued item, with nothing left queued
        let starts = chain.cli.invocations("claude");
        assert!(starts[1].argv.iter().any(|a| a.contains("the steer text")), "{:?}", starts[1].argv);
        assert!(starts[2].argv.iter().any(|a| a.contains("edited queued")), "{:?}", starts[2].argv);
        assert!(queued(&mut rca, "t1").is_empty());
        // (the turn rca started for the queued item is not a run of the phone's rows yet: they hold the one run the message started)
        assert_eq!(projection["runs"].as_array().unwrap().len(), 1);
    });
}

fn turns_finished(rca: &mut Direct, thread: &str) -> usize {
    rca.events(thread, "turn.finished").len()
}

fn wait_finished(rca: &mut Direct, thread: &str, count: usize) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while turns_finished(rca, thread) < count {
        assert!(std::time::Instant::now() < deadline, "{thread} never finished {count} turns");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
fn a_force_stop_holds_the_queue_and_a_queued_run_starts_only_when_the_phone_starts_it_or_resumes_the_queue_and_rca_refuses_it_on_an_archived_thread() {
    exec_testkit::case!(SUITE, |case| {
        // Given a thread whose first turn stays and whose later turns write queued.txt, and three messages queued behind the running turn
        let chain = Chain::start();
        chain.cli.script("claude", &Script::of(vec![stays(), writes("queued.txt", "from the queue")]));
        let mut phone = Phone::connect(case, &chain);
        let mut rca = chain.direct();
        phone.seed(case, &chain);
        success(&phone.command(case, &chain, message("m1", "t1", "work")));
        wait_started(&chain, "claude", 1);
        let run = phone.run_of("t1");
        for (item, prompt) in [("q1", "one"), ("q2", "two"), ("q3", "three")] {
            rca.call("agent.queue", "queue.add", json!({"thread": "t1", "item": item, "prompt": prompt})).expect("rca queues it");
        }
        // When the phone force stops the thread for its run
        let stopped = case.call(&chain.tap, &rows(&["rpc:orchestration.forceStopThread"]), || {
            let exit = phone.calls.call("orchestration.forceStopThread", json!({"threadId": "t1", "runId": run}));
            let projection = phone.settled("t1");
            let ended = projection["runs"].as_array().is_some_and(|r| r.iter().all(|r| r["status"] != "running"));
            (exit["_tag"] == "Success" && ended, exit)
        });
        // Then rca answered the force stop (T3's result shape: an outcome and the threads it was shared with), and the CLI was killed
        assert!(answered(case, "thread.forceStop"), "{:?}", asked(case));
        let result = success(&stopped);
        assert!(result["outcome"].is_string() && result.get("sharedWithThreadIds").is_none_or(Value::is_array), "{result}");
        // And the queue is held: nothing starts by itself and all three items are still queued
        std::thread::sleep(std::time::Duration::from_millis(1500));
        assert_eq!(chain.cli.invocations("claude").len(), 1);
        assert_eq!(queued(&mut rca, "t1").len(), 3);
        // When the thread is archived and the phone starts a queued run
        success(&phone.command(case, &chain, cmd("thread.archive", "t1", json!({}))));
        let archived = phone.command(case, &chain, cmd("queued-run.start", "t1", json!({"runId": "q2"})));
        // Then rca refused the start itself: the thread is not active
        assert!(refused_by_rca(case, "queue.resume"), "the request never reached rca: {:?}", asked(case));
        assert_eq!(failure(&archived)["message"], "Thread t1 is not active.");
        success(&phone.command(case, &chain, cmd("thread.unarchive", "t1", json!({}))));
        // When the phone starts the second queued run on the stopped thread
        let before = turns_finished(&mut rca, "t1");
        case.call(&chain.tap, &rows(&["rpc:orchestration.dispatchCommand", "command:queued-run.start"]), || {
            let exit = phone.calls.call("orchestration.dispatchCommand", cmd("queued-run.start", "t1", json!({"runId": "q2"})));
            wait_finished(&mut rca, "t1", before + 1);
            (exit["_tag"] == "Success", exit)
        });
        // Then rca resumed that item, the executor wrote the queued file for it, and the CLI ran with that item's text
        let (agent, executor) = last_window(case);
        assert!(agent.contains(&("queue.resume".to_string(), true)), "{agent:?}");
        assert!(hit(&executor, "tool", "write_file").is_some_and(|h| h["ok"] == true), "{executor}");
        assert!(chain.cli.invocations("claude")[1].argv.iter().any(|a| a.contains("two")), "{:?}", chain.cli.invocations("claude")[1].argv);
    });
}

#[test]
fn resuming_a_held_queue_starts_its_items_one_after_the_other_through_rca_with_the_executor_writing_for_each() {
    exec_testkit::case!(SUITE, |case| {
        // Given a thread force stopped with two items queued behind its turn, so the queue is held
        let chain = Chain::start();
        chain.cli.script("claude", &Script::of(vec![stays(), writes("first.txt", "one"), writes("second.txt", "two")]));
        let mut phone = Phone::connect(case, &chain);
        let mut rca = chain.direct();
        phone.seed(case, &chain);
        success(&phone.command(case, &chain, message("m1", "t1", "work")));
        wait_started(&chain, "claude", 1);
        let run = phone.run_of("t1");
        for (item, prompt) in [("q1", "one"), ("q2", "two")] {
            rca.call("agent.queue", "queue.add", json!({"thread": "t1", "item": item, "prompt": prompt})).expect("rca queues it");
        }
        success(&phone.call(case, &chain, &["rpc:orchestration.forceStopThread"], "orchestration.forceStopThread", json!({"threadId": "t1", "runId": run})));
        wait_finished(&mut rca, "t1", 1);
        // When the phone resumes the queue
        case.call(&chain.tap, &rows(&["rpc:orchestration.dispatchCommand", "command:queue.resume"]), || {
            let exit = phone.calls.call("orchestration.dispatchCommand", cmd("queue.resume", "t1", json!({})));
            wait_finished(&mut rca, "t1", 3);
            (exit["_tag"] == "Success", exit)
        });
        // Then rca answered the resume, both items ran in order through the CLI, and the executor wrote each item's file
        let (agent, executor) = last_window(case);
        assert!(agent.contains(&("queue.resume".to_string(), true)), "{agent:?}");
        assert_eq!(executor.as_array().unwrap().iter().filter(|h| h["tool"] == "write_file" && h["ok"] == true).count(), 2, "{executor}");
        assert_eq!((std::fs::read_to_string(chain.repo.path().join("first.txt")).unwrap(), std::fs::read_to_string(chain.repo.path().join("second.txt")).unwrap()), ("one".to_string(), "two".to_string()));
        assert!(queued(&mut rca, "t1").is_empty());
        let starts = chain.cli.invocations("claude");
        assert_eq!(starts.len(), 3);
        assert!(starts[1].argv.iter().any(|a| a.contains("one")) && starts[2].argv.iter().any(|a| a.contains("two")), "{starts:?}");
    });
}

#[test]
fn a_workflow_stop_and_a_subagent_stop_each_end_the_running_turn_through_rca_and_a_stop_with_nothing_running_is_not_asked_of_rca() {
    exec_testkit::case!(SUITE, |case| {
        // Given a thread whose CLI starts a turn and stays, each time
        let chain = Chain::start();
        chain.cli.script("claude", &Script::of(vec![stays()]));
        let mut phone = Phone::connect(case, &chain);
        phone.seed(case, &chain);
        // When the phone stops a workflow of a thread that runs nothing
        let quiet = phone.call(case, &chain, &["rpc:orchestration.stopWorkflow"], "orchestration.stopWorkflow", json!({"threadId": "t1", "subagentId": "n1"}));
        // Then it is refused as not running and rca was not asked
        assert_eq!(failure(&quiet)["reason"], "not-running");
        assert!(asked(case).is_empty(), "{:?}", asked(case));
        // When a message starts a turn and the workflow is stopped through the RPC
        success(&phone.command(case, &chain, message("m1", "t1", "work")));
        wait_started(&chain, "claude", 1);
        let first = nth_run(&mut phone, "t1", 1);
        let stopped = case.call(&chain.tap, &rows(&["rpc:orchestration.stopWorkflow"]), || {
            let exit = phone.calls.call("orchestration.stopWorkflow", json!({"threadId": "t1", "subagentId": "n1"}));
            let projection = phone.settled("t1");
            (exit["_tag"] == "Success" && none_running(&projection), exit)
        });
        // Then rca was asked to stop the turn's workflow and answered, and the run is no longer running
        assert!(answered(case, "workflow.stop"), "{:?}", asked(case));
        assert!(stopped["_tag"] == "Success" && !first.is_empty());
        // When another turn starts and the phone stops it with the subagent command
        success(&phone.command(case, &chain, message("m2", "t1", "work again")));
        wait_started(&chain, "claude", 2);
        let second = nth_run(&mut phone, "t1", 2);
        phone.command_settled(case, &chain, cmd("subagent.stop", "t1", json!({"runId": second, "subagentId": "n1"})), "t1", none_running);
        // Then rca stopped that turn's workflow too, and the CLI of each turn is gone
        assert!(answered(case, "workflow.stop"), "{:?}", asked(case));
        assert_eq!(chain.cli.invocations("claude").len(), 2);
    });
}

fn none_running(p: &Value) -> bool {
    p["runs"].as_array().is_some_and(|r| !r.is_empty() && r.iter().all(|r| r["status"] != "running"))
}

/// The id of the thread's `n`th run, once the thread has that many.
fn nth_run(phone: &mut Phone, thread: &str, n: usize) -> String {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let projection = success(&phone.calls.call("orchestration.getThreadProjection", json!({"threadId": thread}))).clone();
        if let Some(id) = projection["runs"].as_array().and_then(|r| r.get(n - 1)).and_then(|r| r["id"].as_str()) {
            return id.to_string();
        }
        assert!(std::time::Instant::now() < deadline, "{thread} never got run {n}");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// A turn that asks through one of the service's own MCP tools (which waits for the person) and, once it has its answer, writes `path`.
fn asking(tool: &str, args: Value, path: &str) -> Turn {
    Turn::replaying(&[said("asking")]).then_calling("ask1", tool, args).then_calling("w1", "write_file", json!({"path": path, "content": "after the answer", "create_dirs": true})).then_line(&said("done"))
}

/// What a tool call of the thread printed, as rca logged it.
fn tool_output(rca: &mut Direct, thread: &str, call: &str) -> String {
    let started = std::time::Instant::now();
    loop {
        let out: Vec<String> = rca.events(thread, "tool.output").iter().filter(|e| e["call"] == call).filter_map(|e| e["data"].as_str().map(str::to_string)).collect();
        if !out.is_empty() || started.elapsed() > std::time::Duration::from_secs(30) {
            return out.join("");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
fn a_turn_waits_for_the_phones_approval_or_answer_and_runs_on_only_when_it_gets_one_and_a_dismissal_ends_the_wait() {
    exec_testkit::case!(SUITE, |case| {
        // Given a thread whose turns ask the person through the service's MCP tools before they write: an approval, a question, a question
        let chain = Chain::start();
        chain.cli.script(
            "claude",
            &Script::of(vec![
                asking("approval_prompt", json!({"tool_name": "write_file", "input": {"path": "a.txt"}}), "a.txt"),
                asking("ask_user", json!({"question": "which one?", "options": ["a", "b"]}), "b.txt"),
                asking("ask_user", json!({"question": "sure?"}), "c.txt"),
            ]),
        );
        let mut phone = Phone::connect(case, &chain);
        let mut rca = chain.direct();
        phone.seed(case, &chain);
        // When the first turn asks for approval, it has not written, and the phone accepts the request rca logged
        success(&phone.command(case, &chain, message("m1", "t1", "go")));
        let approval = rca.wait_event("t1", "approval.requested");
        let id = approval["approval"].as_str().unwrap().to_string();
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert!(!chain.repo.path().join("a.txt").exists(), "the turn ran past the approval");
        let accept = cmd("runtime-request.respond", "t1", json!({"requestId": id, "decision": "accept"}));
        phone.command_settled(case, &chain, accept, "t1", completed(1));
        // Then rca took the answer, the turn went on and the executor wrote, and the log says what was decided
        let (agent, executor) = last_window(case);
        assert!(agent.contains(&("approval.answer".to_string(), true)), "{agent:?}");
        assert!(hit(&executor, "tool", "write_file").is_some_and(|h| h["ok"] == true), "{executor}");
        assert_eq!(std::fs::read_to_string(chain.repo.path().join("a.txt")).unwrap(), "after the answer");
        assert_eq!(rca.events("t1", "approval.answered").len(), 1);
        assert!(tool_output(&mut rca, "t1", "ask1").contains("allow"), "the tool was not told to allow");
        // When the second turn asks a question and the phone answers it
        success(&phone.command(case, &chain, message("m2", "t1", "again")));
        let question = rca.wait_event("t1", "input.requested");
        let answer = cmd("runtime-request.respond", "t1", json!({"requestId": question["request"], "answers": {"q1": "b"}}));
        phone.command_settled(case, &chain, answer, "t1", completed(2));
        // Then rca took the answer as the question's text, the turn wrote after it, and the tool's result carried the answer
        assert!(answered(case, "input.answer"), "{:?}", asked(case));
        assert_eq!(std::fs::read_to_string(chain.repo.path().join("b.txt")).unwrap(), "after the answer");
        assert!(tool_output(&mut rca, "t1", "ask1").contains('b'));
        // When the third turn asks and the phone dismisses the question
        success(&phone.command(case, &chain, message("m3", "t1", "last")));
        let last = rca.events("t1", "input.requested").len();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while rca.events("t1", "input.requested").len() < last.max(2) {
            assert!(std::time::Instant::now() < deadline, "the third question never came");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let requested = rca.events("t1", "input.requested");
        let dismiss = cmd("thread.user-input.dismiss", "t1", json!({"requestId": requested.last().unwrap()["request"]}));
        phone.command_settled(case, &chain, dismiss, "t1", completed(3));
        // Then rca closed the request as dismissed
        assert!(answered(case, "input.dismiss"), "{:?}", asked(case));
        assert_eq!(rca.events("t1", "input.dismissed").len(), 1);
    });
}

fn state_value(rca: &mut Direct, key: &str) -> Value {
    rca.call("state", "state.get", json!({"scope": "t3", "key": key})).unwrap_or_else(|e| panic!("rca has no state {key}: {e:?}"))["value"].clone()
}

#[test]
fn the_device_state_commands_leave_their_values_in_rcas_state_group_where_the_side_door_reads_them() {
    exec_testkit::case!(SUITE, |case| {
        // Given a thread, and rca's state read through the side door
        let chain = Chain::start();
        let mut phone = Phone::connect(case, &chain);
        let mut rca = chain.direct();
        phone.seed(case, &chain);
        let model = json!({"instanceId": "claudeAgent", "model": "sonnet"});
        let cases = [
            (cmd("thread.pin", "t1", json!({"orderKey": "k1"})), "thread.pins:t1", "pinOrderKey", Some(json!("k1"))),
            (cmd("thread.pin.reorder", "t1", json!({"orderKey": "k2"})), "thread.pins:t1", "pinOrderKey", Some(json!("k2"))),
            (cmd("thread.unpin", "t1", json!({})), "thread.pins:t1", "pinnedAt", Some(Value::Null)),
            (cmd("thread.active.reorder", "t1", json!({"orderKey": "k3"})), "thread.activeOrder:t1", "orderKey", Some(json!("k3"))),
            (cmd("thread.auto-settle.set", "t1", json!({"enabled": false})), "thread.autoSettle:t1", "autoSettleDisabledAt", None),
            (cmd("thread.visit", "t1", json!({})), "thread.visits:t1", "lastVisitedAt", None),
            (cmd("thread.mark-unread", "t1", json!({})), "thread.visits:t1", "lastVisitedAt", None),
            (cmd("thread.metadata.update", "t1", json!({"title": "Renamed"})), "thread.settings:t1", "title", Some(json!("Renamed"))),
            (cmd("thread.runtime-mode.set", "t1", json!({"runtimeMode": "approval-required"})), "thread.settings:t1", "runtimeMode", Some(json!("approval-required"))),
            (cmd("thread.interaction-mode.set", "t1", json!({"interactionMode": "plan"})), "thread.settings:t1", "interactionMode", Some(json!("plan"))),
            (cmd("thread.model-selection.set", "t1", json!({"modelSelection": model})), "thread.settings:t1", "modelSelection", Some(model.clone())),
        ];
        for (command, key, field, literal) in cases {
            let kind = command["type"].as_str().unwrap().to_string();
            // When the phone sends the command
            success(&phone.command(case, &chain, command));
            // Then rca's state.set answered in its window, and rca's own state holds what the thread's row has (and the value the command set)
            assert!(answered(case, "state.set"), "{kind}: {:?}", asked(case));
            let kept = state_value(&mut rca, key)[field].clone();
            if field != "orderKey" {
                let headers = [("Authorization", phone.bearer.as_str()), ("x-t3-orchestration-protocol", "2")];
                let shell = http(chain.addr, "GET", "/api/orchestration/shell", &headers, "");
                let row = shell.body["threads"].as_array().unwrap().iter().find(|t| t["id"] == "t1").expect("the shell lists t1").clone();
                assert_eq!(kept, row[field], "{kind}: the row and rca disagree");
            }
            if let Some(literal) = literal {
                assert_eq!(kept, literal, "{kind}");
            }
        }
    });
}

#[test]
fn a_turn_item_its_output_a_search_the_family_and_the_snapshots_are_read_from_what_ran_through_rca() {
    exec_testkit::case!(SUITE, |case| {
        // Given a thread whose turn wrote a note and ran a command through the executor
        let chain = Chain::start();
        let turn = Turn::replaying(&[said("working")])
            .then_calling("k1", "write_file", json!({"path": "notes/a.txt", "content": "hello from the model", "create_dirs": true}))
            .then_calling("k2", "run_command", json!({"command": "printf 'out-%s' whales"}))
            .then_line(&said("done"));
        chain.cli.script("claude", &Script::of(vec![turn]));
        let mut phone = Phone::connect(case, &chain);
        let mut rca = chain.direct();
        phone.seed(case, &chain);
        let projection = phone.message(case, &chain, "t1", "c-m1", "make a note about whales");
        let tool = projection["turnItems"].as_array().unwrap().iter().find(|i| i["name"] == "run_command").expect("the turn left its command row").clone();
        // And the executor ran the command to completion
        let (_, executor) = last_window(case);
        assert_eq!(hit(&executor, "tool", "run_command"), Some(&json!({"tool": "run_command", "ok": true})), "the command did not run through rca-exec: {executor}");
        let item = tool["id"].as_str().unwrap();
        // When the phone fetches the item and subscribes to its output
        let fetched = phone.call(case, &chain, &["rpc:orchestration.getTurnItem"], "orchestration.getTurnItem", json!({"threadId": "t1", "itemId": item}));
        let (_, frames) = phone.stream(case, &chain, &["rpc:orchestration.subscribeCommandOutput"], "orchestration.subscribeCommandOutput", json!({"threadId": "t1", "itemId": item}));
        // Then the item is the row the projection has, and the output is one finished replace frame with exactly what rca logged the tool printing
        assert_eq!(success(&fetched)["item"], tool);
        let printed = tool_output(&mut rca, "t1", "k2");
        assert_eq!(printed, "out-whales");
        assert_eq!(frames, [json!({"kind": "replace", "text": printed, "truncated": false, "running": false})]);
        // When the phone searches for a word of its message
        let found = phone.call(case, &chain, &["rpc:orchestration.searchThreads"], "orchestration.searchThreads", json!({"query": "whales", "limit": 5}));
        // Then rca was asked, and the match is the thread, in its project, as a user message with the text around the word
        assert!(answered(case, "thread.search"), "{:?}", asked(case));
        let matches = success(&found)["matches"].as_array().unwrap();
        assert_eq!((matches[0]["threadId"].as_str(), matches[0]["projectId"].as_str(), matches[0]["source"].as_str()), (Some("t1"), Some("p1"), Some("user")));
        assert!(matches[0]["snippet"].as_str().is_some_and(|s| s.contains("whales")), "{matches:?}");
        // When the thread is forked, the family of the fork is asked for, and the fork is archived and the archived snapshot read
        success(&phone.command(case, &chain, json!({"type": "thread.fork", "commandId": "c-fork", "sourceThreadId": "t1", "targetThreadId": "f1", "sourcePoint": {}})));
        let family = phone.call(case, &chain, &["rpc:orchestration.getThreadFamily"], "orchestration.getThreadFamily", json!({"threadId": "f1"}));
        assert!(answered(case, "thread.family"), "{:?}", asked(case));
        assert_eq!(success(&family)["threads"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect::<Vec<_>>(), ["t1", "f1"]);
        success(&phone.command(case, &chain, cmd("thread.archive", "f1", json!({}))));
        let archived = phone.call(case, &chain, &["rpc:orchestration.getArchivedShellSnapshot"], "orchestration.getArchivedShellSnapshot", json!({}));
        // Then the snapshot holds the fork alone, and the shell over HTTP lists the thread live and the fork archived at the same sequence
        assert_eq!(success(&archived)["threads"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect::<Vec<_>>(), ["f1"]);
        let shell = phone.get(case, &chain, &["http:orchestration.shellSnapshot"], "/api/orchestration/shell");
        assert_eq!((shell.body["threads"][0]["id"].as_str(), shell.body["archivedThreads"][0]["id"].as_str()), (Some("t1"), Some("f1")));
        assert_eq!(shell.body["snapshotSequence"], success(&archived)["snapshotSequence"]);
        // And the bounded snapshot of the thread over HTTP is its projection with the run and the note's item
        let bounded = phone.get(case, &chain, &["http:orchestration.threadBoundedSnapshot"], "/api/orchestration/threads/t1/bounded");
        assert_eq!((bounded.body["projection"]["thread"]["id"].as_str(), bounded.body["projection"]["runs"][0]["status"].as_str()), (Some("t1"), Some("completed")));
        assert!(bounded.body["projection"]["turnItems"].as_array().unwrap().iter().any(|i| i["id"] == item));
    });
}

#[test]
fn older_settled_threads_are_paged_from_rcas_shelf_until_the_limit_of_the_asked_project_is_met_across_pages() {
    exec_testkit::case!(SUITE, |case| {
        // Given 56 settled threads in two projects: the 16 of p1 settled first and the 40 of p2 after them, so rca's newest-first pages of 50 hold 40 of p2 and 10 of p1
        let chain = Chain::start();
        let mut phone = Phone::connect(case, &chain);
        phone.seed(case, &chain);
        let spare = json!({"type": "project.create", "commandId": "c-p2", "projectId": "p2", "title": "Spare", "workspaceRoot": chain.repo.path().display().to_string()});
        success(&phone.call(case, &chain, &["rpc:projects.mutate", "mutation:project.create"], "projects.mutate", spare));
        let base = 1_790_000_000_000_i64;
        let threads: Vec<(String, &str, String)> = (0..56)
            .map(|i| {
                let at = iso(base + i * 60_000);
                (format!("s{i:02}"), if i < 16 { "p1" } else { "p2" }, at)
            })
            .collect();
        for (id, project, at) in &threads {
            let create = json!({"type": "thread.create", "commandId": format!("c-{id}"), "threadId": id, "projectId": project, "title": id, "modelSelection": {"instanceId": "claudeAgent", "model": "opus"}});
            success(&phone.calls.call("orchestration.dispatchCommand", create));
            success(&phone.calls.call("orchestration.dispatchCommand", json!({"type": "thread.settle", "commandId": format!("c-settle-{id}"), "threadId": id, "settledAt": at})));
        }
        // When the phone asks for 12 older settled threads of p1
        let before = json!({"settledAt": iso(base + 100 * 60_000)});
        let page = phone.call(case, &chain, &["rpc:orchestration.getOlderSettledThreads"], "orchestration.getOlderSettledThreads", json!({"before": before, "projectIds": ["p1"], "limit": 12}));
        let (agent, _) = last_window(case);
        // Then it took rca's shelf in more than one page, and the rows are the 12 newest settled threads of p1, newest first
        assert!(agent.iter().filter(|(m, ok)| m == "thread.shelf" && *ok).count() >= 2, "{agent:?}");
        let mut expected: Vec<&(String, &str, String)> = threads.iter().filter(|(_, p, _)| *p == "p1").collect();
        expected.sort_by(|a, b| b.2.cmp(&a.2));
        assert_eq!(success(&page)["threads"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_string()).collect::<Vec<_>>(), expected.iter().take(12).map(|(id, _, _)| id.clone()).collect::<Vec<_>>());
        // When only the counts are asked for
        let counts = phone.call(case, &chain, &["rpc:orchestration.getOlderSettledThreads"], "orchestration.getOlderSettledThreads", json!({"before": before, "limit": 0}));
        // Then each project's count is the number of its settled threads
        let mut counted: Vec<(String, u64)> = success(&counts)["counts"].as_array().unwrap().iter().map(|c| (c["projectId"].as_str().unwrap().to_string(), c["count"].as_u64().unwrap())).collect();
        counted.sort();
        assert_eq!(counted, [("p1".to_string(), threads.iter().filter(|(_, p, _)| *p == "p1").count() as u64), ("p2".to_string(), threads.iter().filter(|(_, p, _)| *p == "p2").count() as u64)]);
    });
}

/// `ms` since the epoch as T3's ISO time.
fn iso(ms: i64) -> String {
    t3_contracts::time::iso(ms)
}

#[test]
fn the_requests_rca_cannot_serve_yet_are_refused_with_t3s_words_and_a_refusal_never_serves_its_row() {
    exec_testkit::case!(SUITE, |case| {
        // Given a thread that ran a turn
        let chain = Chain::start();
        chain.cli.script("claude", &Script::of(vec![writes("notes/a.txt", "hello")]));
        let mut phone = Phone::connect(case, &chain);
        phone.seed(case, &chain);
        let projection = phone.message(case, &chain, "t1", "c-m1", "make a note");
        let run = projection["runs"][0]["id"].as_str().unwrap().to_string();
        // When a prepared run is retried, a workflow script and a secret are asked for
        let retry = phone.command(case, &chain, cmd("prepared-run.retry", "t1", json!({"runId": run})));
        assert!(asked(case).is_empty());
        assert_eq!(failure(&retry)["message"], format!("Run {run} has no failed workspace preparation to retry."));
        let script = phone.call(case, &chain, &["rpc:orchestration.getWorkflowScript"], "orchestration.getWorkflowScript", json!({"threadId": "t1", "scriptPath": "/home/u/.claude/projects/x/wf.js"}));
        assert_eq!((failure(&script)["reason"].as_str(), asked(case).is_empty()), (Some("root-unavailable"), true));
        let secret = phone.call(case, &chain, &["rpc:secrets.answerRequest"], "secrets.answerRequest", json!({"threadId": "t1", "turnItemId": "i1", "answer": {"type": "decline"}}));
        // Then each is T3's own refusal, rca was never asked, and no call of them is a served one
        assert_eq!((failure(&secret)["reason"].as_str(), asked(case).is_empty()), (Some("not_found"), true));
        let refused: Vec<&Value> = case.calls().iter().filter(|c| c["rows"].as_array().unwrap().iter().any(|r| ["command:prepared-run.retry", "rpc:orchestration.getWorkflowScript", "rpc:secrets.answerRequest"].contains(&r.as_str().unwrap()))).collect();
        assert!(refused.len() == 3 && refused.iter().all(|c| c["ok"] == false), "{refused:?}");
    });
}
