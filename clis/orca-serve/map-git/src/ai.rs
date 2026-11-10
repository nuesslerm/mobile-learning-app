//! `git.generateCommitMessage`: rca's `git.ai` group has a model write the message from the executor's diff and commits nothing; the phone sends the message
//! to `git.commit`. Orca's per-request overrides of the agent and model are not honoured: the service's own model API is the one that writes.
use crate::{AgentError, Repo};
use agent_api_git::ai::{code, GENERATE_COMMIT_MESSAGE};
use orca_core::wire::{Failure, RUNTIME_ERROR};
use serde_json::{json, Value};

pub(crate) fn generate_commit_message(repo: &Repo) -> Result<Value, Failure> {
    match repo.try_ask(GENERATE_COMMIT_MESSAGE, json!({"staged": true})) {
        Ok(answer) => match answer["message"].as_str().filter(|m| !m.trim().is_empty()) {
            Some(message) => Ok(json!({"success": true, "message": message})),
            None => Err(Failure::new(RUNTIME_ERROR, "rca's git.generateCommitMessage answered no message")),
        },
        // The host answers a message it could not write as a failed generation, which the phone shows as the reason.
        Err(AgentError::Refused { code: c, message }) if c == code::NOTHING_TO_COMMIT || c == code::MODEL_API_UNAVAILABLE => Ok(json!({"success": false, "error": message})),
        Err(e) => Err(crate::refusal(e)),
    }
}
