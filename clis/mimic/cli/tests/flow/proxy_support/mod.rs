//! The world of the record and replay tests: a T3-shaped upstream that hands out credentials, the verbs run as the thin binary,
//! and a client that makes the same journey a phone does (pair, ticket, session, thread, socket).
pub use serde_json::{json, Value};
use mimic_core::wire::{encode_frame, FrameParser, WsMessage, OP_CLOSE, OP_TEXT};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The credentials the upstream hands out. Each holds the word CANARY, so a test can scan for any of them at once.
pub const PAIRING: &str = "CANARY-PAIRING-GRANT-71f0c2";
pub const TOKEN: &str = "CANARY-ACCESS-TOKEN-9f3a71c2d4";
pub const COOKIE: &str = "CANARY-COOKIE-5b8e02aa17";
pub const TICKET: &str = "CANARY-WS-TICKET-c41d9930fe";
pub const SESSION: &str = "CANARY-SESSION-TOKEN-0d6a4be1";
pub const THREAD: &str = "thread-fixture-1";

pub struct Upstream {
    pub addr: SocketAddr,
    serving: Arc<AtomicBool>,
    after_stop: Arc<AtomicU32>,
    /// Whether any request asked the upstream for a compressed answer.
    pub asked_for_encoding: Arc<AtomicBool>,
    /// What the fixture answers `GET /api/extra` with: a header line and a body.
    pub extra: Arc<std::sync::Mutex<(String, Vec<u8>)>>,
    /// How the fixture words its pushed reply to a command, and the home directory its session names.
    pub wording: Arc<std::sync::Mutex<(String, String)>>,
}

#[derive(Clone)]
struct Knobs {
    extra: Arc<std::sync::Mutex<(String, Vec<u8>)>>,
    wording: Arc<std::sync::Mutex<(String, String)>>,
    asked_for_encoding: Arc<AtomicBool>,
}

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

impl Upstream {
    pub fn start() -> Upstream {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let addr = listener.local_addr().expect("its address");
        let (serving, after_stop) = (Arc::new(AtomicBool::new(true)), Arc::new(AtomicU32::new(0)));
        let extra = Arc::new(std::sync::Mutex::new((String::new(), b"{}".to_vec())));
        let asked_for_encoding = Arc::new(AtomicBool::new(false));
        let wording = Arc::new(std::sync::Mutex::new(("reply to: ".to_string(), std::env::var("HOME").unwrap_or_default())));
        let knobs = Knobs { extra: extra.clone(), wording: wording.clone(), asked_for_encoding: asked_for_encoding.clone() };
        let (on, late) = (serving.clone(), after_stop.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if !on.load(Ordering::SeqCst) {
                    late.fetch_add(1, Ordering::SeqCst);
                    continue;
                }
                let knobs = knobs.clone();
                std::thread::spawn(move || serve(stream, &knobs));
            }
        });
        Upstream { addr, serving, after_stop, asked_for_encoding, extra, wording }
    }

    /// The server is gone: whatever still reaches its address is counted and dropped.
    pub fn stop(&self) {
        self.serving.store(false, Ordering::SeqCst);
    }

    pub fn reached_after_stop(&self) -> u32 {
        self.after_stop.load(Ordering::SeqCst)
    }
}

fn serve(mut stream: TcpStream, knobs: &Knobs) {
    let (reply_wording, home) = knobs.wording.lock().unwrap().clone();
    let (head, rest) = head_of(&mut stream);
    let mut lines = head.lines();
    let start = lines.next().unwrap_or("").to_string();
    let mut parts = start.split_whitespace();
    let (method, target) = (parts.next().unwrap_or("GET").to_string(), parts.next().unwrap_or("/").to_string());
    let headers: Vec<(String, String)> = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string())).collect();
    let header = |name: &str| headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str()).unwrap_or_default();
    if !header("accept-encoding").is_empty() {
        knobs.asked_for_encoding.store(true, Ordering::SeqCst);
    }
    if header("upgrade").eq_ignore_ascii_case("websocket") {
        if !target.contains(&format!("wsTicket={TICKET}")) {
            let _ = write!(stream, "HTTP/1.1 401 Unauthorized\r\ncontent-type: application/json\r\ncontent-length: 21\r\nconnection: close\r\n\r\n{{\"error\":\"no ticket\"}}");
            return;
        }
        let _ = write!(stream, "HTTP/1.1 101 Switching Protocols\r\nupgrade: websocket\r\nconnection: Upgrade\r\nsec-websocket-accept: fixture\r\n\r\n");
        socket(stream, rest, &reply_wording);
        return;
    }
    let length = header("content-length").parse::<usize>().unwrap_or(0);
    let (mut body, mut chunk) = (rest, [0u8; 1024]);
    while body.len() < length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    let body = String::from_utf8_lossy(&body).into_owned();
    let authorised = header("authorization") == format!("Bearer {TOKEN}");
    let route = target.split('?').next().unwrap_or("");
    let ok = |text: String| ("200 OK", String::new(), text.into_bytes());
    let (status, more, answer) = match (method.as_str(), route) {
        ("POST", "/oauth/token") if body.contains(&format!("subject_token={PAIRING}")) => {
            ("200 OK", format!("set-cookie: sid={COOKIE}; Path=/; HttpOnly\r\n"), json!({"access_token": TOKEN, "token_type": "Bearer", "expires_in": 3600}).to_string().into_bytes())
        }
        ("POST", "/api/auth/websocket-ticket/rpc") if authorised => ok(json!({"ticket": TICKET}).to_string()),
        ("GET", "/api/auth/session") if authorised => ok(json!({"user": "fixture user", "sessionToken": SESSION, "home": home}).to_string()),
        ("GET", p) if authorised && p == format!("/api/orchestration/threads/{THREAD}") => ok(json!({"id": THREAD, "messages": messages()}).to_string()),
        ("GET", "/api/extra") => {
            let (line, bytes) = knobs.extra.lock().unwrap().clone();
            ("200 OK", line, bytes)
        }
        _ => ("401 Unauthorized", String::new(), br#"{"error":"not allowed"}"#.to_vec()),
    };
    let _ = write!(stream, "HTTP/1.1 {status}\r\ncontent-type: application/json\r\n{more}content-length: {}\r\nconnection: close\r\n\r\n", answer.len());
    let _ = stream.write_all(&answer);
}

pub fn messages() -> Value {
    json!([{"role": "user", "text": "hello fixture"}, {"role": "assistant", "text": "hello from the fixture agent"}])
}

/// The orchestration socket: a thread subscription stays open and gets a pushed chunk after every command.
fn socket(mut stream: TcpStream, rest: Vec<u8>, reply_wording: &str) {
    let (mut parser, mut chunk, mut pending) = (FrameParser::default(), [0u8; 4096], rest);
    let mut subscription: Option<Value> = None;
    let say = |stream: &mut TcpStream, frame: Value| {
        let _ = stream.write_all(&encode_frame(OP_TEXT, frame.to_string().as_bytes(), None));
    };
    loop {
        for message in parser.push(&pending) {
            if message.opcode == OP_CLOSE {
                let _ = stream.write_all(&encode_frame(OP_CLOSE, &message.payload, None));
                return;
            }
            let frame: Value = serde_json::from_slice(&message.payload).unwrap_or(Value::Null);
            let id = frame["id"].clone();
            match frame["tag"].as_str().unwrap_or_default() {
                "orchestration.subscribeThread" => {
                    say(&mut stream, json!({"_tag": "Chunk", "requestId": id, "values": [{"kind": "snapshot", "messages": messages()}]}));
                    subscription = Some(id);
                }
                "orchestration.dispatchCommand" => {
                    say(&mut stream, json!({"_tag": "Exit", "requestId": id, "exit": {"_tag": "Success", "value": {"accepted": true}}}));
                    if let Some(sub) = &subscription {
                        let text = format!("{reply_wording}{}", frame["payload"]["text"].as_str().unwrap_or_default());
                        say(&mut stream, json!({"_tag": "Chunk", "requestId": sub, "values": [{"kind": "message", "role": "assistant", "text": text}]}));
                    }
                }
                _ if frame["_tag"] == "Ping" => say(&mut stream, json!({"_tag": "Pong"})),
                _ => say(&mut stream, json!({"_tag": "Exit", "requestId": id, "exit": {"_tag": "Success", "value": {}}})),
            }
        }
        pending.clear();
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => pending.extend_from_slice(&chunk[..n]),
        }
    }
}

fn binary() -> PathBuf {
    std::env::var_os("WF_BIN").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_wf-mimic")))
}

/// What a verb left when it ended.
pub struct Ended {
    pub code: i32,
    pub json: Value,
    pub stdout: String,
    pub stderr: String,
}

/// A long-running `wf mimic proxy <verb>`: started, pointed at through its ready file, stopped with a signal.
pub struct Tool {
    child: Option<Child>,
    pub addr: SocketAddr,
}

impl Tool {
    pub fn start(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Tool {
        let ready = dir.join(format!("ready-{}.json", args[0]));
        let _ = std::fs::remove_file(&ready);
        let beside = Path::new(env!("CARGO_BIN_EXE_wf-mimic")).parent().expect("a directory").display().to_string();
        let mut command = Command::new(binary());
        command.arg("mimic").arg("proxy").args(args).arg("--listen").arg("127.0.0.1:0").arg("--ready").arg(&ready);
        command.env("PATH", format!("{beside}:{}", std::env::var("PATH").unwrap_or_default())).env_remove("WF_TEST_OUTPUT_DIR").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        for (name, value) in env {
            command.env(name, value);
        }
        let mut child = command.spawn().expect("the verb starts");
        let deadline = Instant::now() + Duration::from_secs(20);
        let addr = loop {
            if let Ok(text) = std::fs::read_to_string(&ready) {
                let listen: Value = serde_json::from_str(&text).expect("the ready file is JSON");
                break listen["listen"].as_str().expect("a listen address").parse().expect("an address");
            }
            if let Ok(Some(_)) = child.try_wait() {
                let out = child.wait_with_output().expect("the verb ended");
                panic!("the verb ended before it listened: {} {}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            }
            assert!(Instant::now() < deadline, "the verb did not come up");
            std::thread::sleep(Duration::from_millis(20));
        };
        Tool { child: Some(child), addr }
    }

    pub fn stop(&mut self) -> Ended {
        let child = self.child.take().expect("the verb runs");
        let _ = Command::new("kill").arg("-TERM").arg(child.id().to_string()).status();
        ended(child.wait_with_output().expect("the verb ends"))
    }
}

impl Drop for Tool {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn ended(out: std::process::Output) -> Ended {
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let line = stdout.lines().rev().find(|l| l.starts_with('{')).unwrap_or("null");
    let json = serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {stdout}"));
    Ended { code: out.status.code().unwrap_or(-1), json, stdout: stdout.clone(), stderr: String::from_utf8_lossy(&out.stderr).into_owned() }
}

/// A verb that ends by itself.
pub fn run(args: &[&str]) -> Ended {
    let beside = Path::new(env!("CARGO_BIN_EXE_wf-mimic")).parent().expect("a directory").display().to_string();
    let out = Command::new(binary()).arg("mimic").arg("proxy").args(args).env("PATH", format!("{beside}:{}", std::env::var("PATH").unwrap_or_default())).stdin(Stdio::null()).output().expect("the verb runs");
    ended(out)
}

/// One HTTP exchange: the status, the header lines and the body.
pub fn http(addr: SocketAddr, method: &str, target: &str, headers: &[(&str, &str)], body: &str) -> (u16, String, String) {
    let mut stream = TcpStream::connect(addr).expect("the address is listening");
    let mut request = format!("{method} {target} HTTP/1.1\r\nhost: phone\r\n");
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str(&format!("content-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()));
    stream.write_all(request.as_bytes()).expect("the request goes out");
    let mut answer = Vec::new();
    let _ = stream.read_to_end(&mut answer);
    let answer = String::from_utf8_lossy(&answer).into_owned();
    let (head, body) = answer.split_once("\r\n\r\n").unwrap_or((answer.as_str(), ""));
    let status = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    (status, head.to_string(), body.to_string())
}

/// A socket of the test: it sends masked frames and reads the server's.
pub struct Client {
    stream: TcpStream,
    parser: FrameParser,
    pending: Vec<u8>,
    frames: Vec<WsMessage>,
}

impl Client {
    /// The socket, or the status the server refused it with.
    pub fn open(addr: SocketAddr, target: &str) -> Result<Client, u16> {
        let mut stream = TcpStream::connect(addr).expect("the address is listening");
        let request = format!("GET {target} HTTP/1.1\r\nhost: phone\r\nupgrade: websocket\r\nconnection: Upgrade\r\nsec-websocket-key: dGhlIHNhbXBsZSBub25jZQ==\r\nsec-websocket-version: 13\r\n\r\n");
        stream.write_all(request.as_bytes()).expect("the handshake goes out");
        let (head, rest) = head_of(&mut stream);
        if !head.starts_with("HTTP/1.1 101") {
            return Err(head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0));
        }
        stream.set_read_timeout(Some(Duration::from_millis(100))).expect("a timeout");
        Ok(Client { stream, parser: FrameParser::default(), pending: rest, frames: Vec::new() })
    }

    pub fn send(&mut self, frame: &Value) {
        self.stream.write_all(&encode_frame(OP_TEXT, frame.to_string().as_bytes(), Some([7, 1, 8, 2]))).expect("the frame goes out");
    }

    fn next(&mut self, wait: Duration) -> Option<WsMessage> {
        let deadline = Instant::now() + wait;
        loop {
            let pending = std::mem::take(&mut self.pending);
            self.frames.extend(self.parser.push(&pending));
            if !self.frames.is_empty() {
                return Some(self.frames.remove(0));
            }
            if Instant::now() >= deadline {
                return None;
            }
            let mut chunk = [0u8; 4096];
            match self.stream.read(&mut chunk) {
                Ok(0) => return None,
                Ok(n) => self.pending.extend_from_slice(&chunk[..n]),
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(_) => return None,
            }
        }
    }

    /// The next frame the server sent, as JSON.
    pub fn read(&mut self) -> Value {
        let message = self.next(Duration::from_secs(10)).expect("a frame");
        assert_eq!(message.opcode, OP_TEXT, "a text frame");
        serde_json::from_slice(&message.payload).expect("a JSON frame")
    }

    /// The close code the server ends the socket with, if it does so soon.
    pub fn closed_with(&mut self) -> Option<u16> {
        let message = self.next(Duration::from_secs(5))?;
        (message.opcode == OP_CLOSE && message.payload.len() >= 2).then(|| u16::from_be_bytes([message.payload[0], message.payload[1]]))
    }

    pub fn close(&mut self) {
        let _ = self.stream.write_all(&encode_frame(OP_CLOSE, &1000u16.to_be_bytes(), Some([9, 9, 9, 9])));
    }
}

/// What a client saw on the journey: every answer and every frame, in order.
#[derive(Debug, PartialEq)]
pub struct Seen {
    pub token: String,
    pub ticket: String,
    pub session: Value,
    pub thread: Value,
    pub frames: Vec<Value>,
}

fn field(body: &str, name: &str) -> String {
    serde_json::from_str::<Value>(body).ok().and_then(|v| v[name].as_str().map(String::from)).unwrap_or_default()
}

/// The journey of a phone: redeem the pairing grant, buy a ticket with the token, read the session and the thread, open the
/// socket, subscribe to the thread and send one command. `ids` are the request ids and the command id this run makes up.
pub fn journey(addr: SocketAddr, ids: [&str; 3]) -> Seen {
    let (_, _, paired) = http(addr, "POST", "/oauth/token", &[("content-type", "application/x-www-form-urlencoded")], &format!("grant_type=pairing&subject_token={PAIRING}"));
    let token = field(&paired, "access_token");
    let bearer = format!("Bearer {token}");
    let auth = [("authorization", bearer.as_str())];
    let (_, _, bought) = http(addr, "POST", "/api/auth/websocket-ticket/rpc", &auth, "");
    let ticket = field(&bought, "ticket");
    let (_, _, session) = http(addr, "GET", "/api/auth/session", &auth, "");
    let (_, _, thread) = http(addr, "GET", &format!("/api/orchestration/threads/{THREAD}"), &auth, "");
    let mut socket = Client::open(addr, &format!("/ws?wsTicket={ticket}")).expect("the socket opens");
    socket.send(&json!({"_tag": "Request", "id": ids[0], "tag": "orchestration.subscribeThread", "payload": {"threadId": THREAD}, "traceId": format!("trace-{}", ids[0])}));
    let mut frames = vec![socket.read()];
    socket.send(&json!({"_tag": "Request", "id": ids[1], "tag": "orchestration.dispatchCommand", "payload": {"commandId": ids[2], "text": "say hello"}, "traceId": format!("trace-{}", ids[1])}));
    frames.push(socket.read());
    frames.push(socket.read());
    socket.close();
    let parsed = |text: &str| serde_json::from_str(text).unwrap_or(Value::Null);
    Seen { token, ticket, session: parsed(&session), thread: parsed(&thread), frames }
}

/// Every file under `dir`, as text.
pub fn files(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(files(&path));
        } else {
            out.push((path.clone(), String::from_utf8_lossy(&std::fs::read(&path).unwrap_or_default()).into_owned()));
        }
    }
    out
}
