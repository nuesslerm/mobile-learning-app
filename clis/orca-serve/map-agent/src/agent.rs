//! What the mapper asks of the agent API, and how a refusal reads on the phone's wire.
use agent_api_agent::code as thread_code;
use orca_core::wire::{Failure, RUNTIME_ERROR};
use orca_core::{AgentApi, AgentError};
use serde_json::{json, Value};

pub(crate) const SCOPE: &str = "orca";

/// The host's plain-error messages are its codes: a refusal the service names with a code of its own reads as the message the host throws for it.
pub(crate) fn refusal(error: AgentError) -> Failure {
    match error {
        AgentError::Refused { code, message } => match code.as_str() {
            "UNKNOWN_WORKSPACE" => Failure::new("selector_not_found", "selector_not_found"),
            "INVALID_PARAMS" | "INVALID_ARGS" => Failure::invalid(message),
            c if c == thread_code::THREAD_NOT_FOUND => Failure::new(RUNTIME_ERROR, "agent_session_not_found"),
            c if c == thread_code::UNKNOWN_DRIVER => Failure::new(RUNTIME_ERROR, "structured_agent_session_unsupported"),
            _ => Failure::new(RUNTIME_ERROR, message),
        },
        AgentError::Failed(_) => Failure::new("runtime_unavailable", "runtime_unavailable"),
    }
}

pub(crate) fn ask(agent: &dyn AgentApi, method: &'static str, params: Value) -> Result<Value, Failure> {
    agent.request(method, params).map_err(refusal)
}

/// The code of a refusal the service answered, for a caller that treats one code as an outcome.
pub(crate) fn refused_code(e: &AgentError) -> Option<&str> {
    match e {
        AgentError::Refused { code, .. } => Some(code),
        AgentError::Failed(_) => None,
    }
}

pub(crate) fn state_set(agent: &dyn AgentApi, key: &str, value: Value) -> Result<(), Failure> {
    ask(agent, agent_api_state::SET.name(), json!({"scope": SCOPE, "key": key, "value": value})).map(drop)
}

pub(crate) fn state_get(agent: &dyn AgentApi, key: &str) -> Result<Option<Value>, Failure> {
    agent.state_get(SCOPE, key).map_err(refusal)
}
