//! The session engine: one in-memory record per login, one reusable link token per session, and the per-tool idempotency
//! rule (a second start for a tool with a login in flight reuses it). In memory on purpose: a listener restart invalidates
//! every outstanding link, and no bearer secret is ever persisted.
use super::descriptor::Descriptor;
use super::desktop::Launcher;
use super::relay;
use base64::Engine as _;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The link is a bearer credential to a live desktop, reusable across devices for its whole TTL: 12 hours, because the
/// human it is handed to is usually away from the machine when it arrives.
pub const LINK_TTL_MS: i64 = 43_200_000;
const AUTOFILL_DELAY: Duration = Duration::from_millis(6000);
/// The email pre-filled into a login page: this desktop is one shared identity, so every login on it is for this account.
const AUTOFILL_EMAIL: &str = "user@example.com";

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).expect("the operating system has no randomness for a link token");
    bytes
}

/// Reusable, short-TTL tokens keyed by the opaque token string.
#[derive(Clone, Default)]
pub struct Tokens {
    by_token: Arc<Mutex<HashMap<String, (String, i64)>>>,
}

impl Tokens {
    pub fn mint(&self, session_id: &str, ttl_ms: i64) -> String {
        let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random_bytes::<32>());
        self.by_token.lock().unwrap_or_else(|e| e.into_inner()).insert(token.clone(), (session_id.to_string(), wf_kit::time::now_ms() + ttl_ms));
        token
    }

    pub fn is_valid(&self, token: &str) -> bool {
        self.by_token.lock().unwrap_or_else(|e| e.into_inner()).get(token).is_some_and(|(_, expires)| wf_kit::time::now_ms() <= *expires)
    }
}

#[derive(Clone)]
pub struct Session {
    pub session_id: String,
    pub tool: String,
    pub token: String,
    pub link_url: String,
    pub status: &'static str,
    pub descriptor: Descriptor,
}

#[derive(Default)]
struct State {
    by_id: HashMap<String, Session>,
    in_flight: HashMap<String, String>,
}

pub struct Engine {
    state: Mutex<State>,
    pub tokens: Tokens,
    link_base: String,
    launcher: Arc<Launcher>,
}

impl Engine {
    pub fn new(link_base: &str, launcher: Arc<Launcher>, tokens: Tokens) -> Engine {
        Engine { state: Mutex::new(State::default()), tokens, link_base: link_base.trim_end_matches('/').to_string(), launcher }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn schedule(&self, delay: Duration, job: impl FnOnce(Arc<Launcher>) + Send + 'static) {
        let launcher = self.launcher.clone();
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            job(launcher);
        });
    }

    pub fn start(&self, descriptor: &Descriptor) -> Session {
        // Idempotent per tool: reuse an in-flight session rather than opening a second one.
        let reuse = {
            let mut state = self.state();
            let existing_id = state.in_flight.get(&descriptor.id).cloned();
            existing_id.and_then(|id| state.by_id.get_mut(&id)).filter(|s| s.status == "awaiting-login" || s.status == "starting").map(|existing| {
                // The token may have outlived its TTL without the session being torn down; a link that 403s on arrival
                // helps nobody, so only the token is re-minted, never a second login command.
                if !self.tokens.is_valid(&existing.token) {
                    existing.token = self.tokens.mint(&existing.session_id, LINK_TTL_MS);
                    existing.link_url = format!("{}/?t={}", self.link_base, existing.token);
                }
                existing.clone()
            })
        };
        if let Some(session) = reuse {
            return session;
        }

        let session_id = uuid::Uuid::new_v4().to_string();
        let token = self.tokens.mint(&session_id, LINK_TTL_MS);
        let session = Session {
            session_id: session_id.clone(),
            tool: descriptor.id.clone(),
            link_url: format!("{}/?t={token}", self.link_base),
            token,
            status: "awaiting-login",
            descriptor: descriptor.clone(),
        };
        {
            let mut state = self.state();
            state.by_id.insert(session_id.clone(), session.clone());
            state.in_flight.insert(descriptor.id.clone(), session_id);
        }
        if let Some(command) = &descriptor.login_cmd {
            self.launcher.run_login(&descriptor.id, command);
            self.schedule(AUTOFILL_DELAY, |launcher| launcher.prefill_login(AUTOFILL_EMAIL));
        } else if let Some(url) = &descriptor.login_url {
            // A loginUrl tool has no CLI to run: the login IS a web page.
            let (launcher, url) = (self.launcher.clone(), url.clone());
            std::thread::spawn(move || launcher.navigate(&url));
            self.schedule(AUTOFILL_DELAY, |launcher| launcher.prefill_login(AUTOFILL_EMAIL));
        }
        relay::watch_and_relay(descriptor, self.launcher.user());
        session
    }

    /// Ends the open login of `tool`, so the next start mints a fresh session and link instead of reusing it.
    pub fn forget_tool(&self, tool: &str) {
        self.state().in_flight.remove(tool);
    }

    pub fn get(&self, session_id: &str) -> Option<Session> {
        self.state().by_id.get(session_id).cloned()
    }

    pub fn mark_complete(&self, session_id: &str) {
        if let Some(s) = self.state().by_id.get_mut(session_id) {
            s.status = "captured";
        }
    }
}
