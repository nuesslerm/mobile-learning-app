use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct MockRequest {
    pub method: String,
    pub url: String,
    pub auth: Option<String>,
    pub headers: HashMap<String, String>,
    pub body: String,
}

#[derive(Debug, Clone, Default)]
pub struct MockReply {
    pub status: Option<u16>,
    pub body: Option<Value>,
    pub headers: HashMap<String, String>,
    /// Close the connection without answering.
    pub drop: bool,
}

impl MockReply {
    pub fn json(status: u16, body: Value) -> Self {
        Self { status: Some(status), body: Some(body), ..Default::default() }
    }

    pub fn text(status: u16, body: &str) -> Self {
        Self { status: Some(status), body: Some(Value::String(body.to_string())), ..Default::default() }
    }
}

#[derive(Default)]
struct State {
    routes: HashMap<String, MockReply>,
    queue: Vec<MockReply>,
    next: usize,
    repeat_last: bool,
    delay: Duration,
    requests: Vec<MockRequest>,
}

/// A mock HTTP service on a thread of this process. Replies come from the route table (`"METHOD /path"`) or from a queue that
/// answers in request order whatever the path; every request is recorded. Port of `mock-http-server.ts`.
pub struct MockServer {
    pub port: u16,
    state: Arc<Mutex<State>>,
    server: Arc<tiny_http::Server>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl MockServer {
    /// `port` pins the port a CLI under test already has in its fixtures; `None` lets the system pick one.
    pub fn start(port: Option<u16>) -> Self {
        // A pinned port may still be closing after the previous test's server dropped.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let server = loop {
            match tiny_http::Server::http(("127.0.0.1", port.unwrap_or(0))) {
                Ok(server) => break Arc::new(server),
                Err(e) if port.is_none() || std::time::Instant::now() > deadline => panic!("the mock server did not start (port {port:?} taken?): {e}"),
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        };
        let bound = server.server_addr().to_ip().expect("ip address").port();
        let state = Arc::new(Mutex::new(State::default()));
        let worker = {
            let (server, state) = (server.clone(), state.clone());
            std::thread::spawn(move || {
                for mut request in server.incoming_requests() {
                    let mut body = String::new();
                    let _ = std::io::Read::read_to_string(request.as_reader(), &mut body);
                    let headers: HashMap<String, String> = request.headers().iter().map(|h| (h.field.to_string().to_ascii_lowercase(), h.value.to_string())).collect();
                    let recorded = MockRequest { method: request.method().to_string(), url: request.url().to_string(), auth: headers.get("authorization").cloned(), headers, body };
                    let (reply, delay) = {
                        let mut state = state.lock().unwrap();
                        let key = format!("{} {}", recorded.method, recorded.url);
                        state.requests.push(recorded);
                        let queued = if state.next < state.queue.len() {
                            state.next += 1;
                            Some(state.queue[state.next - 1].clone())
                        } else if state.repeat_last {
                            state.queue.last().cloned()
                        } else {
                            None
                        };
                        (queued.or_else(|| state.routes.get(&key).cloned()).unwrap_or_else(|| MockReply::json(404, serde_json::json!({ "message": "no route" }))), state.delay)
                    };
                    std::thread::sleep(delay);
                    if reply.drop {
                        // Behind a `MockTailnet` the marker makes the proxy end the connection; a direct client sees an empty 200.
                        let marker = tiny_http::Header::from_bytes(DROP_MARKER.as_bytes(), &b"1"[..]).expect("a valid header");
                        let _ = request.respond(tiny_http::Response::empty(200).with_header(marker));
                        continue;
                    }
                    let text = match &reply.body {
                        Some(Value::String(text)) => text.clone(),
                        Some(other) => other.to_string(),
                        None => "{}".to_string(),
                    };
                    let mut response = tiny_http::Response::from_string(text).with_status_code(reply.status.unwrap_or(200));
                    let mut headers = reply.headers.clone();
                    headers.entry("Content-Type".into()).or_insert_with(|| "application/json".into());
                    for (key, value) in headers {
                        if let Ok(header) = tiny_http::Header::from_bytes(key.as_bytes(), value.as_bytes()) {
                            response.add_header(header);
                        }
                    }
                    let _ = request.respond(response);
                }
            })
        };
        Self { port: bound, state, server, worker: Some(worker) }
    }

    pub fn requests(&self) -> Vec<MockRequest> {
        self.state.lock().unwrap().requests.clone()
    }

    /// Answers every later request only after `delay`, like a service that takes its time.
    pub fn delay(&self, delay: Duration) {
        self.state.lock().unwrap().delay = delay;
    }
    pub fn clear(&self) {
        self.state.lock().unwrap().requests.clear();
    }

    pub fn routes(&self, table: &[(&str, MockReply)]) {
        self.state.lock().unwrap().routes = table.iter().map(|(key, reply)| (key.to_string(), reply.clone())).collect();
    }

    /// Replies in request order, whatever the path; later requests use the routes (or the last reply again with `repeat_last`).
    pub fn queue(&self, replies: Vec<MockReply>, repeat_last: bool) {
        let mut state = self.state.lock().unwrap();
        state.queue = replies;
        state.next = 0;
        state.repeat_last = repeat_last;
    }

}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// The header a dropped reply carries, in lower case.
pub(crate) const DROP_MARKER: &str = "x-mock-drop";
