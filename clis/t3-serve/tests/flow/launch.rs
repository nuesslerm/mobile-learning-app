//! `orchestration.launchThread` through the socket: a new task in one call, a retry that writes nothing, the workspaces this
//! server builds (the project root, an existing worktree, a reused empty thread) and the launches it refuses.
use super::support::*;
use t3_core::ports::{AgentEventKind, EventStore};

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
    let mut attachment = launch("c-att", Some("t-att"));
    attachment["initialMessage"]["attachments"] = json!([{"type": "image", "id": "not an id", "name": "shot.png", "mimeType": "image/png", "sizeBytes": 3}]);
    let mut orphan = launch("c-orphan", Some("t-orphan"));
    orphan["projectId"] = json!("nope");
    let mut reuse = launch("c-reuse", None);
    reuse["reuseExistingThread"] = json!(true);
    for (payload, says) in [(worktree, "worktree"), (attachment, "every attachment needs an id"), (orphan, "Project not found"), (reuse, "requires a thread id")] {
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
