//! `orchestration.launchThread` through the socket: a new task in one call, a retry that writes nothing, the workspaces this
//! server builds (the project root, an existing worktree, a reused empty thread) and the launches it refuses.
use super::support::*;
use std::time::{Duration, Instant};
use t3_core::ports::{AgentError, AgentEventKind, EventStore, Op, SetupBatch, SetupEvent};

/// The launch the phone sends for a new task on the project root (`startThreadTurn` with a thread draft).
fn launch(command_id: &str, thread: Option<&str>) -> Value {
    let mut payload = json!({
        "commandId": command_id,
        "creationSource": "mobile",
        "projectId": "p1",
        "title": "Fix the build",
        "generateTitle": true,
        "modelSelection": {"instanceId": "opencode", "model": "rca-default"},
        "runtimeMode": "full-access",
        "interactionMode": "default",
        "workspaceStrategy": {"type": "root"},
        "initialMessage": {"messageId": format!("m-{command_id}"), "text": "make it build", "attachments": []},
    });
    if let Some(thread) = thread {
        payload["threadId"] = json!(thread);
    }
    payload
}

fn launch_error(exit: &Value) -> Value {
    let error = failure(exit).clone();
    assert_eq!(error["_tag"], "OrchestrationV2ThreadLaunchError", "{exit}");
    error
}

fn started() -> (Harness, Rpc) {
    let h = Harness::start();
    let mut rpc = connect(&h);
    seed(&mut rpc);
    forget_seeding(&h);
    (h, rpc)
}

#[test]
fn a_new_task_is_one_launch_that_creates_the_thread_runs_its_first_message_and_answers_the_projection() {
    // Given a project and an agent API whose turn answers with text
    let (h, mut rpc) = started();
    *h.agent.script.lock().unwrap() = vec![AgentEventKind::Text("built".into())];
    // When the phone launches a new task with a thread id it chose
    let exit = rpc.call("orchestration.launchThread", launch("c-new", Some("t-new")));
    // Then the answer names the thread, is not a resumption, carries the message's sequence and a projection of the new thread
    let answer = success(&exit).clone();
    assert_eq!((answer["threadId"].as_str(), answer["resumed"].as_bool()), (Some("t-new"), Some(false)));
    assert!(answer["sequence"].as_u64().is_some_and(|s| s > 2 && s <= h.store.last_sequence().unwrap()), "{answer}");
    let thread = &answer["projection"]["thread"];
    assert_eq!((thread["id"].as_str(), thread["title"].as_str(), thread["creationSource"].as_str()), (Some("t-new"), Some("Fix the build"), Some("mobile")));
    // And the agent API was asked to start the message's turn on the thread, and the thread holds both sides of the turn
    let p = settled(&mut rpc, "t-new");
    let turn = h.agent.turns.lock().unwrap()[0].clone();
    assert_eq!((turn.thread_id.as_str(), turn.prompt.as_str(), turn.instance_id.as_str(), turn.model.as_deref()), ("t-new", "make it build", "opencode", Some("rca-default")));
    let messages: Vec<(&str, &str)> = p["messages"].as_array().unwrap().iter().map(|m| (m["id"].as_str().unwrap(), m["text"].as_str().unwrap())).collect();
    assert_eq!(messages.first(), Some(&("m-c-new", "make it build")));
    assert_eq!(messages.last().map(|m| m.1), Some("built"));
    // And the shell lists it next to the seeded thread
    let shell = rpc.send("orchestration.subscribeShell", json!({}));
    let ids: Vec<String> = rpc.chunk(&shell)[0]["snapshot"]["threads"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_string()).collect();
    assert!(ids.contains(&"t-new".to_string()) && ids.contains(&"t1".to_string()), "{ids:?}");
}

#[test]
fn a_retried_launch_is_resumed_writes_nothing_and_without_a_thread_id_lands_on_the_thread_the_first_made() {
    // Given a launch without a thread id that was answered and whose turn ended
    let (h, mut rpc) = started();
    let first = success(&rpc.call("orchestration.launchThread", launch("c-retry", None))).clone();
    let thread = first["threadId"].as_str().unwrap().to_string();
    settled(&mut rpc, &thread);
    let written = h.store.last_sequence().unwrap();
    // When the phone sends the same launch again
    let again = success(&rpc.call("orchestration.launchThread", launch("c-retry", None))).clone();
    // Then it is resumed on the same thread with the same message sequence, nothing more was stored, and one turn was started
    assert_eq!((again["threadId"].as_str(), again["resumed"].as_bool(), &again["sequence"]), (Some(thread.as_str()), Some(true), &first["sequence"]));
    assert_eq!(h.store.last_sequence().unwrap(), written);
    assert_eq!(h.agent.turns.lock().unwrap().len(), 1);
}

#[test]
fn a_worktree_launch_a_malformed_attachment_a_missing_project_and_a_reuse_without_a_thread_are_launch_errors_and_write_nothing() {
    // Given launches this server refuses, each the phone's launch with one field changed
    let (h, mut rpc) = started();
    let before = h.store.last_sequence().unwrap();
    let mut worktree = launch("c-wt", Some("t-wt"));
    worktree["workspaceStrategy"] = json!({"type": "worktree", "baseRef": "main", "branch": "fix"});
    worktree.as_object_mut().unwrap().remove("initialMessage");
    let mut attachment = launch("c-att", Some("t-att"));
    attachment["initialMessage"]["attachments"] = json!([{"type": "image", "id": "not an id", "name": "shot.png", "mimeType": "image/png", "sizeBytes": 3}]);
    let mut orphan = launch("c-orphan", Some("t-orphan"));
    orphan["projectId"] = json!("nope");
    let mut reuse = launch("c-reuse", None);
    reuse["reuseExistingThread"] = json!(true);
    for (payload, says) in [(worktree, "needs its first message"), (attachment, "every attachment needs an id"), (orphan, "Project not found"), (reuse, "requires a thread id")] {
        // When each is launched
        let error = launch_error(&rpc.call("orchestration.launchThread", payload.clone()));
        // Then the error echoes the command and project and says why
        assert_eq!((&error["commandId"], &error["projectId"]), (&payload["commandId"], &payload["projectId"]));
        let message = error["message"].as_str().unwrap();
        assert!(message.starts_with("Failed to launch thread") && message.contains(says), "{message}");
    }
    // And nothing was stored or run, the agent API was not asked, and the log names each refusal
    assert_eq!(h.store.last_sequence().unwrap(), before);
    assert!(h.agent.turns.lock().unwrap().is_empty() && h.agent.calls.lock().unwrap().is_empty());
    assert_eq!(h.log.lines().iter().filter(|l| l.starts_with("launch.refused commandId=c-")).count(), 4);
    // And a launch without its workspace strategy is a decoder defect naming the key
    let mut undecodable = launch("c-bad", None);
    undecodable.as_object_mut().unwrap().remove("workspaceStrategy");
    assert_eq!(rpc.call("orchestration.launchThread", undecodable)["cause"][0]["defect"], "Missing key\n  at [\"workspaceStrategy\"]");
}

#[test]
fn an_existing_worktree_launch_records_the_worktree_and_a_launch_without_a_message_only_creates_the_thread() {
    // Given a launch on an existing worktree, and one with no first message
    let (h, mut rpc) = started();
    let mut on_worktree = launch("c-ew", Some("t-ew"));
    on_worktree["workspaceStrategy"] = json!({"type": "existing_worktree", "worktreePath": "/workspace/demo-fix", "branch": "fix"});
    let mut bare = launch("c-bare", Some("t-bare"));
    bare.as_object_mut().unwrap().remove("initialMessage");
    // When both are launched
    let ew = success(&rpc.call("orchestration.launchThread", on_worktree)).clone();
    let created = success(&rpc.call("orchestration.launchThread", bare)).clone();
    settled(&mut rpc, "t-ew");
    // Then the worktree thread records its path and branch, and only it started a turn: the turn runs in the workspace the executor serves
    assert_eq!((ew["projection"]["thread"]["worktreePath"].as_str(), ew["projection"]["thread"]["branch"].as_str()), (Some("/workspace/demo-fix"), Some("fix")));
    let started: Vec<String> = h.agent.turns.lock().unwrap().iter().map(|t| t.thread_id.clone()).collect();
    assert_eq!(started, ["t-ew"]);
    // And the bare launch made a thread with no message, no run and no sequence in its answer
    assert_eq!((created["threadId"].as_str(), created["resumed"].as_bool(), created.get("sequence")), (Some("t-bare"), Some(false), None));
    assert_eq!((created["projection"]["messages"].as_array().map(Vec::len), created["projection"]["runs"].as_array().map(Vec::len)), (Some(0), Some(0)));
}

#[test]
fn an_empty_thread_can_be_reused_for_a_launch_and_takes_its_workspace_but_one_with_history_cannot() {
    // Given the seeded thread, still empty
    let (h, mut rpc) = started();
    let mut reuse = launch("c-r1", Some("t1"));
    reuse["reuseExistingThread"] = json!(true);
    reuse["workspaceStrategy"] = json!({"type": "existing_worktree", "worktreePath": "/workspace/demo-r", "branch": "r"});
    // When a launch reuses it
    let answer = success(&rpc.call("orchestration.launchThread", reuse.clone())).clone();
    settled(&mut rpc, "t1");
    // Then the thread keeps its id and title, takes the worktree, and its turn is the thread's own
    let thread = &answer["projection"]["thread"];
    assert_eq!((thread["id"].as_str(), thread["title"].as_str(), thread["worktreePath"].as_str()), (Some("t1"), Some("Thread t1"), Some("/workspace/demo-r")));
    assert_eq!(h.agent.turns.lock().unwrap()[0].thread_id, "t1");
    // And a second reuse, now that it holds a turn, is refused
    reuse["commandId"] = json!("c-r2");
    let error = launch_error(&rpc.call("orchestration.launchThread", reuse));
    assert!(error["message"].as_str().unwrap().contains("Only an empty active thread"), "{error}");
}

/// A launch that asks for a worktree made from `main` for its first message.
fn worktree_launch(command_id: &str, thread: &str) -> Value {
    let mut payload = launch(command_id, Some(thread));
    payload["workspaceStrategy"] = json!({"type": "worktree", "baseRef": "main", "branch": "feature/fix"});
    payload
}

/// The agent API's answer to a made worktree, and the events of a setup that ran one command and ended `ok`.
fn made(path: &str) -> Value {
    json!({"workspace": "feature-fix-abc", "repo": "w1", "executor": "w1", "path": path, "branch": "feature/fix", "head": "0000000", "setup": true})
}

fn setup(ok: bool) -> SetupBatch {
    let event = |seq, kind: &str, data: Value| SetupEvent { seq, kind: kind.into(), data };
    SetupBatch { ran: true, events: vec![event(1, "setup.started", json!({"steps": 1})), event(2, "step.started", json!({"command": "pnpm install"})), event(3, "step.finished", json!({"step": 0, "exit_code": i32::from(!ok)})), event(4, "setup.finished", json!({"ok": ok}))] }
}

/// The thread's projection once its first run is `status`, within 5 s.
fn run_is(rpc: &mut Rpc, thread: &str, status: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let projection = success(&rpc.call("orchestration.getThreadProjection", json!({"threadId": thread}))).clone();
        if projection["runs"][0]["status"] == status || Instant::now() > deadline {
            return projection;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn worktree_ops(h: &Harness) -> Vec<Op> {
    h.agent.ops.lock().unwrap().iter().filter(|op| matches!(op, Op::WorktreeCreate { .. } | Op::WorktreeRemove { .. })).cloned().collect()
}

#[test]
fn a_worktree_launch_answers_with_the_run_preparing_and_then_makes_the_worktree_records_it_follows_the_setup_and_starts_the_turn_in_it() {
    // Given an agent API that makes the worktree and runs a setup that succeeds, and a turn that answers with text
    let (h, mut rpc) = started();
    h.agent.replies.lock().unwrap().insert("worktree.create", Ok(made("/base/feature-fix-abc")));
    h.agent.setups.lock().unwrap().insert("/base/feature-fix-abc".into(), setup(true));
    *h.agent.script.lock().unwrap() = vec![AgentEventKind::Text("built".into())];
    // When the phone launches a task on a new worktree from main
    let exit = rpc.call("orchestration.launchThread", worktree_launch("c-wt", "t-wt"));
    let answer = success(&exit).clone();
    // Then the launch is answered at once: the thread exists and its run is not running
    assert_eq!((answer["threadId"].as_str(), answer["resumed"].as_bool()), (Some("t-wt"), Some(false)));
    assert_ne!(answer["projection"]["runs"][0]["status"], "running");
    // And the worktree is made from main on the branch asked for, in the project's directory, and recorded on the thread
    let projection = run_is(&mut rpc, "t-wt", "running");
    let thread = &projection["thread"];
    assert_eq!((thread["worktreePath"].as_str(), thread["branch"].as_str()), (Some("/base/feature-fix-abc"), Some("feature/fix")));
    let ops = worktree_ops(&h);
    let [Op::WorktreeCreate { root, branch, base, .. }] = ops.as_slice() else { panic!("{ops:?}") };
    assert_eq!((root.as_str(), branch.as_str(), base.as_deref()), ("/workspace/demo", "feature/fix", Some("main")));
    // And the workspace row went through both phases and ended ready, and the turn started on the first message
    let settled = settled(&mut rpc, "t-wt");
    let row = settled["turnItems"].as_array().unwrap().iter().find(|i| i["type"] == "command_execution").cloned().unwrap();
    assert_eq!((row["status"].as_str(), row["title"].as_str(), row["exitCode"].as_i64()), (Some("completed"), Some("Workspace ready"), Some(0)));
    let turns = h.agent.turns.lock().unwrap().clone();
    assert_eq!((turns.len(), turns[0].thread_id.as_str(), turns[0].prompt.as_str()), (1, "t-wt", "make it build"));
    let calls = h.log.lines();
    assert!(calls.iter().all(|l| !l.starts_with("run.preparation.failed")), "{calls:?}");
}

#[test]
fn a_preparation_that_fails_leaves_the_run_failed_with_the_reason_and_a_retry_prepares_again_and_starts_the_turn() {
    // Given an agent API that cannot make the worktree
    let (h, mut rpc) = started();
    h.agent.replies.lock().unwrap().insert("worktree.create", Err(AgentError::Failed("base main is not a branch".into())));
    // When the phone launches on a new worktree
    success(&rpc.call("orchestration.launchThread", worktree_launch("c-wt", "t-wt")));
    let failed = run_is(&mut rpc, "t-wt", "failed");
    // Then the run is failed, the workspace row and the error row say why, and no turn was started
    let items = failed["turnItems"].as_array().unwrap();
    let row = items.iter().find(|i| i["type"] == "command_execution").unwrap();
    let error = items.iter().find(|i| i["type"] == "error").unwrap();
    assert_eq!((row["status"].as_str(), row["output"].as_str(), error["failure"]["code"].as_str(), error["failure"]["message"].as_str()), (Some("failed"), Some("base main is not a branch"), Some("workspace_preparation_failed"), Some("base main is not a branch")));
    assert!(h.agent.turns.lock().unwrap().is_empty());
    // And when the agent API can make it and the phone retries the preparation of that run
    h.agent.replies.lock().unwrap().insert("worktree.create", Ok(made("/base/feature-fix-abc")));
    h.agent.setups.lock().unwrap().insert("/base/feature-fix-abc".into(), setup(true));
    let run = failed["runs"][0]["id"].as_str().unwrap().to_string();
    let retry = json!({"type": "prepared-run.retry", "commandId": "c-retry", "threadId": "t-wt", "runId": run});
    success(&rpc.call("orchestration.dispatchCommand", retry.clone()));
    // Then the run prepares again, starts, and the error row is taken back
    let started = run_is(&mut rpc, "t-wt", "running");
    assert_eq!(started["thread"]["worktreePath"], "/base/feature-fix-abc");
    let items = started["turnItems"].as_array().unwrap();
    assert_eq!(items.iter().find(|i| i["type"] == "error").unwrap()["status"], "cancelled");
    settled(&mut rpc, "t-wt");
    assert_eq!(h.agent.turns.lock().unwrap().len(), 1);
    // And a retry of a run that is no longer failed is refused in T3's words
    let again = rpc.call("orchestration.dispatchCommand", json!({"type": "prepared-run.retry", "commandId": "c-retry-2", "threadId": "t-wt", "runId": run}));
    assert!(failure(&again)["message"].as_str().is_some_and(|m| m.contains("has no failed workspace preparation to retry")), "{again}");
}

#[test]
fn a_setup_script_that_fails_removes_the_worktree_forgets_it_on_the_thread_and_a_retry_makes_it_again() {
    // Given a worktree whose setup fails
    let (h, mut rpc) = started();
    h.agent.replies.lock().unwrap().insert("worktree.create", Ok(made("/base/feature-fix-abc")));
    h.agent.setups.lock().unwrap().insert("/base/feature-fix-abc".into(), setup(false));
    // When the phone launches on a new worktree
    success(&rpc.call("orchestration.launchThread", worktree_launch("c-wt", "t-wt")));
    let failed = run_is(&mut rpc, "t-wt", "failed");
    // Then the failure names the exit code, the worktree was removed, and the thread no longer names it
    let error = failed["turnItems"].as_array().unwrap().iter().find(|i| i["type"] == "error").cloned().unwrap();
    assert_eq!(error["failure"]["message"], "Setup script exited with 1.");
    assert_eq!((failed["thread"]["worktreePath"].clone(), failed["thread"]["branch"].clone()), (Value::Null, Value::Null));
    assert!(matches!(worktree_ops(&h).as_slice(), [Op::WorktreeCreate { .. }, Op::WorktreeRemove { root }] if root == "/base/feature-fix-abc"), "{:?}", worktree_ops(&h));
    // And a retry with a setup that now succeeds makes the worktree a second time
    h.agent.setups.lock().unwrap().insert("/base/feature-fix-abc".into(), setup(true));
    let run = failed["runs"][0]["id"].as_str().unwrap().to_string();
    success(&rpc.call("orchestration.dispatchCommand", json!({"type": "prepared-run.retry", "commandId": "c-retry", "threadId": "t-wt", "runId": run})));
    run_is(&mut rpc, "t-wt", "running");
    assert_eq!(worktree_ops(&h).iter().filter(|op| matches!(op, Op::WorktreeCreate { .. })).count(), 2);
}

#[test]
fn a_preparation_that_fails_three_times_in_a_row_is_reported_each_time_and_the_next_retry_still_records_its_worktree_and_starts_the_turn() {
    // Given an agent API that cannot make the worktree
    let (h, mut rpc) = started();
    h.agent.replies.lock().unwrap().insert("worktree.create", Err(AgentError::Failed("base main is not a branch".into())));
    success(&rpc.call("orchestration.launchThread", worktree_launch("c-wt", "t-wt")));
    let mut failed = run_is(&mut rpc, "t-wt", "failed");
    let run = failed["runs"][0]["id"].as_str().unwrap().to_string();
    // When the phone retries twice and each retry fails again
    for n in 0..2 {
        success(&rpc.call("orchestration.dispatchCommand", json!({"type": "prepared-run.retry", "commandId": format!("c-retry-{n}"), "threadId": "t-wt", "runId": run})));
        failed = run_is(&mut rpc, "t-wt", "failed");
        // Then the run ends failed again instead of staying preparing
        assert_eq!(failed["runs"][0]["status"], "failed", "retry {n}");
    }
    // And a third retry that can make the worktree records it on the thread and starts the turn in it
    h.agent.replies.lock().unwrap().insert("worktree.create", Ok(made("/base/feature-fix-abc")));
    h.agent.setups.lock().unwrap().insert("/base/feature-fix-abc".into(), setup(true));
    success(&rpc.call("orchestration.dispatchCommand", json!({"type": "prepared-run.retry", "commandId": "c-retry-2", "threadId": "t-wt", "runId": run})));
    let started = run_is(&mut rpc, "t-wt", "running");
    assert_eq!(started["thread"]["worktreePath"], "/base/feature-fix-abc");
    settled(&mut rpc, "t-wt");
    assert_eq!(h.agent.turns.lock().unwrap()[0].worktree.as_deref(), Some("/base/feature-fix-abc"));
}
