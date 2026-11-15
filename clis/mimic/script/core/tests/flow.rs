//! The interpreter through its ports with the in-memory doubles; the last test replays the committed T3 cassettes as the oracle of its steps.
use mimic_core::cassette::{Dir, Status, Step};
use mimic_core::ports::Observed;
use mimic_script_core::case::Case;
use mimic_script_core::testing::{text, CassetteEchoTransport, CassettePhone, ScriptedPhone, FakeClock, FixedTarget, ScriptedTransport};
use mimic_script_core::{Clock, Event, Interpreter, Policy, PhoneEvent};
use serde_json::{json, Value};

fn run_with(case: Value, transport: Box<dyn mimic_script_core::Transport>, clock: &FakeClock, policy: Policy, target: FixedTarget) -> Observed {
    let mut interpreter = Interpreter::new(transport, Box::new(target), Box::new(clock.clone()), policy);
    interpreter.run_case(&Case::parse(&case).unwrap()).unwrap()
}

fn run(case: Value, transport: &ScriptedTransport, clock: &FakeClock) -> Observed {
    run_with(case, Box::new(transport.clone()), clock, Policy::default(), FixedTarget::new("/tmp/ws", "cred-1"))
}

fn channels(o: &Observed) -> Vec<String> {
    o.steps.iter().map(|s| format!("{:?}/{}", s.dir, s.channel.as_deref().unwrap_or("-"))).collect()
}

#[test]
fn given_a_token_answer_when_the_next_call_names_it_then_the_bearer_is_filled_and_the_first_body_stays_raw() {
    let clock = FakeClock::default();
    let transport = ScriptedTransport::new(&clock);
    transport.reply(200, &[("Content-Type", "application/json")], r#"{"access_token":"tok-1"}"#).reply(200, &[], "");

    let observed = run(json!({"script": [{"http": {"method": "POST", "path": "/oauth/token"}}, {"http": {"path": "/me", "headers": {"authorization": "Bearer {access_token}"}}}]}), &transport, &clock);

    assert_eq!(transport.calls()[1].headers, [("authorization".to_string(), "Bearer tok-1".to_string())]);
    assert_eq!(observed.steps[1].payload["body"], json!(r#"{"access_token":"tok-1"}"#));
    assert_eq!(observed.steps[3].payload["body"], Value::Null);
}

#[test]
fn given_a_form_post_with_an_object_body_when_run_then_the_call_step_shows_the_urlsearchparams_text() {
    let clock = FakeClock::default();
    let transport = ScriptedTransport::new(&clock);
    transport.reply(200, &[], "{}");

    let observed = run(json!({"script": [{"http": {"method": "POST", "path": "/f", "headers": {"content-type": "application/x-www-form-urlencoded"}, "body": {"a b": "x~y"}}}]}), &transport, &clock);

    assert_eq!(observed.steps[0].payload["body"], json!("a+b=x%7Ey"));
}

#[test]
fn given_a_reply_with_repeated_headers_and_no_body_when_run_then_names_are_lower_case_joined_and_the_body_is_null() {
    let clock = FakeClock::default();
    let transport = ScriptedTransport::new(&clock);
    transport.reply(204, &[("Set-Cookie", "a=1"), ("set-cookie", "b=2")], "");

    let observed = run(json!({"script": [{"http": {"path": "/x"}}]}), &transport, &clock);

    assert_eq!(observed.steps[1].payload, json!({"status": 204, "headers": {"set-cookie": "a=1, b=2"}, "body": null}));
}

#[test]
fn given_a_captured_number_when_used_as_a_whole_string_then_it_keeps_its_type_and_embedded_it_becomes_text() {
    let clock = FakeClock::default();
    let transport = ScriptedTransport::new(&clock);
    transport.reply(200, &[], r#"{"run":{"id":42}}"#).reply(200, &[], "{}");

    run(json!({"script": [{"http": {"path": "/a"}, "capture": {"runId": {"from": ["/run/id"]}}}, {"http": {"path": "/b/{runId}", "method": "POST", "body": "{runId}"}}]}), &transport, &clock);

    let call = &transport.calls()[1];
    assert_eq!((call.path.as_str(), call.body.as_deref()), ("/b/42", Some("42")));
}

#[test]
fn given_a_declared_capture_that_nothing_produced_when_an_entry_uses_it_then_nothing_is_sent_and_the_script_goes_on() {
    let clock = FakeClock::default();
    let transport = ScriptedTransport::new(&clock);
    transport.reply(200, &[], "{}").reply(200, &[], "{}");

    let observed = run(json!({"script": [{"http": {"path": "/a"}, "capture": {"runId": {"from": ["/none"]}}}, {"http": {"path": "/b/{runId}"}}, {"http": {"path": "/c"}}]}), &transport, &clock);

    assert_eq!(transport.calls().iter().map(|c| c.path.as_str()).collect::<Vec<_>>(), ["/a", "/c"]);
    assert_eq!(observed.steps[2].payload, json!({"client_error": "unbound variable runId", "op": "http"}));
    assert_eq!(observed.outcome.status, Status::Ok);
}

#[test]
fn given_scripts_when_run_then_a_credential_is_minted_once_and_only_when_named() {
    let clock = FakeClock::default();
    let transport = ScriptedTransport::new(&clock);
    transport.reply(200, &[], "{}").reply(200, &[], "{}").reply(200, &[], "{}");
    let target = FixedTarget::new("/tmp/ws", "cred-1");
    let mints = target.mints.clone();

    let observed = run_with(json!({"script": [{"http": {"path": "/a", "body": "{credential}"}}, {"http": {"path": "/b", "body": "{credential}"}}]}), Box::new(transport.clone()), &clock, Policy::default(), target);
    assert_eq!((mints.get(), observed.steps[0].payload["body"].clone()), (1, json!("cred-1")));

    let target = FixedTarget::new("/tmp/ws", "cred-1");
    let mints = target.mints.clone();
    run_with(json!({"script": [{"http": {"path": "/c"}}]}), Box::new(transport.clone()), &clock, Policy::default(), target);
    assert_eq!(mints.get(), 0);
}

#[test]
fn given_an_exit_that_comes_after_three_seconds_when_awaited_then_the_quiet_window_runs_after_the_exit_not_before() {
    let clock = FakeClock::default();
    let transport = ScriptedTransport::new(&clock);
    transport.open(101, vec![]).answer_send(vec![(3000, text(r#"{"_tag":"Exit","requestId":"7","exit":{"_tag":"Success"}}"#))]);

    let observed = run(json!({"script": [{"ws-open": {"path": "/ws"}}, {"ws-send": {"_tag": "Request", "id": "7", "tag": "t", "payload": {}, "headers": []}, "await": "exit"}]}), &transport, &clock);

    assert_eq!(channels(&observed), ["S2c/ws:open", "C2s/ws", "S2c/ws"]);
    assert!(clock.now_ms() >= 3000 + 250, "the run ended at {}", clock.now_ms());
}

#[test]
fn given_an_exit_that_never_comes_when_awaited_then_the_script_goes_on_after_fifteen_seconds_without_a_busy_loop() {
    let clock = FakeClock::default();
    let transport = ScriptedTransport::new(&clock);
    transport.open(101, vec![]);

    let observed = run(json!({"script": [{"ws-open": {"path": "/ws"}}, {"ws-send": {"_tag": "Request", "id": "7", "tag": "t"}, "await": "exit"}]}), &transport, &clock);

    assert_eq!(observed.outcome.status, Status::Ok);
    assert!(clock.sleeps() <= 15000 / 25 + 250 / 25 + 2, "{} sleeps", clock.sleeps());
}

#[test]
fn given_a_request_answered_after_two_seconds_when_sent_without_await_then_the_first_answer_wait_catches_it() {
    let clock = FakeClock::default();
    let transport = ScriptedTransport::new(&clock);
    transport.open(101, vec![]).answer_send(vec![(2000, text(r#"{"_tag":"Exit","requestId":"1"}"#))]);

    let observed = run(json!({"script": [{"ws-open": {"path": "/ws"}}, {"ws-send": {"_tag": "Request", "id": "1", "tag": "t"}}]}), &transport, &clock);

    assert!(observed.steps.iter().any(|s| s.dir == Dir::S2c && s.payload["_tag"] == "Exit"));
}

#[test]
fn given_a_chunk_frame_when_it_arrives_then_the_ack_step_follows_it_at_once_and_is_sent() {
    let clock = FakeClock::default();
    let transport = ScriptedTransport::new(&clock);
    transport.open(101, vec![]).answer_send(vec![(10, text(r#"{"_tag":"Chunk","requestId":"9","values":[1]}"#)), (20, text(r#"{"_tag":"Chunk","requestId":"9","values":[2]}"#))]);

    let observed = run(json!({"script": [{"ws-open": {"path": "/ws"}}, {"ws-send": {"_tag": "Request", "id": "9", "tag": "t"}}]}), &transport, &clock);

    let tags: Vec<(Dir, String)> = observed.steps.iter().skip(2).map(|s| (s.dir, s.payload["_tag"].as_str().unwrap_or("").to_string())).collect();
    assert_eq!(tags, [(Dir::S2c, "Chunk".into()), (Dir::C2s, "Ack".into()), (Dir::S2c, "Chunk".into()), (Dir::C2s, "Ack".into())]);
    assert_eq!(transport.sent().len(), 3);
}

#[test]
fn given_a_failing_transport_when_run_then_a_transport_error_step_ends_the_script_with_an_error_outcome() {
    let clock = FakeClock::default();
    let transport = ScriptedTransport::new(&clock);
    transport.fail_http("ECONNREFUSED");

    let observed = run(json!({"script": [{"http": {"path": "/a"}}, {"http": {"path": "/b"}}]}), &transport, &clock);

    assert_eq!(observed.steps.len(), 1);
    assert_eq!(observed.steps[0].payload, json!({"transport_error": "ECONNREFUSED"}));
    assert_eq!((observed.outcome.status, observed.outcome.code.as_deref()), (Status::Error, Some("transport")));
}

#[test]
fn given_a_refused_upgrade_when_a_frame_is_sent_then_the_real_status_is_recorded_and_the_send_has_no_socket() {
    let clock = FakeClock::default();
    let transport = ScriptedTransport::new(&clock);
    transport.open(403, vec![]);

    let observed = run(json!({"script": [{"ws-open": {"path": "/ws"}}, {"ws-send": {"id": "1"}}]}), &transport, &clock);

    assert_eq!(observed.steps[0].payload, json!({"path": "/ws", "status": 403}));
    assert_eq!(observed.steps.last().unwrap().payload, json!({"transport_error": "no socket"}));
}

#[test]
fn given_a_peer_that_answers_a_close_when_closed_then_both_closes_are_steps_and_the_connection_end_is_last() {
    let clock = FakeClock::default();
    let transport = ScriptedTransport::new(&clock);
    transport.open(101, vec![]);

    let observed = run(json!({"script": [{"ws-open": {"path": "/ws"}}, {"ws-close": {"code": 1000, "reason": "bye"}}]}), &transport, &clock);

    let closes: Vec<Value> = observed.steps.iter().filter(|s| s.channel.as_deref() == Some("ws:close")).map(|s| s.payload.clone()).collect();
    assert_eq!(closes, [json!({"code": 1000, "reason": "bye", "tcp": false}), json!({"code": null, "reason": "", "tcp": true})]);
    assert_eq!(transport.closed(), [(1000, "bye".to_string())]);
}

#[test]
fn given_a_peer_that_closes_the_connection_after_its_close_frame_when_closed_then_the_connection_end_is_one_step_not_two() {
    let clock = FakeClock::default();
    let transport = ScriptedTransport::new(&clock);
    transport.open(101, vec![]);
    transport.answer_close(vec![(10, Event::Close { code: Some(1000), reason: String::new() }), (20, Event::Dropped)]);

    let observed = run(json!({"script": [{"ws-open": {"path": "/ws"}}, {"ws-close": {}}]}), &transport, &clock);

    let ends = observed.steps.iter().filter(|s| s.channel.as_deref() == Some("ws:close") && s.payload["tcp"] == json!(true)).count();
    assert_eq!(ends, 1, "{:?}", channels(&observed));
}

#[test]
fn given_the_workspace_path_in_an_answer_when_run_then_every_step_shows_it_masked() {
    let clock = FakeClock::default();
    let transport = ScriptedTransport::new(&clock);
    transport.reply(200, &[], r#"{"cwd":"/tmp/ws/project"}"#);

    let observed = run(json!({"script": [{"http": {"path": "/a", "headers": {"x-ws": "{workspace}"}}}]}), &transport, &clock);

    assert_eq!(observed.steps[0].payload["headers"]["x-ws"], json!("<workspace>"));
    assert_eq!(observed.steps[1].payload["body"], json!(r#"{"cwd":"<workspace>/project"}"#));
}

#[test]
fn given_the_same_case_and_transport_behaviour_when_run_twice_then_the_steps_are_identical() {
    let once = || {
        let clock = FakeClock::default();
        let transport = ScriptedTransport::new(&clock);
        transport.reply(200, &[], r#"{"ticket":"t1"}"#).open(101, vec![(5, text(r#"{"hello":1}"#))]).answer_send(vec![(40, text(r#"{"_tag":"Exit","requestId":"1"}"#))]);
        run(json!({"script": [{"http": {"path": "/t"}}, {"ws-open": {"path": "/ws?t={ticket}"}}, {"ws-send": {"_tag": "Request", "id": "1", "tag": "x"}}]}), &transport, &clock).steps
    };

    assert_eq!(once(), once());
}

/// The five entries every session-prelude case starts with, as the first recorded case carries them inline.
fn session_prelude(root: &std::path::Path) -> Vec<mimic_script_core::case::Entry> {
    let first = cassette_steps(&root.join("t3-recorded-1")).into_iter().find(|(_, input, _)| input["script"].as_array().is_some_and(|s| s.len() > 5)).expect("a recorded case with the full session");
    Case::parse(&first.1).unwrap().entries.into_iter().take(5).collect()
}

fn prelude_entries() -> Vec<mimic_script_core::case::Entry> {
    Case::entries_of(&json!([
        {"http": {"method": "POST", "path": "/oauth/token", "body": {"subject_token": "{credential}"}}},
        {"http": {"method": "POST", "path": "/ticket", "headers": {"authorization": "Bearer {access_token}"}}},
        {"ws-open": {"path": "/ws?t={ticket}"}},
        {"ws-send": {"_tag": "Request", "id": "p1", "tag": "setup"}, "await": "exit"},
        {"ws-send": {"_tag": "Request", "id": "p2", "tag": "setup"}, "await": "exit"}
    ]))
    .unwrap()
}

fn exit_of(id: &str) -> Event {
    text(&json!({"_tag": "Exit", "requestId": id, "exit": {"_tag": "Success"}}).to_string())
}

/// What a server answers to the prelude and to `cases` own sockets after it.
fn session_server(clock: &FakeClock, own_sockets: usize) -> ScriptedTransport {
    let transport = ScriptedTransport::new(clock);
    transport.reply(200, &[], r#"{"access_token":"tok"}"#).reply(200, &[], r#"{"ticket":"tk-0"}"#).open(101, vec![]).answer_send(vec![(5, exit_of("p1"))]).answer_send(vec![(5, exit_of("p2"))]);
    for n in 0..own_sockets {
        transport.reply(200, &[], &format!(r#"{{"ticket":"tk-{}"}}"#, n + 1)).open(101, vec![]).answer_send(vec![(5, exit_of("c"))]);
    }
    transport
}

fn session_case() -> Value {
    json!({"prelude": "session", "script": [{"ws-send": {"_tag": "Request", "id": "c", "tag": "work"}}]})
}

#[test]
fn given_session_mode_when_two_cases_run_then_the_prelude_runs_once_and_both_begin_with_its_steps() {
    let clock = FakeClock::default();
    let transport = session_server(&clock, 2);
    let target = FixedTarget::new("/tmp/ws", "cred-1");
    let mints = target.mints.clone();
    let mut interpreter = Interpreter::new(Box::new(transport.clone()), Box::new(target), Box::new(clock.clone()), Policy::default()).with_prelude(prelude_entries()).with_session(true);

    let first = interpreter.run_case(&Case::parse(&session_case()).unwrap()).unwrap();
    let second = interpreter.run_case(&Case::parse(&session_case()).unwrap()).unwrap();

    let token_calls = transport.calls().iter().filter(|c| c.path == "/oauth/token").count();
    assert_eq!((token_calls, mints.get()), (1, 1));
    assert_eq!(first.steps[..second.steps.len() - 2], second.steps[..second.steps.len() - 2]);
    assert_eq!(channels(&first).last().unwrap(), "S2c/ws");
    assert!(first.steps.iter().all(|s| s.channel.as_deref() != Some("ws:close")));
    assert_eq!(transport.calls()[1].headers, [("authorization".to_string(), "Bearer tok".to_string())]);
}

#[test]
fn given_session_mode_when_the_server_restarts_then_the_prelude_runs_again() {
    let clock = FakeClock::default();
    let transport = session_server(&clock, 1);
    let target = FixedTarget::new("/tmp/ws", "cred-1");
    let (mints, generation) = (target.mints.clone(), target.generation.clone());
    let mut interpreter = Interpreter::new(Box::new(transport.clone()), Box::new(target), Box::new(clock.clone()), Policy::default()).with_prelude(prelude_entries()).with_session(true);
    interpreter.run_case(&Case::parse(&session_case()).unwrap()).unwrap();

    generation.set(1);
    transport.reply(200, &[], r#"{"access_token":"tok2"}"#).reply(200, &[], r#"{"ticket":"tk-r"}"#).open(101, vec![]).answer_send(vec![(5, exit_of("p1"))]).answer_send(vec![(5, exit_of("p2"))]);
    transport.reply(200, &[], r#"{"ticket":"tk-r2"}"#).open(101, vec![]).answer_send(vec![(5, exit_of("c"))]);
    interpreter.run_case(&Case::parse(&session_case()).unwrap()).unwrap();

    assert_eq!((transport.calls().iter().filter(|c| c.path == "/oauth/token").count(), mints.get()), (2, 2));
}

fn cassette_steps(dir: &std::path::Path) -> Vec<(String, Value, Vec<Step>)> {
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap_or_else(|e| panic!("{}: {e}", dir.join(name).display()));
    let cassette = mimic_core::cassette::Cassette::parse(&read("cassette.json"), &read("interactions.jsonl")).unwrap();
    cassette.interactions.into_iter().map(|i| (i.id, i.input, i.steps)).collect()
}

#[test]
fn given_the_committed_t3_recordings_when_their_scripts_run_against_their_own_steps_then_the_interpreter_reproduces_them() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../t3-serve/cassettes");
    let (mut checked, mut mismatched) = (0, Vec::new());
    let prelude = session_prelude(&root);
    for set in ["t3-recorded-1", "t3-recorded-2", "t3-recorded-3", "t3-crawl", "t3-scenarios"] {
        let dir = root.join(set);
        let rules: mimic_core::rules::RuleSet = serde_json::from_str(&std::fs::read_to_string(dir.join("rules.json")).unwrap()).unwrap();
        let compiled = rules.compile().unwrap();
        for (id, input, recorded) in cassette_steps(&dir) {
            let Ok(case) = Case::parse(&input) else { continue };
            let clock = FakeClock::default();
            let echo = CassetteEchoTransport::new(recorded.clone());
            let mut interpreter = Interpreter::new(Box::new(echo), Box::new(FixedTarget::new("/tmp/mimic-ws", "cred-1")), Box::new(clock), Policy::default()).with_prelude(prelude.clone());
            let observed = interpreter.run_case(&case).unwrap();
            let masked = compiled.interaction(&mimic_core::cassette::Interaction { id: id.clone(), row: None, op: None, input: input.clone(), steps: observed.steps, effects: vec![], outcome: observed.outcome, tags: vec![] });
            checked += 1;
            if masked.steps != recorded {
                let at = masked.steps.iter().zip(&recorded).position(|(a, b)| a != b).unwrap_or(masked.steps.len().min(recorded.len()));
                let show = |steps: &[Step]| steps.get(at).map_or("(none)".to_string(), |s| format!("{:?}/{:?} {}", s.dir, s.channel, s.payload.to_string().chars().take(300).collect::<String>()));
                mismatched.push(format!("{set}/{id} step {at} of {}/{}: got {} | recorded {}", masked.steps.len(), recorded.len(), show(&masked.steps), show(&recorded)));
            }
        }
    }
    assert!(checked > 100, "only {checked} recorded scripts were found");
    assert!(mismatched.is_empty(), "{} of {checked} differ, first: {:#?}", mismatched.len(), &mismatched[..mismatched.len().min(3)]);
}

#[test]
fn given_the_committed_orca_recordings_when_their_requests_run_against_their_own_answers_then_the_interpreter_reproduces_them() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../orca-serve/cassettes");
    let (mut checked, mut mismatched) = (0, Vec::new());
    for set in ["orca-recorded", "orca-crawl", "orca-scenarios"] {
        let dir = root.join(set);
        let rules: mimic_core::rules::RuleSet = serde_json::from_str(&std::fs::read_to_string(dir.join("rules.json")).unwrap()).unwrap();
        let compiled = rules.compile().unwrap();
        for (id, input, recorded) in cassette_steps(&dir) {
            let case = Case::parse(&input).unwrap();
            let phone = CassettePhone::new(&recorded);
            let mut interpreter = Interpreter::new(Box::new(ScriptedTransport::new(&FakeClock::default())), Box::new(FixedTarget::new("", "cred-1")), Box::new(FakeClock::default()), Policy::default()).with_phone(Box::new(phone));
            let observed = interpreter.run_case(&case).unwrap();
            let masked = compiled.interaction(&mimic_core::cassette::Interaction { id: id.clone(), row: None, op: None, input: input.clone(), steps: observed.steps, effects: vec![], outcome: observed.outcome, tags: vec![] });
            checked += 1;
            if masked.steps != recorded {
                mismatched.push(format!("{set}/{id}: got {} steps, recorded {}", masked.steps.len(), recorded.len()));
            }
        }
    }
    assert!(checked > 250, "only {checked} recorded Orca cases were found");
    assert!(mismatched.is_empty(), "{} of {checked} differ, first: {:#?}", mismatched.len(), &mismatched[..mismatched.len().min(3)]);
}

fn orca_run(case: Value, phone: &ScriptedPhone, clock: &FakeClock) -> Observed {
    let mut interpreter = Interpreter::new(Box::new(ScriptedTransport::new(clock)), Box::new(FixedTarget::new("", "cred")), Box::new(clock.clone()), Policy::default()).with_phone(Box::new(phone.clone()));
    interpreter.run_case(&Case::parse(&case).unwrap()).unwrap()
}

fn frame(payload: Value) -> PhoneEvent {
    PhoneEvent::Frame { sub: String::new(), binary: false, payload, len: None }
}

fn shape(o: &Observed) -> Vec<(String, Option<String>, Option<String>, Value)> {
    o.steps.iter().map(|s| (format!("{:?}", s.dir), s.channel.clone(), s.rid.clone(), s.payload.clone())).collect()
}

#[test]
fn given_a_stream_when_opened_and_listened_to_then_the_opener_and_the_frames_inside_the_window_are_steps_with_its_rid() {
    let clock = FakeClock::default();
    let phone = ScriptedPhone::new(&clock);
    phone.streams(vec![(50, frame(json!({"n": 1}))), (200, frame(json!({"n": 2}))), (900, frame(json!({"n": 3})))]);

    let observed = orca_run(json!({"script": [{"connect": {}}, {"subscribe": {"method": "notifications.subscribe", "params": {}, "rid": "s0", "listen_ms": 300}}]}), &phone, &clock);

    assert_eq!(
        shape(&observed),
        [
            ("C2s".to_string(), None, Some("s0".to_string()), json!({"method": "notifications.subscribe", "params": {}})),
            ("S2c".to_string(), None, Some("s0".to_string()), json!({"n": 1})),
            ("S2c".to_string(), None, Some("s0".to_string()), json!({"n": 2})),
        ]
    );
    assert_eq!(phone.subscribed().len(), 1);
}

#[test]
fn given_a_binary_frame_when_it_arrives_then_the_step_is_binary_with_its_length_and_base64_payload() {
    let clock = FakeClock::default();
    let phone = ScriptedPhone::new(&clock);
    phone.streams(vec![(10, PhoneEvent::Frame { sub: String::new(), binary: true, payload: json!("AQID"), len: Some(3) })]);

    let observed = orca_run(json!({"script": [{"connect": {}}, {"subscribe": {"method": "terminal.subscribe", "rid": "s0", "listen_ms": 100}}]}), &phone, &clock);

    let step = &observed.steps[1];
    assert_eq!((format!("{:?}", step.kind), step.len, step.payload.clone()), ("Binary".to_string(), Some(3), json!("AQID")));
}

#[test]
fn given_an_open_stream_when_unsubscribed_then_the_phone_is_told_and_frames_still_on_their_way_are_taken() {
    let clock = FakeClock::default();
    let phone = ScriptedPhone::new(&clock);
    phone.streams(vec![(300, frame(json!({"late": true})))]);

    let observed = orca_run(json!({"script": [{"connect": {}}, {"subscribe": {"method": "m", "rid": "s0", "listen_ms": 100}}, {"unsubscribe": {"rid": "s0"}}], "settle_ms": 400}), &phone, &clock);

    assert_eq!(phone.unsubscribed(), ["sub-0"]);
    assert_eq!(observed.steps.last().unwrap().payload, json!({"late": true}));
}

#[test]
fn given_a_phone_that_refuses_a_stream_when_subscribed_then_the_failure_is_the_answer_step_and_the_script_goes_on() {
    let clock = FakeClock::default();
    let phone = ScriptedPhone::new(&clock);
    phone.refusing("Connection interrupted");

    let observed = orca_run(json!({"script": [{"connect": {}}, {"subscribe": {"method": "m", "rid": "s0"}}, {"request": {"method": "files.list", "params": {}, "rid": "0"}}]}), &phone, &clock);

    assert_eq!(observed.steps[1].payload, json!({"client_error": "Connection interrupted"}));
    assert_eq!(observed.steps.len(), 4);
    assert_eq!(observed.outcome.status, Status::Ok);
}

#[test]
fn given_the_handshake_tap_when_connected_then_each_stage_is_a_handshake_step_and_without_the_tap_there_are_none() {
    let clock = FakeClock::default();
    let phone = ScriptedPhone::new(&clock);
    phone.handshake(vec![
        PhoneEvent::Handshake { stage: "hello".into(), client_to_server: true, payload: json!({"key": "<b64:32>"}) },
        PhoneEvent::Handshake { stage: "ready".into(), client_to_server: false, payload: json!({"key": "<b64:32>"}) },
    ]);

    let tapped = orca_run(json!({"script": [{"connect": {"capture_handshake": true}}]}), &phone, &clock);
    let plain = orca_run(json!({"script": [{"connect": {}}]}), &ScriptedPhone::new(&clock), &clock);

    assert_eq!(shape(&tapped).iter().map(|s| (s.0.as_str(), s.1.as_deref())).collect::<Vec<_>>(), [("C2s", Some("handshake")), ("S2c", Some("handshake"))]);
    assert_eq!(tapped.steps[0].payload, json!({"stage": "hello", "frame": {"key": "<b64:32>"}}));
    assert!(plain.steps.is_empty());
}

#[test]
fn given_the_close_tap_when_a_case_ends_then_the_end_of_the_connection_is_its_last_step_and_without_the_tap_there_is_none() {
    let clock = FakeClock::default();
    let tapped = orca_run(json!({"script": [{"connect": {"capture_close": true}}]}), &ScriptedPhone::new(&clock), &clock);
    let plain = orca_run(json!({"script": [{"connect": {}}]}), &ScriptedPhone::new(&clock), &clock);

    assert_eq!(shape(&tapped), [("S2c".to_string(), Some("ws:close".to_string()), None, json!({"code": 1000, "reason": ""}))]);
    assert!(plain.steps.is_empty());
}

#[test]
fn given_a_connect_with_a_bad_credential_when_run_then_the_phone_is_given_it_filled_from_the_variables() {
    let clock = FakeClock::default();
    let phone = ScriptedPhone::new(&clock);

    orca_run(json!({"script": [{"connect": {"device_token": "{bad-credential}", "server_public_key_b64": "AQEB"}}]}), &phone, &clock);

    assert_eq!(phone.overrides(), [json!({"device_token": "NOTACREDENTIAL", "server_public_key_b64": "AQEB"})]);
}

#[allow(dead_code)]
fn unused(_: Event) {}
