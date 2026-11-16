//! Scenarios over the message, file and turn RPCs, whose shapes were designed here and never recorded from a real turn with a populated input:
//! each is one sequence of calls a phone makes, read back through the RPCs that name what the sequence changed.
use super::support::*;
use std::sync::atomic::Ordering;
use t3_core::ports::{AgentEventKind, Op};

fn all_scopes() -> PairingRequest {
    PairingRequest { scopes: t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect(), ..PairingRequest::default() }
}

/// A project `p1` (root `/workspace/demo`) with a thread `t1`, a socket with every scope, and nothing asked of the agent API yet.
fn started() -> (Harness, Rpc) {
    let h = Harness::start();
    let mut rpc = connect_with(&h, all_scopes());
    seed(&mut rpc);
    forget_seeding(&h);
    (h, rpc)
}

fn message(rpc: &mut Rpc, id: &str, text: &str) {
    success(&rpc.call("orchestration.dispatchCommand", json!({"type": "message.dispatch", "commandId": format!("c-{id}"), "threadId": "t1", "message": {"messageId": format!("m-{id}"), "text": text}})));
}

fn texts(projection: &Value) -> Vec<(String, String)> {
    projection["messages"].as_array().unwrap().iter().map(|m| (m["role"].as_str().unwrap().to_string(), m["text"].as_str().unwrap().to_string())).collect()
}

#[test]
fn two_messages_in_turn_are_two_runs_each_with_its_answer_and_the_agent_api_is_asked_for_a_turn_per_message() {
    // Given a thread whose agent answers with one command and a sentence
    let (h, mut rpc) = started();
    *h.agent.script.lock().unwrap() = vec![
        AgentEventKind::ToolStarted { id: "k".into(), name: "bash".into(), input: json!({"command": "ls"}) },
        AgentEventKind::ToolOutput { id: "k".into(), data: "a\n".into() },
        AgentEventKind::ToolFinished { id: "k".into(), ok: true },
        AgentEventKind::Text("done".into()),
    ];
    // When a message is sent and its turn has ended, then a second one
    message(&mut rpc, "1", "list it");
    let first = settled(&mut rpc, "t1");
    let tool = first["turnItems"].as_array().unwrap().iter().find(|i| i["type"] == "command_execution").unwrap().clone();
    message(&mut rpc, "2", "again");
    let second = settled(&mut rpc, "t1");
    // Then the first projection holds one completed run, the question and the answer, and the command as a turn item that can be fetched alone
    assert_eq!(first["runs"].as_array().unwrap().iter().map(|r| r["status"].as_str().unwrap()).collect::<Vec<_>>(), ["completed"]);
    assert_eq!(texts(&first), [("user".to_string(), "list it".to_string()), ("assistant".to_string(), "done".to_string())]);
    assert_eq!(success(&rpc.call("orchestration.getTurnItem", json!({"threadId": "t1", "itemId": tool["id"]})))["item"], tool);
    // And the second holds both runs completed and both exchanges in order, with the first run's item unchanged
    assert_eq!(second["runs"].as_array().unwrap().iter().map(|r| r["status"].as_str().unwrap()).collect::<Vec<_>>(), ["completed", "completed"]);
    assert_eq!(texts(&second).iter().map(|(r, t)| (r.as_str(), t.as_str())).collect::<Vec<_>>(), [("user", "list it"), ("assistant", "done"), ("user", "again"), ("assistant", "done")]);
    assert_eq!(success(&rpc.call("orchestration.getTurnItem", json!({"threadId": "t1", "itemId": tool["id"]})))["item"], tool);
    // And the agent API was asked for one turn per message with its text
    assert_eq!(h.agent.turns.lock().unwrap().iter().map(|t| t.prompt.clone()).collect::<Vec<_>>(), ["list it", "again"]);
}

#[test]
fn a_file_written_is_listed_and_read_back_by_the_next_calls_and_a_second_write_replaces_it() {
    // Given an agent API that keeps what it is told to write under the project's root, and one file already there
    let (h, mut rpc) = started();
    h.agent.files.lock().unwrap().insert("/workspace/demo/README.md".into(), "# demo".into());
    let files = h.agent.clone();
    *h.agent.responder.lock().unwrap() = Some(Box::new(move |op| match op {
        Op::FileWrite { root, path, content } => {
            files.files.lock().unwrap().insert(format!("{root}/{path}"), content.clone());
            Some(Ok(json!({"path": path, "bytes": content.len()})))
        }
        Op::FileStat { path, .. } => Some(files.files.lock().unwrap().get(path).map_or_else(|| Err(t3_core::ports::AgentError::Failed("missing".into())), |c| Ok(json!({"kind": "file", "size": c.len(), "modified_ms": 0})))),
        _ => None,
    }));
    let list = |rpc: &mut Rpc, path: &str| -> Vec<String> {
        success(&rpc.call("projects.listEntries", json!({"projectId": "p1", "relativePath": path})))["entries"].as_array().unwrap().iter().map(|e| e["path"].as_str().unwrap().to_string()).collect()
    };
    let read = |rpc: &mut Rpc, path: &str| rpc.call("projects.readFile", json!({"cwd": "/workspace/demo", "relativePath": path}));
    // When the phone lists the root, writes a file below a new directory, lists, reads it back, writes it again and reads and measures it
    let before = list(&mut rpc, "");
    let wrote = rpc.call("projects.writeFile", json!({"cwd": "/workspace/demo", "relativePath": "src/new.txt", "contents": "one\n"}));
    let (root, src) = (list(&mut rpc, ""), list(&mut rpc, "src"));
    let once = read(&mut rpc, "src/new.txt");
    success(&rpc.call("projects.writeFile", json!({"cwd": "/workspace/demo", "relativePath": "src/new.txt", "contents": "two lines\n"})));
    let twice = read(&mut rpc, "src/new.txt");
    let measured = rpc.call("filesystem.getMetadata", json!({"paths": ["/workspace/demo/src/new.txt", "/workspace/demo/gone"]}));
    // Then the write is acknowledged by its path, the directory appears in the root listing and the file in its own, and each read is what was last written
    assert_eq!(before, ["README.md"]);
    assert_eq!(success(&wrote), &json!({"relativePath": "src/new.txt"}));
    assert_eq!((root, src), (vec!["README.md".to_string(), "src".to_string()], vec!["src/new.txt".to_string()]));
    assert_eq!((success(&once)["contents"].as_str(), success(&twice)["contents"].as_str()), (Some("one\n"), Some("two lines\n")));
    // And the measure is the byte length of the second write, and null for the path that is not there
    assert_eq!(success(&measured)["entries"][0]["byteLength"], 10);
    assert_eq!(success(&measured)["entries"][1], Value::Null);
}

#[test]
fn the_diff_of_a_thread_can_be_taken_only_at_its_finished_turns_so_a_running_turn_moves_it_when_it_is_interrupted() {
    // Given a thread with one finished turn and an agent API that diffs the thread's directory
    let (h, mut rpc) = started();
    h.agent.replies.lock().unwrap().insert("turn.diff", Ok(json!({"diff": "diff --git a/f b/f\n"})));
    message(&mut rpc, "1", "go");
    settled(&mut rpc, "t1");
    let turn_diff = |rpc: &mut Rpc, to: u64| rpc.call("orchestration.getTurnDiff", json!({"threadId": "t1", "fromTurnCount": 0, "toTurnCount": to}));
    // When the diff of turns 0 to 1 is asked, a second turn is started and held, 0 to 1 and 0 to 2 are asked, and the turn is interrupted
    let one = turn_diff(&mut rpc, 1);
    h.agent.hold.store(true, Ordering::SeqCst);
    message(&mut rpc, "2", "wait");
    let while_running = (turn_diff(&mut rpc, 1), turn_diff(&mut rpc, 2));
    success(&rpc.call("orchestration.dispatchCommand", json!({"type": "run.interrupt", "commandId": "c-int", "threadId": "t1"})));
    let after = settled(&mut rpc, "t1");
    let (stale, now) = (turn_diff(&mut rpc, 1), turn_diff(&mut rpc, 2));
    // Then the first diff is the agent API's text, and while the second turn ran the diff of turns 0 to 1 still was and 0 to 2 was refused as unfinished
    assert_eq!(success(&one)["diff"], "diff --git a/f b/f\n");
    assert_eq!(success(&while_running.0)["toTurnCount"], 1);
    assert!(failure(&while_running.1)["message"].as_str().unwrap().contains("0 to 1"));
    // And once the run ended interrupted it counts as finished: 0 to 2 is given and 0 to 1 is refused naming 0 to 2
    assert_eq!(after["runs"][1]["status"], "interrupted");
    assert_eq!(success(&now)["toTurnCount"], 2);
    assert!(failure(&stale)["message"].as_str().unwrap().contains("0 to 2"));
}
