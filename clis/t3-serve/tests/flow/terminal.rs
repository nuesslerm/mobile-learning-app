//! The M8 rows through the real router over the agent double: the terminals of a thread opened, written to, resized, cleared and closed, their
//! output followed as snapshots and outputs numbered by byte offset (resumed after the offset reached), and the list of terminals followed as
//! upserts and removes. The double answers what the agent API's contract recorded for each method; what a request becomes (the `Op` the agent
//! API is asked) and what the phone is told are both read from data the contract owns.
use super::support::*;
use std::time::Duration;
use t3_core::ports::{AgentError, Op, TerminalEvent, TerminalSummary};

const NAME: &str = "t1.term-1";

/// The result the contract recorded for a terminal method, with the fields that differ between runs given values.
fn recorded(method: &str, name: &str) -> Value {
    let fixtures = agent_api_terminal::testing::fixtures();
    let found = fixtures.iter().flat_map(|f| &f.exchanges).find(|e| e.request["method"] == method && e.name.contains(name) && e.succeeds());
    let mut result = found.unwrap_or_else(|| panic!("no recorded {method} exchange named like {name}")).response["result"].clone();
    for (key, value) in [("pid", json!(4242)), ("offset", json!(0)), ("cwd", json!("/workspace/demo")), ("updated_at", json!(1_700_000_000_000u64))] {
        if result.get(key).is_some_and(Value::is_string) {
            result[key] = value;
        }
    }
    result
}

fn code_of(method: &str, name: &str) -> (String, String) {
    let fixtures = agent_api_terminal::testing::fixtures();
    let found = fixtures.iter().flat_map(|f| &f.exchanges).find(|e| e.request["method"] == method && e.name.contains(name) && !e.succeeds()).unwrap_or_else(|| panic!("no recorded refusal of {method} named like {name}"));
    (found.response["error"]["code"].as_str().unwrap().to_string(), found.response["error"]["message"].as_str().unwrap().to_string())
}

fn started() -> (Harness, Rpc) {
    let h = Harness::start();
    let all = PairingRequest { scopes: t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect(), ..PairingRequest::default() };
    let mut rpc = connect_with(&h, all);
    seed(&mut rpc);
    (h, rpc)
}

fn ops(h: &Harness) -> Vec<Op> {
    h.agent.ops.lock().unwrap().clone()
}

fn reply(h: &Harness, method: &'static str, answer: Result<Value, AgentError>) {
    h.agent.replies.lock().unwrap().insert(method, answer);
}

fn refused(method: &str, name: &str) -> AgentError {
    let (code, message) = code_of(method, name);
    AgentError::Refused { code, message }
}

fn stream(h: &Harness, events: Vec<TerminalEvent>) {
    h.agent.terminal_streams.lock().unwrap().insert(NAME.into(), events);
}

fn snapshot(history: &str, offset: u64, running: bool) -> TerminalEvent {
    TerminalEvent::Snapshot { start: 0, offset, data: history.as_bytes().to_vec(), reset: false, cols: 80, rows: 24, running, exit_code: None }
}

fn output(from: u64, text: &str) -> TerminalEvent {
    TerminalEvent::Output { offset: from, end: from + text.len() as u64, data: text.as_bytes().to_vec() }
}

fn summary(name: &str, running: bool) -> TerminalSummary {
    TerminalSummary { name: name.into(), cwd: "/workspace/demo".into(), worktree_path: None, title: "build".into(), running, exit_code: None, pid: Some(4242), updated_at_ms: 1_700_000_000_000 }
}

#[test]
fn a_terminal_opened_written_resized_cleared_and_closed_is_asked_of_the_agent_api_by_its_joined_name_under_the_threads_directory() {
    // Given a thread on a project at /workspace/demo and an agent API that answers as the contract recorded
    let (h, mut rpc) = started();
    reply(&h, "terminal.open", Ok(recorded("terminal.open", "opens a terminal")));
    // When the phone opens a terminal with a size, writes to it, resizes it, clears it and closes it
    let opened = rpc.call("terminal.open", json!({"threadId": "t1", "terminalId": "term-1", "cwd": "/workspace/demo", "cols": 100, "rows": 30}));
    let wrote = rpc.call("terminal.write", json!({"threadId": "t1", "terminalId": "term-1", "data": "ls\n"}));
    let resized = rpc.call("terminal.resize", json!({"threadId": "t1", "terminalId": "term-1", "cols": 120, "rows": 40}));
    let cleared = rpc.call("terminal.clear", json!({"threadId": "t1", "terminalId": "term-1"}));
    let closed = rpc.call("terminal.close", json!({"threadId": "t1", "terminalId": "term-1"}));
    // Then each was one request of the agent API for the terminal named by the two ids, in the thread's directory
    let root = "/workspace/demo".to_string();
    let terminal = NAME.to_string();
    assert_eq!(
        ops(&h).iter().filter(|op| op.method().starts_with("terminal.")).cloned().collect::<Vec<_>>(),
        [
            Op::TerminalOpen { root: root.clone(), terminal: terminal.clone(), cwd: root.clone(), cols: Some(100), rows: Some(30), env: vec![] },
            Op::TerminalWrite { root: root.clone(), terminal: terminal.clone(), data: "ls\n".into() },
            Op::TerminalResize { root: root.clone(), terminal: terminal.clone(), cols: 120, rows: 40 },
            Op::TerminalClear { root: root.clone(), terminal: terminal.clone() },
            Op::TerminalClose { root, terminal },
        ]
    );
    // And the open is the snapshot of a running terminal in the size the agent API opened it, and the others answer nothing
    let snap = success(&opened);
    assert_eq!((snap["threadId"].as_str(), snap["terminalId"].as_str(), snap["status"].as_str(), snap["history"].as_str(), snap["cols"].clone(), snap["rows"].clone()), (Some("t1"), Some("term-1"), Some("running"), Some(""), json!(100), json!(30)));
    assert_eq!(snap["pid"], json!(4242));
    for exit in [&wrote, &resized, &cleared, &closed] {
        assert_eq!(exit["_tag"], "Success");
    }
}

#[test]
fn a_terminal_the_agent_api_refuses_is_the_apps_own_terminal_error_and_ids_that_cannot_be_joined_are_a_defect_that_asks_nothing() {
    // Given an agent API that refuses with the codes the contract recorded
    let (h, mut rpc) = started();
    reply(&h, "terminal.write", Err(refused("terminal.write", "unknown")));
    // When the phone writes to a terminal that is not there, to one whose shell ended, opens one in a directory the executor refuses, and sends ids with a dot
    let unknown = rpc.call("terminal.write", json!({"threadId": "t1", "terminalId": "term-1", "data": "x"}));
    reply(&h, "terminal.write", Err(AgentError::Refused { code: "TERMINAL_EXITED".into(), message: "terminal t1.term-1 has ended".into() }));
    let exited = rpc.call("terminal.write", json!({"threadId": "t1", "terminalId": "term-1", "data": "x"}));
    reply(&h, "terminal.open", Err(refused("terminal.open", "outside the root")));
    let outside = rpc.call("terminal.open", json!({"threadId": "t1", "terminalId": "term-1", "cwd": "/workspace/demo/../.."}));
    let asked = ops(&h).len();
    let dotted = rpc.call("terminal.write", json!({"threadId": "t1", "terminalId": "a.b", "data": "x"}));
    // Then each is the declared error naming the thread and terminal, and the dotted ids are a defect with no request
    assert_eq!((failure(&unknown)["_tag"].as_str(), failure(&unknown)["threadId"].as_str(), failure(&unknown)["terminalId"].as_str()), (Some("TerminalSessionLookupError"), Some("t1"), Some("term-1")));
    assert_eq!(failure(&exited)["_tag"], "TerminalNotRunningError");
    let error = failure(&outside);
    assert_eq!((error["_tag"].as_str(), error["cwd"].as_str()), (Some("TerminalCwdStatError"), Some("/workspace/demo/../..")));
    assert_eq!(error["cause"], json!(code_of("terminal.open", "outside the root").1));
    assert_eq!(dotted["cause"][0]["_tag"], "Die");
    assert_eq!(ops(&h).len(), asked);
}

#[test]
fn an_attach_is_a_snapshot_of_the_history_and_then_the_output_numbered_by_offset_resumed_after_the_offset_reached_and_the_exit_is_told_once() {
    // Given a terminal whose agent API stream has a snapshot of what it printed and an output after it
    let (h, mut rpc) = started();
    stream(&h, vec![snapshot("$ ls\r\n", 6, true), output(6, "a.txt\r\n")]);
    // When the phone attaches to it
    let id = rpc.send("terminal.attach", json!({"threadId": "t1", "terminalId": "term-1"}));
    let first = rpc.chunk(&id);
    // Then the first frame is the snapshot with the history as text, then the output with its sequence, the offset after its bytes
    assert_eq!(first[0]["type"], "snapshot");
    assert_eq!((first[0]["snapshot"]["history"].as_str(), first[0]["snapshot"]["status"].as_str(), first[0]["snapshot"]["cols"].clone()), (Some("$ ls\r\n"), Some("running"), json!(80)));
    let next = first.get(1).cloned().unwrap_or_else(|| rpc.chunk(&id)[0].clone());
    assert_eq!((next["type"].as_str(), next["data"].as_str(), next["sequence"].clone(), next["threadId"].as_str()), (Some("output"), Some("a.txt\r\n"), json!(13), Some("t1")));
    // And when more output comes and then the shell exits, the next looks ask after offset 13, the output follows, and the exit is told once
    stream(&h, vec![snapshot("$ ls\r\n", 6, true), output(6, "a.txt\r\n"), output(13, "bye\r\n"), TerminalEvent::Exited { exit_code: Some(3) }]);
    let mut told: Vec<Value> = Vec::new();
    while !told.iter().any(|f| f["type"] == "exited") {
        told.extend(rpc.chunk(&id));
    }
    assert!(rpc.try_recv(Duration::from_millis(1500)).is_none(), "the exit is told again");
    assert_eq!(told.iter().map(|f| f["type"].as_str().unwrap()).collect::<Vec<_>>(), ["output", "exited"]);
    assert_eq!((told[0]["data"].as_str(), told[0]["sequence"].clone(), told[1]["exitCode"].clone()), (Some("bye\r\n"), json!(18), json!(3)));
    let asks = h.agent.terminal_asks.lock().unwrap().clone();
    assert_eq!(asks[0], (NAME.to_string(), None));
    assert!(asks[1..].iter().all(|(name, after)| name == NAME && after.is_some_and(|a| a >= 13)), "{asks:?}");
}

#[test]
fn an_observe_never_opens_a_terminal_while_an_attach_with_a_directory_opens_the_one_the_agent_api_has_not_got_and_then_streams_it() {
    // Given a terminal the agent API does not have, and an agent API that makes the stream when it is asked to open one
    let (h, mut rpc) = started();
    let opened = recorded("terminal.open", "opens a terminal");
    let agent = h.agent.clone();
    *h.agent.responder.lock().unwrap() = Some(Box::new(move |op| match op {
        Op::TerminalOpen { terminal, .. } => {
            agent.terminal_streams.lock().unwrap().insert(terminal.clone(), vec![snapshot("", 0, true)]);
            Some(Ok(opened.clone()))
        }
        _ => None,
    }));
    // When the phone observes it, and then attaches with a directory
    let observed = rpc.call("terminal.observe", json!({"threadId": "t1", "terminalId": "term-1"}));
    let opens = |h: &Harness| ops(h).iter().filter(|op| op.method() == "terminal.open").count();
    let after_observe = opens(&h);
    let id = rpc.send("terminal.attach", json!({"threadId": "t1", "terminalId": "term-1", "cwd": "/workspace/demo"}));
    let frames = rpc.chunk(&id);
    // Then the observe is the declared lookup error and opened nothing, and the attach opened the terminal once and streams its snapshot
    assert_eq!((failure(&observed)["_tag"].as_str(), after_observe), (Some("TerminalSessionLookupError"), 0));
    assert_eq!((opens(&h), frames[0]["type"].as_str(), frames[0]["snapshot"]["status"].as_str()), (1, Some("snapshot"), Some("running")));
}

#[test]
fn a_multibyte_character_cut_between_two_output_events_reaches_the_phone_whole() {
    // Given a stream whose second output starts in the middle of the box character the first one ends with
    let (h, mut rpc) = started();
    let bytes = "a\u{2500}\u{1F600}b".as_bytes().to_vec();
    let raw = |from: usize, to: usize| TerminalEvent::Output { offset: from as u64, end: to as u64, data: bytes[from..to].to_vec() };
    stream(&h, vec![snapshot("", 0, true), raw(0, 2), raw(2, 6), raw(6, bytes.len())]);
    // When the phone attaches
    let id = rpc.send("terminal.attach", json!({"threadId": "t1", "terminalId": "term-1"}));
    let mut text = String::new();
    while !text.ends_with('b') {
        for frame in rpc.chunk(&id) {
            if frame["type"] == "output" {
                text.push_str(frame["data"].as_str().unwrap());
            }
        }
    }
    // Then the text is what the shell printed, with no replacement character
    assert_eq!(text, "a\u{2500}\u{1F600}b");
}

#[test]
fn the_terminal_list_is_a_snapshot_and_then_an_upsert_for_each_change_and_a_remove_for_each_terminal_gone() {
    // Given two terminals of the agent API, one of a thread and one that is no thread's
    let (h, mut rpc) = started();
    *h.agent.terminal_list.lock().unwrap() = vec![summary(NAME, true), summary("other", true)];
    // When the phone subscribes to the metadata
    let id = rpc.send("subscribeTerminalMetadata", json!({}));
    let first = rpc.chunk(&id);
    // Then the snapshot has the thread's terminal only, with its label and status
    assert_eq!(first[0]["type"], "snapshot");
    let terminals = first[0]["terminals"].as_array().unwrap();
    assert_eq!(terminals.len(), 1);
    assert_eq!((terminals[0]["threadId"].as_str(), terminals[0]["terminalId"].as_str(), terminals[0]["label"].as_str(), terminals[0]["status"].as_str()), (Some("t1"), Some("term-1"), Some("build"), Some("running")));
    // And when its shell exits the phone is sent an upsert, and when it is closed a remove
    *h.agent.terminal_list.lock().unwrap() = vec![summary(NAME, false)];
    let upsert = rpc.chunk(&id)[0].clone();
    assert_eq!((upsert["type"].as_str(), upsert["terminal"]["status"].as_str()), (Some("upsert"), Some("exited")));
    h.agent.terminal_list.lock().unwrap().clear();
    let remove = rpc.chunk(&id)[0].clone();
    assert_eq!(remove, json!({"type": "remove", "threadId": "t1", "terminalId": "term-1"}));
}

#[test]
fn closing_a_thread_with_no_terminal_named_closes_every_terminal_of_that_thread_and_none_of_another() {
    // Given three terminals, two of the thread t1 and one of another thread
    let (h, mut rpc) = started();
    *h.agent.terminal_list.lock().unwrap() = vec![summary("t1.a", true), summary("t1.b", true), summary("t9.a", true)];
    // When the phone closes the thread's terminals without naming one
    let closed = rpc.call("terminal.close", json!({"threadId": "t1"}));
    // Then the agent API was asked to close the two of t1 only
    assert_eq!(closed["_tag"], "Success");
    let closes: Vec<String> = ops(&h).iter().filter_map(|op| if let Op::TerminalClose { terminal, .. } = op { Some(terminal.clone()) } else { None }).collect();
    assert_eq!(closes, ["t1.a", "t1.b"]);
}
