//! What the CLIs printed, as the neutral result of the action: snake_case fields, milliseconds, the same shape for GitHub and GitLab. Pure:
//! `gh` and `glab` output goes in, a JSON value or a refusal comes out. A field the provider does not give is null, never invented.
use super::time::parse_ms;
use exec_hosted::{HostedCall, HostedOp, Provider};
use exec_proto::{Code, ExecError, ExecResult};
use serde_json::{json, Value};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Output {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub truncated: bool,
}

fn io(message: impl Into<String>) -> ExecError {
    ExecError::new(Code::Io, message)
}

fn json_of(program: &str, out: &Output) -> ExecResult<Value> {
    if out.truncated {
        return Err(io(format!("{program} printed more than the output cap, so its JSON is cut")));
    }
    serde_json::from_str(&out.stdout).map_err(|e| io(format!("{program} printed no JSON ({e}): {}", brief(&out.stdout))))
}

/// The start of some output, for a message: one line, at most 200 characters.
pub fn brief(text: &str) -> String {
    let line = text.trim().lines().next().unwrap_or("");
    line.chars().take(200).collect()
}

fn text(v: &Value, key: &str) -> Value {
    match v.get(key) {
        Some(Value::String(s)) if !s.is_empty() => json!(s),
        _ => Value::Null,
    }
}

fn lower(v: &Value, key: &str) -> Value {
    match v.get(key).and_then(Value::as_str) {
        Some(s) if !s.is_empty() => json!(s.to_ascii_lowercase()),
        _ => Value::Null,
    }
}

fn ms(v: &Value, key: &str) -> Value {
    v.get(key).and_then(Value::as_str).and_then(parse_ms).map_or(Value::Null, |t| json!(t))
}

fn id_string(v: &Value, key: &str) -> Value {
    match v.get(key) {
        Some(Value::String(s)) => json!(s),
        Some(Value::Number(n)) => json!(n.to_string()),
        _ => Value::Null,
    }
}

fn num(v: &Value, key: &str) -> Value {
    match v.get(key) {
        Some(Value::Number(n)) => Value::Number(n.clone()),
        Some(Value::String(s)) => s.parse::<u64>().map_or(Value::Null, |n| json!(n)),
        _ => Value::Null,
    }
}

fn items<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key).and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

fn names(v: &Value, key: &str, field: &str) -> Value {
    json!(items(v, key).iter().filter_map(|i| i.get(field).and_then(Value::as_str)).collect::<Vec<_>>())
}

fn login_of(v: &Value, key: &str, field: &str) -> Value {
    v.get(key).and_then(|a| a.get(field)).and_then(Value::as_str).map_or(Value::Null, |s| json!(s))
}

/// Added and removed lines of a unified diff, headers excluded.
pub fn diff_counts(diff: &str) -> (u64, u64) {
    let (mut added, mut removed) = (0, 0);
    for line in diff.lines() {
        if line.starts_with('+') && !line.starts_with("+++") {
            added += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            removed += 1;
        }
    }
    (added, removed)
}

fn first_url(text: &str) -> Option<&str> {
    text.split_whitespace().find(|w| w.starts_with("https://") || w.starts_with("http://")).map(|w| w.trim_end_matches(['.', ',', ')']))
}

/// The number at the end of `.../<marker>/<n>` in `text`.
fn number_after(text: &str, marker: &str) -> Option<u64> {
    let at = text.rfind(marker)? + marker.len();
    let digits: String = text[at..].chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn gh_pr(v: &Value) -> Value {
    let review = match v.get("reviewDecision").and_then(Value::as_str) {
        Some("") | None => Value::Null,
        Some(d) => json!(d.to_ascii_lowercase()),
    };
    json!({
        "number": v.get("number"), "title": text(v, "title"), "state": lower(v, "state"), "draft": v.get("isDraft").and_then(Value::as_bool).unwrap_or(false),
        "author": login_of(v, "author", "login"), "head_ref": text(v, "headRefName"), "base_ref": text(v, "baseRefName"), "labels": names(v, "labels", "name"),
        "url": text(v, "url"), "created_at_ms": ms(v, "createdAt"), "updated_at_ms": ms(v, "updatedAt"), "review_decision": review,
    })
}

fn gl_state(v: &Value) -> Value {
    match v.get("state").and_then(Value::as_str) {
        Some("opened") => json!("open"),
        Some("locked") | Some("closed") => json!("closed"),
        Some("merged") => json!("merged"),
        _ => Value::Null,
    }
}

fn gl_mr(v: &Value) -> Value {
    json!({
        "number": v.get("iid"), "title": text(v, "title"), "state": gl_state(v), "draft": v.get("draft").or_else(|| v.get("work_in_progress")).and_then(Value::as_bool).unwrap_or(false),
        "author": login_of(v, "author", "username"), "head_ref": text(v, "source_branch"), "base_ref": text(v, "target_branch"),
        "labels": json!(items(v, "labels").iter().filter_map(Value::as_str).collect::<Vec<_>>()), "url": text(v, "web_url"),
        "created_at_ms": ms(v, "created_at"), "updated_at_ms": ms(v, "updated_at"), "review_decision": Value::Null,
    })
}

fn extend(base: Value, more: Value) -> Value {
    let mut base = base;
    if let (Some(b), Some(m)) = (base.as_object_mut(), more.as_object()) {
        for (k, v) in m {
            b.insert(k.clone(), v.clone());
        }
    }
    base
}

fn check_state(status: &str) -> &'static str {
    match status {
        "success" | "passed" | "pass" => "pass",
        "failed" | "fail" => "fail",
        "canceled" | "cancelled" | "cancel" => "cancel",
        "skipped" | "skipping" => "skipped",
        _ => "pending",
    }
}

fn run_status(status: &str) -> (&'static str, Value) {
    match status {
        "success" => ("completed", json!("success")),
        "failed" => ("completed", json!("failure")),
        "canceled" => ("completed", json!("cancelled")),
        "skipped" => ("completed", json!("skipped")),
        "running" => ("in_progress", Value::Null),
        _ => ("queued", Value::Null),
    }
}

fn gl_run(v: &Value) -> Value {
    let (status, conclusion) = run_status(v.get("status").and_then(Value::as_str).unwrap_or(""));
    json!({"id": v.get("id"), "name": "pipeline", "title": Value::Null, "status": status, "conclusion": conclusion, "branch": text(v, "ref"), "sha": text(v, "sha"), "event": text(v, "source"), "url": text(v, "web_url"), "created_at_ms": ms(v, "created_at"), "updated_at_ms": ms(v, "updated_at")})
}

fn gh_run(v: &Value) -> Value {
    let name = if v.get("workflowName").and_then(Value::as_str).is_some_and(|s| !s.is_empty()) { text(v, "workflowName") } else { text(v, "name") };
    json!({"id": v.get("databaseId"), "name": name, "title": text(v, "displayTitle"), "status": lower(v, "status"), "conclusion": lower(v, "conclusion"), "branch": text(v, "headBranch"), "sha": text(v, "headSha"), "event": text(v, "event"), "url": text(v, "url"), "created_at_ms": ms(v, "createdAt"), "updated_at_ms": ms(v, "updatedAt")})
}

fn permission_from_level(level: u64) -> &'static str {
    match level {
        50.. => "admin",
        40..=49 => "maintain",
        30..=39 => "write",
        20..=29 => "triage",
        _ => "read",
    }
}

/// A native stack of GitHub's REST answer as the neutral one: the layers run bottom to top.
fn stack_of(raw: &Value) -> Value {
    let id = match raw.get("id") {
        Some(Value::Number(n)) => json!(n.to_string()),
        Some(Value::String(t)) if !t.is_empty() => json!(t),
        _ => match text(raw, "node_id") {
            Value::Null => id_string(raw, "number"),
            node => node,
        },
    };
    let base = raw["base"].as_str().or_else(|| raw["base"]["ref"].as_str());
    let layers: Vec<Value> = items(raw, "pull_requests")
        .iter()
        .map(|p| {
            let state = if p.get("merged_at").is_some_and(|m| !m.is_null()) { "merged" } else if p.get("state").and_then(Value::as_str) == Some("closed") { "closed" } else { "open" };
            json!({"number": p.get("number"), "title": text(p, "title"), "draft": p.get("draft").and_then(Value::as_bool), "head_ref": text(&p["head"], "ref"), "head_sha": text(&p["head"], "sha"), "state": state})
        })
        .collect();
    json!({"id": id, "number": raw.get("number"), "url": text(raw, "html_url").as_str().or_else(|| raw["url"].as_str()), "base": base, "layers": layers})
}

/// The result of the action from the outputs of its steps (see `plan`).
pub fn shape(op: &HostedOp, outputs: &[Output]) -> ExecResult<Value> {
    let github = op.provider == Provider::Github;
    let program = op.provider.program();
    let last = outputs.last().cloned().unwrap_or_default();
    let body = |out: &Output| json_of(program, out);
    let done = || Ok(json!({"done": true}));
    match &op.call {
        HostedCall::RepoView => {
            let v = body(&last)?;
            let repo = if github {
                json!({"slug": text(&v, "nameWithOwner"), "name": text(&v, "name"), "description": text(&v, "description"), "url": text(&v, "url"), "ssh_url": text(&v, "sshUrl"), "private": v.get("isPrivate").and_then(Value::as_bool).unwrap_or(false),
                    "default_branch": login_of(&v, "defaultBranchRef", "name"), "permission": lower(&v, "viewerPermission"), "fork": v.get("isFork").and_then(Value::as_bool).unwrap_or(false)})
            } else {
                let level = v["permissions"]["project_access"]["access_level"].as_u64().max(v["permissions"]["group_access"]["access_level"].as_u64());
                json!({"slug": text(&v, "path_with_namespace"), "name": text(&v, "name"), "description": text(&v, "description"), "url": text(&v, "web_url"), "ssh_url": text(&v, "ssh_url_to_repo"), "private": v.get("visibility").and_then(Value::as_str) != Some("public"),
                    "default_branch": text(&v, "default_branch"), "permission": level.map_or(Value::Null, |l| json!(permission_from_level(l))), "fork": v.get("forked_from_project").is_some_and(|f| !f.is_null())})
            };
            Ok(json!({"repo": repo}))
        }
        HostedCall::RepoLabels { .. } => {
            let v = body(&last)?;
            let labels: Vec<Value> = v.as_array().map_or(&[][..], Vec::as_slice).iter().map(|l| json!({"name": text(l, "name"), "color": l.get("color").and_then(Value::as_str).map(|c| c.trim_start_matches('#')), "description": text(l, "description")})).collect();
            Ok(json!({"labels": labels}))
        }
        HostedCall::RepoReviewers { .. } => {
            let v = body(&last)?;
            let key = if github { "login" } else { "username" };
            let users: Vec<Value> = v.as_array().map_or(&[][..], Vec::as_slice).iter().map(|u| json!({"login": text(u, key), "name": text(u, "name")})).collect();
            Ok(json!({"users": users}))
        }
        HostedCall::RepoClone { .. } => Ok(json!({"repo": op.repo})),
        HostedCall::RepoPublish { .. } => {
            let printed = outputs.iter().map(|o| format!("{}\n{}", o.stdout, o.stderr)).collect::<String>();
            Ok(json!({"repo": op.repo, "url": first_url(&printed), "remote": "origin"}))
        }
        HostedCall::PrList { limit, .. } => {
            let v = body(&last)?;
            let rows = v.as_array().ok_or_else(|| io(format!("{program} did not print a list")))?;
            let prs: Vec<Value> = rows
                .iter()
                .map(|r| {
                    let reviewers: Value = if github { items(r, "reviewRequests").iter().filter_map(|q| q.get("login").or_else(|| q.get("name")).cloned()).collect() } else { names(r, "reviewers", "username") };
                    extend(if github { gh_pr(r) } else { gl_mr(r) }, json!({"reviewers": reviewers}))
                })
                .collect();
            Ok(json!({"truncated": rows.len() as u64 >= *limit, "prs": prs}))
        }
        HostedCall::PrView { .. } => {
            let v = body(&last)?;
            let pr = if github {
                let mergeable = match v.get("mergeable").and_then(Value::as_str) {
                    Some("MERGEABLE") => "mergeable",
                    Some("CONFLICTING") => "conflicting",
                    _ => "unknown",
                };
                let reviewers: Vec<Value> = items(&v, "reviewRequests").iter().filter_map(|r| r.get("login").or_else(|| r.get("name")).cloned()).collect();
                extend(gh_pr(&v), json!({"body": v.get("body").and_then(Value::as_str).unwrap_or(""), "merged_at_ms": ms(&v, "mergedAt"), "closed_at_ms": ms(&v, "closedAt"), "mergeable": mergeable, "head_sha": text(&v, "headRefOid"),
                    "additions": num(&v, "additions"), "deletions": num(&v, "deletions"), "changed_files": num(&v, "changedFiles"), "reviewers": reviewers, "assignees": names(&v, "assignees", "login")}))
            } else {
                let mergeable = if v.get("has_conflicts").and_then(Value::as_bool) == Some(true) {
                    "conflicting"
                } else if v.get("detailed_merge_status").and_then(Value::as_str) == Some("mergeable") {
                    "mergeable"
                } else {
                    "unknown"
                };
                extend(gl_mr(&v), json!({"body": v.get("description").and_then(Value::as_str).unwrap_or(""), "merged_at_ms": ms(&v, "merged_at"), "closed_at_ms": ms(&v, "closed_at"), "mergeable": mergeable, "head_sha": text(&v, "sha"),
                    "additions": Value::Null, "deletions": Value::Null, "changed_files": num(&v, "changes_count"), "reviewers": names(&v, "reviewers", "username"), "assignees": names(&v, "assignees", "username")}))
            };
            Ok(json!({"pr": pr}))
        }
        HostedCall::PrDiff { .. } => Ok(json!({"diff": last.stdout, "truncated": last.truncated})),
        HostedCall::PrChecks { .. } => {
            if last.stdout.trim().is_empty() && last.stderr.contains("no checks reported") {
                return Ok(json!({"checks": [], "pass": 0, "fail": 0, "pending": 0}));
            }
            let v = body(&last)?;
            let rows = v.as_array().ok_or_else(|| io(format!("{program} did not print a list")))?;
            let checks: Vec<Value> = rows
                .iter()
                .map(|c| {
                    if github {
                        json!({"name": text(c, "name"), "state": check_state(c.get("bucket").and_then(Value::as_str).unwrap_or("")), "workflow": text(c, "workflow"), "url": text(c, "link"), "started_at_ms": ms(c, "startedAt"), "completed_at_ms": ms(c, "completedAt")})
                    } else {
                        let status = c.get("status").and_then(Value::as_str).unwrap_or("");
                        let state = check_state(status);
                        json!({"name": format!("pipeline #{}", c.get("id").map_or(String::new(), |i| i.to_string())), "state": state, "workflow": Value::Null, "url": text(c, "web_url"), "started_at_ms": ms(c, "created_at"), "completed_at_ms": if state == "pending" { Value::Null } else { ms(c, "updated_at") }})
                    }
                })
                .collect();
            let count = |s: &str| checks.iter().filter(|c| c["state"] == s).count();
            Ok(json!({"pass": count("pass"), "fail": count("fail"), "pending": count("pending"), "checks": checks}))
        }
        HostedCall::PrComments { .. } => {
            let v = body(&last)?;
            let comments: Vec<Value> = if github {
                items(&v, "comments").iter().map(|c| json!({"id": id_string(c, "id"), "author": login_of(c, "author", "login"), "body": c.get("body").and_then(Value::as_str).unwrap_or(""), "created_at_ms": ms(c, "createdAt"), "url": text(c, "url")})).collect()
            } else {
                v.as_array().map_or(&[][..], Vec::as_slice).iter().filter(|n| n.get("system").and_then(Value::as_bool) != Some(true)).map(|n| json!({"id": id_string(n, "id"), "author": login_of(n, "author", "username"), "body": n.get("body").and_then(Value::as_str).unwrap_or(""), "created_at_ms": ms(n, "created_at"), "url": Value::Null})).collect()
            };
            Ok(json!({"comments": comments}))
        }
        HostedCall::PrReviews { .. } => {
            let v = body(&last)?;
            let reviews: Vec<Value> = if github {
                items(&v, "reviews").iter().map(|r| json!({"id": id_string(r, "id"), "author": login_of(r, "author", "login"), "state": lower(r, "state"), "body": r.get("body").and_then(Value::as_str).unwrap_or(""), "submitted_at_ms": ms(r, "submittedAt")})).collect()
            } else {
                items(&v, "approved_by").iter().map(|a| json!({"id": id_string(&a["user"], "id"), "author": login_of(a, "user", "username"), "state": "approved", "body": "", "submitted_at_ms": Value::Null})).collect()
            };
            Ok(json!({"reviews": reviews}))
        }
        HostedCall::PrThreads { .. } => {
            let v = body(&last)?;
            let threads: Vec<Value> = if github {
                let nodes = v["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"].as_array().map_or(&[][..], Vec::as_slice);
                nodes
                    .iter()
                    .map(|t| {
                        let comments: Vec<Value> = items(&t["comments"], "nodes").iter().map(|c| json!({"id": id_string(c, "databaseId"), "author": login_of(c, "author", "login"), "body": c.get("body").and_then(Value::as_str).unwrap_or(""), "created_at_ms": ms(c, "createdAt"), "url": text(c, "url")})).collect();
                        json!({"id": text(t, "id"), "resolved": t.get("isResolved").and_then(Value::as_bool).unwrap_or(false), "outdated": t.get("isOutdated").and_then(Value::as_bool).unwrap_or(false), "path": text(t, "path"), "line": num(t, "line"), "comments": comments})
                    })
                    .collect()
            } else {
                v.as_array()
                    .map_or(&[][..], Vec::as_slice)
                    .iter()
                    .filter(|d| items(d, "notes").first().and_then(|n| n.get("resolvable")).and_then(Value::as_bool) == Some(true))
                    .map(|d| {
                        let notes = items(d, "notes");
                        let resolved = notes.iter().filter(|n| n.get("resolvable").and_then(Value::as_bool) == Some(true)).all(|n| n.get("resolved").and_then(Value::as_bool) == Some(true));
                        let comments: Vec<Value> = notes.iter().map(|n| json!({"id": id_string(n, "id"), "author": login_of(n, "author", "username"), "body": n.get("body").and_then(Value::as_str).unwrap_or(""), "created_at_ms": ms(n, "created_at"), "url": Value::Null})).collect();
                        json!({"id": text(d, "id"), "resolved": resolved, "outdated": false, "path": notes.first().map_or(Value::Null, |n| text(&n["position"], "new_path")), "line": notes.first().map_or(Value::Null, |n| num(&n["position"], "new_line")), "comments": comments})
                    })
                    .collect()
            };
            Ok(json!({"threads": threads}))
        }
        HostedCall::PrFiles { .. } => {
            let v = body(&last)?;
            let files: Vec<Value> = if github {
                items(&v, "files").iter().map(|f| json!({"path": text(f, "path"), "additions": num(f, "additions"), "deletions": num(f, "deletions"), "status": lower(f, "changeType")})).collect()
            } else {
                items(&v, "changes")
                    .iter()
                    .map(|c| {
                        let status = if c["new_file"] == true { "added" } else if c["deleted_file"] == true { "removed" } else if c["renamed_file"] == true { "renamed" } else { "modified" };
                        let (a, d) = diff_counts(c.get("diff").and_then(Value::as_str).unwrap_or(""));
                        json!({"path": text(c, "new_path"), "additions": a, "deletions": d, "status": status})
                    })
                    .collect()
            };
            Ok(json!({"files": files}))
        }
        HostedCall::PrCreate { .. } => {
            let printed = format!("{}\n{}", last.stdout, last.stderr);
            let marker = if github { "/pull/" } else { "/merge_requests/" };
            let url = printed.split_whitespace().rev().find(|w| w.contains(marker));
            match (url.and_then(|u| number_after(u, marker)), url) {
                (Some(number), Some(url)) => Ok(json!({"number": number, "url": url.trim_end_matches(['.', ',', ')'])})),
                _ => Err(io(format!("{program} printed no address for the new request: {}", brief(&printed)))),
            }
        }
        HostedCall::PrComment { .. } => Ok(json!({"done": true, "url": first_url(&last.stdout)})),
        HostedCall::PrReply { .. } => {
            let v = body(&last)?;
            if github {
                let c = &v["data"]["addPullRequestReviewThreadReply"]["comment"];
                Ok(json!({"id": text(c, "id"), "url": text(c, "url")}))
            } else {
                Ok(json!({"id": id_string(&v, "id"), "url": Value::Null}))
            }
        }
        HostedCall::PrReact { content, undo: true, .. } => Ok(json!({"removed": outputs.len() == 3, "content": content})),
        HostedCall::PrReact { content, .. } => {
            let v = body(&last)?;
            Ok(json!({"id": id_string(&v, "id"), "content": content}))
        }
        HostedCall::PrStack { .. } => {
            let Some(first) = outputs.first() else { return Ok(json!({"stack": null})) };
            let listed = body(first)?;
            let Some(found) = listed.as_array().and_then(|a| a.first()) else { return Ok(json!({"stack": null})) };
            let detail = outputs.get(1).map(body).transpose()?;
            Ok(json!({"stack": stack_of(detail.as_ref().unwrap_or(found))}))
        }
        HostedCall::PrCommentEdit { .. } => Ok(json!({"done": true})),
        HostedCall::PrResolveThread { thread, resolved, .. } => {
            let v = body(&last)?;
            let state = if github {
                let t = &v["data"]["resolveReviewThread"]["thread"];
                let t = if t.is_null() { &v["data"]["unresolveReviewThread"]["thread"] } else { t };
                t.get("isResolved").and_then(Value::as_bool).unwrap_or(*resolved)
            } else {
                items(&v, "notes").iter().filter(|n| n.get("resolvable").and_then(Value::as_bool) == Some(true)).all(|n| n.get("resolved").and_then(Value::as_bool) == Some(true))
            };
            Ok(json!({"thread": thread, "resolved": state}))
        }
        HostedCall::PrEdit { .. } | HostedCall::PrReady { .. } | HostedCall::PrReview { .. } | HostedCall::PrRequestReviewers { .. } | HostedCall::PrLabels { .. } | HostedCall::PrUpdateBranch { .. } | HostedCall::PrMerge { .. } => done(),
        HostedCall::RunList { .. } => {
            let v = body(&last)?;
            let rows = v.as_array().ok_or_else(|| io(format!("{program} did not print a list")))?;
            Ok(json!({"runs": rows.iter().map(|r| if github { gh_run(r) } else { gl_run(r) }).collect::<Vec<_>>()}))
        }
        HostedCall::RunView { .. } => {
            if github {
                let v = body(&last)?;
                let jobs: Vec<Value> = items(&v, "jobs").iter().map(|j| json!({"id": id_string(j, "databaseId"), "name": text(j, "name"), "status": lower(j, "status"), "conclusion": lower(j, "conclusion"), "started_at_ms": ms(j, "startedAt"), "completed_at_ms": ms(j, "completedAt"), "url": text(j, "url")})).collect();
                Ok(json!({"run": gh_run(&v), "jobs": jobs}))
            } else {
                let run = body(outputs.first().unwrap_or(&last))?;
                let jobs_json = body(&last)?;
                let jobs: Vec<Value> = jobs_json
                    .as_array()
                    .map_or(&[][..], Vec::as_slice)
                    .iter()
                    .map(|j| {
                        let (status, conclusion) = run_status(j.get("status").and_then(Value::as_str).unwrap_or(""));
                        json!({"id": id_string(j, "id"), "name": text(j, "name"), "status": status, "conclusion": conclusion, "started_at_ms": ms(j, "started_at"), "completed_at_ms": ms(j, "finished_at"), "url": text(j, "web_url")})
                    })
                    .collect();
                Ok(json!({"run": gl_run(&run), "jobs": jobs}))
            }
        }
        HostedCall::AuthStatus => {
            let both = format!("{}\n{}", last.stdout, last.stderr);
            let status = parse_auth_status(&both);
            Ok(json!({"provider": op.provider.as_str(), "installed": true, "signed_in": status.signed_in, "host": status.host.unwrap_or_else(|| op.host.clone().unwrap_or_else(|| op.provider.default_host().to_string())), "account": status.account, "scopes": status.scopes}))
        }
        HostedCall::LoginStart | HostedCall::LoginWait { .. } | HostedCall::LoginCancel { .. } => Err(io("a sign-in flow is answered by the executor, not shaped from a program's output")),
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AuthStatus {
    pub signed_in: bool,
    pub host: Option<String>,
    pub account: Option<String>,
    pub scopes: Vec<String>,
}

/// `gh auth status` and `glab auth status` text: the line `Logged in to <host> account <login>` (older gh: `as <login>`) and
/// `Token scopes: 'a', 'b'`. The token line is never read, so no token can leave through here.
pub fn parse_auth_status(text: &str) -> AuthStatus {
    let mut status = AuthStatus::default();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.split("Logged in to ").nth(1).filter(|_| !line.to_ascii_lowercase().contains("failed")) {
            let mut words = rest.split_whitespace();
            status.signed_in = true;
            status.host = words.next().map(str::to_string);
            let mut previous = "";
            for w in words {
                if previous == "account" || previous == "as" {
                    status.account = Some(w.trim_matches(|c| c == '(' || c == ')').to_string());
                    break;
                }
                previous = w;
            }
        } else if let Some(rest) = line.split("Token scopes:").nth(1) {
            status.scopes = rest.split(',').map(|s| s.trim().trim_matches('\'').trim_matches('"').to_string()).filter(|s| !s.is_empty() && s != "none").collect();
        }
    }
    status
}

/// A sign-in prompt: the one-time code (GitHub) and the address to open. `None` until the address has been printed.
pub fn parse_login_prompt(text: &str) -> Option<(Option<String>, String)> {
    let url = first_url(text)?.to_string();
    let code = text.split("one-time code:").nth(1).and_then(|rest| rest.split_whitespace().next()).map(str::to_string);
    Some((code, url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_the_status_text_of_each_cli_when_read_then_the_account_host_and_scopes_come_out_and_the_token_line_is_never_kept() {
        // Given what gh and glab print, a token line among them
        let gh = "github.com\n  ✓ Logged in to github.com account octocat (keyring)\n  - Token: gho_SECRETSECRET\n  - Token scopes: 'gist', 'repo'\n";
        let old = "git.example.com\n  ✓ Logged in to git.example.com as hubot (/home/me/.config/gh/hosts.yml)\n";
        let glab = "gitlab.com\n  ✓ Logged in to gitlab.com as tanuki (/c/config.yml)\n  ✓ Token found: ********\n";
        let out = "You are not logged into any GitHub hosts. To log in, run: gh auth login\n";
        // When read
        let parsed: Vec<AuthStatus> = [gh, old, glab, out].iter().map(|t| parse_auth_status(t)).collect();
        // Then each is what it says, and nothing of the token is in any of them
        assert_eq!((parsed[0].account.as_deref(), parsed[0].host.as_deref(), parsed[0].scopes.clone()), (Some("octocat"), Some("github.com"), vec!["gist".to_string(), "repo".to_string()]));
        assert_eq!((parsed[1].account.as_deref(), parsed[2].account.as_deref(), parsed[2].host.as_deref()), (Some("hubot"), Some("tanuki"), Some("gitlab.com")));
        assert_eq!(parsed[3], AuthStatus::default());
        assert!(!format!("{parsed:?}").contains("SECRET"));
    }

    #[test]
    fn given_the_text_of_a_sign_in_when_read_then_the_prompt_is_ready_only_once_the_address_is_printed_and_the_code_is_the_one_after_its_label() {
        assert_eq!(parse_login_prompt("! First copy your one-time code: ABCD-1234\n"), None);
        assert_eq!(parse_login_prompt("! First copy your one-time code: ABCD-1234\nOpen this URL to continue in your web browser: https://github.com/login/device\n"), Some((Some("ABCD-1234".into()), "https://github.com/login/device".into())));
        assert_eq!(parse_login_prompt("Opening https://gitlab.com/oauth/authorize?x=1 in your browser."), Some((None, "https://gitlab.com/oauth/authorize?x=1".into())));
    }

    #[test]
    fn given_a_unified_diff_when_its_lines_are_counted_then_headers_are_not_changes() {
        let diff = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,2 +1,3 @@\n keep\n-old\n+new\n+more\n";
        assert_eq!(diff_counts(diff), (2, 1));
        assert_eq!(diff_counts(""), (0, 0));
    }
}
