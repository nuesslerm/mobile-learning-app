//! The use cases. Every external effect goes through a port.

use super::error::AppError;
use super::ports::Ports;
use super::requests::{FormatRequest, RunRequest, SubmitRequest, TestRequest};
use super::session::{parse_cookie, CaptureRegistry, Session, LOGIN_CAPTURE_TTL_MS};
use serde_json::{json, Value};

/// A stored session is re-verified with LeetCode when it was last verified longer ago than this.
const REFRESH_INTERVAL_MS: f64 = 60.0 * 60.0 * 1000.0;

pub struct LoginCapture {
    /// Whether a capture is running and when it ends.
    pub active_until: Option<u64>,
}

pub struct Service {
    ports: Ports,
    registry: CaptureRegistry,
}

impl Service {
    pub fn new(ports: Ports) -> Self {
        Self { ports, registry: CaptureRegistry::default() }
    }

    /// The stored session, re-verified when it is over an hour old; without one, the capture tool is asked to start a login.
    async fn session(&self) -> Result<Option<Session>, AppError> {
        let now = self.ports.clock.now_ms();
        let mut session = self.ports.store.get().await.map_err(AppError::Store)?;
        if let Some(current) = &session {
            if current.last_verified_at().is_some_and(|at| now as f64 - at > REFRESH_INTERVAL_MS) {
                session = self.refresh(now).await.map_err(AppError::Upstream)?;
            }
        }
        if session.is_none() {
            self.notify_capture_needed(now);
        }
        Ok(session)
    }

    async fn required_session(&self) -> Result<Session, AppError> {
        self.session().await?.ok_or(AppError::NoSession)
    }

    async fn refresh(&self, now: u64) -> Result<Option<Session>, String> {
        let Some(current) = self.ports.store.get().await? else { return Ok(None) };
        let status = self.ports.leetcode.user_status(&current).await?;
        if status["isSignedIn"] != json!(true) {
            self.ports.store.mark_expired().await?;
            return Ok(None);
        }
        let refreshed = current.verified(now);
        self.ports.store.set(&refreshed).await?;
        Ok(Some(refreshed))
    }

    /// Only a peek at the registry: the capture tool registers the attempt itself, through `start_login_capture`.
    fn notify_capture_needed(&self, now: u64) {
        if self.registry.status(now).is_none() {
            self.ports.capture.request_login();
        }
    }

    pub async fn user_status(&self) -> Result<Value, AppError> {
        match self.session().await? {
            None => Ok(json!({ "isSignedIn": false, "username": null })),
            Some(session) => self.ports.leetcode.user_status(&session).await.map_err(AppError::Upstream),
        }
    }

    pub fn login_capture(&self) -> LoginCapture {
        LoginCapture { active_until: self.registry.status(self.ports.clock.now_ms()) }
    }

    /// `(started, expires_at)`.
    pub fn start_login_capture(&self) -> (bool, u64) {
        self.registry.try_start(LOGIN_CAPTURE_TTL_MS, self.ports.clock.now_ms())
    }

    pub async fn test(&self, request: &TestRequest) -> Result<Value, AppError> {
        let session = self.required_session().await?;
        self.ports.leetcode.test(&session, request).await.map_err(AppError::Upstream)
    }

    pub async fn submit(&self, request: &SubmitRequest) -> Result<Value, AppError> {
        let session = self.required_session().await?;
        self.ports.leetcode.submit(&session, request).await.map_err(AppError::Upstream)
    }

    pub async fn poll(&self, id: &str) -> Result<Value, AppError> {
        let session = self.required_session().await?;
        self.ports.leetcode.poll(&session, id).await.map_err(AppError::Upstream)
    }

    /// Running and formatting happen on this server, with no LeetCode session.
    pub async fn run(&self, request: &RunRequest) -> Result<Value, AppError> {
        self.ports.runner.run(request).await.map_err(AppError::Upstream)
    }

    pub async fn format(&self, request: &FormatRequest) -> Result<Value, AppError> {
        self.ports.runner.format(request).await.map_err(AppError::Upstream)
    }

    pub async fn set_session(&self, cookie: &str) -> Result<(), AppError> {
        let parsed = parse_cookie(cookie);
        let Some(session_id) = parsed.session_id else {
            return Err(AppError::InvalidCookie("Cookie string has no LEETCODE_SESSION value".to_string()));
        };
        let session = Session::new(&session_id, parsed.csrf_token.as_deref().unwrap_or(""), self.ports.clock.now_ms());
        self.ports.store.set(&session).await.map_err(AppError::Store)
    }
}
