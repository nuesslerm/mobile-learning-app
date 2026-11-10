//! `worktree.resolvePrBase` and `worktree.resolveMrBase`: where a worktree made for a pull or merge request starts. The request's head and its target
//! branch are fetched by `git.fetch` into refs of their own (`refs/rca/pr/<n>/head` and `.../base`, so no branch, tag or checkout moves), and the
//! worktree starts at the head's commit. The phone's own hints (`headRefName`, `baseRefName`, `isCrossRepository`) are not trusted over the provider: the
//! request is read with `pr.view`. A failure is the host's `{error, errorCode}` answer, as the host words it for the phone.
use crate::{number, refusal, Target};
use agent_api_git::FETCH;
use agent_api_hosted::pr;
use orca_core::wire::Failure;
use orca_core::AgentError;
use serde_json::{json, Value};

/// The host's start-point failure: its words and, where the phone translates them, its code.
fn failure(code: Option<&str>, error: String) -> Value {
    let mut out = json!({"error": error});
    if let Some(code) = code {
        out["errorCode"] = json!(code);
    }
    out
}

pub fn resolve(t: &Target, params: &Value, merge_request: bool) -> Result<Value, Failure> {
    let (key, noun, sigil) = if merge_request { ("mrIid", "MR", "!") } else { ("prNumber", "PR", "#") };
    let n = number(params, key, noun)?;
    let provider = if merge_request { "gitlab" } else { "github" };
    let view = match t.try_ask(if merge_request { pr::MR_VIEW } else { pr::VIEW }, json!({"number": n})) {
        Ok(view) => view,
        Err(AgentError::Refused { code, message }) if code != "UNKNOWN_WORKSPACE" && code != "INVALID_PARAMS" && code != "INVALID_ARGS" => return Ok(failure(None, message)),
        Err(other) => return Err(refusal(other)),
    };
    let found = view["pr"].as_object().ok_or_else(|| Failure::new(orca_core::wire::RUNTIME_ERROR, "rca's pr.view answered no pr"))?;
    let named = |field: &str| found.get(field).and_then(Value::as_str).filter(|s| !s.is_empty());
    let Some(head_ref) = named("head_ref") else { return Ok(failure(Some(if merge_request { "gitlab.mrSourceBranchMissing" } else { "github.prHeadBranchMissing" }), format!("{noun} {sigil}{n} has no {} branch.", if merge_request { "source" } else { "head" }))) };
    let Some(base_ref) = named("base_ref") else { return Ok(failure(Some("remoteRefMissing"), format!("{noun} {sigil}{n} names no target branch."))) };
    let source = if merge_request { format!("refs/merge-requests/{n}/head") } else { format!("refs/pull/{n}/head") };
    let (head_ref_name, base_ref_name) = (format!("refs/rca/pr/{n}/head"), format!("refs/rca/pr/{n}/base"));
    let refspecs = [format!("+{source}:{head_ref_name}"), format!("+refs/heads/{base_ref}:{base_ref_name}")];
    let fetched = match t.try_ask(FETCH, json!({"refspecs": refspecs})) {
        Ok(fetched) => fetched,
        Err(AgentError::Refused { code, message }) if code != "UNKNOWN_WORKSPACE" => return Ok(failure(Some("fetchFailed"), format!("Failed to fetch {head_ref}: {message}"))),
        Err(other) => return Err(refusal(other)),
    };
    let sha_of = |name: &str| fetched["refs"].as_array().into_iter().flatten().find(|r| r["ref"] == name).and_then(|r| r["sha"].as_str());
    let Some(head_sha) = sha_of(&head_ref_name) else { return Ok(failure(Some(&format!("{provider}.forkHeadUnresolved")), format!("Could not resolve fork {noun} {sigil}{n} head after fetch."))) };
    if sha_of(&base_ref_name).is_none() {
        return Ok(failure(Some("remoteRefMissing"), format!("Remote ref {base_ref} does not exist after fetch.")));
    }
    let mut start = json!({"baseBranch": head_sha, "compareBaseRef": base_ref_name});
    if !merge_request {
        start["headSha"] = json!(head_sha);
    }
    Ok(start)
}
