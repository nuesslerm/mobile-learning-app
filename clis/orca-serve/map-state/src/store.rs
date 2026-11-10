//! The values under the scope `orca`: read with the port's `state_get`, written with `state.set`. A stored value of the wrong shape is a refusal, not an empty one.
use agent_api_state::SET;
use orca_core::wire::{Failure, RUNTIME_ERROR};
use orca_core::{AgentApi, AgentError};
use serde_json::{json, Map, Value};

pub(crate) const SCOPE: &str = "orca";
pub(crate) const SETTINGS: &str = "settings";
pub(crate) const UI: &str = "ui";
pub(crate) const QUICK_COMMANDS: &str = "terminalQuickCommands";

fn refusal(error: AgentError) -> Failure {
    match error {
        AgentError::Refused { message, .. } => Failure::new(RUNTIME_ERROR, message),
        AgentError::Failed(_) => Failure::new("runtime_unavailable", "runtime_unavailable"),
    }
}

fn kept(agent: &dyn AgentApi, key: &str) -> Result<Option<Value>, Failure> {
    Ok(agent.state_get(SCOPE, key).map_err(refusal)?.filter(|v| !v.is_null()))
}

pub(crate) fn read_object(agent: &dyn AgentApi, key: &str) -> Result<Map<String, Value>, Failure> {
    match kept(agent, key)? {
        None => Ok(Map::new()),
        Some(Value::Object(map)) => Ok(map),
        Some(_) => Err(Failure::new(RUNTIME_ERROR, format!("the value kept for {key} is not an object"))),
    }
}

pub(crate) fn read_list(agent: &dyn AgentApi, key: &str) -> Result<Vec<Value>, Failure> {
    match kept(agent, key)? {
        None => Ok(Vec::new()),
        Some(Value::Array(list)) => Ok(list),
        Some(_) => Err(Failure::new(RUNTIME_ERROR, format!("the value kept for {key} is not a list"))),
    }
}

pub(crate) fn write(agent: &dyn AgentApi, key: &str, value: Value) -> Result<(), Failure> {
    agent.request(SET.name(), json!({"scope": SCOPE, "key": key, "value": value})).map(|_| ()).map_err(refusal)
}

/// What Orca derives for every key a phone has not set: the pinned host's defaults (`settings.get` and `ui.get` of a fresh host), under what was set.
pub(crate) fn with_defaults(key: &str, kept: Map<String, Value>) -> Value {
    const DEFAULTS: &str = include_str!("defaults.json");
    let mut all: Value = serde_json::from_str(DEFAULTS).expect("defaults.json parses");
    let mut merged = all[key].take().as_object().cloned().unwrap_or_default();
    merged.extend(kept);
    Value::Object(merged)
}
