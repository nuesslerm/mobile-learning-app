use serde_json::{json, Value};
use service_kit::telemetry::{Destination, Source};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use vm_ops_api::infrastructure::event_log::QUEUE_CAPACITY;
use vm_ops_api::infrastructure::{OtlpEventLog, StdoutEventLog, TeeEventLog};
use vm_ops_api_core::application::ports::{Event, EventLog, Level};
use vm_ops_api_core::testing::FakeEventLog;

const AUTH: &str = "vm-ops-auth-token-0123456789";
const GITHUB: &str = "ghp_exampleGithubToken0123456789";
const FLUSHED_WITHIN: Duration = Duration::from_secs(10);

/// What the adapter wrote, readable from the test.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("the capture").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Captured {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().expect("the capture").clone()).expect("UTF-8")
    }
}

#[test]
fn given_an_event_when_emitted_then_one_line_holds_level_message_and_the_attributes_in_order() {
    // Given an event log over a capture
    let out = Captured::default();
    let log = StdoutEventLog::to(out.clone());
    // When the listening event is emitted
    log.emit(Event::info("vm-ops-api.listening").with("port", 8080u16).with("host", "main"));
    // Then it is one JSON line, as Node prints it
    assert_eq!(out.text(), "{\"level\":\"info\",\"message\":\"vm-ops-api.listening\",\"port\":8080,\"host\":\"main\"}\n");
}

#[test]
fn given_events_of_each_level_when_emitted_then_each_is_its_own_line_with_its_level_name() {
    let out = Captured::default();
    let log = StdoutEventLog::to(out.clone());

    log.emit(Event::info("a"));
    log.emit(Event::error("b").with("err", "boom"));
    log.emit(Event { level: Level::Warn, message: "c", attributes: Vec::new() });

    assert_eq!(out.text(), "{\"level\":\"info\",\"message\":\"a\"}\n{\"level\":\"error\",\"message\":\"b\",\"err\":\"boom\"}\n{\"level\":\"warn\",\"message\":\"c\"}\n");
}

#[test]
fn given_text_that_json_must_escape_when_emitted_then_the_line_stays_one_line_and_escapes_as_json_stringify_does() {
    let out = Captured::default();
    let log = StdoutEventLog::to(out.clone());

    log.emit(Event::error("http.handler_failed").with("err", "line one\nline \"two\"\t\u{1f}"));

    assert_eq!(out.text(), "{\"level\":\"error\",\"message\":\"http.handler_failed\",\"err\":\"line one\\nline \\\"two\\\"\\t\\u001f\"}\n");
}

#[test]
fn given_an_attribute_named_like_a_leading_key_when_emitted_then_its_value_wins_in_the_keys_first_place() {
    let out = Captured::default();
    let log = StdoutEventLog::to(out.clone());

    log.emit(Event::info("a").with("level", "custom").with("n", 1u64));

    assert_eq!(out.text(), "{\"level\":\"custom\",\"message\":\"a\",\"n\":1}\n");
}

#[test]
fn given_an_event_log_shared_by_threads_when_each_emits_then_no_line_is_torn() {
    let out = Captured::default();
    let log = Arc::new(StdoutEventLog::to(out.clone()));

    let threads: Vec<_> = (0..8u64)
        .map(|n| {
            let log = log.clone();
            std::thread::spawn(move || (0..50u64).for_each(|i| log.emit(Event::info("beat").with("n", n).with("i", i))))
        })
        .collect();
    threads.into_iter().for_each(|t| t.join().expect("a thread"));

    let text = out.text();
    assert_eq!(text.lines().count(), 400);
    for line in text.lines() {
        let event: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|e| panic!("{line:?}: {e}"));
        assert_eq!(event["message"], "beat");
    }
}

#[test]
fn given_events_with_fractions_and_whole_floats_when_emitted_then_numbers_print_as_node_prints_them() {
    let out = Captured::default();
    let log = StdoutEventLog::to(out.clone());

    log.emit(Event::info("self_build.waiting_for_headroom").with("avg10", 12.5).with("total", 5.0).with("delayMs", 1000u64));

    assert_eq!(out.text(), "{\"level\":\"info\",\"message\":\"self_build.waiting_for_headroom\",\"avg10\":12.5,\"total\":5,\"delayMs\":1000}\n");
}

#[test]
fn given_secrets_when_an_event_carries_them_then_stdout_has_them_replaced_and_every_other_byte_is_nodes() {
    // Given a log that scrubs the auth token, the GitHub token and a secret too short to be told from a word
    let out = Captured::default();
    let log = StdoutEventLog::to(out.clone()).scrubbing([AUTH.to_string(), GITHUB.to_string(), "short".to_string()]);
    // When an event carries them inside text
    log.emit(
        Event::error("self_redeploy.failed")
            .with("err", format!("clone https://x:{GITHUB}@github.com failed with {AUTH}"))
            .with("note", "short stays")
            .with("n", 3u64),
    );
    // Then both are replaced where they stand, and the short one is not
    let want = "{\"level\":\"error\",\"message\":\"self_redeploy.failed\",\"err\":\"clone https://x:[REDACTED]@github.com failed with [REDACTED]\",\"note\":\"short stays\",\"n\":3}\n";
    assert_eq!(out.text(), want);
}

#[test]
fn given_a_secret_with_a_quote_in_it_when_it_is_nested_in_an_attribute_then_it_is_replaced_before_the_json_escapes_it() {
    let out = Captured::default();
    let secret = "pa\"ss\\word-1234";
    let log = StdoutEventLog::to(out.clone()).scrubbing([secret.to_string()]);

    log.emit(Event::warn("a").with("detail", json!({ "list": [secret, 2], "plain": format!("x{secret}y") })));

    let line = out.text();
    assert!(!line.contains("pa\\\"ss"), "{line}");
    let event: Value = serde_json::from_str(&line).expect("one JSON line");
    assert_eq!(event["detail"], json!({ "list": ["[REDACTED]", 2], "plain": "x[REDACTED]y" }));
}

#[test]
fn given_no_secrets_when_events_are_emitted_then_a_line_is_the_same_bytes_as_without_scrubbing() {
    let plain = Captured::default();
    let scrubbing = Captured::default();
    let event = || Event::info("a").with("err", "docker API returned 500: a@b.example 4111111111111111");

    StdoutEventLog::to(plain.clone()).emit(event());
    StdoutEventLog::to(scrubbing.clone()).scrubbing([AUTH.to_string()]).emit(event());

    assert_eq!(plain.text(), "{\"level\":\"info\",\"message\":\"a\",\"err\":\"docker API returned 500: a@b.example 4111111111111111\"}\n");
    assert_eq!(scrubbing.text(), plain.text());
}

#[derive(Clone, Copy, PartialEq)]
enum Answer {
    Accept,
    Refuse(u16),
    Hang,
}

struct Request {
    path: String,
    authorization: Option<String>,
    body: Value,
}

#[derive(Clone)]
struct Shared {
    requests: Arc<Mutex<Vec<Request>>>,
    answer: Arc<Mutex<Answer>>,
    stop: Arc<AtomicBool>,
}

impl Shared {
    fn answer(&self) -> Answer {
        *self.answer.lock().expect("the answer")
    }
}

/// A log ingest on a loopback port that keeps what it is sent. Dropping it closes every connection it holds.
struct Ingest {
    addr: SocketAddr,
    shared: Shared,
}

impl Ingest {
    fn start(answer: Answer) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a free loopback port");
        listener.set_nonblocking(true).expect("a non-blocking listener");
        let addr = listener.local_addr().expect("a bound address");
        let shared = Shared { requests: Arc::default(), answer: Arc::new(Mutex::new(answer)), stop: Arc::default() };
        let accepting = shared.clone();
        std::thread::spawn(move || {
            while !accepting.stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let shared = accepting.clone();
                        std::thread::spawn(move || serve(stream, shared));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        Self { addr, shared }
    }

    fn destination(&self) -> Destination {
        Destination::new(format!("http://{}", self.addr), "ingest-token")
    }

    fn answers(&self, answer: Answer) {
        *self.shared.answer.lock().expect("the answer") = answer;
    }

    fn requests(&self) -> usize {
        self.shared.requests.lock().expect("the requests").len()
    }

    /// Every log record received, in the order it arrived.
    fn records(&self) -> Vec<Value> {
        let requests = self.shared.requests.lock().expect("the requests");
        let scopes = requests.iter().flat_map(|request| request.body["resourceLogs"].as_array().into_iter().flatten());
        scopes.flat_map(|resource| resource["scopeLogs"].as_array().into_iter().flatten()).flat_map(|scope| scope["logRecords"].as_array().into_iter().flatten()).cloned().collect()
    }

    fn body_text(&self) -> String {
        self.shared.requests.lock().expect("the requests").iter().map(|request| request.body.to_string()).collect()
    }

    fn first_request(&self) -> (String, Option<String>, Value) {
        let requests = self.shared.requests.lock().expect("the requests");
        let first = requests.first().expect("a request");
        (first.path.clone(), first.authorization.clone(), first.body.clone())
    }
}

impl Drop for Ingest {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }
}

fn read_request(stream: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let mut data = Vec::new();
    let mut chunk = [0u8; 4096];
    let end = loop {
        if let Some(at) = data.windows(4).position(|window| window == b"\r\n\r\n") {
            break at + 4;
        }
        let read = stream.read(&mut chunk).ok().filter(|read| *read > 0)?;
        data.extend_from_slice(&chunk[..read]);
    };
    let head = String::from_utf8_lossy(&data[..end]).to_string();
    let length = head
        .lines()
        .find_map(|line| line.split_once(':').filter(|(name, _)| name.eq_ignore_ascii_case("content-length")).and_then(|(_, value)| value.trim().parse::<usize>().ok()))
        .unwrap_or(0);
    while data.len() < end + length {
        let read = stream.read(&mut chunk).ok().filter(|read| *read > 0)?;
        data.extend_from_slice(&chunk[..read]);
    }
    Some((head, data[end..end + length].to_vec()))
}

fn serve(mut stream: TcpStream, shared: Shared) {
    stream.set_nonblocking(false).expect("a blocking stream");
    stream.set_read_timeout(Some(Duration::from_secs(10))).expect("a read timeout");
    let Some((head, body)) = read_request(&mut stream) else { return };
    let path = head.lines().next().and_then(|line| line.split(' ').nth(1)).unwrap_or_default().to_string();
    let authorization = head
        .lines()
        .find_map(|line| line.split_once(':').filter(|(name, _)| name.eq_ignore_ascii_case("authorization")).map(|(_, value)| value.trim().to_string()));
    shared.requests.lock().expect("the requests").push(Request { path, authorization, body: serde_json::from_slice(&body).unwrap_or(Value::Null) });
    match shared.answer() {
        Answer::Accept => reply(&mut stream, 200),
        Answer::Refuse(status) => reply(&mut stream, status),
        // Closed unanswered once the ingest is dropped or told to answer.
        Answer::Hang => {
            while !shared.stop.load(Ordering::Relaxed) && shared.answer() == Answer::Hang {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

fn reply(stream: &mut TcpStream, status: u16) {
    let _ = write!(stream, "HTTP/1.1 {status} Answer\r\ncontent-type: application/json\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{{}}");
}

fn source() -> Source {
    Source { service: "vm-ops-api".to_string(), version: "0.1.0", commit: Some("abc1234".to_string()), host: Some("vm-main".to_string()) }
}

fn export_to(destination: Destination, host_label: &str, problems: &Arc<FakeEventLog>) -> OtlpEventLog {
    OtlpEventLog::new(source(), destination, host_label, vec![AUTH.to_string(), GITHUB.to_string()], problems.clone())
}

/// The sink whose failures are told to a stdout log, as the composition root wires it.
fn export_telling(stdout: &Arc<StdoutEventLog<Captured>>, ingest: &Ingest) -> OtlpEventLog {
    OtlpEventLog::new(source(), ingest.destination(), "main", vec![AUTH.to_string()], stdout.clone())
}

fn attributes(record: &Value) -> Vec<(String, String)> {
    let text = |value: &Value| value.as_str().unwrap_or("<not a string value>").to_string();
    record["attributes"].as_array().into_iter().flatten().map(|attribute| (text(&attribute["key"]), text(&attribute["value"]["stringValue"]))).collect()
}

fn pair(key: &str, value: &str) -> (String, String) {
    (key.to_string(), value.to_string())
}

fn body_of(record: &Value) -> &str {
    record["body"]["stringValue"].as_str().unwrap_or("<no body>")
}

fn problem_events(problems: &FakeEventLog) -> Vec<Event> {
    problems.events().into_iter().filter(|event| event.message == "event_log.export_failed").collect()
}

fn attribute_of<'a>(event: &'a Event, key: &str) -> Option<&'a Value> {
    event.attributes.iter().find(|(name, _)| *name == key).map(|(_, value)| value)
}

fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + FLUSHED_WITHIN;
    while !ready() {
        assert!(Instant::now() < deadline, "still waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn given_events_of_each_level_when_exported_then_the_ingest_gets_what_wf_log_emit_sent_for_each() {
    // Given an ingest and the log sink pointed at it, as host main
    let ingest = Ingest::start(Answer::Accept);
    let problems = Arc::new(FakeEventLog::default());
    let log = export_to(ingest.destination(), "main", &problems);
    // When the three levels are emitted and the export is flushed
    log.emit(Event::info("vm-ops-api.listening").with("port", 8080u16).with("host", "main"));
    log.emit(Event::warn("self_build.pressure_unavailable").with("ref", "dev"));
    log.emit(Event::error("http.handler_failed").with("method", "GET").with("err", "boom"));
    assert!(log.flush(FLUSHED_WITHIN));
    // Then each record has its level, its message as the body, and source, hostLabel and its attributes as text, in order
    let records = ingest.records();
    let levels: Vec<(&Value, &Value, &str)> = records.iter().map(|r| (&r["severityText"], &r["severityNumber"], body_of(r))).collect();
    let want = [(json!("INFO"), json!(9), "vm-ops-api.listening"), (json!("WARN"), json!(13), "self_build.pressure_unavailable"), (json!("ERROR"), json!(17), "http.handler_failed")];
    assert_eq!(levels, want.iter().map(|(text, number, body)| (text, number, *body)).collect::<Vec<_>>());
    assert_eq!(attributes(&records[0]), [pair("source", "vm-ops-api"), pair("hostLabel", "main"), pair("port", "8080"), pair("host", "main")]);
    assert_eq!(attributes(&records[1]), [pair("source", "vm-ops-api"), pair("hostLabel", "main"), pair("ref", "dev")]);
    assert_eq!(attributes(&records[2]), [pair("source", "vm-ops-api"), pair("hostLabel", "main"), pair("method", "GET"), pair("err", "boom")]);
    assert_eq!(problem_events(&problems), Vec::<Event>::new());
}

#[test]
fn given_an_ingest_when_an_event_is_exported_then_it_is_posted_to_the_logs_path_with_the_bearer_token_and_the_source_as_resource() {
    let ingest = Ingest::start(Answer::Accept);
    let log = export_to(ingest.destination(), "main", &Arc::new(FakeEventLog::default()));

    log.emit(Event::info("health.heartbeat").with("ok", true));
    assert!(log.flush(FLUSHED_WITHIN));

    let (path, authorization, body) = ingest.first_request();
    assert_eq!((path.as_str(), authorization.as_deref()), ("/v1/logs", Some("Bearer ingest-token")));
    let resource: Vec<(String, String)> = body["resourceLogs"][0]["resource"]["attributes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|a| (a["key"].as_str().unwrap_or_default().to_string(), a["value"]["stringValue"].as_str().unwrap_or_default().to_string()))
        .collect();
    for want in [pair("service.name", "vm-ops-api"), pair("service.version", "0.1.0"), pair("vcs.ref.head.revision", "abc1234"), pair("host.name", "vm-main")] {
        assert!(resource.contains(&want), "{want:?} is not among {resource:?}");
    }
}

#[test]
fn given_attributes_of_every_json_kind_when_exported_then_each_is_the_text_of_string_of_the_value() {
    let ingest = Ingest::start(Answer::Accept);
    let log = export_to(ingest.destination(), "main", &Arc::new(FakeEventLog::default()));

    log.emit(
        Event::info("self_build.waiting_for_headroom")
            .with("ok", true)
            .with("avg10", 12.5)
            .with("whole", 5.0)
            .with("delayMs", 1000u64)
            .with("nothing", Value::Null)
            .with("list", json!([1, null, "b"]))
            .with("object", json!({ "a": 1 })),
    );
    assert!(log.flush(FLUSHED_WITHIN));

    let want = [
        pair("source", "vm-ops-api"),
        pair("hostLabel", "main"),
        pair("ok", "true"),
        pair("avg10", "12.5"),
        pair("whole", "5"),
        pair("delayMs", "1000"),
        pair("nothing", "null"),
        pair("list", "1,,b"),
        pair("object", "[object Object]"),
    ];
    assert_eq!(attributes(&ingest.records()[0]), want);
}

#[test]
fn given_no_host_label_when_exported_then_the_record_has_no_host_label_attribute() {
    let ingest = Ingest::start(Answer::Accept);
    let unlabelled = export_to(ingest.destination(), "", &Arc::new(FakeEventLog::default()));

    unlabelled.emit(Event::info("a").with("k", "v"));
    assert!(unlabelled.flush(FLUSHED_WITHIN));

    assert_eq!(attributes(&ingest.records()[0]), [pair("source", "vm-ops-api"), pair("k", "v")]);
}

#[test]
fn given_attributes_named_like_the_leading_ones_or_repeated_when_exported_then_the_first_place_takes_the_last_value() {
    let ingest = Ingest::start(Answer::Accept);
    let log = export_to(ingest.destination(), "main", &Arc::new(FakeEventLog::default()));

    log.emit(Event::info("a").with("source", "other").with("k", "v").with("hostLabel", "h2").with("k", "w"));
    assert!(log.flush(FLUSHED_WITHIN));

    assert_eq!(attributes(&ingest.records()[0]), [pair("source", "other"), pair("hostLabel", "h2"), pair("k", "w")]);
}

#[test]
fn given_secrets_and_credential_like_fields_when_exported_then_none_reaches_the_ingest() {
    // Given the sink told of the auth token and the GitHub token
    let ingest = Ingest::start(Answer::Accept);
    let log = export_to(ingest.destination(), "main", &Arc::new(FakeEventLog::default()));
    // When an event carries them in text, under a credential-like name, and with an email address
    log.emit(
        Event::error("self_redeploy.failed")
            .with("err", format!("pull failed for {GITHUB} then {AUTH}"))
            .with("token", "abcdefghijkl")
            .with("contact", "ops@example.com")
            .with("run", 12u64),
    );
    assert!(log.flush(FLUSHED_WITHIN));
    // Then each is replaced as `wf log emit` replaced it, and no request holds a secret
    let record = &ingest.records()[0];
    let want = [
        pair("source", "vm-ops-api"),
        pair("hostLabel", "main"),
        pair("err", "pull failed for [REDACTED] then [REDACTED]"),
        pair("token", "[REDACTED]"),
        pair("contact", "[REDACTED]"),
        pair("run", "12"),
    ];
    assert_eq!(attributes(record), want);
    let everything = ingest.body_text();
    for secret in [GITHUB, AUTH, "abcdefghijkl", "ops@example.com"] {
        assert!(!everything.contains(secret), "{secret} reached the ingest: {everything}");
    }
}

#[test]
fn given_a_flush_with_nothing_queued_when_it_is_asked_then_it_answers_at_once_and_sends_nothing() {
    let ingest = Ingest::start(Answer::Accept);
    let log = export_to(ingest.destination(), "main", &Arc::new(FakeEventLog::default()));

    let started = Instant::now();
    assert!(log.flush(FLUSHED_WITHIN));

    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(ingest.requests(), 0);
}

#[test]
fn given_many_events_when_emitted_in_a_burst_then_they_arrive_in_order_in_a_few_batches() {
    let ingest = Ingest::start(Answer::Accept);
    let log = export_to(ingest.destination(), "main", &Arc::new(FakeEventLog::default()));

    (0..50u64).for_each(|n| log.emit(Event::info("beat").with("n", n)));
    assert!(log.flush(FLUSHED_WITHIN));

    let numbers: Vec<String> = ingest.records().iter().map(|record| attributes(record)[2].1.clone()).collect();
    assert_eq!(numbers, (0..50).map(|n| n.to_string()).collect::<Vec<_>>());
    assert!(ingest.requests() < 10, "{} requests for 50 events", ingest.requests());
}

#[test]
fn given_an_ingest_that_refuses_when_events_are_emitted_then_the_caller_is_unharmed_the_failure_is_told_and_the_next_event_still_goes() {
    // Given an ingest that answers 500
    let ingest = Ingest::start(Answer::Refuse(500));
    let problems = Arc::new(FakeEventLog::default());
    let log = export_to(ingest.destination(), "main", &problems);
    // When an event is emitted
    log.emit(Event::info("a"));
    assert!(log.flush(FLUSHED_WITHIN));
    // Then the failure is an event of its own, with the status and the number of events it cost
    let told = problem_events(&problems);
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(told[0].level, Level::Warn);
    assert!(attribute_of(&told[0], "reason").and_then(Value::as_str).is_some_and(|reason| reason.contains("500")), "{:?}", told[0]);
    assert_eq!(attribute_of(&told[0], "events"), Some(&json!(1)));
    // And when the ingest recovers, the next event is delivered by the same sink
    ingest.answers(Answer::Accept);
    log.emit(Event::info("b"));
    assert!(log.flush(FLUSHED_WITHIN));
    let bodies: Vec<String> = ingest.records().iter().map(|record| body_of(record).to_string()).collect();
    assert_eq!(bodies, ["a", "b"]);
    assert_eq!(problem_events(&problems).len(), 1);
}

#[test]
fn given_an_ingest_nobody_listens_on_when_an_event_is_emitted_then_it_returns_at_once_and_the_failure_is_told() {
    let closed = TcpListener::bind("127.0.0.1:0").expect("a free loopback port");
    let destination = Destination::new(format!("http://{}", closed.local_addr().expect("a bound address")), "ingest-token");
    drop(closed);
    let problems = Arc::new(FakeEventLog::default());
    let log = export_to(destination, "main", &problems);

    let started = Instant::now();
    log.emit(Event::info("a"));
    let emitted_in = started.elapsed();
    assert!(log.flush(FLUSHED_WITHIN));

    assert!(emitted_in < Duration::from_millis(500), "emit took {emitted_in:?}");
    let told = problem_events(&problems);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(attribute_of(&told[0], "reason").and_then(Value::as_str).is_some_and(|reason| !reason.is_empty()), "{:?}", told[0]);
}

#[test]
fn given_an_ingest_that_never_answers_when_events_are_emitted_then_no_emit_waits_and_a_flush_gives_up_when_asked() {
    // Given an ingest that takes the request and says nothing
    let ingest = Ingest::start(Answer::Hang);
    let problems = Arc::new(FakeEventLog::default());
    let log = export_to(ingest.destination(), "main", &problems);
    // When events are emitted
    let started = Instant::now();
    (0..3).for_each(|_| log.emit(Event::info("a")));
    let emitted_in = started.elapsed();
    // Then no emit waited for it, and a flush that is asked to wait 300 ms says it did not finish
    assert!(emitted_in < Duration::from_millis(500), "three emits took {emitted_in:?}");
    assert!(!log.flush(Duration::from_millis(300)));
    // And once the ingest lets go, the failure is told and the log is whole again
    drop(ingest);
    wait_for("the failed export to be told", || !problem_events(&problems).is_empty());
}

#[test]
fn given_an_ingest_that_never_answers_when_far_more_events_than_the_queue_holds_are_emitted_then_the_overflow_is_lost_counted_and_told() {
    // Given an ingest that is stuck on its first request
    let ingest = Ingest::start(Answer::Hang);
    let problems = Arc::new(FakeEventLog::default());
    let log = export_to(ingest.destination(), "main", &problems);
    log.emit(Event::info("first"));
    wait_for("the first request to be taken", || ingest.requests() == 1);
    // When three times the queue is emitted
    let total = QUEUE_CAPACITY * 3;
    let log = Arc::new(log);
    let (emitted, done) = std::sync::mpsc::channel();
    let emitter = log.clone();
    std::thread::spawn(move || {
        (0..total).for_each(|n| emitter.emit(Event::info("beat").with("n", n as u64)));
        let _ = emitted.send(());
    });
    // Then emitting finishes at once, with the ingest still stuck
    assert!(done.recv_timeout(Duration::from_secs(5)).is_ok(), "{total} emits were still waiting after 5 s");
    // And once the ingest answers again, what the queue held is delivered and the rest is told as lost
    ingest.answers(Answer::Accept);
    assert!(log.flush(FLUSHED_WITHIN));
    let delivered = ingest.records().iter().filter(|record| body_of(record) == "beat").count();
    let lost: u64 = problem_events(&problems)
        .iter()
        .filter(|event| attribute_of(event, "reason") == Some(&json!("queue full")))
        .filter_map(|event| attribute_of(event, "events").and_then(Value::as_u64))
        .sum();
    assert_eq!(delivered, QUEUE_CAPACITY);
    assert_eq!(delivered as u64 + lost, total as u64);
}

#[test]
fn given_two_sinks_when_events_are_emitted_through_a_tee_then_each_gets_every_event_in_order() {
    let first = Arc::new(FakeEventLog::default());
    let second = Arc::new(FakeEventLog::default());
    let tee = TeeEventLog::new(vec![first.clone(), second.clone()]);

    tee.emit(Event::info("a").with("n", 1u64));
    tee.emit(Event::error("b"));

    let want = vec![Event::info("a").with("n", 1u64), Event::error("b")];
    assert_eq!((first.events(), second.events()), (want.clone(), want));
}

#[test]
fn given_stdout_and_a_stuck_ingest_when_an_event_is_emitted_through_a_tee_then_the_line_is_written_before_emit_returns() {
    // Given the stdout line and an export that is stuck
    let ingest = Ingest::start(Answer::Hang);
    let out = Captured::default();
    let stdout = Arc::new(StdoutEventLog::to(out.clone()));
    let export = export_telling(&stdout, &ingest);
    let tee = TeeEventLog::new(vec![stdout.clone(), Arc::new(export)]);
    // When an event is emitted
    tee.emit(Event::info("vm-ops-api.listening").with("port", 8080u16).with("host", "main"));
    // Then its line is on stdout already, once, and Node's bytes
    assert_eq!(out.text(), "{\"level\":\"info\",\"message\":\"vm-ops-api.listening\",\"port\":8080,\"host\":\"main\"}\n");
}

#[test]
fn given_an_export_that_fails_when_it_is_told_to_the_stdout_log_then_it_is_a_warn_line_in_nodes_format_with_no_secret_in_it() {
    // Given an ingest that refuses and a stdout log that scrubs the auth token, told of the failures
    let ingest = Ingest::start(Answer::Refuse(401));
    let out = Captured::default();
    let stdout = Arc::new(StdoutEventLog::to(out.clone()).scrubbing([AUTH.to_string()]));
    let log = export_telling(&stdout, &ingest);
    // When an event is emitted and the export flushed
    log.emit(Event::info("a"));
    assert!(log.flush(FLUSHED_WITHIN));
    // Then stdout has the failure as one JSON line with the level, the event name, the reason and the count
    let text = out.text();
    let line: Value = serde_json::from_str(text.trim_end()).expect("one JSON line");
    assert_eq!((&line["level"], &line["message"], &line["events"]), (&json!("warn"), &json!("event_log.export_failed"), &json!(1)));
    assert!(line["reason"].as_str().is_some_and(|reason| reason.contains("401")), "{line}");
    assert!(!text.contains(AUTH));
}
