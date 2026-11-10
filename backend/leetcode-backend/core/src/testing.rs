//! In-memory doubles of the ports a flow test cannot run for real: leetcode.com, the capture tool and the clock.

use crate::application::ports::{CaptureTool, Clock, Leetcode, PortResult};
use crate::application::requests::{SubmitRequest, TestRequest};
use crate::application::session::Session;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub struct FakeLeetcode {
    status: Mutex<PortResult<Value>>,
    relay: Mutex<PortResult<Value>>,
    calls: Mutex<Vec<String>>,
}

impl FakeLeetcode {
    pub fn signed_in() -> Arc<Self> {
        Arc::new(Self {
            status: Mutex::new(Ok(json!({ "isSignedIn": true, "username": "ada" }))),
            relay: Mutex::new(Ok(json!({ "interpret_id": "i-1" }))),
            calls: Mutex::new(Vec::new()),
        })
    }

    pub fn answer_status(&self, answer: PortResult<Value>) {
        *lock(&self.status) = answer;
    }

    pub fn answer_relay(&self, answer: PortResult<Value>) {
        *lock(&self.relay) = answer;
    }

    pub fn calls(&self) -> Vec<String> {
        lock(&self.calls).clone()
    }

    fn record(&self, call: &str) {
        lock(&self.calls).push(call.to_string());
    }
}

#[async_trait]
impl Leetcode for FakeLeetcode {
    async fn user_status(&self, _session: &Session) -> PortResult<Value> {
        self.record("user_status");
        lock(&self.status).clone()
    }

    async fn test(&self, _session: &Session, request: &TestRequest) -> PortResult<Value> {
        self.record(&format!("test {}", request.slug.as_str()));
        lock(&self.relay).clone()
    }

    async fn submit(&self, _session: &Session, request: &SubmitRequest) -> PortResult<Value> {
        self.record(&format!("submit {}", request.slug.as_str()));
        lock(&self.relay).clone()
    }

    async fn poll(&self, _session: &Session, id: &str) -> PortResult<Value> {
        self.record(&format!("poll {id}"));
        lock(&self.relay).clone()
    }
}

#[derive(Default)]
pub struct FakeCapture {
    asked: AtomicUsize,
}

impl FakeCapture {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }
}

impl CaptureTool for FakeCapture {
    fn request_login(&self) {
        self.asked.fetch_add(1, Ordering::SeqCst);
    }
}

pub struct FixedClock(AtomicU64);

impl FixedClock {
    pub fn at(now_ms: u64) -> Arc<Self> {
        Arc::new(Self(AtomicU64::new(now_ms)))
    }

    pub fn advance(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for FixedClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}
