//! What an agent CLI reports while it works, as `{ type, properties }`; `translate` turns these into the engine's commands.

use serde_json::Value;

#[derive(Debug, Clone)]
pub struct AgentEvent {
    pub kind: String,
    pub props: Value,
}

impl AgentEvent {
    pub fn new(kind: &str, props: Value) -> Self {
        Self { kind: kind.into(), props }
    }

    pub fn session_id(&self) -> Option<&str> {
        self.props.get("sessionID").and_then(Value::as_str)
    }

    pub fn str_prop(&self, name: &str) -> Option<&str> {
        self.props.get(name).and_then(Value::as_str)
    }
}
