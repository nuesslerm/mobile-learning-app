//! `opencode serve` as the scripted CLI plays it: the HTTP subset the executor's bridge uses (health, the `/global/event` SSE stream, sessions, `prompt_async`,
//! `abort`, the permission and question replies), under HTTP Basic auth with the password the executor put in `OPENCODE_SERVER_PASSWORD`. A prompt plays the
//! next turn of the script, whose lines are the `opencode run --format json` ones the older scripts hold, as the events the real server sends for them. A
//! script's tool calls run the CLI's own tools, `ask_user` is a `question.asked`, and `approval_prompt` a `permission.asked`, each waiting for the reply.
//! Sessions are kept in a file beside the program, so a server started again knows the sessions of the one before it, as the real one's database does.
//! The requests it received (never the credential) and the password of the start are written for the test to read. Authored double, not a recording.
use crate::script::{Script, Step, Turn};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const SERVER_ARGS: &str = "serve";

pub fn is_serve(args: &[String]) -> bool {
    args.first().map(String::as_str) == Some(SERVER_ARGS)
}

/// The log of the requests the server received, one JSON object a line: `{"start", "method", "path", "body", "authorized"}`.
pub fn http_log_file(program: &str) -> String {
    format!("{program}.http.jsonl")
}

/// The password the last start of the server was given, so a test can look for it where it must never be.
pub fn password_file(program: &str) -> String {
    format!("{program}.password")
}

fn sessions_file(program: &str) -> String {
    format!("{program}.sessions.json")
}

fn turns_file(program: &str) -> String {
    format!("{program}.serve-turns")
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

enum Answer {
    Reply(Value),
    Abort,
}

#[derive(Default)]
struct Session {
    directory: String,
    busy: bool,
    abort: bool,
    users: u64,
}

struct Server {
    script: Script,
    dir: PathBuf,
    program: String,
    start: usize,
    password: String,
    subscribers: Mutex<Vec<TcpStream>>,
    sessions: Mutex<BTreeMap<String, Session>>,
    waiting: Mutex<HashMap<String, Sender<Answer>>>,
    counter: Mutex<u64>,
}

impl Server {
    fn id(&self, prefix: &str) -> String {
        let mut counter = self.counter.lock().unwrap();
        *counter += 1;
        format!("{prefix}_s{}{:03}", self.start, *counter)
    }

    fn emit(&self, directory: &str, kind: &str, properties: Value) {
        let frame = json!({"directory": directory, "project": "global", "payload": {"id": self.id("evt"), "type": kind, "properties": properties}});
        let chunk = format!("data: {frame}\n\n");
        let framed = format!("{:x}\r\n{chunk}\r\n", chunk.len());
        self.subscribers.lock().unwrap().retain_mut(|stream| stream.write_all(framed.as_bytes()).and_then(|()| stream.flush()).is_ok());
    }

    fn save(&self) {
        let listed: BTreeMap<String, String> = self.sessions.lock().unwrap().iter().map(|(id, s)| (id.clone(), s.directory.clone())).collect();
        let _ = std::fs::write(self.dir.join(sessions_file(&self.program)), serde_json::to_string(&listed).unwrap_or_default());
    }

    fn log(&self, method: &str, path: &str, body: &Value, authorized: bool) {
        let line = json!({"start": self.start, "method": method, "path": path, "body": body, "authorized": authorized});
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(self.dir.join(http_log_file(&self.program))) {
            let _ = writeln!(file, "{line}");
        }
    }

    /// The turns played by every start so far, so a restarted server plays the script's next turn.
    fn next_turn(&self) -> Turn {
        let path = self.dir.join(turns_file(&self.program));
        let played = std::fs::read_to_string(&path).map_or(0, |t| t.lines().count());
        let elsewhere = std::fs::read_to_string(self.dir.join(crate::script::log_file(&self.program))).map_or(0, |t| t.lines().filter_map(|l| serde_json::from_str::<crate::script::Invocation>(l).ok()).filter(|i| !is_serve(&i.argv)).count());
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = writeln!(file, "{}", std::process::id());
        }
        self.script.turn_for(elsewhere + played)
    }

    fn directory(&self, session: &str) -> Option<String> {
        self.sessions.lock().unwrap().get(session).map(|s| s.directory.clone())
    }

    fn aborted(&self, session: &str) -> bool {
        self.sessions.lock().unwrap().get(session).is_some_and(|s| s.abort)
    }

    /// Waits for the reply of the request `id`, or the abort of the session; a reply that never comes ends with the process.
    fn wait_for(&self, id: &str, session: &str) -> Answer {
        let (tx, rx) = channel();
        self.waiting.lock().unwrap().insert(id.to_string(), tx);
        loop {
            if let Ok(answer) = rx.recv_timeout(Duration::from_millis(20)) {
                return answer;
            }
            if self.aborted(session) {
                self.waiting.lock().unwrap().remove(id);
                return Answer::Abort;
            }
        }
    }

    fn user_message(&self, session: &str, directory: &str, text: &str) -> String {
        let message = self.id("msg");
        self.emit(directory, "message.updated", json!({"sessionID": session, "info": {"id": message, "sessionID": session, "role": "user", "time": {"created": now_ms()}, "agent": "build", "model": {"providerID": "scripted", "modelID": "scripted"}}}));
        self.emit(directory, "message.part.updated", json!({"sessionID": session, "part": {"id": self.id("prt"), "sessionID": session, "messageID": message, "type": "text", "text": text}, "time": now_ms()}));
        message
    }

    fn status(&self, session: &str, directory: &str, kind: &str) {
        self.emit(directory, "session.status", json!({"sessionID": session, "status": {"type": kind}}));
    }

    fn idle(&self, session: &str, directory: &str) {
        if let Some(s) = self.sessions.lock().unwrap().get_mut(session) {
            s.busy = false;
            s.abort = false;
        }
        self.status(session, directory, "idle");
        self.emit(directory, "session.idle", json!({"sessionID": session}));
    }

    fn part(&self, session: &str, directory: &str, message: &str, id: &str, body: Value) {
        let mut part = json!({"id": id, "sessionID": session, "messageID": message});
        for (k, v) in body.as_object().cloned().unwrap_or_default() {
            part[k] = v;
        }
        self.emit(directory, "message.part.updated", json!({"sessionID": session, "part": part, "time": now_ms()}));
    }

    fn text(&self, session: &str, directory: &str, message: &str, text: &str) {
        let id = self.id("prt");
        self.part(session, directory, message, &id, json!({"type": "text", "text": "", "time": {"start": now_ms()}}));
        self.emit(directory, "message.part.delta", json!({"sessionID": session, "messageID": message, "partID": id, "field": "text", "delta": text}));
        self.part(session, directory, message, &id, json!({"type": "text", "text": text, "time": {"start": now_ms(), "end": now_ms()}}));
    }

    fn tool(&self, at: (&str, &str, &str), call: &str, tool: &str, running_input: &Value, done: Option<Value>) {
        let (session, directory, message) = at;
        let id = self.id("prt");
        let part = |state: Value| json!({"type": "tool", "callID": call, "tool": tool, "state": state});
        self.part(session, directory, message, &id, part(json!({"status": "running", "input": running_input, "time": {"start": now_ms()}})));
        if let Some(mut done) = done {
            done["input"] = running_input.clone();
            done["time"] = json!({"start": now_ms(), "end": now_ms()});
            self.part(session, directory, message, &id, part(done));
        }
    }

    /// One step of the script, as the events of the turn it stands for; false when the turn ends here (an error, an abort).
    fn play_step(&self, step: &Step, session: &str, directory: &str, message: &str) -> bool {
        match step {
            Step::Sleep { ms } => {
                let until = Instant::now() + Duration::from_millis(*ms);
                while Instant::now() < until && !self.aborted(session) {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            Step::Line { text } => {
                let Ok(line) = serde_json::from_str::<Value>(text) else { return true };
                match line["type"].as_str() {
                    Some("text") => self.text(session, directory, message, line["part"]["text"].as_str().unwrap_or_default()),
                    Some("tool_use") => {
                        let part = &line["part"];
                        let state = &part["state"];
                        let done = matches!(state["status"].as_str(), Some("completed" | "error")).then(|| state.clone());
                        self.tool((session, directory, message), part["callID"].as_str().unwrap_or_default(), part["tool"].as_str().unwrap_or_default(), &state["input"], done);
                    }
                    Some("error") => {
                        let error = &line["error"];
                        let name = error["name"].as_str().unwrap_or("UnknownError");
                        self.emit(directory, "session.error", json!({"sessionID": session, "error": {"name": name, "data": {"message": error["data"]["message"].as_str().unwrap_or(name)}}}));
                        return false;
                    }
                    _ => {}
                }
            }
            Step::McpCall { id, tool, args } => return self.call(session, directory, message, id, tool, args),
            Step::ReadLine | Step::Credential { .. } => {}
        }
        !self.aborted(session)
    }

    fn call(&self, session: &str, directory: &str, message: &str, call: &str, tool: &str, args: &Value) -> bool {
        let name = format!("executor_{tool}");
        match tool {
            "ask_user" => {
                let request = self.id("que");
                let options: Vec<Value> = args["options"].as_array().cloned().unwrap_or_default().into_iter().map(|o| json!({"label": o, "description": ""})).collect();
                self.emit(directory, "question.asked", json!({"id": request, "sessionID": session, "questions": [{"question": args["question"], "header": "Question", "options": options}], "tool": {"messageID": message, "callID": call}}));
                self.tool((session, directory, message), call, "question", args, None);
                match self.wait_for(&request, session) {
                    Answer::Abort => return false,
                    Answer::Reply(reply) => {
                        let text = match reply["answers"][0][0].as_str() {
                            Some(answer) => format!("User answered: {answer}"),
                            None => "The user dismissed the question".to_string(),
                        };
                        self.emit(directory, "question.replied", json!({"sessionID": session, "requestID": request, "answers": reply["answers"]}));
                        self.tool((session, directory, message), call, "question", args, Some(json!({"status": "completed", "output": text, "title": "", "metadata": {}})));
                    }
                }
            }
            "approval_prompt" => {
                let request = self.id("per");
                let (permission, input) = (args["tool_name"].as_str().unwrap_or("bash"), args["input"].clone());
                self.emit(directory, "permission.asked", json!({"id": request, "sessionID": session, "permission": permission, "patterns": [input.to_string()], "metadata": input, "always": [permission], "tool": {"messageID": message, "callID": call}}));
                match self.wait_for(&request, session) {
                    Answer::Abort => return false,
                    Answer::Reply(reply) => {
                        let verdict = reply["reply"].as_str().unwrap_or("reject").to_string();
                        self.emit(directory, "permission.replied", json!({"sessionID": session, "requestID": request, "reply": verdict}));
                        let done = if verdict == "reject" { json!({"status": "error", "error": "The user rejected this tool call"}) } else { json!({"status": "completed", "output": format!("allowed ({verdict})"), "title": "", "metadata": {}}) };
                        self.tool((session, directory, message), call, permission, &args["input"], Some(done));
                    }
                }
            }
            _ => {
                let done = match own_tool(tool, args, directory) {
                    (text, false) => json!({"status": "completed", "output": text, "title": "", "metadata": {}}),
                    (text, true) => json!({"status": "error", "error": text}),
                };
                self.tool((session, directory, message), call, &name, args, Some(done));
            }
        }
        !self.aborted(session)
    }

    /// One turn of a session, from the user's message to idle: the events of the script's steps, then the end the turn has.
    fn play(&self, session: &str, directory: &str) {
        let turn = self.next_turn();
        self.status(session, directory, "busy");
        let message = self.id("msg");
        let assistant = |done: bool| {
            let mut info = json!({"id": message, "sessionID": session, "role": "assistant", "parentID": "msg_user", "modelID": "scripted", "providerID": "scripted", "mode": "build", "agent": "build", "path": {"cwd": directory, "root": directory}, "cost": 0, "tokens": {"total": 15, "input": 10, "output": 5, "reasoning": 0, "cache": {"read": 0, "write": 0}}, "time": {"created": now_ms()}});
            if done {
                info["time"]["completed"] = json!(now_ms());
                info["finish"] = json!("stop");
            }
            json!({"sessionID": session, "info": info})
        };
        self.emit(directory, "message.updated", assistant(false));
        self.part(session, directory, &message, &self.id("prt"), json!({"type": "step-start"}));
        let mut ended = true;
        for step in &turn.steps {
            if !self.play_step(step, session, directory, &message) {
                ended = false;
                break;
            }
        }
        if turn.exit_code != 0 {
            if !turn.stderr.is_empty() {
                eprintln!("{}", turn.stderr);
            }
            std::process::exit(turn.exit_code);
        }
        if turn.hang && ended {
            while !self.aborted(session) {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        if self.aborted(session) {
            self.emit(directory, "session.error", json!({"sessionID": session, "error": {"name": "MessageAbortedError", "data": {"message": "The operation was aborted."}}}));
        } else if ended {
            self.part(session, directory, &message, &self.id("prt"), json!({"type": "step-finish", "reason": "stop", "cost": 0, "tokens": {"total": 15, "input": 10, "output": 5, "reasoning": 0, "cache": {"read": 0, "write": 0}}}));
            self.emit(directory, "message.updated", assistant(true));
        }
        self.idle(session, directory);
    }
}

/// What the CLI's own file and shell tools do in the directory the session works in, as `(text, is_error)`.
fn own_tool(tool: &str, input: &Value, directory: &str) -> (String, bool) {
    let path = Path::new(directory).join(input["path"].as_str().unwrap_or_default());
    match tool {
        "write_file" => {
            let content = input["content"].as_str().unwrap_or_default();
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match std::fs::write(&path, content) {
                Ok(()) => (format!("wrote {} bytes", content.len()), false),
                Err(e) => (format!("{}: {e}", path.display()), true),
            }
        }
        "read_file" => match std::fs::read_to_string(&path) {
            Ok(content) => (content, false),
            Err(e) => (format!("{}: {e}", path.display()), true),
        },
        "run_command" => match std::process::Command::new("sh").arg("-c").arg(input["command"].as_str().unwrap_or_default()).current_dir(directory).output() {
            Ok(out) => {
                let (mut text, code) = (String::from_utf8_lossy(&out.stdout).into_owned(), out.status.code().unwrap_or(-1));
                if code != 0 {
                    text.push_str(&format!("\n[exit code {code}]"));
                }
                (text, code != 0)
            }
            Err(e) => (e.to_string(), true),
        },
        other => (format!("no tool {other}"), true),
    }
}

fn query_of(path: &str, name: &str) -> Option<String> {
    let query = path.split_once('?')?.1;
    let raw = query.split('&').find_map(|pair| pair.strip_prefix(&format!("{name}=")))?;
    let (mut out, bytes) = (Vec::new(), raw.as_bytes());
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'%' if at + 2 < bytes.len() && u8::from_str_radix(&raw[at + 1..at + 3], 16).is_ok() => {
                out.push(u8::from_str_radix(&raw[at + 1..at + 3], 16).unwrap_or(b'?'));
                at += 3;
            }
            byte => {
                out.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

fn respond(stream: &mut TcpStream, status: u16, body: &Value) {
    let text = if body.is_null() { String::new() } else { body.to_string() };
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        401 => "Unauthorized",
        404 => "Not Found",
        409 => "Conflict",
        _ => "Bad Request",
    };
    let head = format!("HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", text.len());
    let _ = stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(text.as_bytes()));
}

fn not_found(stream: &mut TcpStream, what: &str, id: &str) {
    respond(stream, 404, &json!({"name": "NotFoundError", "data": {"message": format!("{what} not found: {id}")}}));
}

fn handle(server: &Arc<Server>, mut stream: TcpStream) {
    let mut reader = BufReader::new(stream.try_clone().expect("a stream handle"));
    let mut request = String::new();
    if reader.read_line(&mut request).unwrap_or(0) == 0 {
        return;
    }
    let (mut length, mut authorization) = (0usize, String::new());
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).unwrap_or(0) == 0 {
            return;
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            match name.to_ascii_lowercase().as_str() {
                "content-length" => length = value.trim().parse().unwrap_or(0),
                "authorization" => authorization = value.trim().to_string(),
                _ => {}
            }
        }
    }
    let mut raw = vec![0; length];
    let _ = reader.read_exact(&mut raw);
    let body: Value = serde_json::from_slice(&raw).unwrap_or(Value::Null);
    let mut words = request.split_whitespace();
    let (method, path) = (words.next().unwrap_or_default().to_string(), words.next().unwrap_or_default().to_string());
    let expected = format!("Basic {}", base64(&format!("rca:{}", server.password)));
    let authorized = authorization == expected;
    server.log(&method, &path, &body, authorized);
    if !authorized {
        let _ = stream.write_all(b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"Secure Area\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return;
    }
    let route = path.split('?').next().unwrap_or_default();
    let parts: Vec<&str> = route.trim_matches('/').split('/').collect();
    let directory = query_of(&path, "directory").unwrap_or_else(|| std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default());
    match (method.as_str(), parts.as_slice()) {
        ("GET", ["global", "health"]) => respond(&mut stream, 200, &json!({"healthy": true, "version": "0.0.0-scripted"})),
        ("GET", ["global", "event"]) => {
            let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nTransfer-Encoding: chunked\r\n\r\n";
            let first = format!("data: {}\n\n", json!({"directory": "global", "payload": {"id": "evt_s0", "type": "server.connected", "properties": {}}}));
            if stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(format!("{:x}\r\n{first}\r\n", first.len()).as_bytes())).is_ok() {
                server.subscribers.lock().unwrap().push(stream);
            }
        }
        ("POST", ["session"]) => {
            let id = format!("ses_scripted{:04}", server.sessions.lock().unwrap().len() + 1);
            server.sessions.lock().unwrap().insert(id.clone(), Session { directory: directory.clone(), ..Session::default() });
            server.save();
            let session = json!({"id": id, "slug": "scripted", "projectID": "global", "directory": directory, "title": body["title"], "version": "0.0.0-scripted", "permission": body["permission"], "time": {"created": now_ms(), "updated": now_ms()}});
            server.emit(&directory, "session.created", json!({"sessionID": id, "info": session}));
            respond(&mut stream, 200, &session);
        }
        ("GET", ["session", "status"]) => {
            let busy: serde_json::Map<String, Value> = server.sessions.lock().unwrap().iter().filter(|(_, s)| s.busy).map(|(id, _)| (id.clone(), json!({"type": "busy"}))).collect();
            respond(&mut stream, 200, &Value::Object(busy));
        }
        ("GET" | "PATCH", ["session", id]) => match server.directory(id) {
            Some(directory) => respond(&mut stream, 200, &json!({"id": id, "directory": directory, "projectID": "global", "version": "0.0.0-scripted", "permission": body["permission"], "time": {"created": 0, "updated": now_ms()}})),
            None => not_found(&mut stream, "Session", id),
        },
        ("POST", ["session", id, "prompt_async"]) => {
            let Some(directory) = server.directory(id) else { return not_found(&mut stream, "Session", id) };
            let text = body["parts"][0]["text"].as_str().unwrap_or_default().to_string();
            let start_turn = {
                let mut sessions = server.sessions.lock().unwrap();
                let session = sessions.get_mut(*id).expect("the session was just found");
                session.users += 1;
                !std::mem::replace(&mut session.busy, true)
            };
            server.user_message(id, &directory, &text);
            respond(&mut stream, 204, &Value::Null);
            if start_turn {
                let (server, id) = (Arc::clone(server), id.to_string());
                std::thread::spawn(move || server.play(&id, &directory));
            }
        }
        ("POST", ["session", id, "abort"]) => {
            let Some(directory) = server.directory(id) else { return not_found(&mut stream, "Session", id) };
            let busy = server.sessions.lock().unwrap().get_mut(*id).is_some_and(|s| {
                s.abort = s.busy;
                s.busy
            });
            if !busy {
                server.idle(id, &directory);
            }
            respond(&mut stream, 200, &json!(true));
        }
        ("POST", ["permission", id, "reply"]) | ("POST", ["question", id, "reply"]) => match server.waiting.lock().unwrap().remove(*id) {
            Some(waiter) => {
                let _ = waiter.send(Answer::Reply(body));
                respond(&mut stream, 200, &json!(true));
            }
            None => respond(&mut stream, 404, &json!({"_tag": "PermissionNotFoundError", "requestID": id, "message": format!("request not found: {id}")})),
        },
        ("POST", ["question", id, "reject"]) => match server.waiting.lock().unwrap().remove(*id) {
            Some(waiter) => {
                let _ = waiter.send(Answer::Reply(json!({"answers": []})));
                respond(&mut stream, 200, &json!(true));
            }
            None => not_found(&mut stream, "Question request", id),
        },
        ("GET", ["permission"]) | ("GET", ["question"]) => respond(&mut stream, 200, &json!([])),
        _ => respond(&mut stream, 404, &json!({"name": "NotFoundError", "data": {"message": format!("no route {method} {route}")}})),
    }
}

fn base64(text: &str) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in text.as_bytes().chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Starts the server on a free loopback port, says where it listens, and serves until it is killed (or a turn of the script ends the process).
pub fn serve(script: Script, dir: &Path, program: &str, start: usize) -> ! {
    let Ok(password) = std::env::var("OPENCODE_SERVER_PASSWORD") else {
        eprintln!("scripted-cli: OPENCODE_SERVER_PASSWORD is not set: refusing to serve unsecured");
        std::process::exit(6);
    };
    let _ = std::fs::write(dir.join(password_file(program)), &password);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| {
        eprintln!("scripted-cli: {e}");
        std::process::exit(1)
    });
    let known: BTreeMap<String, String> = std::fs::read_to_string(dir.join(sessions_file(program))).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let sessions = known.into_iter().map(|(id, directory)| (id, Session { directory, ..Session::default() })).collect();
    let server = Arc::new(Server { script, dir: dir.to_path_buf(), program: program.to_string(), start, password, subscribers: Mutex::default(), sessions: Mutex::new(sessions), waiting: Mutex::default(), counter: Mutex::new(0) });
    let port = listener.local_addr().map_or(0, |a| a.port());
    println!("opencode server listening on http://127.0.0.1:{port}");
    let _ = std::io::stdout().flush();
    for stream in listener.incoming().flatten() {
        let server = Arc::clone(&server);
        std::thread::spawn(move || handle(&server, stream));
    }
    std::process::exit(0)
}
