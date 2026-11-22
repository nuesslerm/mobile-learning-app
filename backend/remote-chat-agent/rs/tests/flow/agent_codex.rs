//! Codex as a live session end to end: the service starts `codex app-server` at the workspace host through the real `rca-exec`, keeps it up across
//! turns, and talks to it over its input and output. The server is the scripted CLI of `exec-testkit` playing recordings of Codex's own app-server
//! (`transcripts/codex-app-server`, written by another client's run of Codex 0.156.1 and 0.157.0) or conversations a test wrote; none is a run of
//! the CLI by this service, which `docs/gaps/codex-live-session-not-run-against-the-real-cli.md` says.
use crate::agent_threads::{chain_with, kinds, Chain};
use crate::harness::{eventually, Env, Options};
use crate::mapper::Agent;
use agent_api_proto::Event;
use exec_testkit::app_server::transcript;
use exec_testkit::script::{Frame, Received, Run, Script};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

fn serve(chain: &Chain, runs: Vec<Vec<Frame>>) {
    chain.cli.script("codex", &Script::default().printing_version("0.0.0-flow").playing_app_server(runs));
}

async fn thread(agent: &mut Agent, id: &str) {
    let made = agent.ask("thread.create", json!({"thread": id, "title": "Codex", "workspace": "w1", "driver": "codex"})).await;
    assert!(made.get("result").is_some(), "{made}");
    agent.ask("thread.subscribe", json!({"thread": id})).await;
}

async fn turn(agent: &mut Agent, id: &str, prompt: &str, runtime_mode: &str) -> Value {
    let started = agent.ask("turn.start", json!({"thread": id, "workspace": "w1", "driver": "codex", "prompt": prompt, "runtime_mode": runtime_mode})).await;
    assert!(started.get("result").is_some(), "{started}");
    started
}

/// The assistant's text in the events, its deltas joined.
fn spoken(events: &[Event]) -> String {
    events.iter().filter(|e| e.kind == "message.delta" && e.data["role"] == json!("assistant")).map(|e| e.data["text"].as_str().unwrap()).collect()
}

fn methods(log: &[Received]) -> Vec<String> {
    log.iter().filter_map(|r| r.message["method"].as_str().map(str::to_string)).collect()
}

fn gone(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => true,
        Ok(stat) => stat.rsplit(')').next().is_some_and(|rest| rest.trim_start().starts_with('Z')),
    }
}

fn reason(events: &[Event]) -> Value {
    events.iter().rev().find(|e| e.kind == "turn.finished").unwrap().data["reason"].clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_recorded_turn_that_runs_a_command_when_it_runs_over_the_agent_socket_then_the_text_and_the_command_arrive_as_events_and_the_turn_ends() {
    // Given the service, the real executor, and a Codex whose server plays the recording of a turn that runs a shell command
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options::default()).await;
    serve(&chain, vec![transcript("command_output_streaming")]);
    let mut agent = chain.agent().await;
    thread(&mut agent, "t1").await;
    // When a turn runs
    turn(&mut agent, "t1", "run the command", "full-access").await;
    let events = agent.events_through("turn.finished").await;
    // Then the events are the turn's: what the person said, the answer that streams around the command, the command as one tool call that succeeded, and the end
    let said = spoken(&events);
    assert!(said.contains("run the command in the foreground") && said.ends_with("command output fixture complete"), "{said:?}");
    let started: Vec<&Event> = events.iter().filter(|e| e.kind == "tool.started").collect();
    assert_eq!((started.len(), started[0].data["tool"].clone()), (1, json!("command")));
    assert!(started[0].data["args"]["command"].as_str().unwrap().contains("for i in"), "{:?}", started[0].data);
    assert_eq!(events.iter().find(|e| e.kind == "tool.finished").unwrap().data["ok"], json!(true));
    assert_eq!((kinds(&events)[0], reason(&events)), ("turn.started", json!("completed")));
    // And the conversation the server named is the thread's, and the server ran in the repository's root, started once
    let log = chain.cli.app_server_log("codex");
    assert_eq!(methods(&log), ["initialize", "initialized", "thread/start", "turn/start"]);
    assert_eq!(log.iter().map(|r| r.start).collect::<Vec<_>>(), [1, 1, 1, 1]);
    let turn_start = &log[3].message["params"];
    assert_eq!((turn_start["approvalPolicy"].clone(), turn_start["sandboxPolicy"]["type"].clone(), turn_start["input"][0]["text"].clone()), (json!("never"), json!("dangerFullAccess"), json!("run the command")));
    let history = agent.ask("thread.get", json!({"thread": "t1"})).await;
    assert_eq!(history["result"]["thread"]["status"], json!("idle"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_recorded_turn_that_asks_to_run_a_command_when_the_person_accepts_it_then_the_ask_is_an_approval_event_the_answer_reaches_the_live_server_and_the_turn_goes_on() {
    // Given a Codex whose server plays the recording of a turn that asks before it writes a file, and a thread in the mode that asks
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options::default()).await;
    serve(&chain, vec![transcript("tool_call_read_only_on_request")]);
    let mut agent = chain.agent().await;
    thread(&mut agent, "t1").await;
    turn(&mut agent, "t1", "write the fixture file", "auto-accept-edits").await;
    // When the turn asks
    let events = agent.events_through("approval.requested").await;
    let asked = events.last().unwrap();
    // Then the request is an event naming the command and the reason Codex gave, and the turn waits on it
    assert_eq!(asked.data["tool"], json!("command"));
    assert!(asked.data["args"]["command"].as_str().unwrap().contains(".codex-probe-write-action.txt") && asked.data["args"]["reason"].as_str().unwrap().contains("May I create or overwrite"), "{:?}", asked.data);
    let waiting = agent.ask("thread.get", json!({"thread": "t1"})).await;
    assert_ne!(waiting["result"]["thread"]["status"], json!("idle"));
    // When the person accepts
    let approval = asked.data["approval"].as_str().unwrap().to_string();
    let answered = agent.ask("approval.answer", json!({"thread": "t1", "approval": approval, "decision": "accept"})).await;
    assert!(answered.get("result").is_some(), "{answered}");
    let rest = agent.events_through("turn.finished").await;
    // Then the turn finished with its final answer, and the server was sent the decision under its own request id, on the process that asked
    assert_eq!(reason(&rest), json!("completed"));
    assert!(spoken(&rest).contains("Created or overwrote"), "{:?}", spoken(&rest));
    let log = chain.cli.app_server_log("codex");
    let answer = log.iter().find(|r| r.message.get("method").is_none() && r.message["id"] == json!(0)).expect("the answer reached the server");
    assert_eq!(answer.message["result"], json!({"decision": "accept"}));
    assert!(log.iter().all(|r| r.pid == log[0].pid));
    let turn_start = log.iter().find(|r| r.message["method"] == "turn/start").unwrap();
    assert_eq!((turn_start.message["params"]["approvalPolicy"].clone(), turn_start.message["params"]["sandboxPolicy"]["type"].clone()), (json!("on-request"), json!("workspaceWrite")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_recorded_turn_that_is_steered_when_the_mapper_steers_it_then_the_prompt_goes_to_the_live_server_and_the_same_turn_ends_once_with_no_process_restarted() {
    // Given a Codex whose server plays the recording of a turn that is steered right after it starts
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options::default()).await;
    serve(&chain, vec![transcript("message_steering")]);
    let mut agent = chain.agent().await;
    thread(&mut agent, "t1").await;
    let started = turn(&mut agent, "t1", "Respond with exactly: steering fixture initial response", "full-access").await;
    eventually("the server started the turn", || chain.cli.app_server_log("codex").iter().any(|r| r.message["method"] == "turn/start")).await;
    // When the mapper steers the turn
    let steered = agent.ask("turn.steer", json!({"thread": "t1", "prompt": "Actually, respond with exactly: steering fixture observed"})).await;
    let events = agent.events_through("turn.finished").await;
    // Then it is the same turn, which says what the steer asked for and finishes once
    assert_eq!(steered["result"]["turn"], started["result"]["turn"]);
    assert!(spoken(&events).ends_with("steering fixture observed"), "{:?}", spoken(&events));
    assert_eq!(events.iter().filter(|e| e.kind == "turn.finished").count(), 1);
    // And the steer was a request of the one process that had the turn, naming the turn it steered, and nothing was started or killed for it
    let log = chain.cli.app_server_log("codex");
    let steer = log.iter().find(|r| r.message["method"] == "turn/steer").unwrap();
    assert_eq!(steer.message["params"]["input"][0]["text"], json!("Actually, respond with exactly: steering fixture observed"));
    assert!(log.iter().all(|r| r.start == 1 && r.pid == log[0].pid), "{:?}", log.iter().map(|r| (r.start, r.pid)).collect::<Vec<_>>());
    assert!(!gone(log[0].pid));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_recorded_turn_that_runs_long_when_it_is_interrupted_then_the_server_is_asked_to_interrupt_it_and_the_turn_ends_interrupted_on_the_process_that_stays_up() {
    // Given a Codex whose server plays the recording of a turn that is interrupted before it ends
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options::default()).await;
    serve(&chain, vec![transcript("turn_interrupt")]);
    let mut agent = chain.agent().await;
    thread(&mut agent, "t1").await;
    turn(&mut agent, "t1", "sleep 30 first", "full-access").await;
    eventually("the server started the turn", || chain.cli.app_server_log("codex").iter().any(|r| r.message["method"] == "turn/start")).await;
    // When the mapper interrupts it
    let stopped = agent.ask("turn.interrupt", json!({"thread": "t1"})).await;
    assert!(stopped.get("result").is_some(), "{stopped}");
    let events = agent.events_through("turn.finished").await;
    // Then the turn ended interrupted with no error, the server was sent the interrupt for its turn, and its process is still up
    assert_eq!((reason(&events), events.iter().any(|e| e.kind == "error")), (json!("interrupted"), false));
    let log = chain.cli.app_server_log("codex");
    let interrupt = log.iter().find(|r| r.message["method"] == "turn/interrupt").unwrap();
    assert!(interrupt.message["params"]["turnId"].is_string() && interrupt.message["params"]["threadId"].is_string());
    assert!(!gone(log[0].pid));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_server_that_stays_without_a_turn_when_the_idle_time_passes_then_it_is_closed_with_its_process_and_the_next_turn_starts_another_that_resumes_the_thread_by_its_id() {
    // Given two runs of the server (the second resumes the thread of the first), and an idle time of 400 ms
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options { codex_idle: Duration::from_millis(400), ..Options::default() }).await;
    let first = Run::new().handshake().thread_start("th-1").turn_saying("th-1", "u-1", "first answer");
    let second = Run::new().handshake().thread_resume("th-1").turn_saying("th-1", "u-2", "second answer");
    serve(&chain, vec![first.into(), second.into()]);
    let mut agent = chain.agent().await;
    thread(&mut agent, "t1").await;
    // When a turn runs, and nothing runs for longer than the idle time
    turn(&mut agent, "t1", "one", "full-access").await;
    let one = agent.events_through("turn.finished").await;
    let before = chain.cli.app_server_log("codex")[0].pid;
    let until = Instant::now() + Duration::from_secs(10);
    while !gone(before) {
        assert!(Instant::now() < until, "the idle server was not closed");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // Then the next turn is answered by a second process that resumed the thread by the id the first named
    turn(&mut agent, "t1", "two", "full-access").await;
    let two = agent.events_through("turn.finished").await;
    assert_eq!((spoken(&one).as_str(), spoken(&two).as_str()), ("first answer", "second answer"));
    let log = chain.cli.app_server_log("codex");
    let resume = log.iter().find(|r| r.message["method"] == "thread/resume").unwrap();
    assert_eq!((resume.start, resume.message["params"]["threadId"].clone()), (2, json!("th-1")));
    assert_ne!(resume.pid, before);
    assert_eq!(log.iter().filter(|r| r.message["method"] == "thread/start").count(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_server_that_is_killed_during_a_turn_when_the_thread_runs_again_then_the_turn_failed_with_the_reason_and_the_next_turn_restarts_the_server_and_resumes_the_thread() {
    // Given a server whose first run dies during its turn and whose second resumes the thread
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options::default()).await;
    let first = Run::new().handshake().thread_start("th-1").turn_begun("th-1", "u-1").notify("item/agentMessage/delta", json!({"threadId": "th-1", "turnId": "u-1", "itemId": "m1", "delta": "half an answ"})).exiting(9);
    let second = Run::new().handshake().thread_resume("th-1").turn_saying("th-1", "u-2", "whole answer");
    serve(&chain, vec![first.into(), second.into()]);
    let mut agent = chain.agent().await;
    thread(&mut agent, "t1").await;
    // When a turn runs and the server dies in it
    turn(&mut agent, "t1", "one", "full-access").await;
    let one = agent.events_through("turn.finished").await;
    // Then the turn finished failed, with what was said and an error naming the end of the server
    assert_eq!((reason(&one), spoken(&one).as_str()), (json!("failed"), "half an answ"));
    assert!(one.iter().any(|e| e.kind == "error" && e.data["message"].as_str().unwrap().contains("ended")), "{one:?}");
    // And the next turn starts the server again and resumes the thread by its id
    turn(&mut agent, "t1", "two", "full-access").await;
    let two = agent.events_through("turn.finished").await;
    assert_eq!((reason(&two), spoken(&two).as_str()), (json!("completed"), "whole answer"));
    let log = chain.cli.app_server_log("codex");
    let resume = log.iter().find(|r| r.message["method"] == "thread/resume").unwrap();
    assert_eq!((resume.start, resume.message["params"]["threadId"].clone()), (2, json!("th-1")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_server_that_exits_leaving_a_process_holding_its_output_when_a_turn_runs_then_the_turn_ends_in_seconds_and_the_process_it_left_is_killed() {
    // Given a Codex that answers the handshake and a turn, then exits leaving a long sleep that holds its output
    use std::os::unix::fs::PermissionsExt;
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options::default()).await;
    let pidfile = env.home.join("leftover.pid");
    let script = format!(
        "#!/bin/sh\n[ \"$1\" = --version ] && {{ echo 0.0.0-flow; exit 0; }}\nsleep 30 &\necho $! > {}\nread -r line; printf '%s\\n' '{{\"id\":1,\"result\":{{}}}}'\nread -r line\nread -r line; printf '%s\\n' '{{\"id\":2,\"result\":{{\"thread\":{{\"id\":\"th-bg\"}}}}}}'\nread -r line; printf '%s\\n' '{{\"id\":3,\"result\":{{\"turn\":{{\"id\":\"u-bg\"}}}}}}'\nexit 0\n",
        pidfile.display()
    );
    let codex = chain.cli.dir().join("codex");
    std::fs::remove_file(&codex).unwrap();
    std::fs::write(&codex, script).unwrap();
    std::fs::set_permissions(&codex, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut agent = chain.agent().await;
    thread(&mut agent, "t1").await;
    // When a turn runs
    let started = Instant::now();
    turn(&mut agent, "t1", "go", "full-access").await;
    let events = agent.events_through("turn.finished").await;
    // Then the turn ended failed in seconds, not when the sleep does, and the sleep is gone
    assert_eq!(reason(&events), json!("failed"));
    assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());
    let pid: u32 = std::fs::read_to_string(&pidfile).unwrap().trim().parse().unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    while !gone(pid) {
        assert!(Instant::now() < until, "the process the server left is still running");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_codex_nobody_scripted_when_two_turns_run_then_both_end_completed_on_the_one_server_that_started() {
    // Given the service and the real executor, with the scripted Codex as it comes (no conversation written for it)
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options::default()).await;
    let mut agent = chain.agent().await;
    thread(&mut agent, "t1").await;
    // When two turns run on the thread
    turn(&mut agent, "t1", "first", "full-access").await;
    let first = agent.events_through("turn.finished").await;
    turn(&mut agent, "t1", "second", "full-access").await;
    let second = agent.events_through("turn.finished").await;
    // Then each ends completed with no error and nothing said, and the one server of the workspace ran both
    for events in [&first, &second] {
        assert_eq!((kinds(events)[0], reason(events), events.iter().any(|e| e.kind == "error")), ("turn.started", json!("completed"), false), "{events:?}");
    }
    assert_eq!(spoken(&second), "");
    assert_eq!(chain.cli.invocations("codex").len(), 1);
}
