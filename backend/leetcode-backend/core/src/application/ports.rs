//! What the use cases need from the outside world. Each is implemented by the service crate (and by the fakes in `testing`).
//! Errors are the message the caller should see.

use super::requests::{FormatRequest, RunRequest, SubmitRequest, TestRequest};
use super::session::Session;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

pub type PortResult<T> = Result<T, String>;

/// Where the LeetCode session lives between requests.
#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn get(&self) -> PortResult<Option<Session>>;
    async fn set(&self, session: &Session) -> PortResult<()>;
    async fn mark_expired(&self) -> PortResult<()>;
}

/// The four relays to leetcode.com. Answers are passed through as LeetCode gave them.
#[async_trait]
pub trait Leetcode: Send + Sync {
    async fn user_status(&self, session: &Session) -> PortResult<Value>;
    async fn test(&self, session: &Session, request: &TestRequest) -> PortResult<Value>;
    async fn submit(&self, session: &Session, request: &SubmitRequest) -> PortResult<Value>;
    async fn poll(&self, session: &Session, id: &str) -> PortResult<Value>;
}

/// The login capture tool. `request_login` is fire and forget: a tool that is down must never change the answer to a request,
/// so the adapter reports its own failure and the caller never waits on it.
pub trait CaptureTool: Send + Sync {
    fn request_login(&self);
}

/// Runs and formats code on this host, never on leetcode.com.
#[async_trait]
pub trait CodeRunner: Send + Sync {
    async fn run(&self, request: &RunRequest) -> PortResult<Value>;
    async fn format(&self, request: &FormatRequest) -> PortResult<Value>;
}

pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}

/// Everything the use cases reach the world through: `main` fills it with the real adapters, a flow test with doubles.
pub struct Ports {
    pub store: Arc<dyn SessionStore>,
    pub leetcode: Arc<dyn Leetcode>,
    pub capture: Arc<dyn CaptureTool>,
    pub runner: Arc<dyn CodeRunner>,
    pub clock: Arc<dyn Clock>,
}
