//! An opencode server on a loopback port: it records every request and answers from a table, and `/event` serves the scripted events as a
//! server-sent stream. What a test drives `Http` and `OpencodeBackend` against, in place of the real `opencode serve`.

use axum::body::{to_bytes, Body};
use axum::extract::State;
use axum::http::{header, Request, StatusCode};
use axum::response::Response;
use axum::Router;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// A request the server received: the method, the path with its query, the `Authorization` header and the body text.
#[derive(Debug, Clone, PartialEq)]
pub struct Seen {
    pub method: String,
    pub target: String,
    pub authorization: Option<String>,
    pub body: String,
}

#[derive(Default)]
struct Table {
    seen: Vec<Seen>,
    replies: HashMap<String, (u16, String)>,
    events: Vec<Value>,
}

pub struct Loopback {
    port: u16,
    table: Arc<Mutex<Table>>,
    task: tokio::task::JoinHandle<()>,
}

impl Loopback {
    pub async fn start() -> Self {
        let table = Arc::new(Mutex::new(Table::default()));
        let app = Router::new().fallback(answer).with_state(Arc::clone(&table));
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("a free loopback port");
        let port = listener.local_addr().expect("a bound address").port();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self { port, table, task }
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// What `METHOD path` (the query left out) answers with; an unset route answers 200 `{}`.
    pub fn reply(&self, method: &str, path: &str, status: u16, body: &str) {
        self.table.lock().unwrap_or_else(|e| e.into_inner()).replies.insert(format!("{method} {path}"), (status, body.to_string()));
    }

    /// The events `GET /event` serves, each as one `data:` line, before the stream ends.
    pub fn events(&self, events: Vec<Value>) {
        self.table.lock().unwrap_or_else(|e| e.into_inner()).events = events;
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.table.lock().unwrap_or_else(|e| e.into_inner()).seen.clone()
    }
}

impl Drop for Loopback {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn answer(State(table): State<Arc<Mutex<Table>>>, request: Request<Body>) -> Response {
    let (parts, body) = request.into_parts();
    let body = to_bytes(body, usize::MAX).await.map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let target = parts.uri.path_and_query().map(|p| p.as_str().to_string()).unwrap_or_default();
    let authorization = parts.headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).map(str::to_string);
    let mut table = table.lock().unwrap_or_else(|e| e.into_inner());
    table.seen.push(Seen { method: parts.method.to_string(), target, authorization, body });
    if parts.method == "GET" && parts.uri.path() == "/event" {
        let stream: String = table.events.iter().map(|event| format!("data: {event}\n\n")).collect();
        return Response::builder().header(header::CONTENT_TYPE, "text/event-stream").body(Body::from(stream)).expect("a valid response");
    }
    let key = format!("{} {}", parts.method, parts.uri.path());
    let (status, text) = table.replies.get(&key).cloned().unwrap_or_else(|| (200, if key == "POST /session" { r#"{"id":"ses_loopback"}"#.into() } else { "{}".into() }));
    Response::builder().status(StatusCode::from_u16(status).unwrap_or(StatusCode::OK)).header(header::CONTENT_TYPE, "application/json").body(Body::from(text)).expect("a valid response")
}
