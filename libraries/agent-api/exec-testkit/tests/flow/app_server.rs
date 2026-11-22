//! The scripted `codex app-server` as a process: it plays a recording or a run across starts, keeps what it was sent, and stops when a client leaves the script.
use exec_testkit::app_server::transcript;
use exec_testkit::{Run, Script, ScriptedCli};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};

fn start(cli: &ScriptedCli) -> Child {
    Command::new(cli.dir().join("codex")).arg("app-server").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap()
}

fn send(child: &mut Child, message: Value) {
    let input = child.stdin.as_mut().unwrap();
    writeln!(input, "{message}").unwrap();
    input.flush().unwrap();
}

/// The lines the server printed until the one for which `last` is true.
fn read_until(out: &mut BufReader<std::process::ChildStdout>, last: impl Fn(&Value) -> bool) -> Vec<Value> {
    let mut seen = Vec::new();
    loop {
        let mut line = String::new();
        assert!(out.read_line(&mut line).unwrap() > 0, "the server closed its output after {seen:?}");
        let frame: Value = serde_json::from_str(&line).unwrap();
        let done = last(&frame);
        seen.push(frame);
        if done {
            return seen;
        }
    }
}

#[test]
fn given_a_recorded_steer_when_a_client_follows_it_then_the_server_answers_in_the_recorded_order_under_the_clients_ids_and_logs_every_message_of_one_process() {
    // Given the scripted Codex playing the recording of a turn that is steered
    let cli = ScriptedCli::install();
    cli.script("codex", &Script::default().playing_app_server(vec![transcript("message_steering")]));
    let mut child = start(&cli);
    let mut out = BufReader::new(child.stdout.take().unwrap());
    // When a client that numbers its requests from 10 does the handshake, starts a thread and a turn, and steers it
    send(&mut child, json!({"id": 10, "method": "initialize", "params": {"clientInfo": {"name": "t"}}}));
    let init = read_until(&mut out, |f| f.get("id").is_some());
    send(&mut child, json!({"method": "initialized"}));
    send(&mut child, json!({"id": 11, "method": "thread/start", "params": {}}));
    let thread = read_until(&mut out, |f| f["id"] == 11);
    send(&mut child, json!({"id": 12, "method": "turn/start", "params": {"threadId": "x", "input": []}}));
    let turn = read_until(&mut out, |f| f["method"] == "turn/started");
    send(&mut child, json!({"id": 13, "method": "turn/steer", "params": {"threadId": "x", "expectedTurnId": "y", "input": []}}));
    let rest = read_until(&mut out, |f| f["method"] == "turn/completed");
    // Then every answer has the id of the request it answers, and the recorded notifications arrive between them
    assert_eq!((init.last().unwrap()["id"].clone(), thread.last().unwrap()["id"].clone()), (json!(10), json!(11)));
    assert!(turn.iter().any(|f| f["id"] == 12 && f["result"]["turn"]["id"].is_string()));
    assert!(rest.iter().any(|f| f["id"] == 13 && f["result"]["turnId"].is_string()));
    assert_eq!(rest.iter().filter(|f| f["method"] == "item/completed" && f["params"]["item"]["type"] == "agentMessage").count(), 2);
    // And the server logged each message with one process id, and ends when its input does
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());
    let log = cli.app_server_log("codex");
    assert_eq!(log.iter().map(|r| r.message["method"].as_str().unwrap_or("").to_string()).collect::<Vec<_>>(), ["initialize", "initialized", "thread/start", "turn/start", "turn/steer"]);
    assert!(log.iter().all(|r| r.start == 1 && r.pid == log[0].pid));
}

#[test]
fn given_runs_for_two_starts_when_the_server_starts_twice_then_the_second_start_plays_the_second_run_and_a_crash_run_ends_with_its_code() {
    // Given a first run that ends a turn and a second that resumes the thread and crashes with 9 during its turn
    let cli = ScriptedCli::install();
    let first = Run::new().handshake().thread_start("th-1").turn_saying("th-1", "u-1", "hi");
    let second = Run::new().handshake().thread_resume("th-1").turn_begun("th-1", "u-2").exiting(9);
    cli.script("codex", &Script::default().playing_app_server(vec![first.into(), second.into()]));
    // When the server starts, does its first run, and starts again
    let mut child = start(&cli);
    let mut out = BufReader::new(child.stdout.take().unwrap());
    for message in [json!({"id": 1, "method": "initialize"}), json!({"method": "initialized"}), json!({"id": 2, "method": "thread/start"}), json!({"id": 3, "method": "turn/start"})] {
        send(&mut child, message);
    }
    read_until(&mut out, |f| f["method"] == "turn/completed");
    drop(child.stdin.take());
    child.wait().unwrap();
    let mut child = start(&cli);
    for message in [json!({"id": 1, "method": "initialize"}), json!({"method": "initialized"}), json!({"id": 2, "method": "thread/resume", "params": {"threadId": "th-1"}}), json!({"id": 3, "method": "turn/start"})] {
        send(&mut child, message);
    }
    let status = child.wait().unwrap();
    // Then the second start played the second run, to its crash, and the log tells the starts apart by number and process
    assert_eq!(status.code(), Some(9));
    let log = cli.app_server_log("codex");
    assert_eq!(log.iter().map(|r| r.start).collect::<Vec<_>>(), [1, 1, 1, 1, 2, 2, 2, 2]);
    assert_ne!(log[0].pid, log[4].pid);
    assert_eq!(log[6].message["params"]["threadId"], "th-1");
}

#[test]
fn given_a_client_that_leaves_the_script_when_it_sends_the_wrong_request_then_the_server_stops_with_code_7_and_says_what_it_waited_for() {
    // Given a run that waits for a thread/start after the handshake
    let cli = ScriptedCli::install();
    cli.script("codex", &Script::default().playing_app_server(vec![Run::new().handshake().thread_start("th-1").into()]));
    let mut child = start(&cli);
    // When the client sends a turn/start instead
    for message in [json!({"id": 1, "method": "initialize"}), json!({"method": "initialized"}), json!({"id": 2, "method": "turn/start"})] {
        send(&mut child, message);
    }
    let done = child.wait_with_output().unwrap();
    // Then the server stopped with 7 and told why
    assert_eq!(done.status.code(), Some(7));
    assert!(String::from_utf8_lossy(&done.stderr).contains("expected a thread/start"), "{}", String::from_utf8_lossy(&done.stderr));
}
