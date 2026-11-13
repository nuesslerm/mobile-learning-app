//! The T3 candidate of a mimic replay: it runs the `script` of an interaction's input (HTTP calls and a WebSocket session, the client side
//! of the goldens imported into `cassettes/t3-v0.1.6`) against a server and reports the wire raw, the way the pinned-server driver of
//! `tooling/mimic-drivers` does; the set's rules mask it. `{credential}`, `{bad-credential}`, `{access_token}`, `{ticket}` and `{workspace}`
//! in the script are the values this run has, and the workspace folder is reported as `<workspace>`.
use super::*;

/// A candidate answers from memory on loopback: a frame that is coming arrives within this, so the recorded reference's longer quiet window is not waited out per case.
const LOCAL_QUIET_MS: u64 = 120;
/// How long a request waits for the first thing the server answers (a reply, an error or the close): the chain behind t3-serve is slower on a loaded
/// runner than the quiet window, and a request the real server always answers must not read as unanswered because the machine was busy.
const FIRST_ANSWER_MS: u64 = 3000;
use mimic_core::allowlist::Allowlist;
use mimic_core::cassette::{Cassette, Dir, DriverInfo, Step};
use mimic_core::differ::Verdict;
use mimic_core::ports::{CassetteSet, Driver, DriverError, Hello, Observed, Request};
use mimic_core::rules::RuleSet;
use mimic_core::wire::{raw_steps, WireEvent, OP_TEXT};
use std::collections::BTreeMap;
use std::path::Path;
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::protocol::CloseFrame;

pub const SET: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/cassettes/t3-v0.1.6");
pub const BAD_CREDENTIAL: &str = "NOTACREDENTIAL";

pub fn load_set(dir: &str) -> CassetteSet {
    let read = |name: &str| std::fs::read_to_string(Path::new(dir).join(name)).unwrap_or_else(|e| panic!("{dir}/{name}: {e}"));
    let cassette = Cassette::parse(&read("cassette.json"), &read("interactions.jsonl")).unwrap_or_else(|e| panic!("{dir}: {e}"));
    let rules: RuleSet = serde_json::from_str(&read("rules.json")).expect("rules.json is a rule set");
    CassetteSet { cassette, rules, corpus: None }
}

pub fn load_allowlist(dir: &str) -> Allowlist {
    Allowlist::parse(&std::fs::read_to_string(Path::new(dir).join("allowlist.json")).expect("the set has its allowlist")).expect("the allowlist parses")
}

/// The scopes the recorded session was granted: the `scope` of the first token exchange any interaction recorded.
pub fn granted_scopes(set: &CassetteSet) -> Vec<String> {
    let grant = set.cassette.interactions.iter().flat_map(|i| i.steps.iter()).find(|s| s.payload["body"].get("scope").is_some()).expect("a token exchange answered with its scope");
    grant.payload["body"]["scope"].as_str().expect("scope is a string").split(' ').map(String::from).collect()
}

/// What a replay found that the allowlist does not name, one line each.
pub fn differences(v: &Verdict) -> Vec<String> {
    let mut out: Vec<String> = v.results.iter().flat_map(|r| r.diffs.iter().filter(|d| d.severity != mimic_core::differ::Severity::None).map(move |d| format!("{} {}: {:?} {:?}", r.id, d.path, d.kind, d.severity))).collect();
    out.extend(v.results.iter().filter_map(|r| r.error.as_ref().map(|e| format!("{}: {e}", r.id))));
    out
}

pub struct Candidate<'a> {
    pub addr: SocketAddr,
    pub pair: &'a dyn Fn(Vec<String>) -> String,
    pub scopes: Vec<String>,
    /// A folder the server serves, which the recorded session makes its project on.
    pub workspace: &'a str,
}

struct Raw {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

fn urlencode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn dechunk(body: &str) -> String {
    let mut out = String::new();
    let mut rest = body;
    while let Some((size, tail)) = rest.split_once("\r\n") {
        let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
        if n == 0 || tail.len() < n {
            break;
        }
        out.push_str(&tail[..n]);
        rest = tail[n..].strip_prefix("\r\n").unwrap_or(&tail[n..]);
    }
    out
}

fn exchange(addr: SocketAddr, method: &str, path: &str, headers: &[(String, String)], body: &str) -> Result<Raw, String> {
    let mut stream = TcpStream::connect(addr).map_err(|e| e.to_string())?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: {}\r\n", body.len());
    for (k, v) in headers {
        request.push_str(&format!("{k}: {v}\r\n"));
    }
    stream.write_all(format!("{request}\r\n{body}").as_bytes()).map_err(|e| e.to_string())?;
    let mut raw = String::new();
    stream.read_to_string(&mut raw).map_err(|e| e.to_string())?;
    let (head, rest) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
    let status = head.split(' ').nth(1).and_then(|s| s.parse().ok()).ok_or("the answer has no status line")?;
    let headers: Vec<(String, String)> = head.lines().skip(1).filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect();
    let chunked = headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("transfer-encoding") && v.contains("chunked"));
    Ok(Raw { status, headers, body: if chunked { dechunk(rest) } else { rest.to_string() } })
}

fn fill(v: &Value, vars: &BTreeMap<&str, String>) -> Value {
    match v {
        Value::String(s) => Value::String(vars.iter().fold(s.clone(), |s, (k, val)| s.replace(&format!("{{{k}}}"), val))),
        Value::Array(a) => Value::Array(a.iter().map(|x| fill(x, vars)).collect()),
        Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (k.clone(), fill(x, vars))).collect()),
        other => other.clone(),
    }
}

fn body_text(body: &Value, content_type: &str) -> String {
    match body {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Object(m) if content_type.contains("x-www-form-urlencoded") => {
            m.iter().map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v.as_str().unwrap_or_default()))).collect::<Vec<_>>().join("&")
        }
        other => other.to_string(),
    }
}

type Socket = tungstenite::WebSocket<TcpStream>;

struct Session {
    ws: Option<Socket>,
    events: Vec<WireEvent>,
    settle: Duration,
}

impl Session {
    fn push(&mut self, dir: Dir, text: &str) {
        self.events.push(WireEvent::Ws { dir, opcode: OP_TEXT, data: text.as_bytes().to_vec() });
    }

    fn send(&mut self, text: &str) -> Result<(), String> {
        self.push(Dir::C2s, text);
        self.ws.as_mut().ok_or("no socket is open")?.send(Message::text(text)).map_err(|e| e.to_string())
    }

    /// Everything the server sends until it is quiet; a chunk is acknowledged as the recorded client did. With `first`, the wait for the first
    /// thing the server sends is that long instead (a request is always answered), and the quiet window `wait` applies after it.
    fn settle_with(&mut self, wait: Duration, first: Option<Duration>) -> Result<(), String> {
        let mut answered = first.is_none();
        loop {
            let Some(ws) = self.ws.as_mut() else { return Ok(()) };
            ws.get_ref().set_read_timeout(Some(if answered { wait } else { first.unwrap_or(wait) })).map_err(|e| e.to_string())?;
            match ws.read() {
                Ok(Message::Text(t)) => {
                    answered = true;
                    let text = t.as_str().to_string();
                    self.push(Dir::S2c, &text);
                    if let Ok(frame) = serde_json::from_str::<Value>(&text) {
                        if frame["_tag"] == "Chunk" {
                            self.send(&json!({"_tag": "Ack", "requestId": frame["requestId"]}).to_string())?;
                        }
                    }
                }
                Ok(Message::Close(frame)) => {
                    answered = true;
                    let (code, reason) = frame.map_or((None, String::new()), |f| (Some(u16::from(f.code)), f.reason.to_string()));
                    self.events.push(WireEvent::WsClose { dir: Some(Dir::S2c), code, reason });
                }
                Ok(_) => {}
                Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                    self.events.push(WireEvent::WsClose { dir: None, code: None, reason: String::new() });
                    self.ws = None;
                    return Ok(());
                }
                Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => return Ok(()),
                Err(e) => return Err(e.to_string()),
            }
        }
    }

    fn settle(&mut self, wait: Duration) -> Result<(), String> {
        self.settle_with(wait, None)
    }
}

fn replace_in(v: &mut Value, from: &str, to: &str) {
    match v {
        Value::String(s) => *s = s.replace(from, to),
        Value::Array(a) => a.iter_mut().for_each(|x| replace_in(x, from, to)),
        Value::Object(m) => m.values_mut().for_each(|x| replace_in(x, from, to)),
        _ => {}
    }
}

impl Candidate<'_> {
    fn script(&self, input: &Value) -> Result<Vec<Step>, String> {
        self.run_script(input, None).map(|(steps, _)| steps)
    }

    /// The script's steps and the access token its token exchange gave; `access_token` is the one a session already holds.
    fn run_script(&self, input: &Value, access_token: Option<&str>) -> Result<(Vec<Step>, Option<String>), String> {
        let script = input["script"].as_array().ok_or("the input has no script")?;
        let mut vars: BTreeMap<&str, String> = BTreeMap::from([("bad-credential", BAD_CREDENTIAL.to_string()), ("workspace", self.workspace.to_string())]);
        if let Some(t) = access_token {
            vars.insert("access_token", t.to_string());
        }
        if input["script"].to_string().contains("{credential}") {
            vars.insert("credential", (self.pair)(self.scopes.clone()));
        }
        let mut session = Session { ws: None, events: Vec::new(), settle: Duration::from_millis(input["settle_ms"].as_u64().unwrap_or(250).min(LOCAL_QUIET_MS)) };
        for step in script {
            let (op, arg) = step.as_object().and_then(|m| m.iter().next()).ok_or("a script step is not an object")?;
            let arg = fill(arg, &vars);
            match op.as_str() {
                "http" => {
                    let headers: Vec<(String, String)> = arg["headers"].as_object().map(|m| m.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string())).collect()).unwrap_or_default();
                    let content_type = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map_or("", |(_, v)| v.as_str());
                    let body = body_text(&arg["body"], content_type);
                    let (method, path) = (arg["method"].as_str().unwrap_or("GET"), arg["path"].as_str().ok_or("an http step has no path")?);
                    let got = exchange(self.addr, method, path, &headers, &body)?;
                    if let Ok(parsed) = serde_json::from_str::<Value>(&got.body) {
                        for (key, var) in [("access_token", "access_token"), ("ticket", "ticket")] {
                            if let Some(v) = parsed[key].as_str() {
                                vars.insert(var, v.to_string());
                            }
                        }
                    }
                    session.events.push(WireEvent::Http { method: method.into(), path: path.into(), req_headers: headers, req_body: body, status: got.status, res_headers: got.headers, res_body: got.body });
                }
                "ws-open" => {
                    let path = arg["path"].as_str().ok_or("ws-open has no path")?;
                    let stream = TcpStream::connect(self.addr).map_err(|e| e.to_string())?;
                    match tungstenite::client(format!("ws://{}{path}", self.addr), stream) {
                        Ok((ws, response)) => {
                            session.events.push(WireEvent::WsOpen { path: path.into(), status: response.status().as_u16() });
                            session.ws = Some(ws);
                        }
                        Err(tungstenite::HandshakeError::Failure(tungstenite::Error::Http(response))) => session.events.push(WireEvent::WsOpen { path: path.into(), status: response.status().as_u16() }),
                        Err(e) => return Err(format!("{e:?}")),
                    }
                }
                "ws-send" => {
                    session.send(&arg.to_string())?;
                    let first = (arg["_tag"] == "Request").then(|| Duration::from_millis(FIRST_ANSWER_MS));
                    session.settle_with(session.settle, first)?;
                }
                "ws-close" => {
                    let (code, reason) = (arg["code"].as_u64().unwrap_or(1000) as u16, arg["reason"].as_str().unwrap_or_default().to_string());
                    session.events.push(WireEvent::WsClose { dir: Some(Dir::C2s), code: Some(code), reason: reason.clone() });
                    if let Some(ws) = session.ws.as_mut() {
                        ws.close(Some(CloseFrame { code: CloseCode::from(code), reason: reason.into() })).map_err(|e| e.to_string())?;
                    }
                    session.settle(Duration::from_secs(2))?;
                }
                other => return Err(format!("unknown script step {other}")),
            }
        }
        let mut steps = raw_steps(&session.events);
        for s in &mut steps {
            replace_in(&mut s.payload, self.workspace, "<workspace>");
        }
        Ok((steps, vars.get("access_token").cloned()))
    }
}

impl Driver for Candidate<'_> {
    fn hello(&mut self) -> Result<Hello, DriverError> {
        Ok(Hello { driver: DriverInfo { name: "t3-candidate".into(), version: "0.1.0".into(), protocol: "mimic-driver/1".into(), toolchain_lock_hash: None }, target_version: None, coverage: false })
    }

    fn run(&mut self, request: &Request) -> Result<Observed, DriverError> {
        self.script(&request.input).map(|steps| Observed { steps, ..Observed::default() }).map_err(DriverError)
    }
}

/// What the recorder's `prelude: "session"` is: a pairing, a socket, a project and a thread made once per server, whose steps open every
/// interaction that asks for it, and a socket of its own for each interaction on that project and thread. The prelude's requests are the
/// ones the cassette recorded (ids `p1` and `p2`), so nothing here names a method.
#[allow(dead_code)] // the integration replay uses it, the flow binary shares this module
pub struct SharedSession<'a> {
    pub inner: Candidate<'a>,
    prelude: Vec<Value>,
    begun: Option<(Vec<Step>, String)>,
}

#[allow(dead_code)]
impl<'a> SharedSession<'a> {
    pub fn new(inner: Candidate<'a>, set: &CassetteSet) -> Self {
        let recorded = |id: &str| set.cassette.interactions.iter().flat_map(|i| i.steps.iter()).find(|s| s.dir == Dir::C2s && s.channel.as_deref() == Some("ws") && s.payload["id"] == id).unwrap_or_else(|| panic!("no recorded prelude request {id}")).payload.clone();
        let exchange = set.cassette.interactions.iter().flat_map(|i| i.steps.iter()).find(|s| s.payload["path"] == "/oauth/token").expect("a recorded token exchange").payload.clone();
        let mut form = exchange["body"].clone();
        form["subject_token"] = json!("{credential}");
        let mut prelude = vec![
            json!({"http": {"method": "POST", "path": "/oauth/token", "headers": exchange["headers"], "body": form}}),
            json!({"http": {"method": "POST", "path": "/api/auth/websocket-ticket/rpc", "headers": {"accept": "*/*", "authorization": "Bearer {access_token}"}, "body": null}}),
            json!({"ws-open": {"path": "/ws?orchestrationProtocol=2&wsTicket={ticket}"}}),
        ];
        prelude.extend(["p1", "p2"].map(|id| {
            let mut frame = recorded(id);
            replace_in(&mut frame, "<workspace>", "{workspace}");
            json!({"ws-send": frame})
        }));
        Self { inner, prelude, begun: None }
    }
}

impl Driver for SharedSession<'_> {
    fn hello(&mut self) -> Result<Hello, DriverError> {
        self.inner.hello()
    }

    fn run(&mut self, request: &Request) -> Result<Observed, DriverError> {
        if request.input["prelude"] != "session" {
            return self.inner.run(request);
        }
        let ws_close = json!({"ws-close": {}});
        if self.begun.is_none() {
            let script: Vec<Value> = self.prelude.iter().cloned().chain([ws_close]).collect();
            let (steps, token) = self.inner.run_script(&json!({"script": script, "settle_ms": 250}), None).map_err(DriverError)?;
            let cut = steps.iter().position(|s| s.channel.as_deref() == Some("ws:close")).unwrap_or(steps.len());
            self.begun = Some((steps[..cut].to_vec(), token.unwrap_or_default()));
        }
        let (begun, token) = self.begun.as_ref().expect("the prelude ran");
        let own: Vec<Value> = self.prelude[1..3].iter().cloned().chain(request.input["script"].as_array().cloned().unwrap_or_default()).collect();
        let input = json!({"script": own, "settle_ms": request.input["settle_ms"]});
        let (steps, _) = self.inner.run_script(&input, Some(token)).map_err(DriverError)?;
        let opened = steps.iter().position(|s| s.channel.as_deref() == Some("ws:open"));
        let mut all = begun.clone();
        all.extend(steps.into_iter().skip(opened.map_or(0, |i| i + 1)));
        Ok(Observed { steps: all, ..Observed::default() })
    }
}
