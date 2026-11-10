//! opencode as the features use it: the calls and the reading of their replies over the `Opencode` port, whichever transport the app was
//! built with. `Client` is what the use cases and routes call; the transport (`infrastructure::opencode::Http`) is behind the port.

use super::environment::Environment;
use remote_chat_agent_core::application::ports::{DataStream, Opencode, Reply};
use serde_json::Value;
use remote_chat_agent_core::domain::ids::Clock;
use std::sync::Arc;
use std::time::Duration;

/// The timeout of every call that is not the event stream.
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// opencode as the features use it, over whichever transport the app was built with.
#[derive(Clone)]
pub struct Client {
    transport: Arc<dyn Opencode>,
    clock: Arc<dyn Clock>,
    env: Arc<dyn Environment>,
}

/// A failed call, displayed as JavaScript's `String(error)` of the one `fetchJson` throws, which is what the proxy routes put in
/// their 502.
#[derive(Debug)]
pub enum FetchError {
    Network,
    Status(String),
    Json(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network => write!(f, "TypeError: fetch failed"),
            Self::Status(message) => write!(f, "Error: {message}"),
            Self::Json(message) => write!(f, "SyntaxError: {message}"),
        }
    }
}

impl Client {
    pub fn new(transport: Arc<dyn Opencode>, clock: Arc<dyn Clock>, env: Arc<dyn Environment>) -> Self {
        Self { transport, clock, env }
    }

    /// The machine's environment the features read their settings from.
    pub fn env(&self) -> &dyn Environment {
        self.env.as_ref()
    }

    pub fn now_ms(&self) -> u64 {
        self.clock.now_ms()
    }

    /// One request, the whole reply. An error is a connection or protocol failure; an HTTP error status is a `Reply`.
    pub async fn request(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Reply, String> {
        self.request_with(method, path, body, FETCH_TIMEOUT).await
    }

    pub async fn request_with(&self, method: &str, path: &str, body: Option<&Value>, timeout: Duration) -> Result<Reply, String> {
        self.transport.request(method, path, body, timeout).await
    }

    /// The body of a successful call as JSON; a non-2xx status is the error of `fetchJson`:
    /// `opencode <path> responded with <status>`.
    pub async fn fetch_json(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, FetchError> {
        let reply = self.request(method, path, body).await.map_err(|_| FetchError::Network)?;
        if !reply.ok() {
            return Err(FetchError::Status(format!("opencode {path} responded with {}", reply.status)));
        }
        reply.json().map_err(FetchError::Json)
    }

    /// Opens a server-sent event stream: the head has been read, so the server holds the subscription when this returns. Dropping
    /// the stream closes the connection.
    pub async fn open_stream(&self, path: &str) -> Result<DataStream, String> {
        self.transport.open_stream(path).await
    }
}
