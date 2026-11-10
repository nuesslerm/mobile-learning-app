//! `git.branchCompare` and `git.commitCompare`: the files a branch or a commit changed, with the summary the phone's review shows.
use crate::{text, Repo};
use agent_api_git::{COMPARE, HISTORY, STATUS};
use orca_core::wire::Failure;
use orca_core::AgentError;
use serde_json::{json, Value};

const FULL_OID: usize = 40;

fn change_status(status: &str) -> &'static str {
    match status {
        "added" => "added",
        "deleted" => "deleted",
        "renamed" => "renamed",
        "copied" => "copied",
        _ => "modified",
    }
}

fn entries(compared: &Value) -> Vec<Value> {
    compared["files"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| {
            let mut row = json!({"path": f["path"], "status": change_status(f["status"].as_str().unwrap_or_default())});
            for (key, from) in [("oldPath", "orig_path"), ("added", "additions"), ("removed", "deletions")] {
                if let (Some(v), Some(map)) = (f.get(from).filter(|v| !v.is_null()), row.as_object_mut()) {
                    map.insert(key.into(), v.clone());
                }
            }
            row
        })
        .collect()
}

/// The commit `rev` names, as the executor's log gives it.
fn commit_of(repo: &Repo, rev: &str) -> Result<Value, AgentError> {
    Ok(repo.try_ask(HISTORY, json!({"rev": rev, "limit": 1}))?["commits"].get(0).cloned().unwrap_or(Value::Null))
}

/// The commits of `head` against `base`: a base git cannot resolve is the summary's `invalid-base`, not a failure of the call.
pub(crate) fn branch(repo: &Repo, params: &Value) -> Result<Value, Failure> {
    let base = text(params, "baseRef").filter(|b| !b.starts_with('-')).ok_or_else(|| Failure::invalid("Missing base ref"))?;
    let status = repo.ask(STATUS, json!({}))?;
    let compare_ref = text(&status, "branch").unwrap_or("HEAD");
    let head_oid = status.get("head").cloned().unwrap_or(Value::Null);
    let compared = match repo.try_ask(COMPARE, json!({"base": base, "head": "HEAD"})) {
        Ok(compared) => compared,
        Err(AgentError::Refused { code, message }) if code != "UNKNOWN_WORKSPACE" => {
            let summary = json!({"baseRef": base, "baseOid": null, "compareRef": compare_ref, "headOid": head_oid, "mergeBase": null, "changedFiles": 0, "status": "invalid-base", "errorMessage": message});
            return Ok(json!({"summary": summary, "entries": []}));
        }
        Err(other) => return Err(crate::refusal(other)),
    };
    let rows = entries(&compared);
    let summary = json!({
        "baseRef": base,
        "baseOid": commit_of(repo, base).map_err(crate::refusal)?["sha"],
        "compareRef": compare_ref,
        "headOid": head_oid,
        "mergeBase": compared["merge_base"],
        "changedFiles": rows.len(),
        "commitsAhead": compared["ahead"],
        "commitsBehind": compared["behind"],
        "status": "ready",
    });
    Ok(json!({"summary": summary, "entries": rows}))
}

fn invalid_commit(id: &str, message: String) -> Value {
    json!({"summary": {"commitOid": id, "parentOid": null, "compareRef": id, "baseRef": "", "changedFiles": 0, "status": "invalid-commit", "errorMessage": message}, "entries": []})
}

/// What one commit changed against its first parent. A root commit has no parent to compare with, which the summary says.
pub(crate) fn commit(repo: &Repo, params: &Value) -> Result<Value, Failure> {
    let id = text(params, "commitId").filter(|id| (id.len() == FULL_OID || id.len() == 2 * FULL_OID) && id.chars().all(|c| c.is_ascii_hexdigit())).ok_or_else(|| Failure::invalid("Expected a full git object id"))?;
    let found = match commit_of(repo, id) {
        Ok(found) if found.is_object() => found,
        Ok(_) => return Ok(invalid_commit(id, "no such commit".into())),
        Err(AgentError::Refused { code, message }) if code != "UNKNOWN_WORKSPACE" => return Ok(invalid_commit(id, message)),
        Err(other) => return Err(crate::refusal(other)),
    };
    let Some(parent) = found["parents"].get(0).and_then(Value::as_str) else {
        return Ok(json!({"summary": {"commitOid": id, "parentOid": null, "compareRef": id, "baseRef": "", "changedFiles": 0, "status": "error", "errorMessage": "a root commit has no parent to compare with"}, "entries": []}));
    };
    let compared = repo.ask(COMPARE, json!({"base": parent, "head": id}))?;
    let rows = entries(&compared);
    Ok(json!({"summary": {"commitOid": id, "parentOid": parent, "compareRef": id, "baseRef": parent, "changedFiles": rows.len(), "status": "ready"}, "entries": rows}))
}
