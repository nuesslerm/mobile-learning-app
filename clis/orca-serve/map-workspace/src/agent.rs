//! What the mapper asks of the agent API, and how a refusal reads on the phone's wire.
use orca_core::wire::{Failure, RUNTIME_ERROR};
use orca_core::{AgentApi, AgentError};
use serde_json::Value;

/// The host's plain-error messages are its codes (`errors.ts`): a refusal the service names with a code of its own reads as the message the
/// host would throw for it, anything else as `runtime_error` with the executor's sentence.
pub(crate) fn refusal(error: AgentError) -> Failure {
    match error {
        AgentError::Refused { code, message } => match code.as_str() {
            "UNKNOWN_WORKSPACE" => Failure::new("selector_not_found", "selector_not_found"),
            "INVALID_PARAMS" | "INVALID_ARGS" => Failure::invalid(message),
            _ => Failure::new(RUNTIME_ERROR, message),
        },
        AgentError::Failed(_) => Failure::new("runtime_unavailable", "runtime_unavailable"),
    }
}

pub(crate) fn ask(agent: &dyn AgentApi, method: &'static str, params: Value) -> Result<Value, Failure> {
    agent.request(method, params).map_err(refusal)
}
