//! The state machine: entries in order, each filled from the variables, run against the transport, the wire reported as steps.
//! Frames arrive on a queue the core drains at every step boundary and during every wait, so the order of steps is the order of arrival.
use crate::case::{Case, Entry};
use crate::policy::Policy;
use crate::ports::{Clock, Event, HttpCall, Phone, PhoneEvent, Target, Transport};
use crate::vars::{captured, fill, js_string, unbound, Vars};
use mimic_core::cassette::{Dir, DriverInfo, Outcome, Status, Step, StepKind};
use mimic_core::ports::{Driver, DriverError, Hello, Observed, Request};
use mimic_core::wire::{raw_steps, WireEvent, OP_TEXT};
use serde_json::{json, Value};
use std::collections::HashMap;

pub const BAD_CREDENTIAL: &str = "NOTACREDENTIAL";

pub struct Interpreter {
    transport: Box<dyn Transport>,
    /// The Orca phone, for cases that use `connect` and `request`.
    phone: Option<Box<dyn Phone>>,
    target: Box<dyn Target>,
    clock: Box<dyn Clock>,
    policy: Policy,
    /// Run before the entries of a case that asks for `prelude: "session"`.
    prelude: Vec<Entry>,
    /// Session mode: the prelude runs once per server generation and its steps open every case that asks for it.
    session: bool,
    begun: Option<Begun>,
    /// The state a failed `connect` of the last run ended in.
    connect_failed: Option<String>,
}

/// What the prelude of one server generation left behind.
struct Begun {
    generation: u64,
    steps: Vec<Step>,
    access_token: Option<Value>,
}

/// The state of one run.
struct Run {
    steps: Vec<Step>,
    vars: Vars,
    last_frame_at: u64,
    settle_ms: Option<u64>,
    socket: bool,
    /// A `connect` that did not reach `connected` ends the script with no steps; the state is the outcome's code.
    connect_failed: Option<String>,
    /// Streams the case opened: the request id of the opener to the id the phone gave the stream, and back.
    sub_of_rid: HashMap<String, String>,
    rid_of_sub: HashMap<String, String>,
}

fn orca_step(dir: Dir, rid: &str, payload: Value) -> Step {
    Step { dir, channel: None, kind: StepKind::Json, rid: Some(rid.into()), len: None, payload }
}

fn driver_step(payload: Value) -> Step {
    Step { dir: Dir::Ret, channel: Some("driver".into()), kind: StepKind::Json, rid: None, len: None, payload }
}

fn tag_of(step: &Step) -> Option<&str> {
    step.payload.get("_tag").and_then(Value::as_str)
}

/// Whether `step` is the Exit frame that answers request `id`.
fn is_exit_of(step: &Step, id: &Value) -> bool {
    step.dir == Dir::S2c && tag_of(step) == Some("Exit") && step.payload.get("requestId").is_some_and(|r| js_string(r) == js_string(id))
}

fn form_encode(object: &serde_json::Map<String, Value>) -> String {
    fn escape(text: &str) -> String {
        let mut out = String::new();
        for byte in text.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => out.push(byte as char),
                b' ' => out.push('+'),
                other => out.push_str(&format!("%{other:02X}")),
            }
        }
        out
    }
    object.iter().map(|(k, v)| format!("{}={}", escape(k), escape(&js_string(v)))).collect::<Vec<_>>().join("&")
}

fn body_text(body: &Value, content_type: &str) -> Option<String> {
    match body {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Object(o) if content_type.to_lowercase().contains("x-www-form-urlencoded") => Some(form_encode(o)),
        other => Some(other.to_string()),
    }
}

/// Header names lower-cased, repeated names joined with ", ", sorted by name (as `Headers` iterate in the TypeScript client).
fn normalise_headers(headers: &[(String, String)]) -> Vec<(String, String)> {
    let mut merged: Vec<(String, String)> = Vec::new();
    for (name, value) in headers {
        let name = name.to_lowercase();
        match merged.iter_mut().find(|(n, _)| *n == name) {
            Some((_, existing)) => {
                existing.push_str(", ");
                existing.push_str(value);
            }
            None => merged.push((name, value.clone())),
        }
    }
    merged.sort_by(|a, b| a.0.cmp(&b.0));
    merged
}

/// Each run-specific path becomes its placeholder wherever it reaches a step (an empty path never matches).
fn mask_paths(v: &mut Value, masks: &[(String, String)]) {
    match v {
        Value::String(s) => {
            for (path, placeholder) in masks.iter().filter(|(p, _)| !p.is_empty()) {
                if s.contains(path.as_str()) {
                    *s = s.replace(path.as_str(), placeholder);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| mask_paths(x, masks)),
        Value::Object(o) => o.values_mut().for_each(|x| mask_paths(x, masks)),
        _ => {}
    }
}

impl Interpreter {
    pub fn new(transport: Box<dyn Transport>, target: Box<dyn Target>, clock: Box<dyn Clock>, policy: Policy) -> Interpreter {
        Interpreter { transport, phone: None, target, clock, policy, prelude: Vec::new(), session: false, begun: None, connect_failed: None }
    }

    /// The phone the `connect` and `request` ops speak through.
    pub fn with_phone(mut self, phone: Box<dyn Phone>) -> Interpreter {
        self.phone = Some(phone);
        self
    }

    /// Session mode: one prelude per server generation instead of one per case; needs a prelude of at least the token, ticket and open entries.
    pub fn with_session(mut self, session: bool) -> Interpreter {
        self.session = session;
        self
    }

    /// The prelude data of the set: entries every case that names `prelude: "session"` starts with.
    pub fn with_prelude(mut self, prelude: Vec<Entry>) -> Interpreter {
        self.prelude = prelude;
        self
    }

    fn push_wire(&self, run: &mut Run, event: WireEvent) {
        run.steps.extend(raw_steps(&[event]));
    }

    /// Takes what has arrived: frames become steps in order, a Chunk is acknowledged at once, a close or drop ends the socket.
    fn drain(&mut self, run: &mut Run) {
        for event in self.transport.ws_drain() {
            match event {
                Event::Frame { opcode, data } => {
                    run.last_frame_at = self.clock.now_ms();
                    self.push_wire(run, WireEvent::Ws { dir: Dir::S2c, opcode, data });
                    let chunk = run.steps.last().filter(|s| s.kind == StepKind::Json && tag_of(s) == Some("Chunk")).map(|s| s.payload.get("requestId").cloned().unwrap_or(Value::Null));
                    if let Some(request_id) = chunk {
                        let ack = json!({"_tag": "Ack", "requestId": request_id}).to_string();
                        self.push_wire(run, WireEvent::Ws { dir: Dir::C2s, opcode: OP_TEXT, data: ack.clone().into_bytes() });
                        let _ = self.transport.ws_send(&ack);
                    }
                }
                Event::Close { code, reason } => {
                    self.push_wire(run, WireEvent::WsClose { dir: Some(Dir::S2c), code, reason });
                    run.socket = false;
                }
                Event::Dropped => {
                    self.push_wire(run, WireEvent::WsClose { dir: None, code: None, reason: String::new() });
                    run.socket = false;
                }
            }
        }
    }

    /// One poll of every wait: sleep a step, then take what came.
    fn tick(&mut self, run: &mut Run, window_ms: u64) {
        self.clock.sleep(window_ms.min(self.policy.poll_ms));
        self.drain(run);
    }

    /// Everything the server sends until it has been quiet for `ms`, measured from the last frame or send.
    fn quiet(&mut self, run: &mut Run, ms: u64) {
        loop {
            self.tick(run, ms);
            if self.clock.now_ms().saturating_sub(run.last_frame_at) >= ms {
                break;
            }
        }
    }

    /// Waits up to `limit_ms` for `done`, in poll steps; with no socket there is nothing to wait for.
    fn wait_until(&mut self, run: &mut Run, limit_ms: u64, done: impl Fn(&Run) -> bool) {
        let mut waited = 0;
        while !done(run) && run.socket && waited < limit_ms {
            self.tick(run, self.policy.poll_ms);
            waited += self.policy.poll_ms;
        }
    }

    fn transport_failed(run: &mut Run, cause: &str) {
        run.steps.push(driver_step(json!({"transport_error": cause})));
    }

    /// Runs one entry; `false` ends the script (a transport failure).
    fn entry(&mut self, run: &mut Run, entry: &Entry, declared: &[String]) -> bool {
        let arg = fill(&entry.arg, &run.vars);
        if entry.op == "request" {
            return self.request(run, entry, &arg, declared);
        }
        if let Some(name) = unbound(&arg, &run.vars, declared) {
            run.steps.push(driver_step(json!({"client_error": format!("unbound variable {name}"), "op": entry.op})));
            return true;
        }
        match entry.op.as_str() {
            "http" => self.http(run, entry, &arg),
            "ws-open" => self.ws_open(run, &arg),
            "ws-send" => self.ws_send(run, entry, &arg),
            "git" => self.git(run, &arg),
            "connect" => self.connect(run, &arg),
            "subscribe" => self.subscribe(run, &arg),
            "listen" => self.listen(run, &arg),
            "unsubscribe" => self.unsubscribe(run, &arg),
            _ => self.ws_close(run, &arg),
        }
    }

    fn git(&mut self, run: &mut Run, arg: &Value) -> bool {
        let name = arg.get("name").map(js_string).unwrap_or_default();
        match self.target.make_git(&name) {
            Ok(path) => {
                run.vars.insert(format!("git.{name}"), Value::String(path));
                true
            }
            Err(e) => {
                Self::transport_failed(run, &e);
                false
            }
        }
    }

    fn connect(&mut self, run: &mut Run, arg: &Value) -> bool {
        let timeout = arg.get("timeout_ms").and_then(Value::as_u64).unwrap_or(10_000);
        let Some(phone) = self.phone.as_mut() else {
            Self::transport_failed(run, "no phone");
            return false;
        };
        phone.tap(arg.get("capture_handshake").and_then(Value::as_bool).unwrap_or(false), arg.get("capture_close").and_then(Value::as_bool).unwrap_or(false));
        let overrides: serde_json::Map<String, Value> = ["device_token", "server_public_key_b64"].iter().filter_map(|k| Some((k.to_string(), arg.get(*k)?.clone()))).collect();
        let state = phone.connect_with(timeout, &Value::Object(overrides));
        if state == "connected" {
            self.pump(run);
            return true;
        }
        run.steps.clear();
        run.connect_failed = Some(state);
        false
    }

    /// Takes what the phone has reported: stream frames, handshake steps and the end of the connection become steps in arrival order.
    fn pump(&mut self, run: &mut Run) {
        let events = self.phone.as_mut().map(|p| p.poll()).unwrap_or_default();
        for event in events {
            run.steps.push(match event {
                PhoneEvent::Frame { sub, binary, payload, len } => {
                    let rid = run.rid_of_sub.get(&sub).cloned().unwrap_or(sub);
                    Step { dir: Dir::S2c, channel: None, kind: if binary { StepKind::Binary } else { StepKind::Json }, rid: Some(rid), len: len.map(|n| n as u64), payload }
                }
                PhoneEvent::Handshake { stage, client_to_server, payload } => {
                    let dir = if client_to_server { Dir::C2s } else { Dir::S2c };
                    Step { dir, channel: Some("handshake".into()), kind: StepKind::Json, rid: None, len: None, payload: json!({"stage": stage, "frame": payload}) }
                }
                PhoneEvent::Close { code, reason } => Step { dir: Dir::S2c, channel: Some("ws:close".into()), kind: StepKind::Json, rid: None, len: None, payload: json!({"code": code, "reason": reason}) },
            });
        }
    }

    /// Everything the phone reports for `ms`, in poll steps.
    fn listen_for(&mut self, run: &mut Run, ms: u64) {
        let mut waited = 0;
        loop {
            self.pump(run);
            if waited >= ms {
                break;
            }
            let step = (ms - waited).min(self.policy.poll_ms);
            self.clock.sleep(step);
            waited += step;
        }
    }

    fn listen(&mut self, run: &mut Run, arg: &Value) -> bool {
        self.listen_for(run, arg.get("ms").and_then(Value::as_u64).unwrap_or(0));
        true
    }

    /// Opens a stream. The opener is recorded like a request; its frames follow with the same rid for `listen_ms`.
    fn subscribe(&mut self, run: &mut Run, arg: &Value) -> bool {
        let rid = arg.get("rid").map(js_string).unwrap_or_default();
        let method = arg.get("method").map(js_string).unwrap_or_default();
        let params = arg.get("params").cloned().unwrap_or(Value::Null);
        run.steps.push(orca_step(Dir::C2s, &rid, json!({"method": method, "params": params})));
        match self.phone.as_mut().map(|p| p.subscribe(&method, &params)) {
            Some(Ok(sub)) => {
                run.rid_of_sub.insert(sub.clone(), rid.clone());
                run.sub_of_rid.insert(rid, sub);
            }
            Some(Err(message)) => run.steps.push(orca_step(Dir::S2c, &rid, json!({"client_error": message}))),
            None => run.steps.push(orca_step(Dir::S2c, &rid, json!({"client_error": "no phone"}))),
        }
        self.listen_for(run, arg.get("listen_ms").and_then(Value::as_u64).unwrap_or(0));
        true
    }

    /// Ends a stream and takes the frames that were still on their way.
    fn unsubscribe(&mut self, run: &mut Run, arg: &Value) -> bool {
        let rid = arg.get("rid").map(js_string).unwrap_or_default();
        if let (Some(sub), Some(phone)) = (run.sub_of_rid.get(&rid).cloned(), self.phone.as_mut()) {
            if let Err(message) = phone.unsubscribe(&sub) {
                run.steps.push(orca_step(Dir::S2c, &rid, json!({"client_error": message})));
            }
        }
        let quiet = self.policy.quiet(run.settle_ms);
        self.listen_for(run, quiet);
        true
    }

    /// One sealed request. The request step is recorded as written; a value an earlier answer did not carry is the finding, never sent as text.
    fn request(&mut self, run: &mut Run, entry: &Entry, arg: &Value, declared: &[String]) -> bool {
        let rid = arg.get("rid").map(js_string).unwrap_or_default();
        let method = arg.get("method").map(js_string).unwrap_or_default();
        let params = arg.get("params").cloned().unwrap_or(Value::Null);
        run.steps.push(orca_step(Dir::C2s, &rid, json!({"method": method, "params": params})));
        let payload = if let Some(name) = unbound(&params, &run.vars, declared) {
            json!({"client_error": format!("unbound variable {name}")})
        } else {
            let timeout = arg.get("timeout_ms").and_then(Value::as_u64).unwrap_or(10_000);
            match self.phone.as_mut().map(|p| p.request(&method, &params, timeout)) {
                Some(Ok(answer)) => {
                    if let Some(capture) = &entry.capture {
                        run.vars.extend(captured(&answer, capture));
                    }
                    answer
                }
                Some(Err(message)) => json!({"client_error": message}),
                None => json!({"client_error": "no phone"}),
            }
        };
        self.pump(run);
        run.steps.push(orca_step(Dir::S2c, &rid, payload));
        true
    }

    fn http(&mut self, run: &mut Run, entry: &Entry, arg: &Value) -> bool {
        let headers: Vec<(String, String)> = arg.get("headers").and_then(Value::as_object).into_iter().flatten().map(|(k, v)| (k.clone(), js_string(v))).collect();
        let content_type = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map(|(_, v)| v.as_str()).unwrap_or("");
        let call = HttpCall {
            method: arg.get("method").map_or_else(|| "GET".into(), js_string),
            path: arg.get("path").map(js_string).unwrap_or_default(),
            body: body_text(arg.get("body").unwrap_or(&Value::Null), content_type),
            headers,
        };
        let reply = match self.transport.http(&call) {
            Ok(reply) => reply,
            Err(e) => {
                Self::transport_failed(run, &e.0);
                return false;
            }
        };
        // frames that came while the call was in flight are before it
        self.drain(run);
        let res_headers = normalise_headers(&reply.headers);
        self.push_wire(
            run,
            WireEvent::Http { method: call.method, path: call.path, req_headers: call.headers, req_body: call.body.unwrap_or_default(), status: reply.status, res_headers, res_body: reply.body.clone() },
        );
        if let Ok(parsed) = serde_json::from_str::<Value>(&reply.body) {
            if let Some(capture) = &entry.capture {
                run.vars.extend(captured(&parsed, capture));
            }
            for name in ["access_token", "ticket"] {
                if let Some(Value::String(s)) = parsed.get(name) {
                    run.vars.insert(name.into(), Value::String(s.clone()));
                }
            }
        }
        true
    }

    fn ws_open(&mut self, run: &mut Run, arg: &Value) -> bool {
        let path = arg.get("path").map(js_string).unwrap_or_default();
        match self.transport.ws_open(&path) {
            Ok(status) => {
                self.push_wire(run, WireEvent::WsOpen { path, status });
                run.socket = status == 101;
                run.last_frame_at = self.clock.now_ms();
                self.drain(run);
                true
            }
            Err(e) => {
                Self::transport_failed(run, &e.0);
                false
            }
        }
    }

    fn ws_send(&mut self, run: &mut Run, entry: &Entry, arg: &Value) -> bool {
        let from = run.steps.len();
        let text = arg.to_string();
        self.push_wire(run, WireEvent::Ws { dir: Dir::C2s, opcode: OP_TEXT, data: text.clone().into_bytes() });
        if let Err(e) = self.transport.ws_send(&text) {
            Self::transport_failed(run, &e.0);
            return false;
        }
        run.last_frame_at = self.clock.now_ms();
        let id = arg.get("id").cloned().unwrap_or(Value::Null);
        let quiet = self.policy.quiet(run.settle_ms);
        if entry.await_exit {
            let limit = self.policy.await_exit_ms;
            self.wait_until(run, limit, |r| r.steps[from..].iter().any(|s| is_exit_of(s, &id)));
        } else if arg.get("_tag").and_then(Value::as_str) == Some("Request") {
            let limit = self.policy.first_answer_ms;
            self.wait_until(run, limit, |r| r.steps[from..].iter().any(|s| s.dir == Dir::S2c));
        }
        self.quiet(run, quiet);
        if let Some(capture) = &entry.capture {
            if let Some(exit) = run.steps[from..].iter().find(|s| is_exit_of(s, &id)) {
                let found = captured(&exit.payload, capture);
                run.vars.extend(found);
            }
        }
        true
    }

    fn ws_close(&mut self, run: &mut Run, arg: &Value) -> bool {
        let code = arg.get("code").and_then(Value::as_u64).unwrap_or(1000) as u16;
        let reason = arg.get("reason").map(js_string).unwrap_or_default();
        run.steps.push(Step { dir: Dir::C2s, channel: Some("ws:close".into()), kind: StepKind::Json, rid: None, len: None, payload: json!({"code": code, "reason": reason, "tcp": false}) });
        if let Err(e) = self.transport.ws_close(code, &reason) {
            Self::transport_failed(run, &e.0);
            return false;
        }
        let before = run.steps.len();
        let limit = self.policy.close_wait_ms;
        self.wait_until(run, limit, |r| r.steps[before..].iter().any(|s| s.channel.as_deref() == Some("ws:close")));
        // a peer's close that arrives as a frame ends the wait; the connection ending is always the last step, once
        let ended = run.steps[before..].iter().any(|s| s.channel.as_deref() == Some("ws:close") && s.payload["tcp"] == Value::Bool(true));
        if !ended {
            self.push_wire(run, WireEvent::WsClose { dir: None, code: None, reason: String::new() });
        }
        run.socket = false;
        true
    }

    /// Runs `entries` from `vars`; the steps and the variables the run ended with.
    fn execute(&mut self, entries: &[Entry], settle_ms: Option<u64>, declared: &[String], vars: Vars) -> (Vec<Step>, Vars) {
        let mut run = Run { steps: Vec::new(), vars, last_frame_at: self.clock.now_ms(), settle_ms, socket: false, connect_failed: None, sub_of_rid: HashMap::new(), rid_of_sub: HashMap::new() };
        for entry in entries {
            if !self.entry(&mut run, entry, declared) {
                break;
            }
        }
        if let Some(phone) = self.phone.as_mut().filter(|_| entries.iter().any(|e| e.op == "connect")) {
            phone.close();
            self.pump(&mut run);
        }
        self.connect_failed = run.connect_failed.take();
        (run.steps, run.vars)
    }

    fn base_vars(&mut self, wants_credential: bool) -> Result<Vars, String> {
        let mut vars = Vars::new();
        vars.insert("workspace".into(), Value::String(self.target.workspace()));
        vars.insert("bad-credential".into(), Value::String(BAD_CREDENTIAL.into()));
        if wants_credential {
            vars.insert("credential".into(), Value::String(self.target.mint()?));
        }
        Ok(vars)
    }

    fn names(entries: &[Entry]) -> Vec<String> {
        entries.iter().filter_map(|e| e.capture.as_ref()?.as_object()).flat_map(|c| c.keys().cloned()).collect()
    }

    fn wants_credential(entries: &[Entry]) -> bool {
        entries.iter().any(|e| crate::vars::placeholders(&e.arg.to_string()).contains(&"credential"))
    }

    /// The prelude of this server generation, run once: its steps up to the close that ends it, and the token it was given.
    fn begin(&mut self) -> Result<(), String> {
        let generation = self.target.generation();
        if self.begun.as_ref().is_some_and(|b| b.generation == generation) {
            return Ok(());
        }
        let mut entries = self.prelude.clone();
        entries.push(Entry { op: "ws-close".into(), arg: serde_json::json!({}), capture: None, await_exit: false });
        let vars = self.base_vars(Self::wants_credential(&entries))?;
        let (mut steps, vars) = self.execute(&entries, Some(250), &Self::names(&entries), vars);
        if let Some(closed) = steps.iter().position(|s| s.channel.as_deref() == Some("ws:close")) {
            steps.truncate(closed);
        }
        self.begun = Some(Begun { generation, steps, access_token: vars.get("access_token").cloned() });
        Ok(())
    }

    /// Runs one case; an `Err` is a request-level failure (a malformed case, no credential), not behaviour.
    pub fn run_case(&mut self, case: &Case) -> Result<Observed, String> {
        let workspace = self.target.workspace();
        let mut steps;
        if case.prelude && self.session && self.prelude.len() >= 3 {
            self.begin()?;
            let (cached, token) = self.begun.as_ref().map(|b| (b.steps.clone(), b.access_token.clone())).expect("the prelude has run");
            let mut entries = vec![self.prelude[1].clone(), self.prelude[2].clone()];
            entries.extend(case.entries.iter().cloned());
            let mut vars = self.base_vars(Self::wants_credential(&entries))?;
            if let Some(token) = token {
                vars.insert("access_token".into(), token);
            }
            let mut declared = case.declared();
            declared.extend(Self::names(&self.prelude));
            let (own, _) = self.execute(&entries, case.settle_ms, &declared, vars);
            // the socket of its own opens like the prelude's: its opening steps are the cached ones
            let opened = own.iter().position(|s| s.channel.as_deref() == Some("ws:open"));
            steps = cached;
            steps.extend(own.into_iter().skip(opened.map_or(0, |i| i + 1)));
        } else {
            let entries: Vec<Entry> = if case.prelude { self.prelude.iter().chain(&case.entries).cloned().collect() } else { case.entries.clone() };
            let vars = self.base_vars(Self::wants_credential(&entries))?;
            let mut declared = case.declared();
            if case.prelude {
                declared.extend(Self::names(&self.prelude));
            }
            steps = self.execute(&entries, case.settle_ms, &declared, vars).0;
        }
        let mut masks = vec![(workspace, "<workspace>".to_string())];
        masks.extend(self.target.masks());
        for step in &mut steps {
            mask_paths(&mut step.payload, &masks);
        }
        if let Some(state) = self.connect_failed.take() {
            return Ok(Observed { steps: Vec::new(), effects: Vec::new(), outcome: Outcome { status: Status::Error, code: Some(state), reference_accepts: None } });
        }
        let transport = steps.last().is_some_and(|s| s.channel.as_deref() == Some("driver") && s.payload.get("transport_error").is_some());
        let outcome = if transport { Outcome { status: Status::Error, code: Some("transport".into()), reference_accepts: None } } else { Outcome::default() };
        Ok(Observed { steps, effects: Vec::new(), outcome })
    }
}

impl Interpreter {
    /// What this driver says about itself in `hello`.
    pub fn hello_info() -> Hello {
        Hello { driver: DriverInfo { name: "mimic-script".into(), version: env!("CARGO_PKG_VERSION").into(), protocol: "mimic-driver/1".into(), toolchain_lock_hash: None }, target_version: None, coverage: false }
    }
}

impl Driver for Interpreter {
    fn hello(&mut self) -> Result<Hello, DriverError> {
        Ok(Interpreter::hello_info())
    }

    fn run(&mut self, request: &Request) -> Result<Observed, DriverError> {
        let case = Case::parse(&request.input).map_err(DriverError)?;
        self.run_case(&case).map_err(DriverError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_an_object_body_and_a_form_content_type_when_encoded_then_it_follows_urlsearchparams() {
        let body = body_text(&json!({"a b": "x~y", "n": 3}), "Application/X-WWW-Form-Urlencoded").unwrap();

        assert_eq!(body, "a+b=x%7Ey&n=3");
        assert_eq!(body_text(&json!({"k": 1}), "application/json").as_deref(), Some("{\"k\":1}"));
        assert_eq!(body_text(&Value::Null, ""), None);
    }

    #[test]
    fn given_repeated_and_mixed_case_headers_when_normalised_then_they_are_lower_case_sorted_and_joined() {
        let headers = normalise_headers(&[("Set-Cookie".into(), "a=1".into()), ("Content-Type".into(), "x".into()), ("set-cookie".into(), "b=2".into())]);

        assert_eq!(headers, [("content-type".to_string(), "x".to_string()), ("set-cookie".to_string(), "a=1, b=2".to_string())]);
    }
}
