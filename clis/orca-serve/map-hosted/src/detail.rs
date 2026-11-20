//! The work-item detail rows, each one answer composed from rca's `issue.view`, `pr.view` (`mr.view`), their comments, checks and files: `github.workItem`,
//! `github.workItemByOwnerRepo` and `github.workItemDetails` (issue or pull request), `gitlab.workItemByPath` and `gitlab.workItemDetails` (issue or merge
//! request). Every read the provider refuses is the host's null. What no rca method reports is left out, never made up: an issue's timeline and participants,
//! a pull request's base SHA, node id, participants and viewed marks, and the reviewers of a merge request, its pipeline jobs and approvals.
use crate::github::{gh_account_binding, int_positive, required_string};
use crate::{checks, refusal, repo_args, required, review, slug, work, Target};
use agent_api_hosted::{issue, pr};
use agent_api_proto::Method as AgentMethod;
use orca_core::wire::{received, Failure, RUNTIME_ERROR};
use orca_core::AgentError;
use serde_json::{json, Map, Value};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Issue,
    Change,
}

pub(crate) fn is_row(method: &str) -> bool {
    matches!(method, "github.workItem" | "github.workItemByOwnerRepo" | "github.workItemDetails" | "gitlab.workItemDetails" | "gitlab.workItemByPath")
}

fn expected(kind: &str, v: &Value) -> Failure {
    Failure::invalid(format!("Invalid input: expected {kind}, received {}", received(v)))
}

fn kind_of(params: &Value, change: &str, required: bool) -> Result<Option<Kind>, Failure> {
    match params.get("type") {
        None if !required => Ok(None),
        Some(Value::String(s)) if s == "issue" => Ok(Some(Kind::Issue)),
        Some(Value::String(s)) if s == change => Ok(Some(Kind::Change)),
        _ => Err(Failure::invalid(format!("Invalid option: expected one of \"issue\"|\"{change}\""))),
    }
}

fn flag(params: &Value, key: &str) -> Result<Option<bool>, Failure> {
    match params.get(key) {
        None => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(other) => Err(expected("boolean", other)),
    }
}

/// The host's `{host, path}` of a GitLab project, checked as its schema does.
fn project_ref(v: &Value) -> Result<(), Failure> {
    match v {
        Value::Object(_) => required_string(v, "host", "Missing GitLab host").and(required_string(v, "path", "Missing GitLab project path")).map(drop),
        other => Err(expected("object", other)),
    }
}

/// The schema of each row in its key order, answering zod's first issue; it is checked before the repo selector is resolved.
pub(crate) fn check(method: &str, params: &Value) -> Result<(), Failure> {
    required_string(params, "repo", "Missing repo selector")?;
    match method {
        "github.workItem" => {
            int_positive(params.get("number"))?;
            kind_of(params, "pr", false)?;
        }
        "github.workItemByOwnerRepo" => {
            required_string(params, "owner", "Missing owner")?;
            required_string(params, "ownerRepo", "Missing repo")?;
            int_positive(params.get("number"))?;
            kind_of(params, "pr", true)?;
        }
        "github.workItemDetails" => {
            int_positive(params.get("number"))?;
            kind_of(params, "pr", false)?;
            if let Some(named) = params.get("ownerRepo") {
                match named {
                    Value::Object(_) => required_string(named, "owner", "Missing owner").and(required_string(named, "repo", "Missing repo")).map(drop)?,
                    other => return Err(expected("object", other)),
                }
            }
            flag(params, "canonical")?;
            match params.get("expectedGhAccount") {
                None | Some(Value::Null) => {}
                Some(account) if gh_account_binding(account) => {}
                Some(_) => return Err(Failure::invalid("Invalid GitHub account expectation")),
            }
        }
        "gitlab.workItemDetails" => {
            int_positive(params.get("iid"))?;
            kind_of(params, "mr", true)?;
            if let Some(named) = params.get("projectRef").filter(|v| !v.is_null()) {
                project_ref(named)?;
            }
            flag(params, "includeReviewDetails")?;
            flag(params, "markViewerComments")?;
        }
        _ => {
            required_string(params, "host", "Missing GitLab host")?;
            required_string(params, "path", "Missing GitLab project path")?;
            int_positive(params.get("iid"))?;
            kind_of(params, "mr", true)?;
        }
    }
    Ok(())
}

/// A read the host answers null for when the provider refuses it: the refusal's code, else the answer.
fn read(t: &Target, method: AgentMethod, args: &Value) -> Result<Result<Value, String>, Failure> {
    match t.try_ask(method, args.clone()) {
        Ok(v) => Ok(Ok(v)),
        Err(AgentError::Refused { code, .. }) if code != "UNKNOWN_WORKSPACE" && code != "INVALID_PARAMS" => Ok(Err(code)),
        Err(other) => Err(refusal(other)),
    }
}

fn object<'a>(answer: &'a Value, key: &str, of: &str) -> Result<&'a Value, Failure> {
    let v = required(answer, key, of)?;
    v.is_object().then_some(v).ok_or_else(|| Failure::new(RUNTIME_ERROR, format!("rca's {of} answered no {key} object")))
}

fn tagged(v: &Value, kind: &str) -> Value {
    let mut v = v.clone();
    v["kind"] = json!(kind);
    v
}

/// The logins as the host's users: its mapping of a login with no profile.
fn users(logins: &Value) -> Value {
    json!(logins.as_array().into_iter().flatten().filter_map(Value::as_str).map(|login| json!({"login": login, "name": null, "avatarUrl": ""})).collect::<Vec<_>>())
}

fn strings(v: &Value) -> Vec<&str> {
    v.as_array().into_iter().flatten().filter_map(Value::as_str).collect()
}

fn args_of(number: u64, repo: &Map<String, Value>, provider: Option<&str>) -> Value {
    let mut args = json!({"number": number});
    args.as_object_mut().expect("an object").extend(repo.clone());
    if let Some(p) = provider {
        args["provider"] = json!(p);
    }
    args
}

fn remotes(t: &Target) -> Result<Vec<Value>, Failure> {
    match t.try_ask(crate::PR_REMOTES, json!({})) {
        Ok(v) => Ok(v["remotes"].as_array().cloned().unwrap_or_default()),
        Err(AgentError::Refused { code, .. }) if code != "UNKNOWN_WORKSPACE" => Ok(Vec::new()),
        Err(other) => Err(refusal(other)),
    }
}

/// The repositories the host may read for a pasted address: the workspace's upstream and origin on github.com, in lower case.
fn github_known(t: &Target) -> Result<Vec<String>, Failure> {
    let remotes = remotes(t)?;
    let slug_of = |name: &str| remotes.iter().find(|r| r["name"] == name).and_then(|r| r["url"].as_str()).and_then(slug::github_com_slug);
    Ok([slug_of("upstream"), slug_of("origin").or_else(|| slug::stand_in(&remotes).and_then(|name| slug_of(&name)))].into_iter().flatten().map(|(o, r)| format!("{o}/{r}").to_ascii_lowercase()).collect())
}

/// The host name a remote URL names (`https://h/…`, `ssh://git@h/…`, `git@h:…`), with the port of a web address.
fn remote_host(url: &str) -> Option<String> {
    let (scheme, rest) = url.trim().split_once("://").map_or((None, url.trim()), |(s, r)| (Some(s), r));
    let authority = if scheme.is_some() { rest.split('/').next()? } else { rest.split(':').next()? };
    let host = authority.rsplit('@').next()?;
    let host = if matches!(scheme, Some("http" | "https")) || scheme.is_none() { host } else { host.split(':').next()? };
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// The args and the `prRepo` of a repository the phone named.
type Named = (Map<String, Value>, Value);

/// The repository args for a GitHub repository the phone named, none when the host would not read it with the account's sign-in: a host other than
/// github.com, or a repository that is neither the upstream nor the origin (a preview reads any repository once the workspace has a github.com remote).
fn github_target(t: &Target, owner: &str, repo: &str, host: Option<&str>, preview: bool) -> Result<Option<Named>, Failure> {
    if host.map(|h| h.trim().to_ascii_lowercase()).is_some_and(|h| !h.is_empty() && h != "github.com") {
        return Ok(None);
    }
    let known = github_known(t)?;
    if !(known.contains(&format!("{owner}/{repo}").to_ascii_lowercase()) || preview && !known.is_empty()) {
        return Ok(None);
    }
    let mut args = Map::new();
    args.insert("repo".into(), json!(format!("{owner}/{repo}")));
    Ok(Some((args, json!({"owner": owner, "repo": repo, "host": "github.com"}))))
}

/// The repository args for a GitLab project the phone named: gitlab.com, or a host one of the workspace's remotes names.
fn gitlab_target(t: &Target, host: &str, path: &str) -> Result<Option<Map<String, Value>>, Failure> {
    let host = host.trim().to_ascii_lowercase();
    if host != "gitlab.com" && !remotes(t)?.iter().filter_map(|r| r["url"].as_str().and_then(remote_host)).any(|h| h == host) {
        return Ok(None);
    }
    Ok(Some(repo_args(&json!({"projectRef": {"host": host, "path": path}}), "projectRef")?))
}

enum Found {
    Issue(Value),
    Change(Value),
}

/// Whether GitHub answered a pull request as an issue (its address says `pull`).
fn pull_address(issue: &Value) -> bool {
    issue["url"].as_str().is_some_and(|u| u.split('/').nth(5) == Some("pull"))
}

/// The host's lookup: an issue before a pull request when the type is not named (and the pull request after an issue refused as not found), a pull request
/// the issue route answers is not an issue unless it is a preview, which reads it as one.
fn github_lookup(t: &Target, number: u64, kind: Option<Kind>, repo: &Map<String, Value>, preview: bool) -> Result<Option<Found>, Failure> {
    let args = args_of(number, repo, None);
    if kind != Some(Kind::Change) {
        match read(t, issue::VIEW, &args)? {
            Ok(v) => {
                let found = object(&v, "issue", "issue.view")?;
                if !pull_address(found) {
                    return Ok(Some(Found::Issue(found.clone())));
                }
                if kind == Some(Kind::Issue) && !preview {
                    return Ok(None);
                }
            }
            Err(code) if kind.is_none() && code == "NOT_FOUND" => {}
            Err(_) => return Ok(None),
        }
    }
    Ok(match read(t, pr::VIEW, &args)? {
        Ok(v) => Some(Found::Change(object(&v, "pr", "pr.view")?.clone())),
        Err(_) => None,
    })
}

fn gitlab_lookup(t: &Target, number: u64, kind: Kind, repo: &Map<String, Value>) -> Result<Option<Value>, Failure> {
    let args = args_of(number, repo, Some("gitlab"));
    Ok(match (kind, read(t, if kind == Kind::Issue { issue::VIEW } else { pr::MR_VIEW }, &args)?) {
        (Kind::Issue, Ok(v)) => Some(object(&v, "issue", "issue.view")?.clone()),
        (Kind::Change, Ok(v)) => Some(object(&v, "pr", "mr.view")?.clone()),
        (_, Err(_)) => None,
    })
}

fn github_issue_item(i: &Value) -> Value {
    let mut item = work::github_item(&tagged(i, "issue"));
    item["assignees"] = users(&i["assignees"]);
    item
}

/// The pull request as the host's work item: the review decision, checks summary and mergeability only for a full read, and the repository when named.
fn github_change_item(p: &Value, checks: Option<&[Value]>, pr_repo: Option<&Value>, preview: bool) -> Value {
    let mut item = work::github_item(&tagged(p, "pr"));
    if let Some(sha) = p["head_sha"].as_str().filter(|s| !s.is_empty()) {
        item["headSha"] = json!(sha);
    }
    if let Some(repo) = pr_repo {
        item["prRepo"] = repo.clone();
    }
    for (key, from) in [("additions", "additions"), ("deletions", "deletions"), ("changedFiles", "changed_files")] {
        if let Some(n) = p[from].as_u64() {
            item[key] = json!(n);
        }
    }
    item["assignees"] = users(&p["assignees"]);
    if let Some(mergeable) = p["mergeable"].as_str().filter(|m| matches!(*m, "mergeable" | "conflicting" | "unknown")) {
        item["mergeable"] = json!(mergeable.to_ascii_uppercase());
    }
    if !preview {
        item["reviewDecision"] = p["review_decision"].as_str().map_or(Value::Null, |d| json!(d.to_ascii_uppercase()));
        if let Some(checks) = checks {
            item["checksSummary"] = checks::summary(checks);
        }
    }
    item
}

/// The check details of a pull request, none when the provider refuses them.
fn change_checks(t: &Target, number: u64, repo: &Map<String, Value>) -> Result<Option<Vec<Value>>, Failure> {
    match read(t, pr::CHECKS, &args_of(number, repo, None))? {
        Ok(v) => Ok(Some(checks::details(&v)?)),
        Err(_) => Ok(None),
    }
}

fn github_item_of(t: &Target, number: u64, found: &Found, repo: &Map<String, Value>, pr_repo: Option<&Value>, preview: bool) -> Result<Value, Failure> {
    Ok(match found {
        Found::Issue(i) => github_issue_item(i),
        Found::Change(p) => {
            let checks = if preview { None } else { change_checks(t, number, repo)? };
            github_change_item(p, checks.as_deref(), pr_repo, preview)
        }
    })
}

/// `github.workItem`: the issue or pull request of the number in the workspace's repository, null when it cannot be read.
pub fn github_work_item(t: &Target, params: &Value) -> Result<Value, Failure> {
    let (number, kind) = (int_positive(params.get("number"))?, kind_of(params, "pr", false)?);
    let none = Map::new();
    match github_lookup(t, number, kind, &none, false)? {
        Some(found) => github_item_of(t, number, &found, &none, None, false),
        None => Ok(Value::Null),
    }
}

/// `github.workItemByOwnerRepo`: the same for the repository the phone names, when the host would read it with the account's sign-in.
pub fn github_by_owner_repo(t: &Target, params: &Value) -> Result<Value, Failure> {
    let (owner, repo) = (required_string(params, "owner", "Missing owner")?, required_string(params, "ownerRepo", "Missing repo")?);
    let (number, kind) = (int_positive(params.get("number"))?, kind_of(params, "pr", true)?);
    let preview = params["preview"] == true;
    let Some((args, pr_repo)) = github_target(t, owner, repo, params.get("host").and_then(Value::as_str), preview)? else { return Ok(Value::Null) };
    match github_lookup(t, number, kind, &args, preview)? {
        Some(found) => github_item_of(t, number, &found, &args, Some(&pr_repo), preview),
        None => Ok(Value::Null),
    }
}

/// The comment rows of an answer's `comments`, oldest first.
fn comment_rows(answer: &Value, of: &str) -> Result<Vec<(u64, Value)>, Failure> {
    let list = answer["comments"].as_array().ok_or_else(|| Failure::new(RUNTIME_ERROR, format!("rca's {of} answered no comments list")))?;
    let mut rows: Vec<(u64, Value)> = list.iter().filter_map(|c| review::comment_row(c, "created_at_ms")).collect();
    rows.sort_by_key(|(at, _)| *at);
    Ok(rows)
}

/// A comment read that a refresh must not turn into an empty list.
fn or_empty(read: Result<Value, Failure>, canonical: bool) -> Result<Value, Failure> {
    match read {
        Err(_) if canonical => Err(Failure::new(RUNTIME_ERROR, "GitHub details unavailable")),
        Err(_) => Ok(json!([])),
        Ok(rows) => Ok(rows),
    }
}

/// `github.workItemDetails`: the item with its body and comments, and for a pull request its head, checks and files.
pub fn github_details(t: &Target, params: &Value) -> Result<Value, Failure> {
    let (number, kind) = (int_positive(params.get("number"))?, kind_of(params, "pr", false)?);
    let canonical = params["canonical"] == true;
    let (repo, pr_repo) = match params.get("ownerRepo").filter(|_| kind.is_some()) {
        Some(named) => match github_target(t, named["owner"].as_str().unwrap_or(""), named["repo"].as_str().unwrap_or(""), named["host"].as_str(), false)? {
            Some((args, pr_repo)) => (args, Some(pr_repo)),
            None => return Ok(Value::Null),
        },
        None => (Map::new(), None),
    };
    let Some(found) = github_lookup(t, number, kind, &repo, false)? else { return Ok(Value::Null) };
    let args = args_of(number, &repo, None);
    match &found {
        Found::Issue(i) => {
            let comments = or_empty(read(t, issue::COMMENTS, &args)?.map_err(|_| Failure::new(RUNTIME_ERROR, "")).and_then(|v| comment_rows(&v, "issue.comments")).map(|rows| json!(rows.into_iter().map(|(_, r)| r).collect::<Vec<_>>())), canonical)?;
            Ok(json!({"item": github_issue_item(i), "body": i["body"].as_str().unwrap_or(""), "comments": comments, "assignees": strings(&i["assignees"])}))
        }
        Found::Change(p) => {
            let checks = change_checks(t, number, &repo)?;
            let comments = or_empty(review::pull_comments(t, args.clone()), canonical)?;
            let files = read(t, pr::FILES, &args)?.ok().map(|v| {
                v["files"].as_array().into_iter().flatten().map(|f| {
                    let mut row = json!({"path": f["path"], "additions": f["additions"].as_u64().unwrap_or(0), "deletions": f["deletions"].as_u64().unwrap_or(0)});
                    if let Some(status) = f["status"].as_str() {
                        row["status"] = json!(status);
                    }
                    row
                }).collect::<Vec<_>>()
            });
            let mut out = json!({"item": github_change_item(p, checks.as_deref(), pr_repo.as_ref(), false), "body": p["body"].as_str().unwrap_or(""), "comments": comments, "checks": checks.clone().unwrap_or_default(), "filesUnavailable": files.is_none()});
            if let Some(sha) = p["head_sha"].as_str().filter(|s| !s.is_empty()) {
                out["headSha"] = json!(sha);
            }
            if let Some(files) = files {
                out["files"] = json!(files);
            }
            Ok(out)
        }
    }
}

/// A GitLab row of a view: the issue or merge request with its project, the mergeability of a merge request.
fn gitlab_item_of(view: &Value, kind: Kind, project: Option<(String, String)>) -> Value {
    let mut item = work::gitlab_item(&tagged(view, if kind == Kind::Issue { "issue" } else { "pr" }), project);
    if let Some(mergeable) = view["mergeable"].as_str().filter(|m| kind == Kind::Change && matches!(*m, "mergeable" | "conflicting" | "unknown")) {
        item["mergeable"] = json!(mergeable.to_ascii_uppercase());
    }
    item
}

/// `gitlab.workItemByPath`: the issue or merge request of the project the phone names.
pub fn gitlab_by_path(t: &Target, params: &Value) -> Result<Value, Failure> {
    let (host, path) = (required_string(params, "host", "Missing GitLab host")?, required_string(params, "path", "Missing GitLab project path")?);
    let (iid, kind) = (int_positive(params.get("iid"))?, kind_of(params, "mr", true)?.unwrap_or(Kind::Issue));
    let Some(args) = gitlab_target(t, host, path)? else { return Ok(Value::Null) };
    Ok(gitlab_lookup(t, iid, kind, &args)?.map_or(Value::Null, |view| gitlab_item_of(&view, kind, Some((host.to_string(), path.to_string())))))
}

/// The notes of a merge request, with the thread, resolved mark and line of each note that starts or answers a thread.
fn merge_request_comments(t: &Target, args: &Value) -> Result<Value, Failure> {
    let mut rows = comment_rows(&t.ask(pr::MR_COMMENTS, args.clone())?, "mr.comments")?;
    if let Ok(threads) = read(t, pr::MR_THREADS, args)? {
        for thread in threads["threads"].as_array().into_iter().flatten() {
            for (index, c) in thread["comments"].as_array().into_iter().flatten().enumerate() {
                let Some((_, row)) = review::comment_row(c, "created_at_ms").and_then(|(_, r)| rows.iter_mut().find(|(_, x)| x["id"] == r["id"])) else { continue };
                row["threadId"] = thread["id"].clone();
                row["isResolved"] = json!(thread["resolved"] == true);
                for (key, from) in [("path", "path"), ("line", "line")] {
                    if index == 0 && !thread[from].is_null() {
                        row[key] = thread[from].clone();
                    }
                }
            }
        }
    }
    Ok(json!(rows.into_iter().map(|(_, r)| r).collect::<Vec<_>>()))
}

/// `gitlab.workItemDetails`: the item with its body and comments, an issue's assignees, and a merge request's head and files.
pub fn gitlab_details(t: &Target, params: &Value) -> Result<Value, Failure> {
    let (iid, kind) = (int_positive(params.get("iid"))?, kind_of(params, "mr", true)?.unwrap_or(Kind::Issue));
    let named = params.get("projectRef").filter(|v| !v.is_null());
    let project = named.map(|p| (p["host"].as_str().unwrap_or("").to_string(), p["path"].as_str().unwrap_or("").to_string()));
    let repo = match &project {
        Some((host, path)) => match gitlab_target(t, host, path)? {
            Some(args) => args,
            None => return Ok(Value::Null),
        },
        None => Map::new(),
    };
    let Some(view) = gitlab_lookup(t, iid, kind, &repo)? else { return Ok(Value::Null) };
    let args = args_of(iid, &repo, Some("gitlab"));
    let mut item = gitlab_item_of(&view, kind, project);
    item.as_object_mut().map(|o| o.remove("repoId"));
    let mut out = json!({"item": item, "body": view["body"].as_str().unwrap_or("")});
    if kind == Kind::Issue {
        out["comments"] = json!(comment_rows(&t.ask(issue::COMMENTS, args)?, "issue.comments")?.into_iter().map(|(_, r)| r).collect::<Vec<_>>());
        out["assignees"] = json!(strings(&view["assignees"]));
        return Ok(out);
    }
    out["comments"] = merge_request_comments(t, &args)?;
    if let Some(sha) = view["head_sha"].as_str().filter(|s| !s.is_empty()) {
        out["headSha"] = json!(sha);
    }
    if params["includeReviewDetails"] != false {
        if let Ok(v) = read(t, pr::MR_FILES, &args)? {
            out["files"] = json!(v["files"].as_array().into_iter().flatten().map(|f| json!({"path": f["path"], "status": f["status"], "additions": f["additions"].as_u64().unwrap_or(0), "deletions": f["deletions"].as_u64().unwrap_or(0)})).collect::<Vec<_>>());
        }
    }
    Ok(out)
}
