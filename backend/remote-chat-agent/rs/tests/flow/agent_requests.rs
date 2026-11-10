//! `agent.requests` end to end over the agent socket: the scripted CLI of `exec-testkit` calls `approval_prompt` and `ask_user` on the thread's MCP
//! server as Claude Code and a model do, the service records the request on the thread and holds the call, and the person's answer on the socket is
//! what the CLI reads back. No executor is asked for any of it: these tools are the service's own.
use crate::agent_threads::{chain, create, kinds, said, seqs};
use crate::harness::Env;
use agent_api_proto::Event;
use exec_testkit::script::Turn;
use serde_json::{json, Value};

fn approval_call() -> Value {
    json!({"tool_name": "write_file", "input": {"path": "a.txt", "content": "x"}})
}

/// What the CLI printed for the call `call`: the text of its `tool.output`.
fn output_of(events: &[Event], call: &str) -> Value {
    let text = events.iter().find(|e| e.kind == "tool.output" && e.data["call"] == json!(call)).unwrap_or_else(|| panic!("no output for {call} in {:?}", kinds(events))).data["data"].as_str().unwrap().to_string();
    serde_json::from_str(&text).unwrap_or(Value::String(text))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_turn_in_the_mode_that_asks_when_the_person_accepts_the_tool_then_the_cli_reads_allow_and_the_log_has_the_request_and_the_answer_in_order() {
    // Given a turn that asks whether write_file may run, and then says it is done, started in the mode that asks before each tool
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    chain.script(vec![Turn::replaying(&[said("asking")]).then_calling("k1", "approval_prompt", approval_call()).then_line(&said("done"))]);
    let mut agent = chain.agent().await;
    create(&mut agent, "t1").await;
    agent.ask("thread.subscribe", json!({"thread": "t1"})).await;
    let started = agent.ask("turn.start", json!({"thread": "t1", "workspace": "w1", "driver": "claudeAgent", "prompt": "write it", "runtime_mode": "approval-required"})).await;
    let turn = started["result"]["turn"].as_str().unwrap().to_string();
    // When the request arrives and is accepted
    let mut seen = agent.events_through("approval.requested").await;
    let asked = seen.last().unwrap().clone();
    let answered = agent.ask("approval.answer", json!({"thread": "t1", "approval": asked.data["approval"], "decision": "accept"})).await;
    seen.extend(agent.events_through("turn.finished").await);
    // Then the request names the tool and its input and is of the running turn, and the answer's seq is the answered event's
    assert_eq!((asked.data["tool"].clone(), asked.data["args"].clone(), asked.turn.as_deref()), (json!("write_file"), approval_call()["input"].clone(), Some(turn.as_str())));
    let recorded = seen.iter().find(|e| e.kind == "approval.answered").unwrap();
    assert_eq!(answered["result"], json!({"thread": "t1", "approval": asked.data["approval"], "decision": "accept", "seq": recorded.seq}));
    // And the events are the CLI's call, the request, its answer, what the CLI read, and the rest of the turn, in seq order
    let mut order: Vec<&str> = kinds(&seen).into_iter().filter(|k| !k.starts_with("message") && !k.starts_with("turn.started")).collect();
    // The CLI's call and the executor's request arrive on two channels, so their mutual order is not fixed
    order[..2].sort();
    assert_eq!(order, ["approval.requested", "tool.started", "approval.answered", "tool.output", "tool.finished", "turn.finished"]);
    assert!(seqs(&seen).windows(2).all(|pair| pair[0] < pair[1]));
    // And the CLI read an allow carrying the input it asked about, and was started with the executor's approval tool as its permission prompt
    assert_eq!(output_of(&seen, "k1"), json!({"behavior": "allow", "updatedInput": approval_call()["input"]}));
    let argv = chain.cli.invocations("claude")[0].argv.clone();
    assert!(argv.windows(2).any(|w| w == ["--permission-prompt-tool", "mcp__executor__approval_prompt"]), "{argv:?}");
    // And nothing was written: the call was the service's to answer, and the executor was not asked
    assert_eq!(chain.repo.git(&["status", "--porcelain"]), "");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_turn_that_is_declined_and_then_asks_a_question_when_each_is_answered_then_the_cli_reads_the_denial_and_then_the_answer() {
    // Given a turn that asks about a tool, then asks which colour
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    chain.script(vec![Turn::replaying(&[]).then_calling("k1", "approval_prompt", approval_call()).then_calling("k2", "ask_user", json!({"question": "Which colour?", "options": ["red", "blue"]})).then_line(&said("ok"))]);
    let mut agent = chain.agent().await;
    create(&mut agent, "t1").await;
    agent.ask("thread.subscribe", json!({"thread": "t1"})).await;
    agent.ask("turn.start", json!({"thread": "t1", "workspace": "w1", "driver": "claudeAgent", "prompt": "go"})).await;
    // When the first is declined and the question answered
    let mut rest = agent.events_through("approval.requested").await;
    let approval = rest.last().unwrap().clone();
    agent.ask("approval.answer", json!({"thread": "t1", "approval": approval.data["approval"], "decision": "decline"})).await;
    rest.extend(agent.events_through("input.requested").await);
    let question = rest.last().unwrap().clone();
    let answered = agent.ask("input.answer", json!({"thread": "t1", "request": question.data["request"], "answer": "blue"})).await;
    rest.extend(agent.events_through("turn.finished").await);
    // Then the question carried its prompt and options, the answer names the request, and the CLI read the denial and then "blue"
    assert_eq!((question.data["prompt"].clone(), question.data["options"].clone()), (json!("Which colour?"), json!(["red", "blue"])));
    assert_eq!(answered["result"]["request"], question.data["request"]);
    assert_eq!(output_of(&rest, "k1")["behavior"], json!("deny"));
    assert_eq!(output_of(&rest, "k2"), json!("blue"));
    // And the turn completed
    assert_eq!(rest.last().unwrap().data["reason"], json!("completed"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_question_that_is_dismissed_and_a_tool_that_is_cancelled_when_answered_then_the_cli_reads_each_and_the_cancel_ends_the_turn_interrupted() {
    // Given a turn that asks a question, then asks about a tool, and would go on for a while
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    chain.script(vec![Turn::replaying(&[]).then_calling("k1", "ask_user", json!({"question": "Sure?"})).then_calling("k2", "approval_prompt", approval_call()).then_sleeping(3000).then_line(&said("never said"))]);
    let mut agent = chain.agent().await;
    create(&mut agent, "t1").await;
    agent.ask("thread.subscribe", json!({"thread": "t1"})).await;
    agent.ask("turn.start", json!({"thread": "t1", "workspace": "w1", "driver": "claudeAgent", "prompt": "go"})).await;
    // When the question is dismissed and the tool call is cancelled
    let mut rest = agent.events_through("input.requested").await;
    let question = rest.last().unwrap().clone();
    agent.ask("input.dismiss", json!({"thread": "t1", "request": question.data["request"]})).await;
    rest.extend(agent.events_through("approval.requested").await);
    let approval = rest.last().unwrap().clone();
    agent.ask("approval.answer", json!({"thread": "t1", "approval": approval.data["approval"], "decision": "cancel"})).await;
    rest.extend(agent.events_through("turn.finished").await);
    // Then the CLI was told the question was closed, the turn ended interrupted, and what the CLI would have said later never came
    assert!(rest.iter().any(|e| e.kind == "input.dismissed"));
    assert_eq!(rest.last().unwrap().data["reason"], json!("interrupted"));
    assert!(!rest.iter().any(|e| e.kind == "message.delta" && e.data["text"] == json!("never said")));
    assert_eq!(agent.ask("thread.get", json!({"thread": "t1"})).await["result"]["thread"]["status"], json!("idle"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_socket_that_drops_while_a_request_waits_when_a_new_one_resumes_then_it_replays_the_request_it_missed_answers_it_and_the_two_together_are_the_log() {
    // Given a turn that asks about a tool, and a mapper that follows it and drops after the CLI's call
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    chain.script(vec![Turn::replaying(&[]).then_calling("k1", "approval_prompt", approval_call()).then_line(&said("done"))]);
    let mut first = chain.agent().await;
    create(&mut first, "t1").await;
    first.ask("thread.subscribe", json!({"thread": "t1"})).await;
    first.ask("turn.start", json!({"thread": "t1", "workspace": "w1", "driver": "claudeAgent", "prompt": "go"})).await;
    let before_drop = first.events_through("tool.started").await;
    let last_seen = before_drop.last().unwrap().seq;
    // When it drops, a new socket resumes after the last seq it saw, and the request it missed is answered there
    drop(first);
    let mut second = chain.agent().await;
    let resumed = second.ask("thread.subscribe", json!({"thread": "t1", "after_seq": last_seen})).await;
    let mut rest = second.events_through("approval.requested").await;
    let asked = rest.last().unwrap().clone();
    second.ask("approval.answer", json!({"thread": "t1", "approval": asked.data["approval"], "decision": "accept"})).await;
    rest.extend(second.events_through("turn.finished").await);
    // Then the new socket's events start after the last one the first saw, and the two together are exactly the thread's log
    assert_eq!(resumed["result"]["after_seq"], json!(last_seen));
    assert!(asked.seq > last_seen);
    let all = second.ask("thread.history", json!({"thread": "t1"})).await;
    let logged: Vec<u64> = all["result"]["events"].as_array().unwrap().iter().map(|e| e["seq"].as_u64().unwrap()).collect();
    let together: Vec<u64> = seqs(&before_drop).into_iter().chain(seqs(&rest)).collect();
    assert_eq!(together, logged);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_request_nobody_asked_or_one_already_answered_when_answered_then_it_is_refused_with_its_code_and_nothing_is_recorded() {
    // Given a thread that is idle and one request answered
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    chain.script(vec![Turn::replaying(&[]).then_calling("k1", "ask_user", json!({"question": "Sure?"}))]);
    let mut agent = chain.agent().await;
    create(&mut agent, "t1").await;
    agent.ask("thread.subscribe", json!({"thread": "t1"})).await;
    agent.ask("turn.start", json!({"thread": "t1", "workspace": "w1", "driver": "claudeAgent", "prompt": "go"})).await;
    let question = agent.events_through("input.requested").await.pop().unwrap();
    agent.ask("input.answer", json!({"thread": "t1", "request": question.data["request"], "answer": "yes"})).await;
    agent.events_through("turn.finished").await;
    let logged = agent.ask("thread.get", json!({"thread": "t1"})).await["result"]["thread"]["last_seq"].clone();
    // When it is answered again, another is answered that was never asked, and a thread nobody made is answered
    let again = agent.ask("input.answer", json!({"thread": "t1", "request": question.data["request"], "answer": "yes"})).await;
    let never = agent.ask("approval.answer", json!({"thread": "t1", "approval": "nope", "decision": "accept"})).await;
    let nobody = agent.ask("input.dismiss", json!({"thread": "nobody", "request": "r"})).await;
    // Then each is refused with its code and the thread's log did not grow
    let codes: Vec<&Value> = [&again, &never, &nobody].iter().map(|a| &a["error"]["code"]).collect();
    assert_eq!(codes, [&json!("REQUEST_NOT_OPEN"), &json!("REQUEST_NOT_OPEN"), &json!("THREAD_NOT_FOUND")]);
    assert_eq!(agent.ask("thread.get", json!({"thread": "t1"})).await["result"]["thread"]["last_seq"], logged);
}
