//! A double of rca's terminal group: terminals held in memory, answered in the shapes of the contract (`agent-api-terminal`). The chain runs
//! the real service and executor against the same calls, so a shape that drifts fails there.
use orca_core::testing::ScriptedAgent;
use orca_core::AgentError;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct State {
    /// (workspace, terminal) -> (title, running).
    terminals: BTreeMap<(String, String), (String, bool)>,
    written: Vec<(String, String, String)>,
    cleared: Vec<String>,
}

#[derive(Clone, Default)]
pub struct Terminals(Arc<Mutex<State>>);

fn refused(code: &str, message: impl Into<String>) -> AgentError {
    AgentError::Refused { code: code.into(), message: message.into() }
}

impl Terminals {
    pub fn terminal(&self, workspace: &str, name: &str, running: bool) -> &Self {
        self.0.lock().unwrap().terminals.insert((workspace.into(), name.into()), (name.into(), running));
        self
    }

    /// What `terminal.write` was sent, as (workspace, terminal, data).
    pub fn written(&self) -> Vec<(String, String, String)> {
        self.0.lock().unwrap().written.clone()
    }

    pub fn cleared(&self) -> Vec<String> {
        self.0.lock().unwrap().cleared.clone()
    }

    pub fn title(&self, workspace: &str, name: &str) -> Option<String> {
        self.0.lock().unwrap().terminals.get(&(workspace.into(), name.into())).map(|(t, _)| t.clone())
    }

    pub fn exists(&self, workspace: &str, name: &str) -> bool {
        self.title(workspace, name).is_some()
    }

    /// Makes `agent` answer the methods of `USES` from this terminal set.
    pub fn install(&self, agent: &ScriptedAgent) {
        let me = self.clone();
        agent.reply("terminal.list", move |p| {
            let w = p["workspace"].as_str().unwrap_or_default();
            let s = me.0.lock().unwrap();
            let rows: Vec<Value> = s.terminals.iter().filter(|((fw, _), _)| fw == w).map(|((fw, name), (title, running))| json!({"workspace": fw, "terminal": name, "title": title, "cwd": "/r", "cols": 80, "rows": 24, "status": if *running { "running" } else { "exited" }, "exit_code": null, "pid": 7, "offset": 0, "base_offset": 0, "updated_at": 1_700_000_000_000u64})).collect();
            Ok(json!({"workspace": w, "terminals": rows}))
        });
        let me = self.clone();
        agent.reply("terminal.write", move |p| {
            let (w, t, data) = (p["workspace"].as_str().unwrap_or_default(), p["terminal"].as_str().unwrap_or_default(), p["data"].as_str().unwrap_or_default());
            let mut s = me.0.lock().unwrap();
            match s.terminals.get(&(w.into(), t.into())) {
                None => Err(refused("UNKNOWN_TERMINAL", format!("no terminal {t}"))),
                Some((_, false)) => Err(refused("TERMINAL_EXITED", format!("{t} has ended"))),
                Some(_) => {
                    s.written.push((w.into(), t.into(), data.into()));
                    Ok(json!({"bytes": data.len()}))
                }
            }
        });
        let me = self.clone();
        agent.reply("terminal.close", move |p| {
            let key = (p["workspace"].as_str().unwrap_or_default().to_string(), p["terminal"].as_str().unwrap_or_default().to_string());
            me.0.lock().unwrap().terminals.remove(&key).map(|_| json!({"workspace": key.0, "terminal": key.1})).ok_or_else(|| refused("UNKNOWN_TERMINAL", format!("no terminal {}", key.1)))
        });
        let me = self.clone();
        agent.reply("terminal.clear", move |p| {
            let key = (p["workspace"].as_str().unwrap_or_default().to_string(), p["terminal"].as_str().unwrap_or_default().to_string());
            let mut s = me.0.lock().unwrap();
            if !s.terminals.contains_key(&key) {
                return Err(refused("UNKNOWN_TERMINAL", format!("no terminal {}", key.1)));
            }
            s.cleared.push(key.1.clone());
            Ok(json!({"workspace": key.0, "terminal": key.1}))
        });
        let me = self.clone();
        agent.reply("terminal.rename", move |p| {
            let key = (p["workspace"].as_str().unwrap_or_default().to_string(), p["terminal"].as_str().unwrap_or_default().to_string());
            let mut s = me.0.lock().unwrap();
            let entry = s.terminals.get_mut(&key).ok_or_else(|| refused("UNKNOWN_TERMINAL", format!("no terminal {}", key.1)))?;
            entry.0 = p["title"].as_str().unwrap_or_default().to_string();
            Ok(json!({"workspace": key.0, "terminal": key.1, "title": entry.0}))
        });
    }
}
