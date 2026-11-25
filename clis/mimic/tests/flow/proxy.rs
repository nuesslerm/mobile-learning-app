//! The `wf mimic proxy` verb through the thin binary: a local T3-shaped fixture (HTTP routes and an orchestration socket) is observed
//! through the proxy, and the file the proxy writes is what the test reads.
use crate::support::*;
use mimic_core::wire::{encode_frame, FrameParser, OP_CLOSE, OP_TEXT};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What the fixture saw of one request: the method, the target, the headers and the body, as they arrived.
#[derive(Clone, Debug)]
struct Seen {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Seen {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

/// The local T3-shaped fixture: the routes the phone uses and an orchestration socket that answers a request with a chunk and an exit.
struct Fixture {
    addr: SocketAddr,
    seen: Arc<Mutex<Vec<Seen>>>,
}

fn start_fixture() -> Fixture {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let addr = listener.local_addr().expect("its address");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (log, listener) = (seen.clone(), listener);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let log = log.clone();
            std::thread::spawn(move || serve(stream, log));
        }
    });
    Fixture { addr, seen }
}

impl Fixture {
    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

/// The head of a message and what came after it.
fn head_of(stream: &mut TcpStream) -> (String, Vec<u8>) {
    let (mut buf, mut chunk) = (Vec::new(), [0u8; 1024]);
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            return (String::from_utf8_lossy(&buf[..end]).into_owned(), buf[end + 4..].to_vec());
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return (String::new(), Vec::new()),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
}

/// One connection of the fixture: an ordinary exchange, or an orchestration socket.
fn serve(mut stream: TcpStream, log: Arc<Mutex<Vec<Seen>>>) {
    let (head, rest) = head_of(&mut stream);
    let mut lines = head.lines();
    let start = lines.next().unwrap_or("").to_string();
    let mut parts = start.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let headers: Vec<(String, String)> = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string())).collect();
    if headers.iter().any(|(k, v)| k == "upgrade" && v.eq_ignore_ascii_case("websocket")) {
        let _ = write!(stream, "HTTP/1.1 101 Switching Protocols\r\nupgrade: websocket\r\nconnection: Upgrade\r\nsec-websocket-accept: fixture\r\n\r\n");
        socket(stream, rest);
        return;
    }
    let length = headers.iter().find(|(k, _)| k == "content-length").and_then(|(_, v)| v.parse::<usize>().ok()).unwrap_or(0);
    let (mut body, mut chunk) = (rest, [0u8; 1024]);
    while body.len() < length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    let body = String::from_utf8_lossy(&body).into_owned();
    log.lock().unwrap().push(Seen { method: method.clone(), target: target.clone(), headers, body });
    let route = target.split('?').next().unwrap_or("");
    let answer = match (method.as_str(), route) {
        ("GET", "/health") => ("200 OK", r#"{"ok":true,"server":"fixture"}"#),
        ("GET", p) if p.starts_with("/api/orchestration/threads/") => ("200 OK", r#"{"history":[]}"#),
        ("POST", "/api/projects/mutate") => ("200 OK", r#"{"mutated":true}"#),
        _ => ("404 Not Found", r#"{"error":"no such route"}"#),
    };
    let _ = write!(stream, "HTTP/1.1 {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}", answer.0, answer.1.len(), answer.1);
}

/// The orchestration socket of the fixture: every request is answered with a chunk and an exit.
fn socket(mut stream: TcpStream, rest: Vec<u8>) {
    let (mut parser, mut chunk) = (FrameParser::default(), [0u8; 4096]);
    let mut pending = rest;
    loop {
        for message in parser.push(&pending) {
            if message.opcode == OP_CLOSE {
                let _ = stream.write_all(&encode_frame(OP_CLOSE, &message.payload, None));
                return;
            }
            let frame: Value = serde_json::from_slice(&message.payload).unwrap_or(Value::Null);
            let id = frame["id"].clone();
            let chunk_frame = json!({"_tag": "Chunk", "requestId": id, "values": [{"kind": "note", "text": "the server's own text"}]});
            let exit = json!({"_tag": "Exit", "requestId": id, "exit": {"_tag": "Success", "value": {"ok": true}}});
            if id == "split-request" {
                let text = chunk_frame.to_string();
                let mut first = vec![0x1e];
                first.extend_from_slice(&text.as_bytes()[..10]);
                stream.write_all(&encode_frame(OP_TEXT, &first, None)).unwrap();
                stream.write_all(&encode_frame(OP_TEXT, &text.as_bytes()[10..], None)).unwrap();
            } else {
                let _ = stream.write_all(&encode_frame(OP_TEXT, chunk_frame.to_string().as_bytes(), None));
            }
            let _ = stream.write_all(&encode_frame(OP_TEXT, exit.to_string().as_bytes(), None));
        }
        pending.clear();
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => pending.extend_from_slice(&chunk[..n]),
        }
    }
}

/// The proxy under test: the thin binary, or the full wf when the test runs through it.
struct Proxy {
    child: Option<Child>,
    addr: SocketAddr,
    out: PathBuf,
}

fn binary() -> PathBuf {
    std::env::var_os("WF_BIN").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_wf-mimic")))
}

impl Proxy {
    fn start(upstream: SocketAddr, dir: &Path) -> Proxy {
        let (out, ready) = (dir.join("proxy.jsonl"), dir.join("ready.json"));
        let beside = Path::new(env!("CARGO_BIN_EXE_wf-mimic")).parent().expect("a directory").display().to_string();
        let child = Command::new(binary())
            .arg("mimic")
            .arg("proxy")
            .arg("--upstream")
            .arg(format!("http://{upstream}"))
            .arg("--listen")
            .arg("127.0.0.1:0")
            .arg("--out")
            .arg(&out)
            .arg("--ready")
            .arg(&ready)
            .env("PATH", format!("{beside}:{}", std::env::var("PATH").unwrap_or_default()))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the proxy starts");
        let deadline = Instant::now() + Duration::from_secs(20);
        let addr = loop {
            if let Ok(text) = std::fs::read_to_string(&ready) {
                let listen: Value = serde_json::from_str(&text).expect("the ready file is JSON");
                break listen["listen"].as_str().expect("a listen address").parse().expect("an address");
            }
            assert!(Instant::now() < deadline, "the proxy did not come up");
            std::thread::sleep(Duration::from_millis(20));
        };
        Proxy { child: Some(child), addr, out }
    }

    /// Stops the proxy (a signal, as a person would) and reads the summary it printed.
    fn stop(&mut self) -> Value {
        let child = self.child.take().expect("the proxy runs");
        let _ = Command::new("kill").arg("-TERM").arg(child.id().to_string()).status();
        let out = child.wait_with_output().expect("the proxy ends");
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        let line = stdout.lines().rev().find(|l| l.starts_with('{')).unwrap_or("null");
        serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {stdout}"))
    }

    /// Waits until the file holds what the test needs, so a stopped proxy never races the traffic.
    fn wait_for(&self, want: impl Fn(&[Value]) -> bool) -> Vec<Value> {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let lines = self.lines();
            if want(&lines) {
                return lines;
            }
            assert!(Instant::now() < deadline, "the proxy wrote no such line: {lines:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The lines of the file the proxy wrote.
    fn lines(&self) -> Vec<Value> {
        std::fs::read_to_string(&self.out)
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}")))
            .collect()
    }
}

/// The lines of a kind, in the order they were written.
fn of_kind(lines: &[Value], kind: &str) -> Vec<Value> {
    lines.iter().filter(|l| l["kind"] == kind).cloned().collect()
}

/// One HTTP exchange through the proxy.
fn http(addr: SocketAddr, method: &str, target: &str, headers: &[(&str, &str)], body: &str) -> String {
    let mut stream = TcpStream::connect(addr).expect("the proxy is listening");
    let mut request = format!("{method} {target} HTTP/1.1\r\nhost: phone\r\n");
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str(&format!("content-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()));
    stream.write_all(request.as_bytes()).expect("the request goes out");
    let mut answer = String::new();
    stream.read_to_string(&mut answer).expect("the answer comes back");
    answer
}

/// A request whose answer may be cut short: the proxy stops when its file is full.
fn ask(addr: SocketAddr) {
    let Ok(mut stream) = TcpStream::connect(addr) else { return };
    let _ = stream.write_all(b"GET /health HTTP/1.1\r\nhost: phone\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
    let mut answer = String::new();
    let _ = stream.read_to_string(&mut answer);
}

/// A socket of the test: it sends masked frames and reads the server's.
struct Client {
    stream: TcpStream,
    parser: FrameParser,
    pending: Vec<u8>,
    frames: Vec<mimic_core::wire::WsMessage>,
    keys: u32,
}

impl Client {
    fn open(addr: SocketAddr, target: &str) -> Client {
        let mut stream = TcpStream::connect(addr).expect("the proxy is listening");
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let request = format!("GET {target} HTTP/1.1\r\nhost: phone\r\nupgrade: websocket\r\nconnection: Upgrade\r\nsec-websocket-key: {key}\r\nsec-websocket-version: 13\r\n\r\n");
        stream.write_all(request.as_bytes()).expect("the handshake goes out");
        let (head, rest) = head_of(&mut stream);
        assert!(head.starts_with("HTTP/1.1 101"), "{head}");
        Client { stream, parser: FrameParser::default(), pending: rest, frames: Vec::new(), keys: 0 }
    }

    fn send(&mut self, frame: &Value) {
        self.keys += 1;
        let key = self.keys.to_be_bytes();
        let wire = encode_frame(OP_TEXT, frame.to_string().as_bytes(), Some(key));
        self.stream.write_all(&wire).expect("the frame goes out");
    }

    /// The next frame the server sent, as JSON.
    fn read(&mut self) -> Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            for message in self.parser.push(&self.pending) {
                self.frames.push(message);
            }
            self.pending.clear();
            if let Some(message) = self.frames.first() {
                if message.opcode == OP_TEXT {
                    let payload = message.payload.clone();
                    self.frames.remove(0);
                    return serde_json::from_slice(&payload).expect("a JSON frame");
                }
            }
            assert!(Instant::now() < deadline, "no frame came");
            let mut chunk = [0u8; 4096];
            match self.stream.read(&mut chunk) {
                Ok(0) | Err(_) => panic!("the socket ended"),
                Ok(n) => self.pending.extend_from_slice(&chunk[..n]),
            }
        }
    }

    /// Closes the socket with a close frame, as a client does.
    fn close(&mut self) {
        let _ = self.stream.write_all(&encode_frame(OP_CLOSE, &1000u16.to_be_bytes(), Some([9, 9, 9, 9])));
    }
}

#[test]
fn the_proxy_forwards_the_credentials_the_app_sends_and_records_only_the_shape_of_what_crossed() {
    // Given a proxy in front of the local server
    let dir = tempfile::tempdir().unwrap();
    let fixture = start_fixture();
    let mut proxy = Proxy::start(fixture.addr, dir.path());
    // When the app asks for its health with a bearer token and mutates a project with a cookie and a body of its own
    let health = http(proxy.addr, "GET", "/health", &[("authorization", "Bearer the-app-token")], "");
    let mutate = http(
        proxy.addr,
        "POST",
        "/api/projects/mutate",
        &[("cookie", "t3_session=the-session-cookie"), ("content-type", "application/json")],
        r#"{"message":"the user's own text","limit":3}"#,
    );
    // Then the server got both, unchanged
    assert!(health.contains("200 OK"), "{health}");
    assert!(mutate.contains("200 OK"), "{mutate}");
    let seen = fixture.seen();
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert_eq!((seen[0].method.as_str(), seen[0].target.as_str()), ("GET", "/health"));
    assert_eq!(seen[0].header("authorization"), Some("Bearer the-app-token"));
    assert_eq!(seen[1].header("cookie"), Some("t3_session=the-session-cookie"));
    assert_eq!(seen[1].body, r#"{"message":"the user's own text","limit":3}"#);
    // And the file holds the shape of the exchange and none of the values
    let summary = proxy.stop();
    assert_eq!(summary["ok"], json!(true), "{summary}");
    let lines = proxy.lines();
    let requests = of_kind(&lines, "http.request");
    let responses = of_kind(&lines, "http.response");
    assert_eq!((requests.len(), responses.len()), (2, 2), "{lines:?}");
    assert_eq!(requests[0]["method"], json!("GET"));
    assert_eq!(requests[0]["route"], json!("/health"));
    assert_eq!(requests[1]["route"], json!("/api/projects/mutate"));
    assert_eq!(requests[0]["exchange"], responses[0]["exchange"]);
    assert_eq!(requests[1]["exchange"], responses[1]["exchange"]);
    // the body is a shape: the keys are named, the values are not
    let body = &requests[1]["body"];
    assert_eq!(body["kind"], json!("object"));
    assert_eq!(body["fields"]["message"], json!({"type": "string"}));
    assert_eq!(body["fields"]["limit"], json!({"type": "number"}));
    // and no value of the traffic is in the file
    let file = std::fs::read_to_string(&proxy.out).unwrap();
    for value in ["the-app-token", "the-session-cookie", "the user's own text", "Bearer", "t3_session"] {
        assert!(!file.contains(value), "{value} is in the file: {file}");
    }
}

#[test]
fn a_path_and_a_query_that_carry_a_value_are_recorded_as_the_route_and_the_names_only() {
    // Given a proxy in front of the local server
    let dir = tempfile::tempdir().unwrap();
    let fixture = start_fixture();
    let mut proxy = Proxy::start(fixture.addr, dir.path());
    // When the app asks for a thread's history with a ticket in the query and an id in the path
    http(proxy.addr, "GET", "/api/orchestration/threads/01JABCDEF123/history?wsTicket=the-ticket", &[], "");
    // Then the file names the route and the parameter, and holds neither the id nor the ticket
    proxy.stop();
    let lines = proxy.lines();
    let request = &of_kind(&lines, "http.request")[0];
    assert_eq!(request["route"], json!("/api/orchestration/threads/<seg>/history"));
    assert_eq!(request["query"], json!(["wsTicket"]));
    let file = std::fs::read_to_string(&proxy.out).unwrap();
    assert!(!file.contains("01JABCDEF123"), "{file}");
    assert!(!file.contains("the-ticket"), "{file}");
}

#[test]
fn a_request_that_closes_the_connection_and_names_no_length_is_answered() {
    // Given a proxy in front of the local server
    let dir = tempfile::tempdir().unwrap();
    let fixture = start_fixture();
    let mut proxy = Proxy::start(fixture.addr, dir.path());
    // When the app asks with a connection it will close and no length of its body
    let mut stream = TcpStream::connect(proxy.addr).expect("the proxy is listening");
    stream.write_all(b"GET /health HTTP/1.1\r\nhost: phone\r\nconnection: close\r\n\r\n").expect("the request goes out");
    let mut answer = String::new();
    stream.read_to_string(&mut answer).expect("the answer comes back");
    // Then the answer comes, and the request is recorded
    assert!(answer.contains("200 OK"), "{answer}");
    proxy.stop();
    let lines = proxy.lines();
    let request = &of_kind(&lines, "http.request")[0];
    assert_eq!(request["method"], json!("GET"));
    assert_eq!(request["route"], json!("/health"));
    assert_eq!(request["body"], json!({"kind": "empty"}));
}

#[test]
fn an_orchestration_socket_is_recorded_with_its_envelopes_and_the_request_that_answers_each() {
    // Given a proxy in front of the local server
    let dir = tempfile::tempdir().unwrap();
    let fixture = start_fixture();
    let mut proxy = Proxy::start(fixture.addr, dir.path());
    // When the app opens the socket and asks for two things
    let mut client = Client::open(proxy.addr, "/ws?wsTicket=the-ticket");
    client.send(&json!({"_tag": "Request", "id": "r-1", "tag": "orchestration.dispatchCommand", "payload": {"commandId": "c-1", "text": "the user's own text"}}));
    assert_eq!(client.read()["_tag"], json!("Chunk"));
    assert_eq!(client.read()["_tag"], json!("Exit"));
    client.send(&json!({"_tag": "Request", "id": "r-2", "tag": "orchestration.subscribeThread", "payload": {}}));
    assert_eq!(client.read()["_tag"], json!("Chunk"));
    client.close();
    drop(client);
    // Then the file holds the open, the envelopes with their methods, and the closes
    proxy.wait_for(|lines| of_kind(lines, "ws.close").iter().any(|c| c["code"] == json!(1000)));
    proxy.stop();
    let lines = proxy.lines();
    let open = &of_kind(&lines, "ws.open")[0];
    assert_eq!(open["route"], json!("/ws"));
    assert_eq!(open["query"], json!(["wsTicket"]));
    assert_eq!(open["status"], json!(101));
    let frames = of_kind(&lines, "ws.frame");
    let requests: Vec<&Value> = frames.iter().filter(|f| f["envelope"] == "Request").collect();
    assert_eq!(requests.len(), 2, "{frames:?}");
    assert_eq!(requests[0]["method"], json!("orchestration.dispatchCommand"));
    assert_eq!(requests[0]["direction"], json!("c2s"));
    assert_eq!(requests[0]["payload"]["fields"]["commandId"], json!({"type": "string"}));
    // the chunks and the exits answer the request they belong to
    let answers: Vec<&Value> = frames.iter().filter(|f| f["envelope"] == "Chunk" || f["envelope"] == "Exit").collect();
    assert_eq!(answers.len(), 4, "{frames:?}");
    assert_eq!(answers[0]["request"], requests[0]["request"]);
    assert_eq!(answers[0]["direction"], json!("s2c"));
    assert!(answers.iter().all(|a| a["request"].as_str().is_some()), "{answers:?}");
    // the two requests have two labels
    let labels: Vec<&Value> = requests.iter().map(|r| &r["request"]).collect();
    assert_ne!(labels[0], labels[1], "{labels:?}");
    // and the closes are there
    let closes = of_kind(&lines, "ws.close");
    assert!(closes.iter().any(|c| c["code"] == json!(1000)), "{closes:?}");
    // no value of the socket is in the file
    let file = std::fs::read_to_string(&proxy.out).unwrap();
    for value in ["the-ticket", "the user's own text", "the server's own text", "r-1", "r-2"] {
        assert!(!file.contains(value), "{value} is in the file: {file}");
    }
}

#[test]
fn a_socket_that_is_dropped_without_a_close_frame_is_recorded_as_a_drop() {
    // Given a proxy in front of the local server
    let dir = tempfile::tempdir().unwrap();
    let fixture = start_fixture();
    let mut proxy = Proxy::start(fixture.addr, dir.path());
    // When the app opens the socket, asks for something and drops the connection
    let mut client = Client::open(proxy.addr, "/ws");
    client.send(&json!({"_tag": "Request", "id": "r-1", "tag": "server.probe", "payload": {}}));
    assert_eq!(client.read()["_tag"], json!("Chunk"));
    drop(client);
    // Then the file holds the drop
    proxy.wait_for(|lines| of_kind(lines, "ws.close").iter().any(|c| c["tcp"] == json!(true)));
    proxy.stop();
    let lines = proxy.lines();
    let closes = of_kind(&lines, "ws.close");
    assert!(closes.iter().any(|c| c["tcp"] == json!(true)), "{closes:?}");
}

#[test]
fn two_requests_at_the_same_time_are_told_apart() {
    // Given a proxy in front of the local server
    let dir = tempfile::tempdir().unwrap();
    let fixture = start_fixture();
    let mut proxy = Proxy::start(fixture.addr, dir.path());
    // When the app asks for two things at once
    let addr = proxy.addr;
    let one = std::thread::spawn(move || http(addr, "GET", "/health", &[], ""));
    let two = std::thread::spawn(move || http(addr, "GET", "/api/orchestration/threads/one/history", &[], ""));
    one.join().unwrap();
    two.join().unwrap();
    // Then each request has its own answer
    proxy.stop();
    let lines = proxy.lines();
    let requests = of_kind(&lines, "http.request");
    let responses = of_kind(&lines, "http.response");
    assert_eq!((requests.len(), responses.len()), (2, 2), "{lines:?}");
    let mut exchanges: Vec<String> = requests.iter().map(|r| r["exchange"].as_str().unwrap_or_default().to_string()).collect();
    exchanges.sort();
    exchanges.dedup();
    assert_eq!(exchanges.len(), 2, "{requests:?}");
    for request in &requests {
        let answer = responses.iter().find(|r| r["exchange"] == request["exchange"]).expect("an answer");
        assert_eq!(answer["status"], json!(200));
    }
    // And the lines are numbered in the order they were written
    let seq: Vec<u64> = lines.iter().filter(|l| l["seq"].is_number()).map(|l| l["seq"].as_u64().unwrap()).collect();
    assert_eq!(seq, (1..=seq.len() as u64).collect::<Vec<_>>(), "{seq:?}");
}

#[test]
fn an_upstream_that_is_not_on_the_loopback_is_refused() {
    // Given no proxy at all
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("proxy.jsonl");
    // When the upstream is another machine
    let run = mimic(&["proxy", "--upstream", "http://10.0.0.1:8787", "--out", out.to_str().unwrap()], &[]);
    // Then the verb refuses it and writes nothing
    assert_eq!(run.code, 2, "{} {}", run.json, run.stderr);
    assert!(run.json["error"].as_str().unwrap_or_default().contains("loopback"), "{} {}", run.json, run.stderr);
    assert!(!out.exists(), "the file was written");
}

#[test]
fn an_upstream_that_carries_a_credential_of_its_own_is_refused_without_echoing_it() {
    // Given no proxy at all
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("proxy.jsonl");
    // When the upstream names a password and a secret of its own
    let run = mimic(&["proxy", "--upstream", "http://user:the-password-secret@127.0.0.1:not-a-port?token=the-query-secret", "--out", out.to_str().unwrap()], &[]);
    // Then the refusal names neither the credential nor the secret
    assert_eq!(run.code, 2, "{} {}", run.json, run.stderr);
    let said = format!("{} {}", run.json, run.stderr);
    for secret in ["the-password-secret", "the-query-secret", "user", "not-a-port"] {
        assert!(!said.contains(secret), "{secret} is in the refusal: {said}");
    }
    assert!(!out.exists(), "the file was written");
}

#[test]
fn a_name_the_traffic_chooses_is_masked_and_no_value_of_it_reaches_the_file() {
    // Given a proxy in front of the local server
    let dir = tempfile::tempdir().unwrap();
    let fixture = start_fixture();
    let mut proxy = Proxy::start(fixture.addr, dir.path());
    // When the app asks with a verb, a query name, a header name and a key of its own
    let answer = http(proxy.addr, "PROPFIND", "/api/projects?the-query-secret=1", &[("x-the-header-secret", "v"), ("content-type", "application/json")], r#"{"the-key-secret":"v"}"#);
    assert!(answer.contains("404 Not Found"), "{answer}");
    // And a socket frame with a tag, a method and a key of its own
    let mut client = Client::open(proxy.addr, "/ws");
    client.send(&json!({"_tag": "the-tag-secret", "id": "1", "tag": "the-method-secret", "payload": {"the-payload-secret": "v"}}));
    client.read();
    client.close();
    drop(client);
    // Then the file masks every name of its own
    proxy.stop();
    let lines = proxy.lines();
    let request = &of_kind(&lines, "http.request")[0];
    assert_eq!(request["method"], json!("<verb>"));
    assert_eq!(request["query"], json!(["<param>"]));
    assert!(request["headers"].as_array().unwrap().contains(&json!("<header>")), "{:?}", request["headers"]);
    assert_eq!(request["body"]["fields"]["<key>"], json!({"type": "string"}));
    let frame = of_kind(&lines, "ws.frame").into_iter().find(|f| f["envelope"] == "<tag>").expect("the frame");
    assert_eq!(frame["method"], json!("<method>"));
    // And no secret of the traffic is in the file
    let file = std::fs::read_to_string(&proxy.out).unwrap();
    for secret in ["the-query-secret", "the-header-secret", "the-key-secret", "the-tag-secret", "the-method-secret", "the-payload-secret"] {
        assert!(!file.contains(secret), "{secret} is in the file: {file}");
    }
}

#[test]
fn two_sockets_that_name_their_requests_alike_are_told_apart() {
    // Given a proxy in front of the local server
    let dir = tempfile::tempdir().unwrap();
    let fixture = start_fixture();
    let mut proxy = Proxy::start(fixture.addr, dir.path());
    // When two sockets each ask with the same request id
    let mut one = Client::open(proxy.addr, "/ws");
    let mut two = Client::open(proxy.addr, "/ws");
    one.send(&json!({"_tag": "Request", "id": "the-same-id", "tag": "orchestration.subscribeThread", "payload": {}}));
    two.send(&json!({"_tag": "Request", "id": "the-same-id", "tag": "orchestration.subscribeThread", "payload": {}}));
    one.read();
    two.read();
    // Then each request has a label of its own and the answer of a socket carries the label of its own request
    proxy.stop();
    let lines = proxy.lines();
    let frames = of_kind(&lines, "ws.frame");
    let labels: Vec<&Value> = frames.iter().filter(|f| f["envelope"] == "Request").map(|f| &f["request"]).collect();
    assert_eq!(labels.len(), 2, "{frames:?}");
    assert_ne!(labels[0], labels[1], "{labels:?}");
    for connection in ["c1", "c2"] {
        let request = frames.iter().find(|f| f["envelope"] == "Request" && f["connection"] == connection).expect("a request");
        let answer = frames.iter().find(|f| f["envelope"] == "Chunk" && f["connection"] == connection).expect("an answer");
        assert_eq!(answer["request"], request["request"], "{connection}");
    }
    // And the id itself is not in the file
    let file = std::fs::read_to_string(&proxy.out).unwrap();
    assert!(!file.contains("the-same-id"), "{file}");
}

#[test]
fn a_file_that_holds_a_run_of_its_own_is_never_written_over() {
    // Given a file a run has written
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("proxy.jsonl");
    let first = mimic(&["proxy", "--upstream", "http://127.0.0.1:1", "--out", out.to_str().unwrap(), "--duration", "1"], &[]);
    assert_eq!(first.code, 0, "{} {}", first.json, first.stderr);
    let written = std::fs::read_to_string(&out).unwrap();
    assert!(written.contains("\"kind\":\"footer\""), "{written}");
    // When a run is made over the same file
    let second = mimic(&["proxy", "--upstream", "http://127.0.0.1:1", "--out", out.to_str().unwrap(), "--duration", "1"], &[]);
    // Then the run refuses and the file still holds the first run
    assert_eq!(second.code, 2, "{} {}", second.json, second.stderr);
    assert_eq!(std::fs::read_to_string(&out).unwrap(), written);
    // And the file is written for its owner alone
    assert_eq!(std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&out).unwrap().permissions()) & 0o777, 0o600);
}

#[test]
fn a_run_whose_file_cannot_be_written_fails_instead_of_pretending_to_have_observed_the_traffic() {
    // Given a proxy whose file cannot grow past a kilobyte
    let dir = tempfile::tempdir().unwrap();
    let fixture = start_fixture();
    let (out, ready) = (dir.path().join("proxy.jsonl"), dir.path().join("ready.json"));
    let script = format!(
        "ulimit -f 1; trap '' XFSZ; exec '{}' mimic proxy --upstream 'http://{}' --listen 127.0.0.1:0 --out '{}' --ready '{}' --duration 5",
        binary().display(),
        fixture.addr,
        out.display(),
        ready.display()
    );
    let child = Command::new("sh")
        .arg("-c")
        .arg(&script)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the proxy starts");
    // When the app asks and the file is full
    let deadline = Instant::now() + Duration::from_secs(20);
    while !ready.exists() {
        assert!(Instant::now() < deadline, "the proxy did not come up");
        std::thread::sleep(Duration::from_millis(20));
    }
    let note: Value = serde_json::from_str(&std::fs::read_to_string(&ready).unwrap()).expect("the ready file is JSON");
    let addr: SocketAddr = note["listen"].as_str().expect("a listen address").parse().expect("an address");
    for _ in 0..5 {
        ask(addr);
    }
    // Then the run fails and says that the file could not be written
    let output = child.wait_with_output().expect("the proxy ends");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let line = stdout.lines().rev().find(|l| l.starts_with('{')).unwrap_or("null");
    let summary: Value = serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {stdout}"));
    assert_eq!(output.status.code(), Some(1), "{summary}");
    assert_eq!(summary["ok"], json!(false), "{summary}");
    assert!(summary["errors"].as_array().unwrap().iter().any(|e| e == "the file could not be written"), "{summary}");
}


#[test]
fn given_the_native_t3_frame_parts_option_when_a_reply_is_split_then_it_still_matches_its_request() {
    let fixture = start_fixture();
    let dir = tempfile::tempdir().unwrap();
    let mut proxy = Proxy::start(fixture.addr, dir.path());
    let mut phone = Client::open(proxy.addr, "/ws?frameParts=1");
    phone.send(&json!({"_tag":"Request","id":"split-request","tag":"server.getConfig","payload":{}}));
    proxy.wait_for(|lines| of_kind(lines, "ws.frame").iter().any(|f| f["envelope"] == "Exit"));
    phone.close();
    proxy.stop();
    let lines = proxy.lines();
    let frames = of_kind(&lines, "ws.frame");
    let reply = frames.iter().find(|f| f["envelope"] == "Chunk").unwrap();
    assert_eq!(reply["request"], frames[0]["request"]);
    assert!(frames.iter().all(|f| f["envelope"] != "text"));
}

#[test]
fn given_a_chunked_json_response_with_a_trailer_when_forwarded_then_the_phone_gets_the_wire_and_the_capture_gets_the_shape() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let wire = "4\r\n{\"ok\r\n7\r\n\":true}\r\n0\r\ntest-trailer: ok\r\n\r\n";
    let upstream = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 8192];
        assert!(stream.read(&mut buf).unwrap() > 0);
        stream.write_all(b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ntransfer-encoding: chunked\r\n\r\n").unwrap();
        for part in wire.as_bytes().chunks(5) { stream.write_all(part).unwrap(); }
    });
    let dir = tempfile::tempdir().unwrap();
    let mut proxy = Proxy::start(addr, dir.path());
    let result = http(proxy.addr, "GET", "/health", &[], "");
    assert!(result.ends_with(wire));
    upstream.join().unwrap();
    proxy.stop();
    let lines = proxy.lines();
    assert_eq!(of_kind(&lines, "http.response")[0]["body"]["fields"]["ok"]["type"], "boolean");
}
