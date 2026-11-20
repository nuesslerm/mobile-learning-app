//! The sign-in rows of Orca's accounts group over rca's `provider.auth.*`: `hostSignInStart`, `Wait`, `SubmitCode` and `Cancel` are one login at the
//! workspace host, started and answered from the phone (decision 9: the CLI keeps its own credential files there). Orca names a flow by an id the
//! host mints and numbers each change of it (`revision`); rca names a login session (`auth`) and says only how it stands now, so this keeps the
//! flow id, its owner and the last state seen, and counts a revision whenever the phase or the prompt changes.
use agent_api_accounts::{AUTH_CANCEL, AUTH_COMPLETE, AUTH_RESPOND, AUTH_START};
use orca_core::wire::{Failure, RUNTIME_ERROR};
use orca_core::AgentApi;
use serde_json::{json, Map, Value};
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const NOT_ACTIVE: &str = "This sign-in is no longer active in this client.";
const WAIT: Duration = Duration::from_secs(20);
const STEP: Duration = Duration::from_millis(150);
/// The pages a provider's sign-in may open: the host publishes the address and the client opens it, so neither trusts the other.
const PAGES: [(&str, &[&str]); 2] = [("claude", &["claude.com", "claude.ai"]), ("codex", &["auth.openai.com"])];

struct Flow {
    id: String,
    owner: String,
    auth: String,
    provider: &'static str,
    revision: u64,
    phase: &'static str,
    prompt: Option<Value>,
    error: Option<String>,
    code_sent: bool,
}

#[derive(Default)]
pub struct SignIns {
    latest: Mutex<Vec<Flow>>,
}

fn uuid() -> String {
    let (a, b) = (RandomState::new().build_hasher().finish(), RandomState::new().build_hasher().finish());
    let hex = format!("{a:016x}{b:016x}");
    format!("{}-{}-4{}-a{}-{}", &hex[0..8], &hex[8..12], &hex[13..16], &hex[17..20], &hex[20..32])
}

fn refused(message: &str) -> Failure {
    Failure::new(RUNTIME_ERROR, message)
}

/// `claude` and `codex` as rca's provider names, and the page host check of the sign-in address.
fn rca_provider(provider: &str) -> (&'static str, &'static str) {
    if provider == "claude" {
        ("claude", "claudeAgent")
    } else {
        ("codex", "codex")
    }
}

fn is_sign_in_page(provider: &str, url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    PAGES.iter().any(|(p, hosts)| *p == provider && hosts.contains(&host))
}

/// The phase and prompt of a login session as rca reports it, with the phase the flow had when a code was already sent.
fn read(flow: &Flow, view: &Value) -> (&'static str, Option<Value>, Option<String>) {
    match view["status"].as_str() {
        Some("succeeded") => ("signedIn", None, None),
        Some("failed") => ("failed", None, Some(view["error"].as_str().unwrap_or("The sign-in failed.").chars().take(600).collect())),
        Some("cancelled") => ("cancelled", None, None),
        _ => match view["url"].as_str().filter(|url| is_sign_in_page(flow.provider, url)) {
            Some(url) if flow.code_sent => ("verifying", Some(prompt(flow.provider, url, &view["code"])), None),
            Some(url) => ("waiting", Some(prompt(flow.provider, url, &view["code"])), None),
            None => (if flow.code_sent { "verifying" } else { "starting" }, None, None),
        },
    }
}

fn prompt(provider: &str, url: &str, code: &Value) -> Value {
    json!({"url": url, "userCode": if provider == "codex" { code.clone() } else { Value::Null }, "needsCode": provider == "claude"})
}

fn settled(phase: &str) -> bool {
    matches!(phase, "signedIn" | "failed" | "cancelled")
}

impl Flow {
    fn observe(&mut self, view: &Value) {
        if settled(self.phase) {
            return;
        }
        let (phase, prompt, error) = read(self, view);
        if phase != self.phase || prompt != self.prompt || error != self.error {
            (self.phase, self.prompt, self.error) = (phase, prompt, error);
            self.revision += 1;
        }
    }

    /// The status as the phone reads it; a signed-in flow carries the account lists `accounts` gives.
    fn status(&self, accounts: &dyn Fn() -> (Value, Value)) -> Value {
        let mut out = Map::new();
        out.insert("flowId".into(), json!(self.id));
        out.insert("provider".into(), json!(self.provider));
        out.insert("revision".into(), json!(self.revision));
        out.insert("phase".into(), json!(self.phase));
        if let Some(prompt) = &self.prompt {
            out.insert("prompt".into(), prompt.clone());
        }
        if let Some(error) = &self.error {
            out.insert("error".into(), json!(error));
        }
        if self.phase == "signedIn" {
            let (claude, codex) = accounts();
            out.insert(self.provider.into(), if self.provider == "claude" { claude } else { codex });
        }
        Value::Object(out)
    }
}

impl SignIns {
    fn with<R>(&self, owner: &str, id: &str, f: impl FnOnce(&mut Flow) -> Result<R, Failure>) -> Result<R, Failure> {
        let mut flows = self.latest.lock().unwrap_or_else(|p| p.into_inner());
        let flow = flows.iter_mut().find(|f| f.owner == owner && f.id == id).ok_or_else(|| refused(NOT_ACTIVE))?;
        f(flow)
    }

    fn refresh(&self, agent: &dyn AgentApi, owner: &str, id: &str) -> Result<(), Failure> {
        let auth = self.with(owner, id, |f| Ok(f.auth.clone()))?;
        // A session that rca no longer knows has ended with nothing to show: the flow stays as it was last seen
        if let Ok(done) = agent.request(AUTH_COMPLETE.name(), json!({"auth": auth, "timeout_ms": 0})) {
            self.with(owner, id, |f| {
                f.observe(&done["auth"]);
                Ok(())
            })?;
        }
        Ok(())
    }

    pub fn start(&self, agent: &dyn AgentApi, owner: &str, provider: &str, accounts: &dyn Fn() -> (Value, Value)) -> Result<Value, Failure> {
        let (shown, rca) = rca_provider(provider);
        {
            let flows = self.latest.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(running) = flows.iter().find(|f| !settled(f.phase)) {
                if running.owner == owner && running.provider == shown {
                    return Ok(running.status(accounts));
                }
                return Err(refused("Another account sign-in is already running on this host. Finish or cancel it first."));
            }
        }
        let started = agent.request(AUTH_START.name(), json!({"provider": rca})).map_err(|e| match e {
            orca_core::AgentError::Refused { code, message } if code == "AUTH_IN_PROGRESS" => refused(&message),
            orca_core::AgentError::Refused { message, .. } => refused(&message),
            orca_core::AgentError::Failed(_) => Failure::new("runtime_unavailable", "runtime_unavailable"),
        })?;
        let mut flow = Flow { id: uuid(), owner: owner.into(), auth: started["auth"]["auth"].as_str().unwrap_or_default().into(), provider: shown, revision: 1, phase: "starting", prompt: None, error: None, code_sent: false };
        let first = flow.status(accounts);
        flow.observe(&started["auth"]);
        // The first answer is the state before the CLI printed anything, as the recorded host's is; a prompt already printed is read by the next wait
        let mut flows = self.latest.lock().unwrap_or_else(|p| p.into_inner());
        flows.retain(|f| f.owner != owner);
        flows.push(flow);
        Ok(first)
    }

    pub fn wait(&self, agent: &dyn AgentApi, owner: &str, id: &str, after: u64, accounts: &dyn Fn() -> (Value, Value)) -> Result<Value, Failure> {
        self.with(owner, id, |_| Ok(()))?;
        let until = Instant::now() + WAIT;
        loop {
            self.refresh(agent, owner, id)?;
            let (changed, status) = self.with(owner, id, |f| Ok((f.revision > after || settled(f.phase), f.status(accounts))))?;
            if changed || Instant::now() >= until {
                return Ok(status);
            }
            std::thread::sleep(STEP);
        }
    }

    pub fn submit_code(&self, agent: &dyn AgentApi, owner: &str, id: &str, code: &str, accounts: &dyn Fn() -> (Value, Value)) -> Result<Value, Failure> {
        self.refresh(agent, owner, id)?;
        let auth = self.with(owner, id, |f| {
            if f.phase != "waiting" || !f.prompt.as_ref().is_some_and(|p| p["needsCode"] == true) {
                return Err(refused("This sign-in is not waiting for a code."));
            }
            if f.code_sent {
                return Err(refused("That code was already sent. Wait for the sign-in to finish."));
            }
            f.code_sent = true;
            Ok(f.auth.clone())
        })?;
        let answered = agent.request(AUTH_RESPOND.name(), json!({"auth": auth, "text": code}));
        self.with(owner, id, |f| {
            match &answered {
                Ok(done) => f.observe(&done["auth"]),
                Err(_) => f.code_sent = false,
            }
            Ok(())
        })?;
        answered.map_err(|e| refused(&e.to_string()))?;
        self.with(owner, id, |f| Ok(f.status(accounts)))
    }

    pub fn cancel(&self, agent: &dyn AgentApi, owner: &str, id: &str, accounts: &dyn Fn() -> (Value, Value)) -> Result<Value, Failure> {
        let (auth, ended) = self.with(owner, id, |f| Ok((f.auth.clone(), settled(f.phase))))?;
        if !ended {
            let done = agent.request(AUTH_CANCEL.name(), json!({"auth": auth}));
            self.with(owner, id, |f| {
                if let Ok(done) = &done {
                    f.observe(&done["auth"]);
                }
                Ok(())
            })?;
        }
        self.refresh(agent, owner, id)?;
        self.with(owner, id, |f| Ok(f.status(accounts)))
    }
}
