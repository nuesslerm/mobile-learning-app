use super::ports::{BrowserSession, LaunchOptions, PortError, Ports};
use crate::domain::config::{CaptureConfig, Mode};
use crate::domain::cookies::session_cookie_header;
use crate::domain::outcome::Outcome;

/// One capture attempt: register with the backend, stand up the display (remote only), open the browser, tell the person where to
/// sign in, wait for the sign-in, take the cookies, push them and tell the person, then tear everything down.
pub struct Capture {
    config: CaptureConfig,
    ports: Ports,
}

enum Waited {
    SignedIn,
    TimedOut,
}

impl Capture {
    pub fn new(config: CaptureConfig, ports: Ports) -> Self {
        Self { config, ports }
    }

    /// `Err` only when the attempt could not begin (the backend or the display refused); every later branch is an `Outcome`.
    pub async fn run(&self) -> Result<Outcome, PortError> {
        let registration = self.ports.backend.register().await?;
        if !registration.started {
            return Ok(Outcome::AlreadyActive { expires_at: registration.expires_at });
        }

        let display = match self.config.mode {
            Mode::Remote => Some(self.ports.displays.start(self.config.novnc_port).await?),
            Mode::Local => None,
        };
        let mut session: Option<Box<dyn BrowserSession>> = None;

        let outcome = match self.attempt(display.as_deref().map(|d| d.display().to_string()), &mut session).await {
            Ok(outcome) => outcome,
            Err(error) => {
                let message = error.0;
                self.tell(&format!("LeetCode login-capture failed: {message}")).await;
                Outcome::Failed { message }
            }
        };

        if let Some(session) = &session {
            session.close().await;
        }
        if let Some(display) = &display {
            display.teardown().await;
        }
        Ok(outcome)
    }

    async fn attempt(&self, display: Option<String>, session_slot: &mut Option<Box<dyn BrowserSession>>) -> Result<Outcome, PortError> {
        let session = self.ports.browsers.launch(&LaunchOptions { display }, &self.config.login_url).await?;
        let session = session_slot.insert(session);

        let login_target = match self.config.mode {
            Mode::Remote => format!("http://leetcode-capture:{}/vnc.html", self.config.novnc_port),
            Mode::Local => "this machine directly (a Chromium window has opened)".to_string(),
        };
        self.tell(&format!("LeetCode session expired — log in now: {login_target}")).await;

        if let Waited::TimedOut = self.wait_for_sign_in(session.as_ref()).await? {
            self.tell("LeetCode login-capture timed out waiting for sign-in.").await;
            return Ok(Outcome::TimedOut);
        }

        let cookies = session.cookies().await?;
        let Some(cookie) = session_cookie_header(&cookies) else {
            self.tell("LeetCode sign-in detected but session cookies were not found.").await;
            return Ok(Outcome::NoCookiesFound);
        };

        self.ports.backend.submit_cookie(&cookie).await?;
        self.tell("LeetCode session captured and pushed to the backend successfully.").await;
        Ok(Outcome::Succeeded)
    }

    /// Polls every `poll_interval` until the person is signed in, giving up once `capture_timeout` has passed (the backend's own
    /// capture registry expires on the same five minutes).
    async fn wait_for_sign_in(&self, session: &dyn BrowserSession) -> Result<Waited, PortError> {
        let timeout_ms = u64::try_from(self.config.capture_timeout.as_millis()).unwrap_or(u64::MAX);
        let interval_ms = u64::try_from(self.config.poll_interval.as_millis()).unwrap_or(u64::MAX);
        let deadline = self.ports.clock.now_ms().saturating_add(timeout_ms);
        loop {
            if session.check_signed_in().await? {
                return Ok(Waited::SignedIn);
            }
            if self.ports.clock.now_ms() >= deadline {
                return Ok(Waited::TimedOut);
            }
            self.ports.clock.sleep_ms(interval_ms).await;
        }
    }

    /// A failed notification must never mask the real outcome.
    async fn tell(&self, text: &str) {
        let _ = self.ports.notifier.notify(text).await;
    }
}
