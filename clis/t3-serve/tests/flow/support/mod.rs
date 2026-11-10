//! A client for the server under test: raw HTTP, and an RPC session over a real WebSocket.
pub mod mimic_t3;
pub use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;
pub use t3_core::usecases::pairing::PairingRequest;
use tungstenite::{Error, Message, WebSocket};
pub use t3_serve::testing::{Harness, ENVIRONMENT_ID, START_MS};

pub struct Reply {
    pub status: u16,
    pub body: Value,
}

pub fn http(addr: SocketAddr, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Reply {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: {}\r\n", body.len());
    for (k, v) in headers {
        request.push_str(&format!("{k}: {v}\r\n"));
    }
    stream.write_all(format!("{request}\r\n{body}").as_bytes()).unwrap();
    let mut raw = String::new();
    stream.read_to_string(&mut raw).unwrap();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
    let status = head.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    Reply { status, body: serde_json::from_str(body).unwrap_or(Value::Null) }
}

pub const GRANT: &str = "urn:ietf:params:oauth:grant-type:token-exchange";

pub fn exchange(addr: SocketAddr, code: &str) -> Reply {
    let body = format!("grant_type={}&subject_token={code}", GRANT.replace(':', "%3A"));
    http(addr, "POST", "/oauth/token", &[("Content-Type", "application/x-www-form-urlencoded")], &body)
}

pub fn ticket(addr: SocketAddr, token: &str) -> Reply {
    http(addr, "POST", "/api/auth/websocket-ticket", &[("Authorization", &format!("Bearer {token}"))], "")
}

/// A paired session's bearer token and a fresh ticket for the socket.
pub fn bearer_of(addr: SocketAddr, code: &str) -> String {
    let reply = exchange(addr, code);
    assert_eq!(reply.status, 200, "{:?}", reply.body);
    reply.body["access_token"].as_str().unwrap().to_string()
}

pub struct Rpc {
    ws: WebSocket<TcpStream>,
    next: u64,
}

/// What a refused upgrade answered, or the open session.
pub fn open_socket(addr: SocketAddr, query: &str) -> Result<Rpc, u16> {
    let stream = TcpStream::connect(addr).unwrap();
    match tungstenite::client(format!("ws://{addr}/ws?{query}"), stream) {
        Ok((ws, _)) => Ok(Rpc { ws, next: 1 }),
        Err(tungstenite::HandshakeError::Failure(Error::Http(response))) => Err(response.status().as_u16()),
        Err(other) => panic!("{other:?}"),
    }
}

pub fn connect_with(h: &Harness, request: PairingRequest) -> Rpc {
    let code = h.pair(request).code;
    let token = bearer_of(h.addr, &code);
    let ticket = ticket(h.addr, &token);
    assert_eq!(ticket.status, 200, "{:?}", ticket.body);
    open_socket(h.addr, &format!("orchestrationProtocol=2&wsTicket={}", ticket.body["ticket"].as_str().unwrap())).expect("the socket opens")
}

pub fn connect(h: &Harness) -> Rpc {
    connect_with(h, PairingRequest::default())
}

impl Rpc {
    pub fn send_raw(&mut self, text: &str) {
        self.ws.send(Message::text(text)).unwrap();
    }

    /// Sends a Request and returns its id.
    pub fn send(&mut self, tag: &str, payload: Value) -> Value {
        let id = json!(self.next.to_string());
        self.next += 1;
        self.send_raw(&json!({"_tag": "Request", "id": id, "tag": tag, "payload": payload, "headers": []}).to_string());
        id
    }

    pub fn try_recv(&mut self, wait: Duration) -> Option<Value> {
        self.ws.get_ref().set_read_timeout(Some(wait)).unwrap();
        loop {
            match self.ws.read() {
                Ok(Message::Text(t)) => return Some(serde_json::from_str(t.as_str()).unwrap()),
                Ok(_) => continue,
                Err(Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => return None,
                Err(e) => panic!("socket: {e}"),
            }
        }
    }

    pub fn recv(&mut self) -> Value {
        self.try_recv(Duration::from_secs(5)).expect("a frame within 5 s")
    }

    /// Request and wait for its Exit; the exit object (`{_tag: Success, value}` or `{_tag: Failure, cause}`).
    pub fn call(&mut self, tag: &str, payload: Value) -> Value {
        let id = self.send(tag, payload);
        let frame = self.recv();
        assert_eq!((frame["_tag"].as_str(), &frame["requestId"]), (Some("Exit"), &id), "{frame}");
        frame["exit"].clone()
    }

    /// The next Chunk of stream `id`: its values, acknowledged.
    pub fn chunk(&mut self, id: &Value) -> Vec<Value> {
        let frame = self.recv();
        assert_eq!((frame["_tag"].as_str(), &frame["requestId"]), (Some("Chunk"), id), "{frame}");
        self.ack(id);
        frame["values"].as_array().unwrap().clone()
    }

    pub fn chunk_unacked(&mut self, id: &Value) -> Vec<Value> {
        let frame = self.recv();
        assert_eq!((frame["_tag"].as_str(), &frame["requestId"]), (Some("Chunk"), id), "{frame}");
        frame["values"].as_array().unwrap().clone()
    }

    pub fn ack(&mut self, id: &Value) {
        self.send_raw(&json!({"_tag": "Ack", "requestId": id}).to_string());
    }
}

pub fn success(exit: &Value) -> &Value {
    assert_eq!(exit["_tag"], "Success", "{exit}");
    &exit["value"]
}

/// The declared error of a failed exit.
pub fn failure(exit: &Value) -> &Value {
    assert_eq!(exit["_tag"], "Failure", "{exit}");
    assert_eq!(exit["cause"][0]["_tag"], "Fail", "{exit}");
    &exit["cause"][0]["error"]
}

pub fn thread_create(command_id: &str, thread: &str, project: &str) -> Value {
    json!({"type": "thread.create", "commandId": command_id, "threadId": thread, "projectId": project, "title": format!("Thread {thread}")})
}

pub fn project_create(command_id: &str, project: &str) -> Value {
    json!({"type": "project.create", "commandId": command_id, "projectId": project, "title": format!("Project {project}"), "workspaceRoot": "/workspace/demo"})
}

/// What the server asked of the agent API while a project and a thread were seeded is not what a test is about: forgotten.
pub fn forget_seeding(h: &Harness) {
    h.agent.ops.lock().unwrap().clear();
    h.agent.calls.lock().unwrap().clear();
}

/// A project `p1` with a thread `t1`: sequences 1 and 2.
pub fn seed(rpc: &mut Rpc) {
    assert_eq!(success(&rpc.call("projects.mutate", project_create("c-p1", "p1")))["id"], "p1");
    assert_eq!(success(&rpc.call("orchestration.dispatchCommand", thread_create("c-t1", "t1", "p1")))["sequence"], 2);
}

/// The thread's projection once none of its runs is running (a turn on the agent double ends on its own), within 5 s.
pub fn settled(rpc: &mut Rpc, thread: &str) -> Value {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let projection = success(&rpc.call("orchestration.getThreadProjection", json!({"threadId": thread}))).clone();
        let running = projection["runs"].as_array().is_some_and(|runs| runs.iter().any(|r| r["status"] == "running"));
        if !running || std::time::Instant::now() > deadline {
            return projection;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
