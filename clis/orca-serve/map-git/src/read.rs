//! The rows that read the repository: status, upstream, history, and the local branches; the status carries the operation a conflict belongs to.
use crate::{text, Repo};
use agent_api_git::{HISTORY, REFS, STATUS, UPSTREAM_STATUS};
use orca_core::wire::{Failure, RUNTIME_ERROR};
use serde_json::{json, Map, Value};

const HISTORY_LIMIT: u64 = 50;

fn file_status(column: &str) -> &'static str {
    match column {
        "A" => "added",
        "D" => "deleted",
        "R" => "renamed",
        "C" => "copied",
        _ => "modified",
    }
}

/// What git's unmerged pair of columns says about who changed the file.
fn conflict_kind(index: &str, worktree: &str) -> &'static str {
    match (index, worktree) {
        ("D", "D") => "both_deleted",
        ("A", "U") => "added_by_us",
        ("U", "D") => "deleted_by_them",
        ("U", "A") => "added_by_them",
        ("D", "U") => "deleted_by_us",
        ("A", "A") => "both_added",
        _ => "both_modified",
    }
}

/// A file of the executor's status as the rows the phone lists: a staged one and an unstaged one when both columns changed, a conflict as one
/// unresolved unstaged row, an untracked file as one untracked row.
fn entries(files: &[Value]) -> Result<Vec<Value>, Failure> {
    let mut rows = Vec::new();
    for f in files {
        let field = |name: &str| f[name].as_str().ok_or_else(|| incomplete("git.status", &format!("a file with no {name}")));
        let (path, index, worktree) = (field("path")?, field("index")?, field("worktree")?);
        if f["conflicted"] == true {
            rows.push(json!({"path": path, "status": "modified", "area": "unstaged", "conflictKind": conflict_kind(index, worktree), "conflictStatus": "unresolved"}));
        } else if index == "?" {
            rows.push(json!({"path": path, "status": "untracked", "area": "untracked"}));
        } else {
            for (area, column) in [("staged", index), ("unstaged", worktree)] {
                if column == "." {
                    continue;
                }
                let mut row = Map::new();
                row.extend([("path".to_string(), json!(path)), ("status".into(), json!(file_status(column))), ("area".into(), json!(area))]);
                if let Some(old) = f["orig_path"].as_str().filter(|_| column == "R" || column == "C") {
                    row.insert("oldPath".into(), json!(old));
                }
                rows.push(Value::Object(row));
            }
        }
    }
    Ok(rows)
}

/// An answer of rca that lacks what its contract promises is refused, never read as an empty one.
fn incomplete(method: &str, what: &str) -> Failure {
    Failure::new(RUNTIME_ERROR, format!("rca's {method} answered {what}"))
}

fn operation_of(status: &Value) -> &'static str {
    match status["operation"].as_str() {
        Some("merge") => "merge",
        Some("rebase") => "rebase",
        Some("cherry-pick") => "cherry-pick",
        _ => "unknown",
    }
}

fn upstream_of(status: &Value) -> Value {
    match text(status, "upstream") {
        Some(name) => json!({"hasUpstream": true, "upstreamName": name, "ahead": status["ahead"], "behind": status["behind"]}),
        None => json!({"hasUpstream": false, "ahead": 0, "behind": 0}),
    }
}

pub(crate) fn status(repo: &Repo) -> Result<Value, Failure> {
    let status = repo.ask(STATUS, json!({}))?;
    let mut result = Map::new();
    let files = status["files"].as_array().ok_or_else(|| incomplete("git.status", "no files list"))?;
    result.insert("entries".into(), json!(entries(files)?));
    result.insert("conflictOperation".into(), json!(operation_of(&status)));
    for (key, from) in [("head", "head"), ("branch", "branch")] {
        if let Some(v) = text(&status, from) {
            result.insert(key.into(), json!(v));
        }
    }
    result.insert("upstreamStatus".into(), upstream_of(&status));
    if status["truncated"] == true {
        result.insert("didHitLimit".into(), json!(true));
    }
    Ok(Value::Object(result))
}

pub(crate) fn upstream_status(repo: &Repo, params: &Value) -> Result<Value, Failure> {
    if params.get("pushTarget").is_some_and(|t| !t.is_null()) {
        return Err(Failure::invalid("pushTarget is not honoured: this host reports the branch's own upstream"));
    }
    let up = repo.ask(UPSTREAM_STATUS, json!({}))?;
    let mut result = json!({"hasUpstream": up["has_upstream"] == true, "ahead": up["ahead"], "behind": up["behind"]});
    if let (Some(name), Some(map)) = (text(&up, "upstream"), result.as_object_mut()) {
        map.insert("upstreamName".into(), json!(name));
    }
    Ok(result)
}

pub(crate) fn local_branches(repo: &Repo) -> Result<Value, Failure> {
    let refs = repo.ask(REFS, json!({"kind": "branch"}))?;
    let names: Vec<Value> = refs["refs"].as_array().into_iter().flatten().filter_map(|r| r.get("name").cloned()).collect();
    Ok(json!({"current": refs.get("current").filter(|c| c.is_string()).cloned().unwrap_or(Value::Null), "branches": names}))
}

fn commit_row(c: &Value) -> Result<Value, Failure> {
    let field = |name: &str| c[name].as_str().ok_or_else(|| incomplete("git.history", &format!("a commit with no {name}")));
    let (subject, body) = (field("subject")?, field("body")?.trim());
    let message = if body.is_empty() { subject.to_string() } else { format!("{subject}\n\n{body}") };
    Ok(json!({"id": field("sha")?, "parentIds": c["parents"], "subject": subject, "message": message, "displayId": c["short"], "author": c["author_name"], "authorEmail": c["author_email"], "timestamp": c["authored_ms"]}))
}

pub(crate) fn history(repo: &Repo, params: &Value) -> Result<Value, Failure> {
    let limit = params.get("limit").and_then(Value::as_u64).unwrap_or(HISTORY_LIMIT);
    let skip = params.get("skip").and_then(Value::as_u64);
    let branch_only = params.get("branchOnly") == Some(&json!(true));
    let mut request = json!({"limit": limit});
    if let Some(skip) = skip {
        request["skip"] = json!(skip);
    }
    if let (Some(base), true) = (text(params, "baseRef"), branch_only) {
        request["rev"] = json!(format!("{base}..HEAD"));
    }
    let log = repo.ask(HISTORY, request)?;
    let status = repo.ask(STATUS, json!({}))?;
    let items = log["commits"].as_array().ok_or_else(|| incomplete("git.history", "no commits list"))?.iter().map(commit_row).collect::<Result<Vec<_>, _>>()?;
    let (ahead, behind) = (status["ahead"].as_u64().unwrap_or(0), status["behind"].as_u64().unwrap_or(0));
    let mut result = json!({"items": items, "hasIncomingChanges": behind > 0, "hasOutgoingChanges": ahead > 0, "hasMore": log["truncated"] == true, "limit": limit});
    if let Some(map) = result.as_object_mut() {
        if let Some(branch) = text(&status, "branch") {
            map.insert("currentRef".into(), json!({"id": format!("refs/heads/{branch}"), "name": branch, "revision": status["head"]}));
        }
        if let Some(upstream) = text(&status, "upstream") {
            map.insert("remoteRef".into(), json!({"id": format!("refs/remotes/{upstream}"), "name": upstream}));
        }
        if let Some(skip) = skip {
            map.insert("skip".into(), json!(skip));
        }
    }
    Ok(result)
}
