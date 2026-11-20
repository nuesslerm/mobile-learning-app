//! A double of rca's accounts group: the three provider CLIs, each signed in or out, one of them selected, answering `provider.list` and
//! `provider.select` in the shapes of the contract (`agent-api-accounts`) and changing as the service does (a signed-out provider cannot be selected).
use orca_core::testing::ScriptedAgent;
use orca_core::AgentError;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

/// The login session the double runs: what its CLI has printed and how it stands.
#[derive(Clone, Default)]
struct Auth {
    provider: String,
    status: String,
    url: Option<String>,
    code: Option<String>,
    error: Option<String>,
    responded: Option<String>,
}

struct State {
    providers: Vec<(String, bool)>,
    selected: String,
    auth: Option<Auth>,
    usage: Option<Value>,
    spends: Vec<(String, String)>,
    spend_outcome: String,
}

#[derive(Clone)]
pub struct Providers(Arc<Mutex<State>>);

impl Default for Providers {
    fn default() -> Self {
        Providers(Arc::new(Mutex::new(State { providers: ["claudeAgent", "codex", "opencode"].iter().map(|p| (p.to_string(), false)).collect(), selected: "opencode".into(), auth: None, usage: None, spends: Vec::new(), spend_outcome: "reset".into() })))
    }
}

fn list(s: &State) -> Value {
    let rows: Vec<Value> = s.providers.iter().map(|(name, in_)| json!({"provider": name, "status": if *in_ { "signed_in" } else { "signed_out" }, "selected": *name == s.selected, "auth_methods": ["device_code"]})).collect();
    json!({"providers": rows, "selected": s.selected})
}

impl Providers {
    pub fn signed_in(&self, provider: &str) -> &Self {
        for (name, in_) in &mut self.0.lock().unwrap().providers {
            if name == provider {
                *in_ = true;
            }
        }
        self
    }

    pub fn selected(&self) -> String {
        self.0.lock().unwrap().selected.clone()
    }

    /// What `usage.summary` answers (the whole result); with none, every provider has no limits and no error.
    pub fn usage_is(&self, summary: Value) -> &Self {
        self.0.lock().unwrap().usage = Some(summary);
        self
    }

    /// What the next spend answers (`reset`, `nothing_to_reset`, `no_credit`, `already_redeemed`).
    pub fn spend_answers(&self, outcome: &str) -> &Self {
        self.0.lock().unwrap().spend_outcome = outcome.into();
        self
    }

    /// The `(provider, key)` of each spend so far.
    pub fn spends(&self) -> Vec<(String, String)> {
        self.0.lock().unwrap().spends.clone()
    }

    /// The running login's CLI prints its address and code.
    pub fn prints(&self, url: &str, code: Option<&str>) -> &Self {
        if let Some(auth) = &mut self.0.lock().unwrap().auth {
            (auth.url, auth.code) = (Some(url.into()), code.map(str::to_string));
        }
        self
    }

    /// The running login ends signed in (the provider holds a login now) or failed with `error`.
    pub fn login_ends(&self, signed_in: bool, error: Option<&str>) -> &Self {
        let mut s = self.0.lock().unwrap();
        if let Some(auth) = &mut s.auth {
            auth.status = if signed_in { "succeeded" } else { "failed" }.into();
            auth.error = error.map(str::to_string);
            let name = auth.provider.clone();
            if signed_in {
                for (n, in_) in &mut s.providers {
                    if *n == name {
                        *in_ = true;
                    }
                }
            }
        }
        self
    }

    /// The pasted text the running login received, if any.
    pub fn pasted(&self) -> Option<String> {
        self.0.lock().unwrap().auth.as_ref().and_then(|a| a.responded.clone())
    }

    /// Makes `agent` answer the methods of `USES` from these providers.
    pub fn install(&self, agent: &ScriptedAgent) {
        let me = self.clone();
        agent.reply("provider.list", move |_| Ok(list(&me.0.lock().unwrap())));
        let me = self.clone();
        agent.reply("provider.select", move |p| {
            let mut s = me.0.lock().unwrap();
            let name = p["provider"].as_str().ok_or_else(|| AgentError::Refused { code: "INVALID_PARAMS".into(), message: "`provider` is required".into() })?.to_string();
            match s.providers.iter().find(|(n, _)| *n == name) {
                None => Err(AgentError::Refused { code: "UNKNOWN_PROVIDER".into(), message: format!("{name} is not a provider of this service") }),
                Some((_, false)) => Err(AgentError::Refused { code: "SIGNED_OUT".into(), message: format!("{name} holds no login; sign it in first") }),
                Some(_) => {
                    s.selected = name;
                    Ok(list(&s))
                }
            }
        });
        let me = self.clone();
        agent.reply("usage.summary", move |_| {
            let s = me.0.lock().unwrap();
            Ok(s.usage.clone().unwrap_or_else(|| json!({"providers": s.providers.iter().map(|(n, in_)| json!({"provider": n, "status": if *in_ { "signed_in" } else { "signed_out" }, "limits": null, "limits_error": null})).collect::<Vec<_>>()})))
        });
        let me = self.clone();
        agent.reply("provider.resetCredit", move |p| {
            let mut s = me.0.lock().unwrap();
            let name = p["provider"].as_str().unwrap_or_default().to_string();
            s.spends.push((name.clone(), p["idempotency_key"].as_str().unwrap_or_default().to_string()));
            Ok(json!({"provider": name, "outcome": s.spend_outcome}))
        });
        let view = |a: &Auth| json!({"auth": {"auth": "auth-1", "provider": a.provider, "method": if a.provider == "codex" { "device_code" } else { "paste" }, "status": a.status, "url": a.url, "code": a.code, "error": a.error}});
        let me = self.clone();
        agent.reply("provider.auth.start", move |p| {
            let mut s = me.0.lock().unwrap();
            let provider = p["provider"].as_str().unwrap_or_default().to_string();
            if s.auth.as_ref().is_some_and(|a| a.status == "pending") {
                return Err(AgentError::Refused { code: "AUTH_IN_PROGRESS".into(), message: "a login is already running".into() });
            }
            let auth = Auth { provider, status: "pending".into(), ..Auth::default() };
            let shown = view(&auth);
            s.auth = Some(auth);
            Ok(shown)
        });
        let me = self.clone();
        agent.reply("provider.auth.complete", move |_| me.0.lock().unwrap().auth.as_ref().map(view).ok_or(AgentError::Refused { code: "UNKNOWN_AUTH".into(), message: "no login".into() }));
        let me = self.clone();
        agent.reply("provider.auth.respond", move |p| {
            let mut s = me.0.lock().unwrap();
            let auth = s.auth.as_mut().ok_or(AgentError::Refused { code: "UNKNOWN_AUTH".into(), message: "no login".into() })?;
            auth.responded = p["text"].as_str().map(str::to_string);
            Ok(view(auth))
        });
        let me = self.clone();
        agent.reply("provider.auth.cancel", move |_| {
            let mut s = me.0.lock().unwrap();
            let auth = s.auth.as_mut().ok_or(AgentError::Refused { code: "UNKNOWN_AUTH".into(), message: "no login".into() })?;
            auth.status = "cancelled".into();
            Ok(view(auth))
        });
    }

    /// Makes `provider.list` answer `answer` whatever the providers are.
    pub fn answer_list_with(&self, agent: &ScriptedAgent, answer: Value) {
        agent.reply("provider.list", move |_| Ok(answer.clone()));
    }
}
