//! A double of rca's accounts group: the three provider CLIs, each signed in or out, one of them selected, answering `provider.list` and
//! `provider.select` in the shapes of the contract (`agent-api-accounts`) and changing as the service does (a signed-out provider cannot be selected).
use orca_core::testing::ScriptedAgent;
use orca_core::AgentError;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

struct State {
    providers: Vec<(String, bool)>,
    selected: String,
}

#[derive(Clone)]
pub struct Providers(Arc<Mutex<State>>);

impl Default for Providers {
    fn default() -> Self {
        Providers(Arc::new(Mutex::new(State { providers: ["claudeAgent", "codex", "opencode"].iter().map(|p| (p.to_string(), false)).collect(), selected: "opencode".into() })))
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
    }

    /// Makes `provider.list` answer `answer` whatever the providers are.
    pub fn answer_list_with(&self, agent: &ScriptedAgent, answer: Value) {
        agent.reply("provider.list", move |_| Ok(answer.clone()));
    }
}
