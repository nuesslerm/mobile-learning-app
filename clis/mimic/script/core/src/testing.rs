//! In-memory doubles of the ports. They share one `FakeClock` handle so a frame can be scheduled to arrive some milliseconds after a send,
//! and a test can read what the interpreter sent and how long it slept. Clones of a double are handles on the same state.
use crate::vars::js_string;
use crate::ports::{Clock, Event, HttpCall, HttpReply, Phone, PhoneEvent, Target, Transport, TransportError};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use mimic_core::cassette::{Dir, Step, StepKind};
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

#[derive(Clone, Default)]
pub struct FakeClock {
    now: Rc<Cell<u64>>,
    sleeps: Rc<Cell<u64>>,
}

impl FakeClock {
    /// How many times the interpreter slept.
    pub fn sleeps(&self) -> u64 {
        self.sleeps.get()
    }
}

impl Clock for FakeClock {
    fn now_ms(&self) -> u64 {
        self.now.get()
    }

    fn sleep(&mut self, ms: u64) {
        self.now.set(self.now.get() + ms);
        self.sleeps.set(self.sleeps.get() + 1);
    }
}

pub struct FixedTarget {
    pub workspace: String,
    pub credential: String,
    pub mints: Rc<Cell<u32>>,
    /// A test bumps it to model the server restarting.
    pub generation: Rc<Cell<u64>>,
}

impl FixedTarget {
    pub fn new(workspace: &str, credential: &str) -> FixedTarget {
        FixedTarget { workspace: workspace.into(), credential: credential.into(), mints: Rc::default(), generation: Rc::default() }
    }
}

impl Target for FixedTarget {
    fn workspace(&self) -> String {
        self.workspace.clone()
    }

    fn mint(&mut self) -> Result<String, String> {
        self.mints.set(self.mints.get() + 1);
        Ok(self.credential.clone())
    }

    fn generation(&self) -> u64 {
        self.generation.get()
    }

    fn make_git(&mut self, name: &str) -> Result<String, String> {
        Ok(format!("{SCRATCH}/{name}"))
    }

    fn masks(&self) -> Vec<(String, String)> {
        vec![(SCRATCH.into(), "<scratch>".into())]
    }
}

/// Where `FixedTarget` makes repositories: one throwaway folder, masked as `<scratch>` like the real target's.
pub const SCRATCH: &str = "/tmp/mimic-scratch-Ab12Cd";

/// A phone that answers from recorded steps: the s2c payloads in order, a `client_error` payload as the client's own failure.
pub struct CassettePhone {
    answers: VecDeque<Value>,
    pub requests: Rc<RefCell<Vec<(String, Value)>>>,
    state: String,
    streams: VecDeque<(String, Vec<Step>)>,
    handshake: Vec<PhoneEvent>,
    close_event: Option<PhoneEvent>,
    due: VecDeque<PhoneEvent>,
    tapped: (bool, bool),
    opened: usize,
}

fn client_error(payload: &Value) -> Option<&str> {
    payload.get("client_error").and_then(Value::as_str)
}

impl CassettePhone {
    pub fn new(steps: &[Step]) -> CassettePhone {
        // an unbound variable is answered by the interpreter itself, never asked of the host
        let answers = steps.iter().filter(|s| s.dir == Dir::S2c).map(|s| s.payload.clone()).filter(|p| !client_error(p).is_some_and(|m| m.starts_with("unbound variable"))).collect();
        CassettePhone { answers, requests: Rc::default(), state: "connected".into(), streams: VecDeque::new(), handshake: Vec::new(), close_event: None, due: VecDeque::new(), tapped: (false, false), opened: 0 }
    }

    /// A phone that plays a recording of the script `input`: the frames of each `subscribe` are delivered after it, the handshake the client tapped
    /// at `connect` and the end of the connection after `close`, and the plain requests are answered in order from what is left.
    pub fn replaying(steps: &[Step], input: &Value) -> CassettePhone {
        let subscribes: Vec<(String, String)> = input["script"].as_array().into_iter().flatten().filter_map(|e| e.get("subscribe")).map(|a| (js_string(&a["rid"]), js_string(&a["method"]))).collect();
        let is_stream = |s: &Step| s.rid.as_ref().is_some_and(|r| subscribes.iter().any(|(rid, _)| rid == r));
        let channel = |s: &Step, name: &str| s.channel.as_deref() == Some(name);
        let mut phone = CassettePhone::new(&[]);
        phone.answers = steps
            .iter()
            .filter(|s| s.dir == Dir::S2c && !is_stream(s) && !channel(s, "handshake") && !channel(s, "ws:close"))
            .map(|s| s.payload.clone())
            .filter(|p| !client_error(p).is_some_and(|m| m.starts_with("unbound variable")))
            .collect();
        phone.streams = subscribes.iter().map(|(rid, method)| (method.clone(), steps.iter().filter(|s| s.dir == Dir::S2c && s.rid.as_deref() == Some(rid.as_str())).cloned().collect())).collect();
        phone.handshake = steps
            .iter()
            .filter(|s| channel(s, "handshake"))
            .map(|s| PhoneEvent::Handshake { stage: js_string(&s.payload["stage"]), client_to_server: s.dir == Dir::C2s, payload: s.payload["frame"].clone() })
            .collect();
        phone.close_event = steps.iter().find(|s| channel(s, "ws:close")).map(|s| PhoneEvent::Close { code: s.payload["code"].as_u64().map(|c| c as u16), reason: js_string(&s.payload["reason"]) });
        phone
    }

    /// The state `connect` ends in, for a host that refuses the phone.
    pub fn refusing(mut self, state: &str) -> CassettePhone {
        self.state = state.into();
        self
    }
}

impl Phone for CassettePhone {
    fn connect(&mut self, _timeout_ms: u64) -> String {
        if self.state == "connected" && self.tapped.0 {
            self.due.extend(self.handshake.drain(..));
        }
        self.state.clone()
    }

    fn request(&mut self, method: &str, params: &Value, _timeout_ms: u64) -> Result<Value, String> {
        self.requests.borrow_mut().push((method.into(), params.clone()));
        match self.answers.pop_front() {
            Some(answer) => match client_error(&answer) {
                Some(message) => Err(message.into()),
                None => Ok(answer),
            },
            None => Err("the cassette has no further answer".into()),
        }
    }

    fn subscribe(&mut self, method: &str, params: &Value) -> Result<String, String> {
        self.requests.borrow_mut().push((method.into(), params.clone()));
        let at = self.streams.iter().position(|(m, _)| m == method).ok_or("the cassette has no stream of this method")?;
        let (_, frames) = self.streams.remove(at).unwrap_or_default();
        if let Some(message) = frames.first().and_then(|f| client_error(&f.payload)) {
            return Err(message.into());
        }
        self.opened += 1;
        let sub = format!("sub-{}", self.opened);
        for f in frames {
            self.due.push_back(PhoneEvent::Frame { sub: sub.clone(), binary: f.kind == StepKind::Binary, payload: f.payload, len: f.len.map(|n| n as usize) });
        }
        Ok(sub)
    }

    fn tap(&mut self, handshake: bool, close: bool) {
        self.tapped = (handshake, close);
    }

    fn poll(&mut self) -> Vec<PhoneEvent> {
        self.due.drain(..).collect()
    }

    fn close(&mut self) {
        if let (true, Some(event)) = (self.tapped.1, self.close_event.take()) {
            self.due.push_back(event);
        }
    }
}

#[derive(Default)]
struct Scripted {
    http: VecDeque<Result<HttpReply, TransportError>>,
    open: VecDeque<(u16, Vec<(u64, Event)>)>,
    on_send: VecDeque<Vec<(u64, Event)>>,
    on_close: Vec<(u64, Event)>,
    due: Vec<(u64, Event)>,
    has_socket: bool,
    calls: Vec<HttpCall>,
    sent: Vec<String>,
    closed: Vec<(u16, String)>,
}

/// Answers by a script: HTTP replies in order, and per send the frames that follow it, each after its own delay on the shared clock.
#[derive(Clone)]
pub struct ScriptedTransport {
    clock: FakeClock,
    state: Rc<RefCell<Scripted>>,
}

impl ScriptedTransport {
    pub fn new(clock: &FakeClock) -> ScriptedTransport {
        ScriptedTransport { clock: clock.clone(), state: Rc::default() }
    }

    pub fn reply(&self, status: u16, headers: &[(&str, &str)], body: &str) -> &Self {
        let headers = headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        self.state.borrow_mut().http.push_back(Ok(HttpReply { status, headers, body: body.into() }));
        self
    }

    pub fn fail_http(&self, cause: &str) -> &Self {
        self.state.borrow_mut().http.push_back(Err(TransportError(cause.into())));
        self
    }

    /// The next `ws-open` answers `status`; `then` arrives after it.
    pub fn open(&self, status: u16, then: Vec<(u64, Event)>) -> &Self {
        self.state.borrow_mut().open.push_back((status, then));
        self
    }

    /// The frames that follow the next send, each `delay` ms after it.
    pub fn answer_send(&self, frames: Vec<(u64, Event)>) -> &Self {
        self.state.borrow_mut().on_send.push_back(frames);
        self
    }

    /// What follows our close frame, each `delay` ms after it.
    pub fn answer_close(&self, frames: Vec<(u64, Event)>) -> &Self {
        self.state.borrow_mut().on_close = frames;
        self
    }

    pub fn calls(&self) -> Vec<HttpCall> {
        self.state.borrow().calls.clone()
    }

    pub fn sent(&self) -> Vec<String> {
        self.state.borrow().sent.clone()
    }

    pub fn closed(&self) -> Vec<(u16, String)> {
        self.state.borrow().closed.clone()
    }
}

pub fn text(frame: &str) -> Event {
    Event::Frame { opcode: 1, data: frame.as_bytes().to_vec() }
}

impl Transport for ScriptedTransport {
    fn http(&mut self, call: &HttpCall) -> Result<HttpReply, TransportError> {
        let mut state = self.state.borrow_mut();
        state.calls.push(call.clone());
        state.http.pop_front().unwrap_or_else(|| Err(TransportError("no scripted reply".into())))
    }

    fn ws_open(&mut self, _path: &str) -> Result<u16, TransportError> {
        let now = self.clock.now_ms();
        let mut state = self.state.borrow_mut();
        let (status, then) = state.open.pop_front().ok_or_else(|| TransportError("no scripted open".into()))?;
        state.has_socket = status == 101;
        state.due.extend(then.into_iter().map(|(delay, e)| (now + delay, e)));
        Ok(status)
    }

    fn ws_send(&mut self, text: &str) -> Result<(), TransportError> {
        let now = self.clock.now_ms();
        let mut state = self.state.borrow_mut();
        if !state.has_socket {
            return Err(TransportError("no socket".into()));
        }
        state.sent.push(text.into());
        if let Some(frames) = state.on_send.pop_front() {
            state.due.extend(frames.into_iter().map(|(delay, e)| (now + delay, e)));
        }
        Ok(())
    }

    fn ws_drain(&mut self) -> Vec<Event> {
        let now = self.clock.now_ms();
        let mut state = self.state.borrow_mut();
        let (ready, later): (Vec<_>, Vec<_>) = std::mem::take(&mut state.due).into_iter().partition(|(at, _)| *at <= now);
        state.due = later;
        let mut ready = ready;
        ready.sort_by_key(|(at, _)| *at);
        ready.into_iter().map(|(_, e)| e).collect()
    }

    fn ws_close(&mut self, code: u16, reason: &str) -> Result<(), TransportError> {
        let mut state = self.state.borrow_mut();
        if !state.has_socket {
            return Err(TransportError("no socket".into()));
        }
        state.closed.push((code, reason.into()));
        let now = self.clock.now_ms();
        let after = std::mem::take(&mut state.on_close);
        state.due.extend(after.into_iter().map(|(delay, e)| (now + delay, e)));
        Ok(())
    }
}

/// Plays a cassette interaction back as the server: what the interpreter does is answered with the recorded steps that followed the same
/// action, so a run of the recorded input should reproduce the recorded steps (the oracle of the interpreter).
#[derive(Clone)]
pub struct CassetteEchoTransport {
    steps: Rc<Vec<Step>>,
    at: Rc<Cell<usize>>,
    pending: Rc<RefCell<VecDeque<Event>>>,
}

impl CassetteEchoTransport {
    pub fn new(steps: Vec<Step>) -> CassetteEchoTransport {
        CassetteEchoTransport { steps: Rc::new(steps), at: Rc::default(), pending: Rc::default() }
    }

    fn channel_is(step: &Step, channel: &str) -> bool {
        step.channel.as_deref() == Some(channel)
    }

    fn is_ack(step: &Step) -> bool {
        step.dir == Dir::C2s && step.payload.get("_tag").and_then(Value::as_str) == Some("Ack")
    }

    /// Moves to the next step satisfying `want`, returning it; the steps skipped are the interpreter's own (acknowledgements).
    fn next(&self, want: impl Fn(&Step) -> bool) -> Option<&Step> {
        let from = self.at.get();
        let found = (from..self.steps.len()).find(|i| want(&self.steps[*i]))?;
        self.at.set(found + 1);
        Some(&self.steps[found])
    }

    /// The frames and closes the server sent next, up to the next step the interpreter itself takes.
    fn queue_arrivals(&self) {
        let mut at = self.at.get();
        while let Some(step) = self.steps.get(at) {
            let event = match (step.dir, step.channel.as_deref()) {
                (Dir::S2c, Some("ws")) => Some(Self::event_of(step)),
                (Dir::S2c, Some("ws:close")) if step.payload["tcp"] == Value::Bool(false) => Some(Event::Close { code: step.payload["code"].as_u64().map(|c| c as u16), reason: step.payload["reason"].as_str().unwrap_or("").into() }),
                (Dir::S2c, Some("ws:close")) => Some(Event::Dropped),
                (Dir::C2s, Some("ws")) if Self::is_ack(step) => None,
                _ => break,
            };
            if let Some(event) = event {
                if matches!(event, Event::Dropped) {
                    break;
                }
                self.pending.borrow_mut().push_back(event);
            }
            at += 1;
        }
        self.at.set(at);
    }

    fn event_of(step: &Step) -> Event {
        match step.kind {
            StepKind::Binary => Event::Frame { opcode: 2, data: STANDARD.decode(step.payload.as_str().unwrap_or("")).unwrap_or_default() },
            StepKind::Text => Event::Frame { opcode: 1, data: step.payload.as_str().unwrap_or("").as_bytes().to_vec() },
            _ => Event::Frame { opcode: 1, data: step.payload.to_string().into_bytes() },
        }
    }
}

impl Transport for CassetteEchoTransport {
    fn http(&mut self, _call: &HttpCall) -> Result<HttpReply, TransportError> {
        self.next(|s| s.dir == Dir::Call && Self::channel_is(s, "http")).ok_or_else(|| TransportError("the cassette has no further http call".into()))?;
        let ret = self.next(|s| s.dir == Dir::Ret && Self::channel_is(s, "http")).ok_or_else(|| TransportError("the cassette has no http return".into()))?;
        let headers = ret.payload["headers"].as_object().into_iter().flatten().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string())).collect();
        Ok(HttpReply { status: ret.payload["status"].as_u64().unwrap_or(0) as u16, headers, body: match &ret.payload["body"] {
                Value::String(s) => s.clone(),
                Value::Null => String::new(),
                // a masked body was parsed to JSON by the rules: the wire carried its text
                other => other.to_string(),
            },
        })
    }

    fn ws_open(&mut self, _path: &str) -> Result<u16, TransportError> {
        let status = self.next(|s| Self::channel_is(s, "ws:open")).ok_or_else(|| TransportError("the cassette has no ws:open".into()))?.payload["status"].as_u64().unwrap_or(0) as u16;
        self.queue_arrivals();
        Ok(status)
    }

    fn ws_send(&mut self, text: &str) -> Result<(), TransportError> {
        // an acknowledgement is the interpreter's own doing: it is no action the cassette recorded an answer to
        if serde_json::from_str::<Value>(text).is_ok_and(|v| v.get("_tag").and_then(Value::as_str) == Some("Ack")) {
            return Ok(());
        }
        self.next(|s| s.dir == Dir::C2s && Self::channel_is(s, "ws") && !Self::is_ack(s)).ok_or_else(|| TransportError("no socket".into()))?;
        self.queue_arrivals();
        Ok(())
    }

    fn ws_drain(&mut self) -> Vec<Event> {
        self.pending.borrow_mut().drain(..).collect()
    }

    fn ws_close(&mut self, _code: u16, _reason: &str) -> Result<(), TransportError> {
        self.next(|s| s.dir == Dir::C2s && Self::channel_is(s, "ws:close")).ok_or_else(|| TransportError("no socket".into()))?;
        self.queue_arrivals();
        Ok(())
    }
}

/// A phone that reports events at set times on the shared clock: what a stream sends after it is opened, and what the handshake tap sees.
#[derive(Clone)]
pub struct ScriptedPhone {
    clock: FakeClock,
    state: Rc<RefCell<PhoneState>>,
}

#[derive(Default)]
struct PhoneState {
    refuse: Option<String>,
    tapped: bool,
    tap_close: bool,
    handshake: Vec<PhoneEvent>,
    stream: Vec<(u64, PhoneEvent)>,
    pending: Vec<(u64, PhoneEvent)>,
    subscribed: Vec<(String, Value)>,
    unsubscribed: Vec<String>,
    overrides: Vec<Value>,
}

impl ScriptedPhone {
    pub fn new(clock: &FakeClock) -> ScriptedPhone {
        ScriptedPhone { clock: clock.clone(), state: Rc::default() }
    }

    /// The frames a stream sends: each after its delay from the moment the stream is opened (its `sub` is filled in).
    pub fn streams(&self, frames: Vec<(u64, PhoneEvent)>) -> &Self {
        self.state.borrow_mut().stream = frames;
        self
    }

    /// What the handshake tap reports when `connect` runs with the tap on.
    pub fn handshake(&self, events: Vec<PhoneEvent>) -> &Self {
        self.state.borrow_mut().handshake = events;
        self
    }

    pub fn refusing(&self, message: &str) -> &Self {
        self.state.borrow_mut().refuse = Some(message.into());
        self
    }

    pub fn subscribed(&self) -> Vec<(String, Value)> {
        self.state.borrow().subscribed.clone()
    }

    pub fn unsubscribed(&self) -> Vec<String> {
        self.state.borrow().unsubscribed.clone()
    }

    /// What each `connect` was given besides the timeout.
    pub fn overrides(&self) -> Vec<Value> {
        self.state.borrow().overrides.clone()
    }
}

impl Phone for ScriptedPhone {
    fn connect(&mut self, _timeout_ms: u64) -> String {
        let mut state = self.state.borrow_mut();
        if state.tapped {
            let now = self.clock.now_ms();
            let events: Vec<_> = state.handshake.drain(..).map(|e| (now, e)).collect();
            state.pending.extend(events);
        }
        "connected".into()
    }

    fn connect_with(&mut self, timeout_ms: u64, overrides: &Value) -> String {
        self.state.borrow_mut().overrides.push(overrides.clone());
        self.connect(timeout_ms)
    }

    fn request(&mut self, _method: &str, _params: &Value, _timeout_ms: u64) -> Result<Value, String> {
        Ok(serde_json::json!({"ok": true, "result": {}}))
    }

    fn subscribe(&mut self, method: &str, params: &Value) -> Result<String, String> {
        let mut state = self.state.borrow_mut();
        if let Some(message) = state.refuse.clone() {
            return Err(message);
        }
        let sub = format!("sub-{}", state.subscribed.len());
        state.subscribed.push((method.into(), params.clone()));
        let now = self.clock.now_ms();
        let frames: Vec<_> = state
            .stream
            .iter()
            .map(|(delay, e)| match e {
                PhoneEvent::Frame { binary, payload, len, .. } => (now + delay, PhoneEvent::Frame { sub: sub.clone(), binary: *binary, payload: payload.clone(), len: *len }),
                other => (now + delay, other.clone()),
            })
            .collect();
        state.pending.extend(frames);
        Ok(sub)
    }

    fn unsubscribe(&mut self, sub: &str) -> Result<(), String> {
        self.state.borrow_mut().unsubscribed.push(sub.into());
        Ok(())
    }

    fn tap(&mut self, handshake: bool, close: bool) {
        let mut state = self.state.borrow_mut();
        state.tapped = handshake;
        state.tap_close = close;
    }

    fn poll(&mut self) -> Vec<PhoneEvent> {
        let now = self.clock.now_ms();
        let mut state = self.state.borrow_mut();
        let (due, later): (Vec<_>, Vec<_>) = state.pending.drain(..).partition(|(at, _)| *at <= now);
        state.pending = later;
        due.into_iter().map(|(_, e)| e).collect()
    }

    fn close(&mut self) {
        let now = self.clock.now_ms();
        let mut state = self.state.borrow_mut();
        if state.tap_close {
            state.pending.push((now, PhoneEvent::Close { code: Some(1000), reason: String::new() }));
        }
    }
}
