//! `wf mimic proxy record` and `wf mimic proxy replay` through the thin binary: a client's journey against a T3-shaped upstream
//! is recorded, the upstream is stopped, and a second client makes the journey against the cassette.
use crate::proxy_support::*;
use std::path::{Path, PathBuf};

const FIRST_RUN: [&str; 3] = ["1", "2", "6f1e6a2e-9d0b-4d7c-8a51-0c2a4b9f1e11"];
const SECOND_RUN: [&str; 3] = ["501", "502", "0b7c2f4d-1a3e-4f5a-9b6c-7d8e9f0a1b2c"];

fn text(path: &Path) -> String {
    path.display().to_string()
}

/// The journey recorded into `<dir>/set`: what the client saw, what the verb printed and where the set is.
fn recorded(dir: &Path, upstream: &Upstream) -> (Seen, Ended, PathBuf) {
    let set = dir.join("set");
    let mut tool = Tool::start(dir, &["record", "--set", &text(&set), "--upstream", &format!("http://{}", upstream.addr)], &[]);
    let seen = journey(tool.addr, FIRST_RUN);
    (seen, tool.stop(), set)
}

#[test]
fn given_a_recorded_journey_when_the_server_is_stopped_and_a_second_client_makes_it_with_other_ids_then_it_sees_the_same_messages_and_nothing_reaches_the_server() {
    // Given a journey recorded against the upstream, which is then stopped
    let dir = tempfile::tempdir().unwrap();
    let upstream = Upstream::start();
    let (first, record, set) = recorded(dir.path(), &upstream);
    assert_eq!((record.code, &record.json["ok"], &record.json["kind"], &record.json["http"], &record.json["ws"]), (0, &json!(true), &json!("authored"), &json!(4), &json!(1)), "{}", record.stdout);
    upstream.stop();

    // When a second client makes the journey against the cassette, with request ids and a command id of its own
    let mut replay = Tool::start(dir.path(), &["replay", &text(&set)], &[]);
    let second = journey(replay.addr, SECOND_RUN);
    let ended = replay.stop();

    // Then it sees the same thread and the same frames, in the same order, under its own ids
    assert_eq!(second.thread, first.thread);
    assert_eq!(second.thread["messages"], messages());
    assert_eq!(second.frames[0], json!({"_tag": "Chunk", "requestId": "501", "values": first.frames[0]["values"]}));
    assert_eq!((&second.frames[1]["_tag"], &second.frames[1]["requestId"]), (&json!("Exit"), &json!("502")));
    assert_eq!(second.frames[2], json!({"_tag": "Chunk", "requestId": "501", "values": [{"kind": "message", "role": "assistant", "text": "reply to: say hello"}]}));
    // and what it was handed are fixture values, never the upstream's credentials
    assert_eq!((second.token.as_str(), second.ticket.as_str()), ("fx-access-token-1", "fx-ticket-1"));
    assert_eq!((&second.session["user"], &second.session["sessionToken"], &second.session["home"]), (&json!("fixture user"), &json!("fx-session-token-1"), &json!("/fixture/home")));
    // and the replay opened no connection, and nothing reached the stopped server
    assert_eq!((ended.code, &ended.json["ok"], &ended.json["upstream_connections"], &ended.json["mismatch_count"]), (0, &json!(true), &json!(0), &json!(0)), "{}", ended.stdout);
    assert_eq!((&ended.json["http"]["matched"], &ended.json["ws"]["matched"], &ended.json["unused"]), (&json!(4), &json!(2), &json!([])), "{}", ended.stdout);
    assert_eq!(upstream.reached_after_stop(), 0);
}

#[test]
fn given_a_replay_when_a_client_asks_for_what_was_not_recorded_then_the_run_fails_with_the_route_the_method_and_the_place_in_the_cassette() {
    // Given a replay of a recorded journey, its upstream stopped
    let dir = tempfile::tempdir().unwrap();
    let upstream = Upstream::start();
    let (_, _, set) = recorded(dir.path(), &upstream);
    upstream.stop();
    let mut replay = Tool::start(dir.path(), &["replay", &text(&set)], &[]);

    // When a client asks for a route, a method and a payload the recording does not have
    let (status, head, _) = http(replay.addr, "GET", "/api/unknown", &[], "");
    let mut socket = Client::open(replay.addr, "/ws?wsTicket=whatever-the-client-holds").expect("the socket opens");
    socket.send(&json!({"_tag": "Request", "id": "9", "tag": "thread.delete", "payload": {}}));
    let unknown_method = socket.closed_with();
    let mut socket = Client::open(replay.addr, "/ws?wsTicket=whatever-the-client-holds").expect("the socket opens");
    socket.send(&json!({"_tag": "Request", "id": "9", "tag": "orchestration.dispatchCommand", "payload": {"commandId": SECOND_RUN[2], "text": "say bye"}, "traceId": "trace-9"}));
    let changed_payload = socket.closed_with();
    let ended = replay.stop();

    // Then each is refused where the client can see it
    assert_eq!((status, head.to_lowercase().contains("x-mimic-mismatch")), (502, true), "{head}");
    assert_eq!((unknown_method, changed_payload), (Some(1011), Some(1011)));
    // and the run fails, naming what was asked and the nearest recording
    let mismatches = ended.json["mismatches"].as_array().expect("mismatches");
    assert_eq!((ended.code, &ended.json["ok"], mismatches.len()), (1, &json!(false), 3), "{}", ended.stdout);
    assert_eq!((&mismatches[0]["what"], &mismatches[1]["what"], &mismatches[2]["what"]), (&json!("GET /api/unknown"), &json!("thread.delete"), &json!("orchestration.dispatchCommand")));
    let nearest = &mismatches[2]["nearest"];
    assert_eq!((&nearest["interaction"], &nearest["cassette"], &nearest["diff"][0]["path"]), (&json!("c01"), &json!("interactions.jsonl:5"), &json!("payload.text")), "{nearest}");
    assert_eq!((&nearest["diff"][0]["expected"], &nearest["diff"][0]["actual"]), (&json!("say hello"), &json!("say bye")));
    // and still nothing left the machine
    assert_eq!((&ended.json["upstream_connections"], upstream.reached_after_stop()), (&json!(0), 0));
}

#[test]
fn given_a_set_that_holds_a_recording_when_another_is_recorded_into_it_then_it_is_refused() {
    // Given
    let dir = tempfile::tempdir().unwrap();
    let upstream = Upstream::start();
    let (_, _, set) = recorded(dir.path(), &upstream);
    let before = std::fs::read_to_string(set.join("interactions.jsonl")).unwrap();

    // When
    let again = run(&["record", "--set", &text(&set), "--upstream", &format!("http://{}", upstream.addr), "--duration", "1"]);

    // Then
    assert_eq!(again.code, 2, "{}", again.stdout);
    assert!(again.json["error"].as_str().unwrap().contains("holds a recording"), "{}", again.stdout);
    assert_eq!(std::fs::read_to_string(set.join("interactions.jsonl")).unwrap(), before);
}

#[test]
fn given_an_answer_the_proxy_cannot_read_when_the_run_ends_then_nothing_is_written() {
    // Given an upstream that compresses an answer whatever the client asked for
    let dir = tempfile::tempdir().unwrap();
    let upstream = Upstream::start();
    *upstream.extra.lock().unwrap() = ("content-encoding: gzip\r\n".to_string(), vec![0x1f, 0x8b, 0x08, 0x00]);
    let set = dir.path().join("set");
    let mut tool = Tool::start(dir.path(), &["record", "--set", &text(&set), "--upstream", &format!("http://{}", upstream.addr)], &[]);

    // When
    http(tool.addr, "GET", "/api/extra", &[("accept-encoding", "gzip")], "");
    let ended = tool.stop();

    // Then
    assert_eq!((ended.code, &ended.json["ok"], &ended.json["wrote"]), (1, &json!(false), &json!([])), "{}", ended.stdout);
    assert!(ended.json["error"].as_str().unwrap().contains("encoded body"), "{}", ended.stdout);
    assert!(!set.join("cassette.json").exists() && !set.join("interactions.jsonl").exists());
    // and the recorder itself never asked the server to compress
    assert!(!upstream.asked_for_encoding.load(std::sync::atomic::Ordering::SeqCst));
}
