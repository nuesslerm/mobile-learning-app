//! `git.generateCommitMessage` and `git.generatePullRequestFields`: rca's `git.ai` group has a model write the message from the executor's diff, or the
//! title, body and draft flag from what the branch changed since its base, and commits and opens nothing; the phone sends the message to `git.commit` and the
//! fields to the pull request it creates. Orca's per-request overrides of the agent and model are not honoured: the service's own model API is the one that writes.
use crate::{AgentError, Repo};
use agent_api_git::ai::{code, GENERATE_COMMIT_MESSAGE, GENERATE_PULL_REQUEST_FIELDS};
use orca_core::wire::{received, Failure, RUNTIME_ERROR};
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

/// The schema of `git.generatePullRequestFields` in its key order: a selector, a base, the form's title and body, and its draft flag.
pub(crate) fn check_pull_request_fields(params: &Value) -> Result<(), Failure> {
    params.get("worktree").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or_else(|| Failure::invalid("Missing worktree selector"))?;
    let wrong = |key: &str, kind: &str| {
        let seen = params.get(key).map_or("undefined", received);
        Failure::invalid(format!("Invalid input: expected {kind}, received {seen}"))
    };
    match params.get("base") {
        Some(Value::String(base)) if base.is_empty() => return Err(Failure::invalid("Missing base branch")),
        Some(Value::String(_)) => {}
        _ => return Err(wrong("base", "string")),
    }
    for key in ["title", "body"] {
        if !params.get(key).is_some_and(Value::is_string) {
            return Err(wrong(key, "string"));
        }
    }
    if !params.get("draft").is_some_and(Value::is_boolean) {
        return Err(wrong("draft", "boolean"));
    }
    Ok(())
}

pub(crate) fn generate_pull_request_fields(repo: &Repo, params: &Value) -> Result<Value, Failure> {
    let asked = json!({"base": params["base"], "title": params["title"], "body": params["body"], "draft": params["draft"]});
    match repo.try_ask(GENERATE_PULL_REQUEST_FIELDS, asked) {
        Ok(answer) => match (answer["base"].as_str(), answer["title"].as_str().filter(|t| !t.trim().is_empty()), answer["body"].as_str(), answer["draft"].as_bool()) {
            (Some(base), Some(title), Some(body), Some(draft)) => Ok(json!({"success": true, "fields": {"base": base, "title": title, "body": body, "draft": draft}})),
            _ => Err(Failure::new(RUNTIME_ERROR, "rca's git.generatePullRequestFields answered no title, body and draft flag")),
        },
        Err(AgentError::Refused { code: c, message }) if c == code::NO_BRANCH_CHANGES || c == code::MODEL_API_UNAVAILABLE => Ok(json!({"success": false, "error": message})),
        Err(e) => Err(crate::refusal(e)),
    }
}
