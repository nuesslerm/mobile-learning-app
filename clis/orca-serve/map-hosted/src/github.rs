//! The `github.*` pull request rows over `pr.*`.
use crate::{checks, number, outcome, required, text, time, with_repo, Target};
use agent_api_hosted::pr;
use orca_core::wire::{Failure, RUNTIME_ERROR};
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

/// `github.prForBranch`: the open pull request of the branch (else the latest one that is not), as the host's summary of it, or null with none.
pub fn pr_for_branch(t: &Target, params: &Value) -> Result<Value, Failure> {
    let branch = text(params, "branch").ok_or_else(|| Failure::invalid("Missing branch"))?;
    let listed = t.ask(pr::LIST, json!({"head": branch, "state": "all", "limit": 20}))?;
    let all = listed["prs"].as_array().ok_or_else(|| Failure::new(RUNTIME_ERROR, "rca's pr.list answered no prs list"))?;
    let Some(found) = all.iter().find(|p| p["state"] == "open").or_else(|| all.first()) else { return Ok(Value::Null) };
    let n = required(found, "number", "pr.list")?.clone();
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
