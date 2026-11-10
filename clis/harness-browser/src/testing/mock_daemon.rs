use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct DaemonRequest {
    pub method: String,
    pub url: String,
    pub body: String,
}

#[derive(Default)]
struct State {
    requests: Vec<DaemonRequest>,
    results: HashMap<String, Value>,
    errors: HashMap<String, String>,
    status: Option<Value>,
    rules: Vec<Rule>,
}

/// Answers for the `/call`s of one method whose args contain a needle: each call takes the next, the last one repeats. An
/// answer `{"error": "..."}` is a failed call.
struct Rule {
    method: String,
    needle: String,
    answers: Vec<Value>,
    taken: usize,
}

/// A fake multi-browser daemon for black-box client tests: the HTTP surface the CLI talks to (`/health`, `/status`, `/stop`,
/// `/call`). `/call` answers `{ok:true,result}` from the results table (by method) or `{ok:false,error}` from the errors table.
/// Port of `mock-daemon.ts`.
pub struct MockDaemon {
    pub url: String,
    state: Arc<Mutex<State>>,
    server: Arc<tiny_http::Server>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl MockDaemon {
    pub fn start() -> Self {
        let server = Arc::new(tiny_http::Server::http(("127.0.0.1", 0)).expect("a daemon port"));
        let port = server.server_addr().to_ip().unwrap().port();
        let state = Arc::new(Mutex::new(State::default()));
        let worker = {
            let (server, state) = (server.clone(), state.clone());
            std::thread::spawn(move || {
                for mut request in server.incoming_requests() {
                    let mut body = String::new();
                    let _ = std::io::Read::read_to_string(request.as_reader(), &mut body);
                    let url = request.url().to_string();
                    let (status, value) = {
                        let mut state = state.lock().unwrap();
                        state.requests.push(DaemonRequest { method: request.method().to_string(), url: url.clone(), body: body.clone() });
                        match url.as_str() {
                            "/health" => (200, json!({ "ok": true })),
                            "/status" => (200, state.status.clone().unwrap_or_else(|| json!({ "activeBrowserName": null, "browserCount": 0, "browsers": [] }))),
                            "/stop" => (200, json!({ "ok": true })),
                            "/call" => {
                                let parsed: Value = serde_json::from_str(if body.is_empty() { "{}" } else { &body }).unwrap_or(Value::Null);
                                let method = parsed["method"].as_str().unwrap_or("").to_string();
                                let args = parsed["args"].to_string();
                                let ruled = state.rules.iter_mut().find(|r| r.method == method && args.contains(&r.needle)).map(|r| {
                                    let answer = r.answers[r.taken.min(r.answers.len() - 1)].clone();
                                    r.taken += 1;
                                    answer
                                });
                                match ruled {
                                    Some(a) if a["error"].is_string() => (200, json!({ "ok": false, "error": a["error"] })),
                                    Some(a) => (200, json!({ "ok": true, "result": a })),
                                    None => match state.errors.get(&method) {
                                        Some(error) => (200, json!({ "ok": false, "error": error })),
                                        None => (200, json!({ "ok": true, "result": state.results.get(&method).cloned().unwrap_or(Value::Null), "echo": parsed["args"] })),
                                    },
                                }
                            }
                            _ => (404, json!({ "ok": false, "error": "Not found" })),
                        }
                    };
                    let mut response = tiny_http::Response::from_string(value.to_string()).with_status_code(status);
                    response.add_header(tiny_http::Header::from_bytes("content-type", "application/json").unwrap());
                    let _ = request.respond(response);
                }
            })
        };
        Self { url: format!("http://127.0.0.1:{port}"), state, server, worker: Some(worker) }
    }

    pub fn requests(&self) -> Vec<DaemonRequest> {
        self.state.lock().unwrap().requests.clone()
    }

    /// The `/call` requests: method and args.
    pub fn calls(&self) -> Vec<(String, Value)> {
        self.requests()
            .into_iter()
            .filter(|r| r.url == "/call")
            .map(|r| {
                let parsed: Value = serde_json::from_str(&r.body).unwrap_or(Value::Null);
                (parsed["method"].as_str().unwrap_or("").to_string(), parsed["args"].clone())
            })
            .collect()
    }

    pub fn clear(&self) {
        self.state.lock().unwrap().requests.clear();
    }

    pub fn results(&self, table: &[(&str, Value)]) {
        self.state.lock().unwrap().results = table.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
    }

    pub fn errors(&self, table: &[(&str, &str)]) {
        self.state.lock().unwrap().errors = table.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    }

    /// Answers `method` calls whose args contain `needle` with `answers` in turn (the last repeats); the first matching rule
    /// wins, ahead of the results and errors tables.
    pub fn rule(&self, method: &str, needle: &str, answers: Vec<Value>) {
        assert!(!answers.is_empty(), "a rule needs an answer");
        self.state.lock().unwrap().rules.push(Rule { method: method.into(), needle: needle.into(), answers, taken: 0 });
    }

    pub fn clear_rules(&self) {
        self.state.lock().unwrap().rules.clear();
    }

    /// How many `/call`s of `method` had args containing `needle`.
    pub fn count(&self, method: &str, needle: &str) -> usize {
        self.calls().iter().filter(|(m, a)| m == method && a.to_string().contains(needle)).count()
    }

    pub fn status(&self, value: Value) {
        self.state.lock().unwrap().status = Some(value);
    }
}

impl Drop for MockDaemon {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
