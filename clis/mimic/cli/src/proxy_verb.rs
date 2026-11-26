//! `wf mimic proxy`: the loopback proxy that observes the native T3 mobile app against the server it is configured for. It forwards
//! every byte (the credentials the app sends go on untouched, so the app keeps working) and writes what crossed as an ordered,
//! correlated JSONL: the method, the envelope and the payload's shape, never a value.
use crate::adapters::proxy::{Event, Proxy, Sink};
use crate::proxy_shape;
use mimic_core::cassette::Dir;
use mimic_core::wire::OP_BINARY;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::os::unix::fs::OpenOptionsExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wf_kit::args::Parsed;

use super::{fail, Done, Fail};

/// The schema of the file the proxy writes and of the object it prints.
pub const SCHEMA: &str = "mimic-proxy/1";

/// The next step every refusal of the upstream names.
const UPSTREAM_NEXT: &str = "wf mimic proxy --upstream http://127.0.0.1:<port> --out <file>";
/// The next step every refusal of the listening address names.
const LISTEN_NEXT: &str = "wf mimic proxy --listen 127.0.0.1:<port>";
/// What a failed write of the file says: a word of the proxy's own, never a value of the traffic.
const WRITE_FAILED: &str = "the file could not be written";

/// The upstream as a socket address of the local machine: nothing else is accepted, so the proxy is never a way into another host. The
/// userinfo, the path and the query of the URL are ignored, and no refusal names the URL: it may carry a credential of its own.
pub(super) fn upstream_addr(url: &str) -> Result<SocketAddr, Fail> {
    let Some(rest) = url.strip_prefix("http://") else {
        return Err(fail("the upstream is not an http:// address", UPSTREAM_NEXT));
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    let Some((host, port)) = authority.rsplit_once(':') else {
        return Err(fail("the upstream names no port", UPSTREAM_NEXT));
    };
    let port: u16 = port.parse().map_err(|_| fail("the upstream names no port number", UPSTREAM_NEXT))?;
    let Some(ip) = host_ip(host) else {
        return Err(fail("the upstream host is not an address of this machine", UPSTREAM_NEXT));
    };
    if !ip.is_loopback() {
        return Err(fail("the upstream is not on the loopback interface", "wf mimic proxy --upstream http://127.0.0.1:<port> --out <file> (the proxy observes one local server)"));
    }
    Ok(SocketAddr::new(ip, port))
}

/// The address the proxy listens on: a loopback address, so no other machine can use it. No refusal names the address: it is a value
/// the caller chose.
pub(super) fn listen_addr(text: &str) -> Result<SocketAddr, Fail> {
    let Some((host, port)) = text.rsplit_once(':') else {
        return Err(fail("the address to listen on names no port", LISTEN_NEXT));
    };
    let port: u16 = port.parse().map_err(|_| fail("the address to listen on names no port number", LISTEN_NEXT))?;
    let Some(ip) = host_ip(host) else {
        return Err(fail("the address to listen on is not an address of this machine", LISTEN_NEXT));
    };
    if !ip.is_loopback() {
        return Err(fail("the address to listen on is not on the loopback interface", LISTEN_NEXT));
    }
    Ok(SocketAddr::new(ip, port))
}

/// The address of this machine a host names: `localhost` or an address of IP.
fn host_ip(host: &str) -> Option<IpAddr> {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    match host {
        "localhost" => Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        other => other.parse::<IpAddr>().ok(),
    }
}

/// The upstream as it is written down: the scheme, the host and the port, never the path, the query or the credentials it may carry.
pub(super) fn upstream_public(addr: SocketAddr) -> String {
    format!("http://{addr}")
}

/// The header value of a name, whatever its case.
fn header_value(headers: &[(String, String)], name: &str) -> String {
    headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone()).unwrap_or_default()
}

/// What the file holds: the lines, the counts and the labels that correlate a request with its answer.
pub(super) struct Recorder {
    out: BufWriter<File>,
    seq: u64,
    http_requests: u64,
    http_responses: u64,
    connections: u64,
    frames: u64,
    routes: BTreeMap<String, u64>,
    methods: BTreeMap<String, u64>,
    ws_methods: BTreeMap<String, u64>,
    requests: BTreeMap<(u32, String), String>,
    errors: Vec<String>,
    max_events: u64,
    finished: bool,
    failed: bool,
    stop: Arc<AtomicBool>,
}

impl Recorder {
    pub(super) fn new(out: File, max_events: u64, stop: Arc<AtomicBool>) -> Recorder {
        Recorder {
            out: BufWriter::new(out),
            seq: 0,
            http_requests: 0,
            http_responses: 0,
            connections: 0,
            frames: 0,
            routes: BTreeMap::new(),
            methods: BTreeMap::new(),
            ws_methods: BTreeMap::new(),
            requests: BTreeMap::new(),
            errors: Vec::new(),
            max_events,
            finished: false,
            failed: false,
            stop,
        }
    }

    /// One line of the file, numbered in the order it was written. A line that arrives after the file's end is dropped: the file
    /// ends with its footer. A line that cannot be written is a failure of the run, not a line that is lost.
    fn line(&mut self, mut value: Value) {
        if self.finished || self.failed {
            return;
        }
        self.seq += 1;
        value["schema"] = json!(SCHEMA);
        value["seq"] = json!(self.seq);
        if writeln!(self.out, "{value}").and_then(|()| self.out.flush()).is_err() {
            self.write_failed();
            return;
        }
        if self.max_events > 0 && self.seq >= self.max_events {
            self.stop.store(true, Ordering::SeqCst);
        }
    }

    /// The file cannot be written: the run fails and the proxy stops, so no traffic is observed without evidence.
    fn write_failed(&mut self) {
        self.failed = true;
        self.errors.push(WRITE_FAILED.to_string());
        self.stop.store(true, Ordering::SeqCst);
    }

    /// The label of a request id: the same id on the same connection keeps the same label, so a request and its answer are told
    /// apart, and two connections that name their requests alike are not told apart from each other.
    fn label(&mut self, connection: u32, raw: &str) -> String {
        let key = (connection, raw.to_string());
        if let Some(label) = self.requests.get(&key) {
            return label.clone();
        }
        let label = format!("r{}", self.requests.len() + 1);
        self.requests.insert(key, label.clone());
        label
    }

    pub(super) fn record(&mut self, event: Event) {
        match event {
            Event::HttpRequest { exchange, method, target, headers, body } => {
                self.http_requests += 1;
                let route = proxy_shape::route(&target);
                *self.routes.entry(route.clone()).or_default() += 1;
                let method = proxy_shape::verb(&method);
                *self.methods.entry(method.clone()).or_default() += 1;
                let content_type = header_value(&headers, "content-type");
                self.line(json!({
                    "kind": "http.request",
                    "exchange": format!("e{exchange}"),
                    "method": method,
                    "route": route,
                    "query": proxy_shape::query_names(&target),
                    "headers": proxy_shape::header_names(&headers),
                    "body": proxy_shape::body(&body, &content_type),
                }));
            }
            Event::HttpResponse { exchange, status, headers, body } => {
                self.http_responses += 1;
                let content_type = header_value(&headers, "content-type");
                self.line(json!({
                    "kind": "http.response",
                    "exchange": format!("e{exchange}"),
                    "status": status,
                    "headers": proxy_shape::header_names(&headers),
                    "body": proxy_shape::body(&body, &content_type),
                }));
            }
            Event::WsOpen { connection, target, status } => {
                self.connections += 1;
                self.line(json!({
                    "kind": "ws.open",
                    "connection": format!("c{connection}"),
                    "route": proxy_shape::route(&target),
                    "query": proxy_shape::query_names(&target),
                    "status": status,
                }));
            }
            Event::WsRefused { connection, target, status, body } => {
                self.line(json!({
                    "kind": "ws.refused",
                    "connection": format!("c{connection}"),
                    "route": proxy_shape::route(&target),
                    "query": proxy_shape::query_names(&target),
                    "status": status,
                    "body": proxy_shape::body(&body, "application/json"),
                }));
            }
            Event::WsFrame { connection, direction, opcode, data } => {
                self.frames += 1;
                for envelope in proxy_shape::envelopes(&data, opcode == OP_BINARY) {
                    if let Some(method) = &envelope.method {
                        *self.ws_methods.entry(method.clone()).or_default() += 1;
                    }
                    let mut line = Map::new();
                    line.insert("kind".into(), json!("ws.frame"));
                    line.insert("connection".into(), json!(format!("c{connection}")));
                    line.insert("direction".into(), json!(direction_name(direction)));
                    line.insert("envelope".into(), json!(envelope.name));
                    if let Some(method) = envelope.method {
                        line.insert("method".into(), json!(method));
                    }
                    if let Some(raw) = envelope.request {
                        line.insert("request".into(), json!(self.label(connection, &raw)));
                    }
                    line.insert("payload".into(), envelope.payload);
                    self.line(Value::Object(line));
                }
            }
            Event::WsClose { connection, direction, code } => {
                let mut line = Map::new();
                line.insert("kind".into(), json!("ws.close"));
                line.insert("connection".into(), json!(format!("c{connection}")));
                line.insert("direction".into(), direction.map(direction_name).map(Value::from).unwrap_or(Value::Null));
                line.insert("code".into(), code.map(|c| json!(c)).unwrap_or(Value::Null));
                line.insert("tcp".into(), json!(direction.is_none()));
                self.line(Value::Object(line));
            }
            Event::Failed { failure } => {
                self.errors.push(failure.label().to_string());
                self.line(json!({"kind": "error", "what": failure.label()}));
            }
        }
    }

    /// The header of the file: what the run was and where it went.
    pub(super) fn header(&mut self, upstream: &str, listen: SocketAddr) {
        let mut line = Map::new();
        line.insert("schema".into(), json!(SCHEMA));
        line.insert("kind".into(), json!("header"));
        line.insert("tool".into(), json!(format!("wf-mimic {}", env!("CARGO_PKG_VERSION"))));
        line.insert("started_at".into(), json!(wf_kit::time::iso(wf_kit::time::now_ms())));
        line.insert("upstream".into(), json!(upstream));
        line.insert("listen".into(), json!(listen.to_string()));
        if writeln!(self.out, "{}", Value::Object(line)).and_then(|()| self.out.flush()).is_err() {
            self.write_failed();
        }
    }

    /// The footer of the file: what the run saw.
    pub(super) fn footer(&mut self) {
        self.finished = true;
        let mut line = Map::new();
        line.insert("schema".into(), json!(SCHEMA));
        line.insert("kind".into(), json!("footer"));
        line.insert("lines".into(), json!(self.seq));
        line.insert("http".into(), self.http_counts());
        line.insert("ws".into(), self.ws_counts());
        line.insert("errors".into(), json!(self.errors.len()));
        if writeln!(self.out, "{}", Value::Object(line)).and_then(|()| self.out.flush()).is_err() && !self.failed {
            self.errors.push(WRITE_FAILED.to_string());
        }
    }

    fn http_counts(&self) -> Value {
        json!({
            "requests": self.http_requests,
            "responses": self.http_responses,
            "methods": self.methods.iter().map(|(k, v)| json!({"method": k, "count": v})).collect::<Vec<_>>(),
            "routes": self.routes.iter().map(|(k, v)| json!({"route": k, "count": v})).collect::<Vec<_>>(),
        })
    }

    fn ws_counts(&self) -> Value {
        json!({
            "connections": self.connections,
            "frames": self.frames,
            "methods": self.ws_methods.iter().map(|(k, v)| json!({"method": k, "count": v})).collect::<Vec<_>>(),
        })
    }
}

/// The name of a direction.
fn direction_name(direction: Dir) -> &'static str {
    match direction {
        Dir::C2s => "c2s",
        Dir::S2c => "s2c",
        _ => "other",
    }
}

/// Waits until the run is stopped or its duration is spent, then up to three seconds for the connections still open.
pub(super) fn serve_until(stop: &AtomicBool, duration: Option<u64>, live: &dyn Fn() -> u32) {
    let started = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        if duration.is_some_and(|limit| started.elapsed() >= Duration::from_secs(limit)) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    stop.store(true, Ordering::SeqCst);
    let deadline = Instant::now() + Duration::from_secs(3);
    while live() > 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Runs the proxy: it listens on the loopback, forwards to the one upstream and writes what crossed until it is stopped.
pub fn proxy_verb(p: &Parsed) -> Done {
    let Some(upstream) = p.val("upstream") else {
        return Err(fail("--upstream <url> is required", "wf mimic proxy --upstream http://127.0.0.1:<port> --out <file>"));
    };
    let Some(out) = p.val("out") else {
        return Err(fail("--out <file> is required", "wf mimic proxy --upstream http://127.0.0.1:<port> --out <file>"));
    };
    let upstream_addr = upstream_addr(upstream)?;
    let listen = listen_addr(p.val("listen").unwrap_or("127.0.0.1:0"))?;
    let duration = match p.val("duration") {
        Some(v) => Some(v.parse::<u64>().map_err(|_| fail("--duration is not a number of seconds", "wf mimic proxy --help"))?),
        None => None,
    };
    // zero means no limit, as it does for --max-events
    let duration = duration.filter(|limit| *limit > 0);
    let max_events = match p.val("max-events") {
        Some(v) => Some(v.parse::<u64>().map_err(|_| fail("--max-events is not a number", "wf mimic proxy --help"))?),
        None => None,
    };
    // The file is evidence of the run: it is never written over, and only its owner can read it.
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(out)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => fail(format!("{out} holds a run of its own"), "name another --out file: the proxy never writes over the evidence of a run"),
            _ => fail(format!("{out}: {e}"), "name a writable --out file"),
        })?;
    let stop = Arc::new(AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        let _ = signal_hook::flag::register(signal, stop.clone());
    }
    let recorder = Arc::new(Mutex::new(Recorder::new(file, max_events.unwrap_or(0), stop.clone())));
    let sink: Sink = {
        let recorder = recorder.clone();
        Arc::new(move |event| {
            let mut recorder = recorder.lock().unwrap_or_else(|e| e.into_inner());
            recorder.record(event);
        })
    };
    let proxy = Proxy::listen(listen, upstream_addr, sink, stop.clone())
        .map_err(|e| fail(format!("the proxy did not listen on {listen}: {e}"), "wf mimic proxy --listen 127.0.0.1:<port> --upstream http://127.0.0.1:<port> --out <file>"))?;
    let bound = proxy.addr();
    let public = upstream_public(upstream_addr);
    recorder.lock().unwrap_or_else(|e| e.into_inner()).header(&public, bound);
    if let Some(ready) = p.val("ready") {
        let note = json!({"listen": bound.to_string(), "upstream": public, "out": out});
        std::fs::write(ready, format!("{note}\n")).map_err(|e| fail(format!("{ready}: {e}"), "name a writable --ready file"))?;
    }
    eprintln!("mimic proxy: listening on {bound}, forwarding to {public}, writing {out}");
    serve_until(&stop, duration, &|| proxy.live());
    drop(proxy);
    let mut recorder = recorder.lock().unwrap_or_else(|e| e.into_inner());
    recorder.footer();
    let summary = json!({
        "schema": SCHEMA,
        "ok": recorder.errors.is_empty(),
        "upstream": public,
        "listen": bound.to_string(),
        "out": out,
        "lines": recorder.seq,
        "http": recorder.http_counts(),
        "ws": recorder.ws_counts(),
        "errors": recorder.errors,
        "warnings": [],
        "next": format!("jq -c 'select(.kind == \"http.request\" or .kind == \"ws.frame\")' \"{out}\""),
    });
    Ok((summary, i32::from(!recorder.errors.is_empty())))
}
