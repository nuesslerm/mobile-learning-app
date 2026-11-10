//! A stand-in for an HTTP service the real adapters dial (leetcode.com, the capture tool): a local listener that records every request
//! and answers with whatever the test last told it to.

use axum::body::Bytes;
use axum::extract::State;
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub headers: HeaderMap,
    pub body: String,
}

impl Seen {
    pub fn header(&self, name: &str) -> &str {
        self.headers.get(name).and_then(|value| value.to_str().ok()).unwrap_or_default()
    }

    pub fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or(Value::Null)
    }
}

type Reply = Arc<dyn Fn(&Seen) -> (u16, String) + Send + Sync>;

#[derive(Clone)]
struct Shared {
    seen: Arc<Mutex<Vec<Seen>>>,
    reply: Arc<Mutex<Reply>>,
}

pub struct Upstream {
    pub url: String,
    shared: Shared,
}

impl Upstream {
    pub async fn start() -> Self {
        let unmocked: Reply = Arc::new(|_| (404, r#"{"error":"unmocked"}"#.to_string()));
        let shared = Shared { seen: Arc::default(), reply: Arc::new(Mutex::new(unmocked)) };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new().fallback(record).with_state(shared.clone());
        tokio::spawn(async move { axum::serve(listener, router).await });
        Self { url, shared }
    }

    /// Every later request is answered with `reply(request)`: a status and a JSON body.
    pub fn reply(&self, reply: impl Fn(&Seen) -> (u16, String) + Send + Sync + 'static) {
        *self.shared.reply.lock().unwrap() = Arc::new(reply);
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.shared.seen.lock().unwrap().clone()
    }
}

async fn record(State(shared): State<Shared>, method: Method, uri: Uri, headers: HeaderMap, body: Bytes) -> Response {
    let path = uri.path_and_query().map(ToString::to_string).unwrap_or_default();
    let seen = Seen { method: method.to_string(), path, headers, body: String::from_utf8_lossy(&body).into_owned() };
    let reply = shared.reply.lock().unwrap().clone();
    let (status, body) = reply(&seen);
    shared.seen.lock().unwrap().push(seen);
    (StatusCode::from_u16(status).unwrap(), [(CONTENT_TYPE, "application/json")], body).into_response()
}

/// Waits up to two seconds for `check`, for effects that land after the response (a fire-and-forget call).
pub async fn eventually(check: impl Fn() -> bool) -> bool {
    for _ in 0..200 {
        if check() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    check()
}
