use crate::domain::cookies::Cookie;
use async_trait::async_trait;
use std::sync::Arc;

/// A failure of a port, as text the user may be told.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct PortError(pub String);

impl PortError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Registration {
    pub started: bool,
    pub expires_at: u64,
}

/// The leetcode-backend: it tracks a single active capture with a TTL, so two attempts cannot race.
#[async_trait]
pub trait Backend: Send + Sync {
    /// Registers this tool as the active capture. Must come before a browser opens.
    async fn register(&self) -> Result<Registration, PortError>;
    /// Pushes the captured `document.cookie`-format string to the backend's session route.
    async fn submit_cookie(&self, cookie: &str) -> Result<(), PortError>;
}

/// Where the person is told what to do. A failure to tell never changes the outcome of an attempt.
#[async_trait]
pub trait Notifier: Send + Sync {
    async fn notify(&self, text: &str) -> Result<(), PortError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchOptions {
    /// The virtual display Chromium targets (and runs without its own sandbox on), or `None` for the host's own display.
    pub display: Option<String>,
}

/// One browser the person signs in with.
#[async_trait]
pub trait BrowserSession: Send + Sync {
    /// Asks LeetCode, from inside the page, whether the person is signed in.
    async fn check_signed_in(&self) -> Result<bool, PortError>;
    async fn cookies(&self) -> Result<Vec<Cookie>, PortError>;
    async fn close(&self);
}

#[async_trait]
pub trait BrowserLauncher: Send + Sync {
    /// Opens a headed Chromium and navigates it to `login_url`.
    async fn launch(&self, options: &LaunchOptions, login_url: &str) -> Result<Box<dyn BrowserSession>, PortError>;
}

/// A virtual display with a VNC and noVNC front, stood up for the length of one attempt.
#[async_trait]
pub trait DisplayStack: Send + Sync {
    fn display(&self) -> &str;
    /// Stops the stack, the thing furthest from the display first.
    async fn teardown(&self);
}

#[async_trait]
pub trait DisplayStacks: Send + Sync {
    async fn start(&self, novnc_port: u16) -> Result<Box<dyn DisplayStack>, PortError>;
}

#[async_trait]
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
    async fn sleep_ms(&self, ms: u64);
}

/// Everything an attempt reaches the world through: `main` fills it with the real adapters, a flow test with doubles.
pub struct Ports {
    pub backend: Arc<dyn Backend>,
    pub notifier: Arc<dyn Notifier>,
    pub browsers: Arc<dyn BrowserLauncher>,
    pub displays: Arc<dyn DisplayStacks>,
    pub clock: Arc<dyn Clock>,
}
