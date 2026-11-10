//! In-memory fakes of the ports, all writing to one shared event log so a test can assert the order things happened in.

use crate::application::ports::{
    Backend, BrowserLauncher, BrowserSession, Clock, DisplayStack, DisplayStacks, LaunchOptions, Notifier, PortError, Registration,
};
use crate::domain::cookies::Cookie;
use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct Events(Arc<Mutex<Vec<String>>>);

impl Events {
    pub fn push(&self, event: impl Into<String>) {
        self.0.lock().unwrap().push(event.into());
    }

    pub fn all(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

pub struct FakeBackend {
    pub events: Events,
    pub registration: Mutex<Result<Registration, String>>,
    pub submit: Mutex<Result<(), String>>,
}

impl FakeBackend {
    pub fn new(events: &Events, started: bool) -> Self {
        Self {
            events: events.clone(),
            registration: Mutex::new(Ok(Registration { started, expires_at: 777 })),
            submit: Mutex::new(Ok(())),
        }
    }
}

#[async_trait]
impl Backend for FakeBackend {
    async fn register(&self) -> Result<Registration, PortError> {
        self.events.push("backend.register");
        self.registration.lock().unwrap().clone().map_err(PortError)
    }

    async fn submit_cookie(&self, cookie: &str) -> Result<(), PortError> {
        self.events.push(format!("backend.submit {cookie}"));
        self.submit.lock().unwrap().clone().map_err(PortError)
    }
}

pub struct FakeNotifier {
    pub events: Events,
    pub fail: bool,
}

#[async_trait]
impl Notifier for FakeNotifier {
    async fn notify(&self, text: &str) -> Result<(), PortError> {
        self.events.push(format!("notify {text}"));
        if self.fail {
            Err(PortError::new("telegram is down"))
        } else {
            Ok(())
        }
    }
}

pub struct FakeBrowser {
    pub events: Events,
    /// What the sign-in check answers, one per call; the last answer repeats.
    pub signed_in: Vec<bool>,
    pub cookies: Vec<Cookie>,
    pub launch_error: Option<String>,
    checks: AtomicU64,
}

impl FakeBrowser {
    pub fn new(events: &Events, signed_in: Vec<bool>, cookies: Vec<Cookie>) -> Arc<Self> {
        Arc::new(Self { events: events.clone(), signed_in, cookies, launch_error: None, checks: AtomicU64::new(0) })
    }

    pub fn failing_to_launch(events: &Events, message: &str) -> Arc<Self> {
        Arc::new(Self { events: events.clone(), signed_in: vec![], cookies: vec![], launch_error: Some(message.into()), checks: AtomicU64::new(0) })
    }
}

struct FakeSession(Arc<FakeBrowser>);

#[async_trait]
impl BrowserSession for FakeSession {
    async fn check_signed_in(&self) -> Result<bool, PortError> {
        let n = usize::try_from(self.0.checks.fetch_add(1, Ordering::SeqCst)).unwrap_or(usize::MAX);
        self.0.events.push("browser.check");
        Ok(*self.0.signed_in.get(n).or(self.0.signed_in.last()).unwrap_or(&false))
    }

    async fn cookies(&self) -> Result<Vec<Cookie>, PortError> {
        self.0.events.push("browser.cookies");
        Ok(self.0.cookies.clone())
    }

    async fn close(&self) {
        self.0.events.push("browser.close");
    }
}

#[async_trait]
impl BrowserLauncher for Arc<FakeBrowser> {
    async fn launch(&self, options: &LaunchOptions, login_url: &str) -> Result<Box<dyn BrowserSession>, PortError> {
        self.events.push(format!("browser.launch display={} url={login_url}", options.display.as_deref().unwrap_or("-")));
        match &self.launch_error {
            Some(message) => Err(PortError::new(message.clone())),
            None => Ok(Box::new(FakeSession(self.clone()))),
        }
    }
}

pub struct FakeDisplays {
    pub events: Events,
}

struct FakeDisplay(Events);

#[async_trait]
impl DisplayStack for FakeDisplay {
    fn display(&self) -> &str {
        ":99"
    }

    async fn teardown(&self) {
        self.0.push("display.teardown");
    }
}

#[async_trait]
impl DisplayStacks for FakeDisplays {
    async fn start(&self, novnc_port: u16) -> Result<Box<dyn DisplayStack>, PortError> {
        self.events.push(format!("display.start {novnc_port}"));
        Ok(Box::new(FakeDisplay(self.events.clone())))
    }
}

/// Time that moves only when something sleeps.
#[derive(Default)]
pub struct FakeClock {
    now: AtomicU64,
}

#[async_trait]
impl Clock for FakeClock {
    fn now_ms(&self) -> u64 {
        self.now.load(Ordering::SeqCst)
    }

    async fn sleep_ms(&self, ms: u64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
    }
}
