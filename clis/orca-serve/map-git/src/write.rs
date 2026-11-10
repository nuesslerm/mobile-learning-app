//! The rows that change the index, the working tree and the branch: stage, unstage, discard, commit, checkout and a conflict's resolution.
use crate::{paths_of, text, Repo};
use agent_api_git::{CHECKOUT, COMMIT, CONFLICT_RESOLVE, DISCARD, STAGE, STATUS, UNSTAGE};
use orca_core::wire::Failure;
use orca_core::AgentError;
use serde_json::{json, Value};

pub(crate) fn checkout(repo: &Repo, params: &Value) -> Result<Value, Failure> {
    let branch = text(params, "branch").ok_or_else(|| Failure::invalid("Missing branch"))?;
    repo.ask(CHECKOUT, json!({"ref": branch}))?;
    Ok(json!({"ok": true, "branch": branch}))
}

pub(crate) fn stage(repo: &Repo, params: &Value) -> Result<Value, Failure> {
    repo.ask(STAGE, json!({"paths": paths_of(params, "filePath")?}))?;
    Ok(json!({"ok": true}))
}

/// `scope` says what to stage whatever the list holds: `all` is the whole tree, `tracked` every file git already knows; with none, the list.
pub(crate) fn bulk_stage(repo: &Repo, params: &Value) -> Result<Value, Failure> {
    let scope = text(params, "scope");
    let paths = match scope {
        Some("all") => vec![".".to_string()],
        Some("tracked") => {
            let status = repo.ask(STATUS, json!({}))?;
            status["files"].as_array().into_iter().flatten().filter(|f| f["index"] != "?").filter_map(|f| f["path"].as_str()).map(str::to_string).collect()
        }
        Some(other) => return Err(Failure::invalid(format!("Unknown stage scope: {other}"))),
        None => paths_of(params, "filePaths")?,
    };
    if !paths.is_empty() {
        repo.ask(STAGE, json!({"paths": paths}))?;
    }
    Ok(match scope {
        Some(scope) => json!({"ok": true, "stagedScope": scope}),
        None => json!({"ok": true}),
    })
}

pub(crate) fn unstage(repo: &Repo, paths: &[String]) -> Result<Value, Failure> {
    if !paths.is_empty() {
        repo.ask(UNSTAGE, json!({"paths": paths}))?;
    }
    Ok(json!({"ok": true}))
}

/// A changed file is restored from the index and an untracked one removed, which is what the phone's discard means for either.
pub(crate) fn discard(repo: &Repo, paths: &[String]) -> Result<Value, Failure> {
    if !paths.is_empty() {
        repo.ask(DISCARD, json!({"paths": paths, "untracked": true}))?;
    }
    Ok(json!({"ok": true}))
}

/// The executor's refusal of a commit (nothing staged, an identity git lacks) is the host's `success: false` with its sentence.
pub(crate) fn commit(repo: &Repo, params: &Value) -> Result<Value, Failure> {
    let message = text(params, "message").filter(|m| !m.trim().is_empty()).ok_or_else(|| Failure::invalid("Missing commit message"))?;
    match repo.try_ask(COMMIT, json!({"message": message})) {
        Ok(_) => Ok(json!({"success": true})),
        Err(AgentError::Refused { message, .. }) => Ok(json!({"success": false, "error": message})),
        Err(other) => Err(crate::refusal(other)),
    }
}

/// `mark_resolved` and `keep_file` stage the file as it lies. `keep_deletion` takes the side that deleted it, which git's unmerged pair of columns
/// names (`DU` deleted by us, `UD` by them, `DD` by both), so the file is removed; a conflict no side deleted has no deletion to keep.
pub(crate) fn resolve_conflict(repo: &Repo, params: &Value) -> Result<Value, Failure> {
    let path = text(params, "filePath").ok_or_else(|| Failure::invalid("Missing file path"))?;
    match text(params, "resolution") {
        Some("mark_resolved" | "keep_file") => repo.ask(CONFLICT_RESOLVE, json!({"path": path, "resolution": "resolved"}))?,
        Some("keep_deletion") => {
            let status = repo.ask(STATUS, json!({}))?;
            let file = status["files"].as_array().into_iter().flatten().find(|f| f["path"] == path && f["conflicted"] == true).ok_or_else(|| Failure::invalid(format!("{path} is not a conflicted file")))?;
            let side = match (file["index"].as_str(), file["worktree"].as_str()) {
                (Some("D"), Some("U" | "D")) => "ours",
                (Some("U"), Some("D")) => "theirs",
                _ => return Err(Failure::invalid(format!("{path} was deleted by neither side: there is no deletion to keep"))),
            };
            repo.ask(CONFLICT_RESOLVE, json!({"path": path, "resolution": side}))?
        }
        _ => return Err(Failure::invalid("Unknown conflict resolution")),
    };
    Ok(json!({"ok": true}))
}
