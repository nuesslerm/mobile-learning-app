//! The `github.*` pull request rows over `pr.*`.
use crate::{checks, number, outcome, required, text, time, with_repo, Target};
use agent_api_hosted::pr;
use orca_core::wire::{received, Failure, RUNTIME_ERROR};
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

fn positive(params: &Value, key: &str) -> Result<u64, Failure> {
    params[key].as_u64().filter(|n| *n >= 1).ok_or_else(|| Failure::invalid(format!("Invalid input: {key} must be a positive integer")))
}

/// The host replies over GitHub's REST replies route by the numeric id of a comment in the thread and answers the new comment as the phone
/// renders it, or the host's failure.
pub fn reply_to_comment(t: &Target, params: &Value) -> Result<Value, Failure> {
    let (number, comment) = (pr_number(params)?, positive(params, "commentId")?);
    let body = crate::text(params, "body").ok_or_else(|| Failure::invalid("Invalid input: expected string, received undefined"))?;
    let args = with_repo(json!({"number": number, "comment_id": comment, "body": body}), params, "prRepo")?;
    let c = match t.try_ask(pr::REPLY, args) {
        Ok(c) => c,
        Err(AgentError::Refused { code, message }) if code != "UNKNOWN_WORKSPACE" && code != "INVALID_PARAMS" => return Ok(crate::failure::gh(&message)),
        Err(other) => return Err(crate::refusal(other)),
    };
    let Some(mut comment) = comment_row(&c, params, body) else { return Ok(crate::failure::code_host("github.unexpectedResponse", None)) };
    if let Some(thread) = params.get("threadId").filter(|v| !v.is_null()) {
        comment["threadId"] = thread.clone();
    }
    Ok(json!({"ok": true, "comment": comment}))
}

/// The host's `mapReviewCommentResponse` of rca's new review comment, with the phone's body, path and line where GitHub gave none; none when the
/// comment has no usable numeric id, which the host answers as an unexpected response.
fn comment_row(c: &Value, params: &Value, body: &str) -> Option<Value> {
    let id = c["database_id"].as_u64().filter(|id| (1..=MAX_SAFE_INTEGER).contains(id))?;
    let mut comment = json!({
        "id": id,
        "author": c["author"].as_str().unwrap_or("You"),
        "authorAvatarUrl": c["avatar_url"].as_str().unwrap_or(""),
        "body": c["body"].as_str().unwrap_or(body),
        "createdAt": c["created_at"],
        "url": c["url"].as_str().unwrap_or(""),
        "isBot": c["is_bot"] == true,
    });
    if let Some(node) = c["id"].as_str().filter(|n| !n.trim().is_empty()) {
        comment["reactionSubjectId"] = json!(node.trim());
    }
    for (key, from, fallback) in [("path", &c["path"], &params["path"]), ("line", &c["line"], &params["line"])] {
        let v = if from.is_null() { fallback } else { from };
        if !v.is_null() {
            comment[key] = v.clone();
        }
    }
    Some(comment)
}

const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

/// zod's `z.number().int().positive()` (whose `int()` is bounded to the safe integers): the first issue it reports for `v`, or the number.
fn int_positive(v: Option<&Value>) -> Result<u64, Failure> {
    let n = match v {
        None => return Err(Failure::invalid("Invalid input: expected number, received undefined")),
        Some(Value::Number(n)) => n,
        Some(other) => return Err(Failure::invalid(format!("Invalid input: expected number, received {}", received(other)))),
    };
    let f = n.as_f64().unwrap_or(0.0);
    if f.fract() != 0.0 {
        return Err(Failure::invalid("Invalid input: expected int, received number"));
    }
    if f > MAX_SAFE_INTEGER as f64 {
        return Err(Failure::invalid(format!("Too big: expected int to be <={MAX_SAFE_INTEGER}")));
    }
    if f < -(MAX_SAFE_INTEGER as f64) {
        return Err(Failure::invalid(format!("Too small: expected int to be >=-{MAX_SAFE_INTEGER}")));
    }
    if f <= 0.0 {
        return Err(Failure::invalid("Too small: expected number to be >0"));
    }
    Ok(n.as_u64().unwrap_or(f as u64))
}

/// zod's `requiredString(message)`: anything but a non-empty string is `message`.
fn required_string<'a>(params: &'a Value, key: &str, message: &str) -> Result<&'a str, Failure> {
    crate::text(params, key).ok_or_else(|| Failure::invalid(message))
}

/// The schema of `github.addPRReviewComment` in its key order, answering zod's first issue; unknown keys are stripped, never refused.
pub(crate) fn check_review_comment_params(params: &Value) -> Result<(), Failure> {
    required_string(params, "repo", "Missing repo selector")?;
    int_positive(params.get("prNumber"))?;
    match params.get("prRepo") {
        None | Some(Value::Null) => {}
        Some(named @ Value::Object(_)) => {
            required_string(named, "owner", "Missing owner")?;
            required_string(named, "repo", "Missing repo")?;
        }
        Some(other) => return Err(Failure::invalid(format!("Invalid input: expected object, received {}", received(other)))),
    }
    required_string(params, "commitId", "Missing PR head SHA")?;
    required_string(params, "path", "File path required")?;
    int_positive(params.get("line"))?;
    if let Some(start) = params.get("startLine") {
        int_positive(Some(start))?;
    }
    required_string(params, "body", "Comment body required")?;
    Ok(())
}

/// `github.addPRReviewComment`: a comment on a line (or a range ending at it) of the head's file over GitHub's REST route, answered as the
/// host's new comment row or its classified failure. As the host does, a range that starts on its own line is a one-line comment.
pub fn review_comment(t: &Target, params: &Value) -> Result<Value, Failure> {
    let (number, line) = (int_positive(params.get("prNumber"))?, int_positive(params.get("line"))?);
    let body = required_string(params, "body", "Comment body required")?;
    let start = params.get("startLine").map(|s| int_positive(Some(s))).transpose()?;
    let mut args = json!({"number": number, "commit_id": required_string(params, "commitId", "Missing PR head SHA")?, "path": required_string(params, "path", "File path required")?, "line": line, "body": body});
    if let Some(s) = start.filter(|s| *s != line) {
        args["start_line"] = json!(s);
    }
    let c = match t.try_ask(pr::REVIEW_COMMENT, with_repo(args, params, "prRepo")?) {
        Ok(c) => c,
        Err(AgentError::Refused { code, message }) if code == "INVALID_ARGS" => return Ok(crate::failure::code_host("invalidUpdate", Some(&message))),
        Err(AgentError::Refused { code, message }) if code != "UNKNOWN_WORKSPACE" && code != "INVALID_PARAMS" => return Ok(crate::failure::gh(&message)),
        Err(other) => return Err(crate::refusal(other)),
    };
    let Some(mut comment) = comment_row(&c, params, body) else { return Ok(crate::failure::code_host("github.unexpectedResponse", None)) };
    if let Some(s) = start {
        comment["startLine"] = json!(s);
    }
    Ok(json!({"ok": true, "comment": comment}))
}

/// `github.updatePRState`: `gh pr close` or `gh pr reopen`; `{ok: true}`, or the host's pull request update failure.
pub fn set_state(t: &Target, params: &Value) -> Result<Value, Failure> {
    let closed = match params["updates"]["state"].as_str() {
        Some("closed") => true,
        Some("open") => false,
        _ => return Err(Failure::invalid("Invalid option: expected one of \"open\"|\"closed\"")),
    };
    let args = with_repo(json!({"number": pr_number(params)?, "state": if closed { "closed" } else { "open" }}), params, "prRepo")?;
    match t.try_ask(pr::SET_STATE, args) {
        Ok(_) => Ok(json!({"ok": true})),
        Err(AgentError::Refused { code, message }) if code != "UNKNOWN_WORKSPACE" && code != "INVALID_PARAMS" => Ok(crate::failure::pr_update(&message)),
        Err(other) => Err(crate::refusal(other)),
    }
}

/// The host resolves (or reopens) a thread by its node id and answers whether GitHub did; any failure is `false`, never an error.
pub fn resolve_thread(t: &Target, params: &Value) -> Result<Value, Failure> {
    let thread = crate::text(params, "threadId").filter(|s| !s.is_empty()).ok_or_else(|| Failure::invalid("Invalid input: expected string, received undefined"))?;
    let args = with_repo(json!({"thread": thread, "resolved": params["resolve"].as_bool().unwrap_or(true)}), params, "prRepo")?;
    match t.try_ask(pr::RESOLVE_THREAD, args) {
        Ok(_) => Ok(json!(true)),
        Err(AgentError::Refused { code, .. }) if code != "UNKNOWN_WORKSPACE" => Ok(json!(false)),
        Err(other) => Err(crate::refusal(other)),
    }
}

/// The schema of `github.setPRFileViewed` in its key order, answering zod's first issue; unknown keys are stripped, never refused.
pub(crate) fn check_file_viewed_params(params: &Value) -> Result<(), Failure> {
    required_string(params, "repo", "Missing repo selector")?;
    match params.get("expectedGhAccount") {
        None | Some(Value::Null) => {}
        Some(account) if gh_account_binding(account) => {}
        Some(_) => return Err(Failure::invalid("Invalid GitHub account expectation")),
    }
    match params.get("prRepo") {
        None | Some(Value::Null) => {}
        Some(named @ Value::Object(_)) => {
            required_string(named, "owner", "Missing owner")?;
            required_string(named, "repo", "Missing repo")?;
        }
        Some(other) => return Err(Failure::invalid(format!("Invalid input: expected object, received {}", received(other)))),
    }
    required_string(params, "pullRequestId", "Missing pull request ID")?;
    required_string(params, "path", "Missing file path")?;
    match params.get("viewed") {
        Some(Value::Bool(_)) => Ok(()),
        other => Err(Failure::invalid(format!("Invalid input: expected boolean, received {}", other.map_or("undefined", received)))),
    }
}

/// The host marks (or unmarks) a file viewed by the pull request's node id and answers whether GitHub did; any failure is `false`, never an error.
pub fn file_viewed(t: &Target, params: &Value) -> Result<Value, Failure> {
    let args = json!({"pull_request": required_string(params, "pullRequestId", "Missing pull request ID")?, "path": required_string(params, "path", "Missing file path")?, "viewed": params["viewed"].as_bool().unwrap_or(true)});
    match t.try_ask(pr::FILE_VIEWED, with_repo(args, params, "prRepo")?) {
        Ok(_) => Ok(json!(true)),
        Err(AgentError::Refused { code, .. }) if code != "UNKNOWN_WORKSPACE" => Ok(json!(false)),
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
