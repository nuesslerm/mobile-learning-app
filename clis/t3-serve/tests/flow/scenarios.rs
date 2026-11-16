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

