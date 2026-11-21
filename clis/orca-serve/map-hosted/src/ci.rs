//! The CI rows of `github.*`: `rerunPRChecks`, `setPRAutoMerge` and `prCheckDetails`, over `pr.checks`, `pr.rerun`, `pr.setAutoMerge` and
//! `pr.checkDetails`. The limits (a check with no Actions run, merge queues, the job log tail) are in docs/gaps/orca-hosted-mapper-limits.md.
use crate::github::{int_positive, required_string};
use crate::{checks, failure, number, time, with_repo, Target};
use agent_api_hosted::pr;
use orca_core::wire::{received, Failure, RUNTIME_ERROR};
use orca_core::AgentError;
use serde_json::{json, Value};

/// The merge method of the host's `setPRAutoMerge` when the phone names none.
const DEFAULT_METHOD: &str = "squash";

/// The schema of the three rows in their key order, answering zod's first issue: the repository selector, then the pull request or the ids, then `prRepo`.
pub(crate) fn check_params(method: &str, params: &Value) -> Result<(), Failure> {
    required_string(params, "repo", "Missing repo selector")?;
    if method == "github.prCheckDetails" {
        for key in ["checkRunId", "workflowRunId"] {
            if params.get(key).is_some_and(|v| !v.is_null()) {
                int_positive(params.get(key))?;
            }
        }
    } else {
        int_positive(params.get("prNumber"))?;
    }
    match params.get("prRepo") {
        None | Some(Value::Null) => {}
        Some(named @ Value::Object(_)) => {
            required_string(named, "owner", "Missing owner")?;
            required_string(named, "repo", "Missing repo")?;
        }
        Some(other) => return Err(Failure::invalid(format!("Invalid input: expected object, received {}", received(other)))),
    }
    if method == "github.setPRAutoMerge" {
        if !params.get("enabled").is_some_and(Value::is_boolean) {
            return Err(Failure::invalid(format!("Invalid input: expected boolean, received {}", params.get("enabled").map_or("undefined", received))));
        }
        if params.get("method").is_some_and(|m| !matches!(m.as_str(), Some("merge" | "squash" | "rebase"))) {
            return Err(Failure::invalid("Invalid option: expected one of \"merge\"|\"squash\"|\"rebase\""));
        }
    }
    Ok(())
}

/// The repositories a request may touch, or None when the host cannot resolve one: the request's own `prRepo`, else the GitHub remotes of the workspace.
/// Only git's "no remotes" failures are unresolved; any other refusal is the real fault.
fn repos(t: &Target, params: &Value) -> Result<Option<Vec<(String, String)>>, Failure> {
    if let Some(named @ Value::Object(_)) = params.get("prRepo") {
        let field = |k: &str| named[k].as_str().unwrap_or_default().to_lowercase();
        return Ok(Some(vec![(field("owner"), field("repo"))]));
    }
    match t.try_ask(pr::REMOTES, json!({})) {
        Ok(answer) => {
            let remotes = answer["remotes"].as_array().map_or(&[][..], Vec::as_slice);
            let slugs = remotes.iter().filter_map(|r| r["url"].as_str()).filter_map(crate::slug::github_com_slug).map(|(o, r)| (o.to_lowercase(), r.to_lowercase())).collect();
            Ok(if remotes.is_empty() { None } else { Some(slugs) })
        }
        Err(AgentError::Refused { code, message }) if code != "UNKNOWN_WORKSPACE" && failure::unresolved(&message.to_lowercase()) => Ok(None),
        Err(other) => Err(crate::refusal(other)),
    }
}

/// The id of the Actions run a check's address names (`/<owner>/<repo>/actions/runs/<id>/job/<job>`) when it is a run of one of `repos`.
fn actions_run(url: &str, repos: &[(String, String)]) -> Option<u64> {
    let at = url.find("/actions/runs/")?;
    let (owner, repo) = crate::slug::github_com_slug(&url[..at])?;
    if !repos.contains(&(owner.to_lowercase(), repo.to_lowercase())) {
        return None;
    }
    let rest = &url[at + "/actions/runs/".len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    match rest[digits.len()..].chars().next() {
        None | Some('/' | '?' | '#') => digits.parse().ok().filter(|id| *id > 0),
        _ => None,
    }
}

/// `github.rerunPRChecks`: the Actions runs of the pull request's checks (the failed ones with `failedOnly`) started again, one rerun each; `{ok, count}`
/// of the runs started. A check with no Actions run in its address cannot be rerun here, so a pull request with only those has nothing to rerun.
pub fn rerun(t: &Target, params: &Value) -> Result<Value, Failure> {
    let number = number(params, "prNumber", "PR")?;
    let failed_only = params["failedOnly"].as_bool().unwrap_or(false);
    let Some(repos) = repos(t, params)? else {
        return Ok(failure::code_host("github.repoUnresolved", None));
    };
    let listed = match t.try_ask(pr::CHECKS, with_repo(json!({"number": number}), params, "prRepo")?) {
        Ok(listed) => listed,
        Err(AgentError::Refused { code, message }) if code != "UNKNOWN_WORKSPACE" && code != "INVALID_PARAMS" => return Ok(failure::rerun(&message, 0)),
        Err(other) => return Err(crate::refusal(other)),
    };
    let mut runs: Vec<u64> = Vec::new();
    for check in checks::details(&listed)?.iter().filter(|c| !failed_only || matches!(c["conclusion"].as_str(), Some("failure" | "cancelled" | "timed_out" | "action_required"))) {
        if let Some(id) = check["url"].as_str().and_then(|u| actions_run(u, &repos)).filter(|id| !runs.contains(id)) {
            runs.push(id);
        }
    }
    if runs.is_empty() {
        return Ok(failure::code_host(if failed_only { "github.noFailedChecks" } else { "github.noRerunnableChecks" }, None));
    }
    for (started, run) in runs.iter().enumerate() {
        match t.try_ask(pr::RUN_RERUN, with_repo(json!({"run": run, "failed_only": failed_only}), params, "prRepo")?) {
            Ok(_) => {}
            Err(AgentError::Refused { code, message }) if code != "UNKNOWN_WORKSPACE" && code != "INVALID_PARAMS" => return Ok(failure::rerun(&message, started)),
            Err(other) => return Err(crate::refusal(other)),
        }
    }
    Ok(json!({"ok": true, "count": runs.len()}))
}

/// `github.setPRAutoMerge`: auto-merge asked for through GitHub's enable mutation (which refuses a pull request that can merge at once) or taken back;
/// `{ok: true}` or the host's classified failure.
pub fn auto_merge(t: &Target, params: &Value) -> Result<Value, Failure> {
    let enabled = params["enabled"].as_bool().unwrap_or(false);
    let method = crate::text(params, "method").unwrap_or(DEFAULT_METHOD);
    if repos(t, params)?.is_none() {
        return Ok(failure::code_host("github.repoUnresolved", None));
    }
    let args = with_repo(json!({"number": number(params, "prNumber", "PR")?, "enabled": enabled, "method": method}), params, "prRepo")?;
    match t.try_ask(pr::SET_AUTO_MERGE, args) {
        Ok(_) => Ok(json!({"ok": true})),
        Err(AgentError::Refused { code, message }) if code != "UNKNOWN_WORKSPACE" && code != "INVALID_PARAMS" => Ok(failure::auto_merge(&message, enabled)),
        Err(other) => Err(crate::refusal(other)),
    }
}

fn string_or(v: &Value, key: &str, fallback: &str) -> Value {
    json!(v[key].as_str().filter(|s| !s.is_empty()).unwrap_or(fallback))
}

fn stamp(v: &Value, key: &str) -> Value {
    v[key].as_u64().map_or(Value::Null, |ms| json!(time::iso(ms)))
}

fn or_null(v: &Value, key: &str) -> Value {
    v[key].as_str().filter(|s| !s.is_empty()).map_or(Value::Null, |s| json!(s))
}

fn job(j: &Value) -> Value {
    let steps: Vec<Value> = j["steps"].as_array().into_iter().flatten().map(|s| json!({"name": string_or(s, "name", "Unnamed step"), "status": or_null(s, "status"), "conclusion": or_null(s, "conclusion"), "startedAt": stamp(s, "started_at_ms"), "completedAt": stamp(s, "completed_at_ms")})).collect();
    json!({"id": j["id"].as_u64().map_or(Value::Null, |id| json!(id)), "name": string_or(j, "name", "Unnamed job"), "status": or_null(j, "status"), "conclusion": or_null(j, "conclusion"), "startedAt": stamp(j, "started_at_ms"), "completedAt": stamp(j, "completed_at_ms"), "url": or_null(j, "url"), "logTail": or_null(j, "log_tail"), "steps": steps})
}

/// `github.prCheckDetails`: a check run's output, annotations and the jobs of its workflow run, or null when the repository is unresolved. The run is the
/// request's `workflowRunId`, else the one its `url` names; with neither id it is the bare row.
pub fn check_details(t: &Target, params: &Value) -> Result<Value, Failure> {
    let (name, url) = (crate::text(params, "checkName"), crate::text(params, "url"));
    let check = params["checkRunId"].as_u64();
    let Some(repos) = repos(t, params)? else {
        return Ok(Value::Null);
    };
    let run = params["workflowRunId"].as_u64().or_else(|| url.and_then(|u| actions_run(u, &repos)));
    let mut row = json!({"name": name.unwrap_or("Check"), "status": null, "conclusion": null, "url": url, "detailsUrl": url, "startedAt": null, "completedAt": null, "title": null, "summary": null, "text": null, "annotations": [], "jobs": []});
    if check.is_none() && run.is_none() {
        return Ok(row);
    }
    let mut args = json!({});
    for (key, id) in [("check", check), ("run", run)] {
        if let Some(id) = id {
            args[key] = json!(id);
        }
    }
    let answer = match t.try_ask(pr::CHECK_DETAILS, with_repo(args, params, "prRepo")?) {
        Ok(answer) => answer,
        Err(AgentError::Refused { code, message }) if code != "UNKNOWN_WORKSPACE" && code != "INVALID_PARAMS" => {
            return if failure::unresolved(&message.to_lowercase()) { Ok(Value::Null) } else { Err(Failure::new(RUNTIME_ERROR, "Could not load check details.")) };
        }
        Err(other) => return Err(crate::refusal(other)),
    };
    let c = crate::required(&answer, "check", "pr.checkDetails")?;
    for (key, from) in [("name", "name"), ("status", "status"), ("conclusion", "conclusion"), ("title", "title"), ("summary", "summary"), ("text", "text")] {
        if let Some(v) = c[from].as_str().filter(|s| !s.is_empty()) {
            row[key] = json!(v);
        }
    }
    for (key, from) in [("url", "url"), ("detailsUrl", "details_url")] {
        if let Some(v) = c[from].as_str().filter(|s| !s.is_empty()) {
            row[key] = json!(v);
        }
    }
    row["startedAt"] = stamp(c, "started_at_ms");
    row["completedAt"] = stamp(c, "completed_at_ms");
    row["annotations"] = json!(c["annotations"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|a| json!({"path": or_null(a, "path"), "startLine": a["start_line"], "endLine": a["end_line"], "annotationLevel": or_null(a, "level"), "title": or_null(a, "title"), "message": a["message"].as_str().unwrap_or(""), "rawDetails": or_null(a, "raw_details")}))
        .collect::<Vec<_>>());
    let jobs: Vec<&Value> = c["jobs"].as_array().into_iter().flatten().collect();
    let exact: Vec<&Value> = name.map(|n| jobs.iter().copied().filter(|j| j["name"] == n).collect()).unwrap_or_default();
    row["jobs"] = json!((if exact.is_empty() { jobs } else { exact }).into_iter().map(job).collect::<Vec<_>>());
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::actions_run;

    #[test]
    fn given_check_addresses_when_the_run_is_read_then_only_an_actions_run_of_the_pull_requests_repository_ends_at_a_boundary() {
        let here = [("octo".to_string(), "hello".to_string())];
        assert_eq!(actions_run("https://github.com/octo/hello/actions/runs/123/job/456", &here), Some(123));
        assert_eq!(actions_run("https://github.com/Octo/Hello/actions/runs/123?pr=4", &here), Some(123));
        assert_eq!(actions_run("https://github.com/octo/hello/actions/runs/123", &here), Some(123));
        assert_eq!(actions_run("https://github.com/octo/hello/actions/runs/12x", &here), None);
        assert_eq!(actions_run("https://ci.example.test/builds/9", &here), None);
        assert_eq!(actions_run("https://github.com/octo/hello/actions/runs/0", &here), None);
        assert_eq!(actions_run("https://evil.example/octo/hello/actions/runs/123", &here), None);
        assert_eq!(actions_run("https://github.com/other/repo/actions/runs/123", &here), None);
    }
}
