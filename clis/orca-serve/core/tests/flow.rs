//! The core with a test area: the envelope, the table assembly and the life of a stream, with no transport and no rca.
use orca_core::testing::{RevocablePrincipals, ScriptedAgent};
use orca_core::wire::{self, Failure};
use orca_core::area::Out;
use orca_core::{Area, Ctx, Host, HostInfo, Kind, Method, NotOffered, ServiceStatus, Subscription};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

static ENDED: AtomicUsize = AtomicUsize::new(0);

struct Ticks {
    id: String,
    sent: u32,
}

impl Subscription for Ticks {
    fn id(&self) -> &str {
        &self.id
    }
    fn poll(&mut self) -> Vec<Value> {
        self.sent += 1;
        vec![json!({"tick": self.sent})]
    }
    fn end(&mut self) -> Vec<Value> {
        ENDED.fetch_add(1, Ordering::SeqCst);
        vec![json!({"type": "end"})]
    }
}

struct Wire {
    id: String,
    number: u32,
    got: Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    opened: bool,
}

impl Subscription for Wire {
    fn id(&self) -> &str {
        &self.id
    }
    fn poll(&mut self) -> Vec<Value> {
        Vec::new()
    }
    fn end(&mut self) -> Vec<Value> {
        Vec::new()
    }
    fn poll_out(&mut self) -> Vec<Out> {
        if std::mem::replace(&mut self.opened, true) {
            return Vec::new();
        }
        vec![Out::Text(json!({"type": "subscribed"}).to_string()), Out::Binary(vec![0x74, 1, 0, 0])]
    }
    fn number(&self) -> Option<u32> {
        Some(self.number)
    }
    fn on_binary(&mut self, frame: &[u8]) -> Vec<Out> {
        self.got.lock().unwrap().push(frame.to_vec());
        Vec::new()
    }
}

struct Binary(Arc<std::sync::Mutex<Vec<Vec<u8>>>>, Arc<AtomicUsize>);

impl Area for Binary {
    fn name(&self) -> &'static str {
        "binary"
    }
    fn methods(&self) -> &'static [Method] {
        &[Method { name: "wire", kind: Kind::Streaming }]
    }
    fn call(&self, _ctx: &mut Ctx, _method: &str, _params: &Value) -> Result<Value, Failure> {
        Err(Failure::internal("boom"))
    }
    fn open(&self, ctx: &mut Ctx, _method: &str, _params: &Value) -> Result<Box<dyn Subscription>, Failure> {
        let number = ctx.stream_number();
        self.1.store(number as usize, Ordering::SeqCst);
        Ok(Box::new(Wire { id: ctx.subscription_id("wire-"), number, got: self.0.clone(), opened: false }))
    }
}

struct Probe(&'static [Method], &'static [NotOffered]);

impl Area for Probe {
    fn name(&self) -> &'static str {
        "probe"
    }
    fn methods(&self) -> &'static [Method] {
        self.0
    }
    fn not_offered(&self) -> &'static [NotOffered] {
        self.1
    }
    fn call(&self, ctx: &mut Ctx, method: &str, params: &Value) -> Result<Value, Failure> {
        match method {
            "echo" => Ok(params.clone()),
            "stop" => Ok(json!({"cancelled": ctx.subscriptions.cancel(params["id"].as_str().unwrap_or_default())})),
            _ => Err(Failure::internal("boom")),
        }
    }
    fn open(&self, ctx: &mut Ctx, _method: &str, _params: &Value) -> Result<Box<dyn Subscription>, Failure> {
        Ok(Box::new(Ticks { id: ctx.subscription_id("ticks-"), sent: 0 }))
    }
}

const METHODS: &[Method] = &[Method { name: "echo", kind: Kind::Unary }, Method { name: "stop", kind: Kind::Unary }, Method { name: "ticks", kind: Kind::Streaming }];
const NO: &[NotOffered] = &[NotOffered { name: "never", reason: "owner said so", decided_by: "M0" }];

fn host(areas: Vec<Box<dyn Area>>) -> Result<Arc<Host>, String> {
    let agent = Arc::new(ScriptedAgent::serving(ServiceStatus { protocol: 1, version: "v".into(), commit: "c".into(), groups: vec![] }));
    Host::new("rt".into(), areas, agent, Arc::new(RevocablePrincipals::default()), HostInfo { platform: "linux".into(), machine_name: "m".into(), app_version: "0".into() })
}

fn frames(out: Vec<String>) -> Vec<Value> {
    out.iter().map(|f| serde_json::from_str(f).unwrap()).collect()
}

#[test]
fn given_requests_when_dispatched_then_answers_carry_the_id_the_runtime_id_and_the_result_or_the_error() {
    // Given a host with one area and a connection
    let host = host(vec![Box::new(Probe(METHODS, NO))]).unwrap();
    let mut conn = host.connection("dev", "tok", vec![]);
    // When a served, a failing, a not-offered and an unknown method are called
    let echo = frames(conn.on_message(r#"{"id":"a","method":"echo","params":{"x":1}}"#));
    let not_offered = frames(conn.on_message(r#"{"id":"b","method":"never"}"#));
    let unknown = frames(conn.on_message(r#"{"id":"c","method":"nope"}"#));
    // Then each is one envelope addressed to its request
    assert_eq!(echo, vec![json!({"id": "a", "ok": true, "result": {"x": 1}, "_meta": {"runtimeId": "rt"}})]);
    assert_eq!((not_offered[0]["error"]["code"].clone(), not_offered[0]["error"]["data"].clone()), (json!("not_offered"), json!({"decidedBy": "M0"})));
    assert_eq!(unknown[0]["error"]["code"], wire::METHOD_NOT_FOUND);
}

#[test]
fn given_a_stream_when_opened_cancelled_and_the_connection_closed_then_each_gets_its_closing_event_once() {
    // Given two open streams on a connection
    let host = host(vec![Box::new(Probe(METHODS, NO))]).unwrap();
    let mut conn = host.connection("dev", "tok", vec![]);
    let first = frames(conn.on_message(r#"{"id":"s1","method":"ticks"}"#));
    conn.on_message(r#"{"id":"s2","method":"ticks"}"#);
    let id = format!("ticks-conn1-{}", 1);
    // Then the open answered with its first event, streaming, addressed to the stream's request
    assert_eq!((first[0]["id"].clone(), first[0]["streaming"].clone(), first[0]["result"].clone()), (json!("s1"), json!(true), json!({"tick": 1})));
    assert_eq!(conn.open_streams(), 2);
    // When the first is cancelled by its id
    let cancelled = frames(conn.on_message(&json!({"id": "x", "method": "stop", "params": {"id": id}}).to_string()));
    // Then the closing event is sent to the stream's request, in the same flush, and it is gone
    assert!(cancelled.iter().any(|f| f["id"] == "s1" && f["result"] == json!({"type": "end"})));
    assert_eq!(conn.open_streams(), 1);
    // When the connection closes
    let before = ENDED.load(Ordering::SeqCst);
    conn.close();
    // Then the remaining stream ended exactly once
    assert_eq!((conn.open_streams(), ENDED.load(Ordering::SeqCst) - before), (0, 1));
}

#[test]
fn given_areas_that_claim_one_name_when_assembled_then_it_is_refused_and_the_table_lists_each_once() {
    // Given two areas that both serve "echo", and a served name that another marks not offered
    let doubled = host(vec![Box::new(Probe(METHODS, NO)), Box::new(Probe(METHODS, &[]))]);
    let clash = host(vec![Box::new(Probe(METHODS, &[NotOffered { name: "echo", reason: "r", decided_by: "M0" }]))]);
    // Then neither assembles
    assert!(doubled.err().unwrap().contains("echo is declared twice"));
    assert!(clash.err().unwrap().contains("echo is declared twice"));
}

#[test]
fn given_frames_that_are_not_requests_when_parsed_then_the_failure_is_addressed_to_what_could_be_read() {
    // Given broken frames, with and without an id
    let cases = [("not json", "unknown", "Invalid JSON request"), (r#"[]"#, "unknown", "Missing request id"), (r#"{"id":"","method":"x"}"#, "unknown", "Missing request id"), (r#"{"id":"q","method":""}"#, "q", "Missing RPC method")];
    for (text, id, message) in cases {
        // When parsed, then the failure names the id it can be addressed to and the reference's message
        let (to, failure) = *wire::parse(text).unwrap_err();
        assert_eq!((to.as_str(), failure.code.as_str(), failure.message.as_str()), (id, wire::BAD_REQUEST, message), "{text}");
    }
}

#[test]
fn given_a_stream_that_emits_json_then_binary_when_opened_then_both_arrive_in_order_and_inbound_binary_is_routed_by_stream_id() {
    // Given a stream that emits a JSON frame then a binary frame
    let got = Arc::new(std::sync::Mutex::new(Vec::new()));
    let number = Arc::new(AtomicUsize::new(0));
    let host = host(vec![Box::new(Binary(got.clone(), number.clone()))]).unwrap();
    let mut conn = host.connection("dev", "tok", vec![]);
    // When it is opened
    let out = conn.on_message_out(r#"{"id":"s","method":"wire"}"#);
    // Then the JSON envelope comes first and the binary frame second
    assert_eq!(out.len(), 2);
    let Out::Text(first) = &out[0] else { panic!("text first") };
    assert_eq!(serde_json::from_str::<Value>(first).unwrap()["result"], json!({"type": "subscribed"}));
    assert_eq!(out[1], Out::Binary(vec![0x74, 1, 0, 0]));
    // When a binary frame with the stream's id and one with another id arrive
    let n = number.load(Ordering::SeqCst) as u32;
    let header = |id: u32| [vec![0x74, 1, 9, 0], id.to_le_bytes().to_vec(), vec![7]].concat();
    conn.on_binary(&header(n));
    conn.on_binary(&header(n + 100));
    // Then only the first reaches the stream
    assert_eq!(got.lock().unwrap().clone(), vec![header(n)]);
}
