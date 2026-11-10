//! The LeetCode session as stored, the cookie string it is made from, and the registry of the one login capture that may run.

use serde_json::{json, Map, Value};
use std::sync::Mutex;

/// A session as stored; fields this service does not know survive a rewrite.
#[derive(Debug, Clone, PartialEq)]
pub struct Session(Map<String, Value>);

impl Session {
    pub fn new(session_id: &str, csrf_token: &str, now: u64) -> Self {
        let mut fields = Map::new();
        fields.insert("sessionId".into(), json!(session_id));
        fields.insert("csrfToken".into(), json!(csrf_token));
        fields.insert("capturedAt".into(), json!(now));
        fields.insert("lastVerifiedAt".into(), json!(now));
        fields.insert("status".into(), json!("active"));
        Self(fields)
    }

    /// The stored JSON object, or `None` when the value is not one.
    pub fn from_value(value: Value) -> Option<Self> {
        match value {
            Value::Object(fields) => Some(Self(fields)),
            _ => None,
        }
    }

    pub fn to_value(&self) -> Value {
        Value::Object(self.0.clone())
    }

    pub fn session_id(&self) -> &str {
        self.0.get("sessionId").and_then(Value::as_str).unwrap_or_default()
    }

    pub fn csrf_token(&self) -> &str {
        self.0.get("csrfToken").and_then(Value::as_str).unwrap_or_default()
    }

    pub fn last_verified_at(&self) -> Option<f64> {
        self.0.get("lastVerifiedAt").and_then(Value::as_f64)
    }

    pub fn expired(mut self) -> Self {
        self.0.insert("status".into(), json!("expired"));
        self
    }

    pub fn verified(mut self, now: u64) -> Self {
        self.0.insert("lastVerifiedAt".into(), json!(now));
        self.0.insert("status".into(), json!("active"));
        self
    }
}

pub struct ParsedCookie {
    pub session_id: Option<String>,
    pub csrf_token: Option<String>,
}

pub fn parse_cookie(raw: &str) -> ParsedCookie {
    let mut session_id = None;
    let mut csrf_token = None;
    for part in raw.split(';').map(str::trim).filter(|part| !part.is_empty()) {
        let Some((name, value)) = part.split_once('=') else { continue };
        match name {
            "LEETCODE_SESSION" => session_id = Some(value.to_string()),
            "csrftoken" => csrf_token = Some(value.to_string()),
            _ => {}
        }
    }
    ParsedCookie { session_id, csrf_token }
}

pub const LOGIN_CAPTURE_TTL_MS: u64 = 5 * 60 * 1000;

/// When the login capture started by the capture tool stops being active.
#[derive(Default)]
pub struct CaptureRegistry(Mutex<Option<u64>>);

impl CaptureRegistry {
    pub fn status(&self, now: u64) -> Option<u64> {
        let expires = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        expires.filter(|&at| now < at)
    }

    /// `(started, expires_at)`: a running capture is never replaced.
    pub fn try_start(&self, ttl_ms: u64, now: u64) -> (bool, u64) {
        let mut expires = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(at) = expires.filter(|&at| now < at) {
            return (false, at);
        }
        let until = now + ttl_ms;
        *expires = Some(until);
        (true, until)
    }
}
