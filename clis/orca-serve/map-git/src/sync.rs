//! The rows that move commits between the repository and its remote, and the merge and rebase a conflict leaves open. A `pushTarget` names the
//! remote and the branch of the same name on it; the remote must be one the repository already configured.
use crate::{text, Repo};
use agent_api_git::{FAST_FORWARD, FETCH, MERGE, PULL, PUSH, REBASE, STATUS};
use agent_api_proto::Method as AgentMethod;
use orca_core::wire::{Failure, RUNTIME_ERROR};
use orca_core::AgentError;
use serde_json::{json, Value};

struct Target {
    remote: String,
    branch: String,
}

fn target(params: &Value) -> Option<Target> {
    let t = params.get("pushTarget").filter(|t| t.is_object())?;
    Some(Target { remote: text(t, "remoteName")?.to_string(), branch: text(t, "branchName")?.to_string() })
}

fn conflicts(done: &Value) -> String {
    done["conflicts"].as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")
}

pub(crate) fn fetch(repo: &Repo, params: &Value) -> Result<Value, Failure> {
    let args = target(params).map_or(json!({}), |t| json!({"remote": t.remote}));
    repo.ask(FETCH, args)?;
    Ok(json!({"ok": true}))
}

pub(crate) fn pull(repo: &Repo, params: &Value) -> Result<Value, Failure> {
    let args = target(params).map_or(json!({}), |t| json!({"remote": t.remote, "branch": t.branch}));
    let done = repo.ask(PULL, args)?;
    if done["ok"] == false {
        return Err(Failure::new(RUNTIME_ERROR, format!("the pull stopped on conflicts: {}", conflicts(&done))));
    }
    Ok(json!({"ok": true}))
}

/// The branch is published and made the upstream, as the host's push always does. A target branch of another name than the current one is refused:
/// the group pushes a branch to its own name.
pub(crate) fn push(repo: &Repo, params: &Value) -> Result<Value, Failure> {
    let mut args = json!({"set_upstream": true});
    if params.get("forceWithLease") == Some(&json!(true)) {
        args["force_with_lease"] = json!(true);
    }
    if let Some(t) = target(params) {
        let current = repo.ask(STATUS, json!({}))?;
        if text(&current, "branch") != Some(t.branch.as_str()) {
            return Err(Failure::invalid(format!("pushTarget names {}, but the current branch is not that: a branch is pushed to its own name", t.branch)));
        }
        args["remote"] = json!(t.remote);
    }
    repo.ask(PUSH, args)?;
    Ok(json!({"ok": true}))
}

pub(crate) fn fast_forward(repo: &Repo, params: &Value) -> Result<Value, Failure> {
    let args = target(params).map_or(json!({}), |t| json!({"to": format!("{}/{}", t.remote, t.branch)}));
    repo.ask(FAST_FORWARD, args)?;
    Ok(json!({"ok": true}))
}

pub(crate) fn abort(repo: &Repo, method: AgentMethod) -> Result<Value, Failure> {
    repo.ask(method, json!({"op": "abort"}))?;
    Ok(json!({"ok": true}))
}

/// A merge that still has conflicts is `success: false` with the files named, as the host words an unfinished merge; a refusal keeps its sentence.
pub(crate) fn complete_merge(repo: &Repo) -> Result<Value, Failure> {
    match repo.try_ask(MERGE, json!({"op": "continue"})) {
        Ok(done) if done["ok"] == false => Ok(json!({"success": false, "error": format!("conflicts remain: {}", conflicts(&done))})),
        Ok(_) => Ok(json!({"success": true})),
        Err(AgentError::Refused { message, .. }) => Ok(json!({"success": false, "error": message})),
        Err(other) => Err(crate::refusal(other)),
    }
}

/// The rebase starts onto `baseRef` as it is; it does not fetch first. One that stops on conflicts stays open for `git.resolveConflict` and a continue
/// or `git.abortRebase`, and answers with the files.
pub(crate) fn rebase_from_base(repo: &Repo, params: &Value) -> Result<Value, Failure> {
    let base = text(params, "baseRef").filter(|b| !b.starts_with('-')).ok_or_else(|| Failure::invalid("Missing base ref"))?;
    let done = repo.ask(REBASE, json!({"op": "start", "ref": base}))?;
    if done["ok"] == false {
        return Err(Failure::new(RUNTIME_ERROR, format!("the rebase stopped on conflicts: {}", conflicts(&done))));
    }
    Ok(json!({"ok": true}))
}
