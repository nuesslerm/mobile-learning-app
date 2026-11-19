//! The `github.*` pull request rows over `pr.*`.
use crate::{checks, number, outcome, required, text, time, with_repo, Target};
use agent_api_hosted::pr;
use orca_core::wire::{Failure, RUNTIME_ERROR};
use orca_core::AgentError;
use serde_json::{json, Value};

/// The merge method of the host's `mergePR` when the phone names none.
const DEFAULT_METHOD: &str = "squash";

fn pr_number(params: &Value) -> Result<u64, Failure> {
    number(params, "prNumber", "PR")
}

/// `github.prChecks`: the check runs of the pull request, each as the details row.
pub fn pr_checks(t: &Target, params: &Value) -> Result<Value, Failure> {
    let args = with_repo(json!({"number": pr_number(params)?}), params, "prRepo")?;
    Ok(Value::Array(checks::details(&t.ask(pr::CHECKS, args)?)?))
}

/// `github.listLabels`: the names of the repository's labels, one page of at most 200 where the host pages through them all. The host answers an empty
/// list for any repository it cannot read, so a refusal of rca's read is an empty list too; only a repository selector that names no workspace is an error.
/// The schema order of `github.listLabels`: the repo selector, the repository it names, then the expected GitHub account (`{host, user}`).
pub(crate) fn check_label_params(params: &Value) -> Result<(), Failure> {
    if crate::text(params, "repo").is_none() {
        return Err(Failure::invalid("Missing repo selector"));
    }
    match params.get("ownerRepo") {
        None | Some(Value::Object(_)) => {}
        Some(other) => return Err(Failure::invalid(format!("Invalid input: expected object, received {}", if other.is_null() { "null" } else { "a value of another type" }))),
    }
    match params.get("expectedGhAccount") {
        None | Some(Value::Null) => Ok(()),
        Some(account) if gh_account_binding(account) => Ok(()),
        Some(_) => Err(Failure::invalid("Invalid GitHub account expectation")),
    }
}

/// `{host, user}`: a host name (optionally with a port) and a user of 1 to 255 characters without whitespace or NUL.
fn gh_account_binding(v: &Value) -> bool {
    let (Some(host), Some(user)) = (v.get("host").and_then(Value::as_str), v.get("user").and_then(Value::as_str)) else { return false };
    let host = host.trim().to_lowercase();
    let user = user.trim();
    let name = match host.rsplit_once(':') {
        Some((n, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => n,
        _ => host.as_str(),
    };
    let first_ok = name.chars().next().is_some_and(|c| c.is_ascii_alphanumeric());
    first_ok && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') && !user.is_empty() && user.chars().count() <= 255 && !user.chars().any(|c| c.is_whitespace() || c == '\0')
}

/// `github.listLabels`: the names of the repository's labels, one page of at most 200 where the host pages through them all. The host answers an empty
/// list for any repository it cannot read, so a refusal of rca's read is an empty list too; only a repository selector that names no workspace is an error.
pub fn list_labels(t: &Target, params: &Value) -> Result<Value, Failure> {
    let args = with_repo(json!({"limit": 200}), params, "ownerRepo")?;
    match t.try_ask(pr::LABEL_CANDIDATES, args) {
        Ok(answer) => Ok(Value::Array(answer["labels"].as_array().into_iter().flatten().filter_map(|l| l["name"].as_str()).filter(|n| !n.is_empty()).map(|n| json!(n)).collect())),
        Err(AgentError::Refused { code, .. }) if code != "UNKNOWN_WORKSPACE" => Ok(json!([])),
        Err(other) => Err(crate::refusal(other)),
    }
}

/// Orca's host lists `repos/{o}/{r}/assignees` as `{login, name, avatarUrl}` and answers `[]` when gh fails.
pub fn list_assignable_users(t: &Target, params: &Value) -> Result<Value, Failure> {
    let args = with_repo(json!({"limit": 100}), params, "ownerRepo")?;
    match t.try_ask(pr::REVIEWER_CANDIDATES, args) {
        Ok(answer) => Ok(Value::Array(
            answer["users"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|u| u["login"].as_str().filter(|l| !l.is_empty()).map(|login| json!({"login": login, "name": u["name"], "avatarUrl": u["avatarUrl"].as_str().unwrap_or("")})))
                .collect(),
        )),
        Err(AgentError::Refused { code, .. }) if code != "UNKNOWN_WORKSPACE" => Ok(json!([])),
        Err(other) => Err(crate::refusal(other)),
    }
}

/// The number of the branch's open pull request, else the latest one that is not.
pub(crate) fn find_for_branch(t: &Target, branch: &str) -> Result<Option<u64>, Failure> {
    let listed = t.ask(pr::LIST, json!({"head": branch, "state": "all", "limit": 20}))?;
    let all = listed["prs"].as_array().ok_or_else(|| Failure::new(RUNTIME_ERROR, "rca's pr.list answered no prs list"))?;
    let Some(found) = all.iter().find(|p| p["state"] == "open").or_else(|| all.first()) else { return Ok(None) };
    Ok(required(found, "number", "pr.list")?.as_u64())
}

/// The host's summary of pull request `n`: its state, rolled-up checks, mergeability, review decision and head.
pub(crate) fn summary(t: &Target, n: u64) -> Result<Value, Failure> {
    let view = t.ask(pr::VIEW, json!({"number": n}))?;
    let p = required(&view, "pr", "pr.view")?;
    let details = checks::details(&t.ask(pr::CHECKS, json!({"number": n}))?)?;
    let state = match (required(p, "state", "pr.view")?.as_str(), p["draft"] == true) {
        (Some("open"), true) => "draft",
        (Some(s @ ("open" | "closed" | "merged")), _) => s,
        (other, _) => return Err(Failure::new(RUNTIME_ERROR, format!("rca's pr.view answered the state {other:?}"))),
    };
    let mergeable = match required(p, "mergeable", "pr.view")?.as_str() {
        Some("mergeable") => "MERGEABLE",
        Some("conflicting") => "CONFLICTING",
        Some("unknown") => "UNKNOWN",
        other => return Err(Failure::new(RUNTIME_ERROR, format!("rca's pr.view answered the mergeability {other:?}"))),
    };
    let updated = required(p, "updated_at_ms", "pr.view")?.as_u64().ok_or_else(|| Failure::new(RUNTIME_ERROR, "rca's pr.view answered an updated_at_ms that is no time"))?;
    let mut info = json!({
        "number": p["number"], "title": required(p, "title", "pr.view")?, "state": state, "url": required(p, "url", "pr.view")?, "checksStatus": checks::rollup(&details),
        "updatedAt": time::iso(updated), "mergeable": mergeable, "headSha": required(p, "head_sha", "pr.view")?, "baseRefName": required(p, "base_ref", "pr.view")?, "headRefName": required(p, "head_ref", "pr.view")?,
    });
    if let Some(decision) = p["review_decision"].as_str() {
        info["reviewDecision"] = json!(decision.to_ascii_uppercase());
    }
    Ok(info)
}

/// `github.prForBranch`: the open pull request of the branch (else the latest one that is not), as the host's summary of it, or null with none.
pub fn pr_for_branch(t: &Target, params: &Value) -> Result<Value, Failure> {
    let branch = text(params, "branch").ok_or_else(|| Failure::invalid("Missing branch"))?;
    match find_for_branch(t, branch)? {
        Some(n) => summary(t, n),
        None => Ok(Value::Null),
    }
}

/// `github.mergePR`: the host's `{ok}` of the merge, `squash` unless the phone names a method.
pub fn merge(t: &Target, params: &Value) -> Result<Value, Failure> {
    let method = text(params, "method").unwrap_or(DEFAULT_METHOD);
    outcome(t.try_ask(pr::MERGE, with_repo(json!({"number": pr_number(params)?, "method": method}), params, "prRepo")?))
}

/// `github.updatePRTitle`: true once the title is changed; a refusal is the call's error, not a false the phone would read as nothing to report.
pub fn update_title(t: &Target, params: &Value) -> Result<Value, Failure> {
    let title = text(params, "title").ok_or_else(|| Failure::invalid("Missing title"))?;
    t.ask(pr::EDIT, with_repo(json!({"number": pr_number(params)?, "title": title}), params, "prRepo")?)?;
    Ok(json!(true))
}

/// `github.updatePR`: the title and the body the phone sends; an update that names neither is refused before rca is asked, as `gitlab.updateMR` does.
pub fn update(t: &Target, params: &Value) -> Result<Value, Failure> {
    let updates = params.get("updates").filter(|u| u.is_object()).ok_or_else(|| Failure::invalid("Missing updates"))?;
    let mut args = json!({"number": pr_number(params)?});
    for key in ["title", "body"] {
        if let Some(value) = updates.get(key).and_then(Value::as_str) {
            args[key] = json!(value);
        }
    }
    if args.as_object().is_some_and(|a| a.len() == 1) {
        return Err(Failure::invalid(crate::NO_UPDATES));
    }
    outcome(t.try_ask(pr::EDIT, with_repo(args, params, "prRepo")?))
}

/// `github.markPRReadyForReview` and, with `draft`, `github.markPRDraft`.
pub fn ready(t: &Target, params: &Value, draft: bool) -> Result<Value, Failure> {
    let mut args = json!({"number": pr_number(params)?});
    if draft {
        args["undo"] = json!(true);
    }
    outcome(t.try_ask(pr::READY, with_repo(args, params, "prRepo")?))
}

/// `github.requestPRReviewers` (`add`) and `github.removePRReviewers` (`remove`).
pub fn reviewers(t: &Target, params: &Value, change: &str) -> Result<Value, Failure> {
    let names: Vec<&str> = params.get("reviewers").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::trim).filter(|r| !r.is_empty()).collect();
    if names.is_empty() {
        return Err(Failure::invalid("Missing reviewers"));
    }
    outcome(t.try_ask(pr::REQUEST_REVIEWERS, with_repo(json!({"number": pr_number(params)?, change: names}), params, "prRepo")?))
}
