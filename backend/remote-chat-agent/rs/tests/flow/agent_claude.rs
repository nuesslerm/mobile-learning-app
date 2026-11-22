//! Claude Code as a live session end to end: the service starts one `claude -p --input-format stream-json` process for each thread at the workspace
//! host through the real `rca-exec`, keeps it up across turns and talks to it over its input and output. The process is the scripted CLI of `exec-testkit`
//! playing conversations a test wrote in the frames the protocol note describes (`docs/solutions/architecture/claude-code-stream-json-protocol.md`); none
//! is a run of the real CLI by this service, which `docs/gaps/claude-live-session-not-run-against-the-real-cli.md` says.
use crate::agent_threads::{chain_with, kinds, said, Chain};
use crate::harness::{eventually, Env, Options};
use crate::mapper::Agent;
use agent_api_proto::Event;
use exec_testkit::script::{Received, Turn};
use serde_json::{json, Value};
use std::time::Duration;

fn text_deltas(message: &str, text: &str) -> Vec<String> {
    vec![
        json!({"type": "stream_event", "parent_tool_use_id": null, "event": {"type": "message_start", "message": {"id": message}}}).to_string(),
        json!({"type": "stream_event", "parent_tool_use_id": null, "event": {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": text}}}).to_string(),
        json!({"type": "assistant", "message": {"id": message, "content": [{"type": "text", "text": text}]}}).to_string(),
    ]
}

fn tool_call(id: &str, name: &str, input: Value) -> String {
    json!({"type": "assistant", "message": {"id": format!("m-{id}"), "content": [{"type": "tool_use", "id": id, "name": name, "input": input}]}}).to_string()
}

fn tool_result(id: &str, text: &str) -> String {
    json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": id, "content": text, "is_error": false}]}}).to_string()
}

fn asks(request: &str, tool: &str, id: &str, input: Value) -> String {
    json!({"type": "control_request", "request_id": request, "request": {"subtype": "can_use_tool", "tool_name": tool, "input": input, "tool_use_id": id}}).to_string()
}

async fn thread(agent: &mut Agent, id: &str) {
    let made = agent.ask("thread.create", json!({"thread": id, "title": "Claude", "workspace": "w1", "driver": "claudeAgent"})).await;
    assert!(made.get("result").is_some(), "{made}");
    agent.ask("thread.subscribe", json!({"thread": id})).await;
}

async fn turn(agent: &mut Agent, id: &str, prompt: &str, runtime_mode: &str) -> Value {
    let started = agent.ask("turn.start", json!({"thread": id, "workspace": "w1", "driver": "claudeAgent", "prompt": prompt, "runtime_mode": runtime_mode})).await;
    assert!(started.get("result").is_some(), "{started}");
    started
}

fn spoken(events: &[Event]) -> String {
    events.iter().filter(|e| e.kind == "message.delta" && e.data["role"] == json!("assistant")).map(|e| e.data["text"].as_str().unwrap()).collect()
}

fn reason(events: &[Event]) -> Value {
    events.iter().rev().find(|e| e.kind == "turn.finished").unwrap().data["reason"].clone()
}

/// What the live process was sent, by kind: control requests by their subtype, the person's messages as `user`, and answers to its own requests.
fn sent(chain: &Chain) -> Vec<(usize, String)> {
    chain.cli.claude_log("claude").into_iter().map(|r: Received| (r.start, if r.message["type"] == "control_request" { r.message["request"]["subtype"].as_str().unwrap_or_default().to_string() } else { r.message["type"].as_str().unwrap_or_default().to_string() })).collect()
}

fn gone(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => true,
        Ok(stat) => stat.rsplit(')').next().is_some_and(|rest| rest.trim_start().starts_with('Z')),
    }
}

fn flag(argv: &[String], name: &str) -> Option<String> {
    argv.iter().position(|a| a == name).and_then(|at| argv.get(at + 1)).cloned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_turn_that_streams_text_and_runs_a_tool_when_it_runs_over_the_agent_socket_then_the_text_and_the_tool_arrive_as_events_and_the_turn_ends() {
    // Given the service, the real executor, and a Claude Code whose turn streams text, runs a command and says it is done
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options::default()).await;
    let mut lines = text_deltas("m1", "looking");
    lines.extend([tool_call("k1", "Bash", json!({"command": "ls"})), tool_result("k1", "a.txt")]);
    lines.extend(text_deltas("m2", "found a.txt"));
    chain.script(vec![Turn::replaying(&lines)]);
    let mut agent = chain.agent().await;
    thread(&mut agent, "t1").await;
    // When a turn runs
    turn(&mut agent, "t1", "list the files", "full-access").await;
    let events = agent.events_through("turn.finished").await;
    // Then the events are the turn's: what the person said, the text once though its complete frame followed it, the tool as one call that succeeded, and the end
    assert_eq!(spoken(&events), "lookingfound a.txt");
    let started: Vec<&Event> = events.iter().filter(|e| e.kind == "tool.started").collect();
    assert_eq!((started.len(), started[0].data["tool"].clone(), started[0].data["args"].clone()), (1, json!("Bash"), json!({"command": "ls"})));
    assert_eq!(events.iter().find(|e| e.kind == "tool.finished").unwrap().data["ok"], json!(true));
    assert_eq!((kinds(&events)[0], reason(&events)), ("turn.started", json!("completed")));
    // And the process was started once at the workspace host's root with the stream-json flags of its own, the conversation the service chose, the mode and the tool server
    let starts = chain.cli.invocations("claude");
    assert_eq!((starts.len(), starts[0].cwd.clone()), (1, chain.repo.path().to_string_lossy().into_owned()));
    let argv = &starts[0].argv;
    assert!(argv.starts_with(&["-p".to_string(), "--input-format".to_string(), "stream-json".to_string()]) && argv.contains(&"--permission-prompt-tool".to_string()), "{argv:?}");
    assert_eq!(flag(argv, "--permission-mode").as_deref(), Some("bypassPermissions"));
    assert!(flag(argv, "--mcp-config").unwrap().contains(&format!("http://127.0.0.1:{}/mcp/t1", chain.port)), "{argv:?}");
    assert_eq!(flag(argv, "--session-id").map(|id| id.len()), Some(36));
    // And the handshake came before the message, and the thread is idle with the conversation kept
    assert_eq!(sent(&chain).into_iter().map(|(_, k)| k).collect::<Vec<_>>(), ["initialize", "user"]);
    assert_eq!(agent.ask("thread.get", json!({"thread": "t1"})).await["result"]["thread"]["status"], json!("idle"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_turn_that_is_working_when_the_mapper_steers_it_then_the_prompt_goes_to_the_live_process_with_priority_now_and_the_same_turn_ends_once_with_no_process_restarted() {
    // Given a Claude Code whose first turn says it is going right and waits, and whose next one says it is going left
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options::default()).await;
    chain.script(vec![Turn::replaying(&text_deltas("m1", "going right")).then_sleeping(60_000), Turn::replaying(&text_deltas("m2", "going left"))]);
    let mut agent = chain.agent().await;
    thread(&mut agent, "t1").await;
    let started = turn(&mut agent, "t1", "go right", "full-access").await;
    let turn_id = started["result"]["turn"].as_str().unwrap().to_string();
    let mut seen = agent.events_through("message.delta").await;
    seen.extend(agent.events_through("message.delta").await);
    // When it is steered
    let steered = agent.ask("turn.steer", json!({"thread": "t1", "turn": turn_id, "prompt": "go left"})).await;
    seen.extend(agent.events_through("turn.finished").await);
    // Then the answer names the turn, the steer is between the two texts, and the turn finished once, completed, in spite of the aborted result the CLI printed between
    assert_eq!(steered["result"]["turn"], json!(turn_id));
    assert_eq!(kinds(&seen).iter().filter(|k| **k == "turn.finished").count(), 1);
    assert_eq!(reason(&seen), json!("completed"));
    assert_eq!(spoken(&seen), "going rightgoing left");
    let order: Vec<&str> = kinds(&seen).into_iter().filter(|k| ["turn.started", "turn.steered", "turn.finished"].contains(k)).collect();
    assert_eq!(order, ["turn.started", "turn.steered", "turn.finished"]);
    // And the process got the steer as a message with priority now, and it is the only process there was
    let users: Vec<Value> = chain.cli.claude_log("claude").into_iter().map(|r| r.message).filter(|m| m["type"] == "user").collect();
    assert_eq!((users.len(), users[1]["priority"].clone(), users[1]["message"]["content"][0]["text"].clone()), (2, json!("now"), json!("go left")));
    assert_eq!(chain.cli.invocations("claude").len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_turn_that_asks_to_use_a_tool_when_the_person_answers_then_the_ask_is_an_approval_event_the_answer_reaches_the_live_process_and_the_turn_goes_on() {
    // Given a Claude Code whose turn asks before it writes a file, waits for the answer on its input, and then says it wrote it, in the mode that asks
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options::default()).await;
    let ask = asks("c1", "Write", "toolu_1", json!({"file_path": "a.txt", "content": "x"}));
    chain.script(vec![Turn::replaying(&[tool_call("toolu_1", "Write", json!({"file_path": "a.txt", "content": "x"})), ask]).then_reading_a_line().then_line(&tool_result("toolu_1", "wrote a.txt")).then_line(&said("written"))]);
    let mut agent = chain.agent().await;
    thread(&mut agent, "t1").await;
    turn(&mut agent, "t1", "write it", "approval-required").await;
    // When the turn asks
    let events = agent.events_through("approval.requested").await;
    let asked = events.last().unwrap();
    // Then the request names the tool and its input, and the turn waits on it
    assert_eq!((asked.data["tool"].clone(), asked.data["args"]["file_path"].clone()), (json!("Write"), json!("a.txt")));
    assert_ne!(agent.ask("thread.get", json!({"thread": "t1"})).await["result"]["thread"]["status"], json!("idle"));
    // When the person accepts it
    let approval = asked.data["approval"].as_str().unwrap().to_string();
    let answered = agent.ask("approval.answer", json!({"thread": "t1", "approval": approval, "decision": "accept"})).await;
    assert!(answered.get("result").is_some(), "{answered}");
    let rest = agent.events_through("turn.finished").await;
    // Then the turn finished with its answer, and the live process was sent an allow under the id of its own request, with the input it asked about
    assert_eq!((reason(&rest), spoken(&rest)), (json!("completed"), "written".to_string()));
    let answer = chain.cli.claude_log("claude").into_iter().map(|r| r.message).find(|m| m["type"] == "control_response").expect("the answer reached the process");
    assert_eq!((answer["response"]["request_id"].clone(), answer["response"]["response"]["behavior"].clone(), answer["response"]["response"]["updatedInput"]["file_path"].clone()), (json!("c1"), json!("allow"), json!("a.txt")));
    assert_eq!(flag(&chain.cli.invocations("claude")[0].argv, "--permission-mode").as_deref(), Some("default"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_turn_that_asks_to_use_a_tool_when_the_person_declines_then_the_live_process_is_sent_a_deny_the_model_reads_and_the_turn_goes_on() {
    // Given a Claude Code whose turn asks before it runs a command and goes on whatever the answer is
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options::default()).await;
    chain.script(vec![Turn::replaying(&[asks("c7", "Bash", "toolu_7", json!({"command": "rm -rf x"}))]).then_reading_a_line().then_line(&said("then I will not"))]);
    let mut agent = chain.agent().await;
    thread(&mut agent, "t1").await;
    turn(&mut agent, "t1", "clean up", "approval-required").await;
    let asked = agent.events_through("approval.requested").await.pop().unwrap();
    // When the person declines
    agent.ask("approval.answer", json!({"thread": "t1", "approval": asked.data["approval"], "decision": "decline"})).await;
    let rest = agent.events_through("turn.finished").await;
    // Then the process was sent a deny with the reason the model reads, and the turn went on to its end
    let answer = chain.cli.claude_log("claude").into_iter().map(|r| r.message).find(|m| m["type"] == "control_response").unwrap();
    assert_eq!((answer["response"]["response"]["behavior"].clone(), answer["response"]["response"]["message"].clone()), (json!("deny"), json!("User declined tool execution.")));
    assert_eq!((reason(&rest), spoken(&rest)), (json!("completed"), "then I will not".to_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_turn_that_is_working_when_it_is_interrupted_then_the_process_is_asked_to_interrupt_the_turn_ends_interrupted_and_the_next_turn_runs_on_the_same_process() {
    // Given a Claude Code whose first turn waits, and whose second says something
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options::default()).await;
    chain.script(vec![Turn::replaying(&text_deltas("m1", "working")).then_sleeping(60_000), Turn::replaying(&text_deltas("m2", "again"))]);
    let mut agent = chain.agent().await;
    thread(&mut agent, "t1").await;
    turn(&mut agent, "t1", "work", "full-access").await;
    agent.events_through("message.delta").await;
    agent.events_through("message.delta").await;
    // When it is interrupted
    let stopped = agent.ask("turn.interrupt", json!({"thread": "t1"})).await;
    let end = agent.events_through("turn.finished").await;
    // Then it ended interrupted with no error, the process was sent an interrupt that cancels what is queued in it, and it lives on
    assert!(stopped.get("result").is_some(), "{stopped}");
    assert_eq!((reason(&end), kinds(&end).contains(&"error")), (json!("interrupted"), false));
    let interrupt = chain.cli.claude_log("claude").into_iter().map(|r| r.message).find(|m| m["request"]["subtype"] == "interrupt").expect("an interrupt was sent");
    assert_eq!(interrupt["request"]["cancel_queued"], json!(true));
    // And the next turn of the thread runs on the same process
    turn(&mut agent, "t1", "again", "full-access").await;
    let next = agent.events_through("turn.finished").await;
    assert_eq!((reason(&next), spoken(&next)), (json!("completed"), "again".to_string()));
    assert_eq!((chain.cli.invocations("claude").len(), chain.cli.claude_prompts("claude")), (1, vec!["work".to_string(), "again".to_string()]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_process_that_sat_idle_when_the_idle_time_passes_then_it_is_closed_and_the_next_turn_starts_another_that_resumes_the_conversation_by_its_id() {
    // Given a thread that ran a turn, and an idle time of 300 ms
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options { claude_idle: Duration::from_millis(300), ..Options::default() }).await;
    chain.script(vec![Turn::replaying(&text_deltas("m1", "first")), Turn::replaying(&text_deltas("m2", "second"))]);
    let mut agent = chain.agent().await;
    thread(&mut agent, "t1").await;
    turn(&mut agent, "t1", "one", "full-access").await;
    agent.events_through("turn.finished").await;
    let pid = chain.cli.claude_log("claude")[0].pid;
    // When nothing runs for longer than that
    eventually("the process to be closed", || gone(pid)).await;
    // Then the thread's next turn starts a second process that resumes the conversation the first was started with
    turn(&mut agent, "t1", "two", "full-access").await;
    let events = agent.events_through("turn.finished").await;
    assert_eq!((reason(&events), spoken(&events)), (json!("completed"), "second".to_string()));
    let starts = chain.cli.invocations("claude");
    assert_eq!(starts.len(), 2);
    assert_eq!((flag(&starts[1].argv, "--resume"), flag(&starts[1].argv, "--session-id")), (flag(&starts[0].argv, "--session-id"), None));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_process_that_is_killed_in_the_middle_of_a_turn_when_the_thread_runs_again_then_the_turn_failed_naming_why_and_a_new_process_resumes_the_conversation() {
    // Given a turn that is working, and a script for the turn after it
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options::default()).await;
    chain.script(vec![Turn::replaying(&text_deltas("m1", "working")).then_sleeping(60_000), Turn::replaying(&text_deltas("m2", "back"))]);
    let mut agent = chain.agent().await;
    thread(&mut agent, "t1").await;
    turn(&mut agent, "t1", "work", "full-access").await;
    agent.events_through("message.delta").await;
    agent.events_through("message.delta").await;
    // When the process is killed from outside
    let pid = chain.cli.claude_log("claude")[0].pid;
    std::process::Command::new("kill").args(["-9", &pid.to_string()]).status().unwrap();
    let end = agent.events_through("turn.finished").await;
    // Then the turn failed with an error that says the process ended, and the thread is idle
    assert_eq!(reason(&end), json!("failed"));
    assert!(end.iter().any(|e| e.kind == "error" && e.data["message"].as_str().is_some_and(|m| m.contains("ended"))), "{:?}", kinds(&end));
    // And the next turn starts a new process and resumes the conversation of the first
    turn(&mut agent, "t1", "again", "full-access").await;
    let next = agent.events_through("turn.finished").await;
    assert_eq!((reason(&next), spoken(&next)), (json!("completed"), "back".to_string()));
    let starts = chain.cli.invocations("claude");
    assert_eq!(starts.len(), 2);
    assert_eq!(flag(&starts[1].argv, "--resume"), flag(&starts[0].argv, "--session-id"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_two_threads_when_each_runs_a_turn_then_each_has_a_process_and_a_conversation_of_its_own() {
    // Given two threads of one workspace
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options::default()).await;
    chain.script(vec![Turn::replaying(&text_deltas("m1", "one")), Turn::replaying(&text_deltas("m2", "two"))]);
    let mut agent = chain.agent().await;
    thread(&mut agent, "a").await;
    thread(&mut agent, "b").await;
    // When each runs a turn
    turn(&mut agent, "a", "first", "full-access").await;
    agent.events_through("turn.finished").await;
    turn(&mut agent, "b", "second", "full-access").await;
    agent.events_through("turn.finished").await;
    // Then two processes ran, with two conversations, two pids, and each was sent only its own prompt
    let starts = chain.cli.invocations("claude");
    assert_eq!(starts.len(), 2);
    assert_ne!(flag(&starts[0].argv, "--session-id"), flag(&starts[1].argv, "--session-id"));
    let log = chain.cli.claude_log("claude");
    let pids: Vec<u32> = log.iter().map(|r| r.pid).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
    assert_eq!(pids.len(), 2);
    let prompts_of = |start: usize| -> Vec<String> { log.iter().filter(|r| r.start == start && r.message["type"] == "user").map(|r| r.message["message"]["content"][0]["text"].as_str().unwrap().to_string()).collect() };
    assert_eq!((prompts_of(1), prompts_of(2)), (vec!["first".to_string()], vec!["second".to_string()]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_thread_that_asked_before_each_tool_when_its_next_turn_is_full_access_then_a_process_launched_in_it_resumes_the_conversation_and_the_turn_runs() {
    // Given a thread whose first turn ran in the mode that asks, in a process the CLI will not take into full access
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options::default()).await;
    chain.script(vec![Turn::replaying(&text_deltas("m1", "first")), Turn::replaying(&text_deltas("m2", "second"))]);
    let mut agent = chain.agent().await;
    thread(&mut agent, "t1").await;
    turn(&mut agent, "t1", "one", "approval-required").await;
    agent.events_through("turn.finished").await;
    // When the thread's next turn is in full access
    turn(&mut agent, "t1", "two", "full-access").await;
    let events = agent.events_through("turn.finished").await;
    // Then the turn completed on a second process that resumed the conversation of the first in full access, and no mode request was refused
    assert_eq!((reason(&events), spoken(&events)), (json!("completed"), "second".to_string()));
    let starts = chain.cli.invocations("claude");
    assert_eq!(starts.len(), 2);
    assert_eq!((flag(&starts[1].argv, "--resume"), flag(&starts[1].argv, "--permission-mode")), (flag(&starts[0].argv, "--session-id"), Some("bypassPermissions".to_string())));
    assert!(!sent(&chain).iter().any(|(_, kind)| kind == "set_permission_mode"), "{:?}", sent(&chain));
}
