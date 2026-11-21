//! The `github.project.*` rows over rca's `hosted.project`, `hosted.pr` and `hosted.issue` groups, asked of an executor whose gh is signed in: the rows
//! carry an owner and repository by slug, or a project, and no workspace, so the service routes them by account. Every answer is the host's
//! `{ok: true, ...}` or `{ok: false, error}` with its classified error. Left out, never made up: `bodyImageUrls` (display data the host never persists),
//! the participants and node id of a work item (no rca method reports them), a state reason when closing an issue (the host's schema strips it), and
//! `github.project.viewTable` (its table of fields, items and views has no rca method).
use crate::github::{int_positive, required_string};
use crate::{refusal, review, text, time, Target};
use agent_api_hosted::{issue, pr, project};
use agent_api_proto::Method as AgentMethod;
use orca_core::wire::{received, Failure};
use orca_core::AgentError;
use serde_json::{json, Map, Value};

const DEFAULT_HOST: &str = "github.com";
const INPUT_MAX_BYTES: usize = 2 * 1024;

pub(crate) const ROWS: &[&str] = &[
    "github.project.listAccessible",
    "github.project.listLabelsBySlug",
    "github.project.listAssignableUsersBySlug",
    "github.project.listIssueTypesBySlug",
    "github.project.resolveRef",
    "github.project.listViews",
    "github.project.workItemDetailsBySlug",
    "github.project.updateItemField",
    "github.project.clearItemField",
    "github.project.updateIssueBySlug",
    "github.project.updatePullRequestBySlug",
    "github.project.updateIssueTypeBySlug",
    "github.project.addIssueCommentBySlug",
    "github.project.updateIssueCommentBySlug",
    "github.project.deleteIssueCommentBySlug",
];

pub(crate) fn is_row(method: &str) -> bool {
    ROWS.contains(&method)
}

fn expected(kind: &str, v: Option<&Value>) -> Failure {
    Failure::invalid(format!("Invalid input: expected {kind}, received {}", v.map_or("undefined", received)))
}

/// zod's `OptionalString`: anything but a non-empty string reads as absent.
fn optional(params: &Value, key: &str) -> Option<String> {
    text(params, key).map(str::to_string)
}

fn host_of(params: &Value) -> String {
    optional(params, "host").unwrap_or_else(|| DEFAULT_HOST.to_string())
}

fn enum_of<'a>(v: Option<&'a Value>, options: &[&str]) -> Result<&'a str, Failure> {
    match v.and_then(Value::as_str).filter(|s| options.contains(s)) {
        Some(s) => Ok(s),
        None => Err(Failure::invalid(format!("Invalid option: expected one of {}", options.iter().map(|o| format!("\"{o}\"")).collect::<Vec<_>>().join("|")))),
    }
}

fn optional_enum<'a>(v: Option<&'a Value>, options: &[&str]) -> Result<Option<&'a str>, Failure> {
    match v {
        None => Ok(None),
        some => enum_of(some, options).map(Some),
    }
}

fn strings(v: Option<&Value>) -> Result<Vec<String>, Failure> {
    match v {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => items.iter().map(|i| i.as_str().map(str::to_string).ok_or_else(|| expected("string", Some(i)))).collect(),
        Some(other) => Err(expected("array", Some(other))),
    }
}

/// The schema of each row in its key order, answering zod's first issue; it is checked before anything is asked.
pub(crate) fn check(method: &str, params: &Value) -> Result<(), Failure> {
    let slug = |params: &Value| required_string(params, "owner", "Missing owner").and(required_string(params, "repo", "Missing repo")).map(drop);
    match method {
        "github.project.listAccessible" => {}
        "github.project.listLabelsBySlug" | "github.project.listAssignableUsersBySlug" | "github.project.listIssueTypesBySlug" => slug(params)?,
        "github.project.resolveRef" => {
            required_string(params, "input", "Missing project reference")?;
        }
        "github.project.listViews" => {
            required_string(params, "owner", "Missing owner")?;
            enum_of(params.get("ownerType"), &["organization", "user"])?;
            int_positive(params.get("projectNumber"))?;
        }
        "github.project.workItemDetailsBySlug" => {
            slug(params)?;
            int_positive(params.get("number"))?;
            enum_of(params.get("type"), &["issue", "pr"])?;
        }
        "github.project.updateItemField" | "github.project.clearItemField" => {
            required_string(params, "projectId", "Missing project ID")?;
            required_string(params, "itemId", "Missing item ID")?;
            required_string(params, "fieldId", "Missing field ID")?;
        }
        "github.project.updateIssueBySlug" | "github.project.updatePullRequestBySlug" => {
            slug(params)?;
            int_positive(params.get("number"))?;
            match params.get("updates") {
                Some(Value::Object(u)) => {
                    optional_enum(u.get("state"), &["open", "closed"])?;
                    if method == "github.project.updateIssueBySlug" {
                        for key in ["addLabels", "removeLabels", "addAssignees", "removeAssignees"] {
                            strings(u.get(key))?;
                        }
                    }
                }
                other => return Err(expected("object", other)),
            }
        }
        "github.project.updateIssueTypeBySlug" => {
            slug(params)?;
            int_positive(params.get("number"))?;
            match params.get("issueTypeId") {
                Some(Value::String(_) | Value::Null) => {}
                other => return Err(expected("string", other)),
            }
        }
        "github.project.addIssueCommentBySlug" => {
            slug(params)?;
            int_positive(params.get("number"))?;
            required_string(params, "body", "Comment body required")?;
        }
        "github.project.updateIssueCommentBySlug" => {
            slug(params)?;
            int_positive(params.get("commentId"))?;
            required_string(params, "body", "Comment body required")?;
        }
        _ => {
            slug(params)?;
            int_positive(params.get("commentId"))?;
        }
    }
    Ok(())
}

/// The host's classified error of a failure: its sign-in, scope, rate, network, not-found and request wording, from what the executor reported.
fn classify(code: &str, message: &str, host: &str) -> Value {
    let s = message.to_ascii_lowercase();
    let not_signed_in = matches!(code, "NO_EXECUTOR" | "PROVIDER_NOT_SIGNED_IN");
    // An instance other than github.com is checked before any request, and the host words that refusal as its own
    if not_signed_in && host != DEFAULT_HOST {
        return json!({"type": "auth_required", "message": format!("GitHub CLI is not authenticated to {host}. Run gh auth login --hostname {host}."), "host": host});
    }
    if not_signed_in || s.contains("authentication required") || s.contains("not logged in") || s.contains("gh auth login") {
        return json!({"type": "auth_required", "message": format!("Sign in to GitHub to load project tasks. Run `gh auth login --hostname {host}`."), "host": host});
    }
    if s.contains("missing required scope") || s.contains("your token has not been granted") || (s.contains("resource not accessible") && (s.contains("project") || s.contains("scope"))) {
        return json!({"type": "scope_missing", "message": format!("GitHub project access needs additional scopes. Run `gh auth refresh --hostname {host} -s project -s read:org -s repo`."), "host": host});
    }
    if s.contains("rate limit") || s.contains("api rate limit exceeded") {
        return json!({"type": "rate_limited", "message": "GitHub rate limit hit. Try again in a few minutes."});
    }
    if s.contains("timeout") || s.contains("did not finish within") || s.contains("no such host") || s.contains("network") || s.contains("could not resolve host") || s.contains("dial tcp") {
        return json!({"type": "network_error", "message": "Network error — check your connection."});
    }
    if code == "NOT_FOUND" || s.contains("http 404") || s.contains("could not resolve to a") {
        return json!({"type": "not_found", "message": "Project or view not found.", "reason": "project_or_view_not_found"});
    }
    if s.contains("http 422") || s.contains("validation failed") || code == "INVALID_ARGS" {
        return json!({"type": "validation_error", "message": format!("Invalid request — {}", message.trim()), "reason": "request_invalid", "detail": message.trim()});
    }
    let first = message.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let safe: String = if first.chars().count() > 200 { first.chars().take(200).chain(std::iter::once('…')).collect() } else { first.to_string() };
    if safe.is_empty() {
        json!({"type": "unknown", "message": "GitHub request failed.", "reason": "request_failed"})
    } else {
        json!({"type": "unknown", "message": format!("GitHub request failed: {safe}"), "reason": "request_failed", "detail": safe})
    }
}

fn validation(message: &str, reason: &str) -> Value {
    json!({"type": "validation_error", "message": message, "reason": reason})
}

/// What a request ends in: the answer, or the host's classified error (a failure of the service itself stays a `Failure`).
type Answer = Result<Value, Value>;

fn fail(error: Value) -> Value {
    json!({"ok": false, "error": error})
}

/// The group's answer with a refusal as its code and message, for a caller that words some of them itself.
fn raw(t: &Target, method: AgentMethod, args: Value) -> Result<Result<Value, (String, String)>, Failure> {
    match t.try_ask(method, args) {
        Ok(v) => Ok(Ok(v)),
        Err(AgentError::Refused { code, message }) if code != "INVALID_PARAMS" && code != "UNKNOWN_WORKSPACE" => Ok(Err((code, message))),
        Err(other) => Err(refusal(other)),
    }
}

fn ask(t: &Target, method: AgentMethod, args: Value, host: &str) -> Result<Answer, Failure> {
    Ok(raw(t, method, with_host(args, host))?.map_err(|(code, message)| classify(&code, &message, host)))
}

/// `args` with the instance named when it is not github.com.
fn with_host(mut args: Value, host: &str) -> Value {
    if host != DEFAULT_HOST {
        args["host"] = json!(host);
    }
    args
}

fn owner_slug_ok(v: &str) -> bool {
    let mut chars = v.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric()) && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn repo_slug_ok(v: &str) -> bool {
    !v.is_empty() && v != "." && v != ".." && v.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// The host's refusal of a slug that is no GitHub name, before anything is asked.
fn slug_error(field: &str, value: &str) -> Value {
    json!({"type": "validation_error", "message": format!("Invalid {field}: \"{value}\" is not a valid GitHub slug."), "reason": "invalid_slug", "detail": field, "value": value})
}

fn slug_of(params: &Value) -> Result<String, Value> {
    let (owner, repo) = (text(params, "owner").unwrap_or(""), text(params, "repo").unwrap_or(""));
    if !owner_slug_ok(owner) {
        return Err(slug_error("owner", owner));
    }
    if !repo_slug_ok(repo) {
        return Err(slug_error("repo", repo));
    }
    Ok(format!("{owner}/{repo}"))
}

pub fn call(t: &Target, method: &str, params: &Value) -> Result<Value, Failure> {
    let host = host_of(params);
    let outcome = match method {
        "github.project.listAccessible" => list_accessible(t, &host)?,
        "github.project.resolveRef" => return resolve_ref(t, params),
        "github.project.listViews" => list_views(t, params, &host)?,
        "github.project.updateItemField" | "github.project.clearItemField" => item_field(t, method, params, &host)?,
        _ => {
            let repo = match slug_of(params) {
                Ok(repo) => repo,
                Err(error) => return Ok(fail(error)),
            };
            by_slug(t, method, params, &repo, &host)?
        }
    };
    Ok(match outcome {
        Ok(ok) => ok_first(ok),
        Err(error) => fail(error),
    })
}

/// `{ok: true, ...}` with `ok` first, as the host writes it.
fn ok_first(rest: Value) -> Value {
    let mut out = Map::new();
    out.insert("ok".into(), json!(true));
    if let Value::Object(map) = rest {
        out.extend(map);
    }
    Value::Object(out)
}

fn list_accessible(t: &Target, host: &str) -> Result<Answer, Failure> {
    Ok(ask(t, project::LIST, json!({}), host)?.map(|a| {
        let projects: Vec<Value> = a["projects"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| json!({"id": p["id"], "host": host, "owner": p["owner"], "ownerType": p["owner_type"], "number": p["number"], "title": p["title"], "url": p["url"], "source": p["source"]}))
            .collect();
        json!({"projects": projects})
    }))
}

fn list_views(t: &Target, params: &Value, host: &str) -> Result<Answer, Failure> {
    let owner = text(params, "owner").unwrap_or("");
    if !owner_slug_ok(owner) {
        return Ok(Err(slug_error("owner", owner)));
    }
    let (owner_type, number) = (text(params, "ownerType").unwrap_or(""), int_positive(params.get("projectNumber"))?);
    let args = json!({"owner": owner, "owner_type": owner_type, "project_number": number});
    Ok(ask(t, project::VIEWS, args, host)?.map(|a| {
        let views: Vec<Value> = a["views"].as_array().into_iter().flatten().map(|v| json!({"id": v["id"], "number": v["number"].as_u64().unwrap_or(0), "name": v["name"].as_str().unwrap_or(""), "layout": v["layout"]})).collect();
        json!({"views": views})
    }))
}

/// The `value` of a project field write as the executor's `kind` and value, or the host's refusal of a kind it does not know.
fn field_value(value: &Value) -> Result<Value, Value> {
    let kind = value.get("kind");
    let named = kind.and_then(Value::as_str).unwrap_or("undefined");
    let (wire, key) = match named {
        "single-select" => ("single_select", "optionId"),
        "iteration" => ("iteration", "iterationId"),
        "text" => ("text", "text"),
        "number" => ("number", "number"),
        "date" => ("date", "date"),
        _ => return Err(json!({"type": "validation_error", "message": format!("Unknown project field mutation kind: {named}")})),
    };
    let field = value.get(key);
    match (wire, field) {
        ("number", Some(Value::Number(n))) => Ok(json!({"kind": wire, "number_value": n})),
        ("number", _) => Err(validation("Invalid number value.", "request_invalid")),
        (_, Some(Value::String(s))) => Ok(json!({"kind": wire, "value": s})),
        _ => Err(validation(&format!("Invalid {named} value."), "request_invalid")),
    }
}

fn item_field(t: &Target, method: &str, params: &Value, host: &str) -> Result<Answer, Failure> {
    let ids = json!({"project": text(params, "projectId"), "item": text(params, "itemId"), "field": text(params, "fieldId")});
    if method == "github.project.clearItemField" {
        return Ok(ask(t, project::CLEAR_FIELD, ids, host)?.map(|_| json!({})));
    }
    let value = match field_value(params.get("value").unwrap_or(&Value::Null)) {
        Ok(v) => v,
        Err(e) => return Ok(Err(e)),
    };
    let mut args = ids;
    args.as_object_mut().expect("an object").extend(value.as_object().cloned().unwrap_or_default());
    Ok(ask(t, project::SET_FIELD, args, host)?.map(|_| json!({})))
}

fn by_slug(t: &Target, method: &str, params: &Value, repo: &str, host: &str) -> Result<Answer, Failure> {
    let number = || int_positive(params.get("number"));
    match method {
        "github.project.listLabelsBySlug" => Ok(ask(t, pr::LABEL_CANDIDATES, json!({"repo": repo, "limit": 200}), host)?.map(|a| json!({"labels": a["labels"].as_array().into_iter().flatten().filter_map(|l| l["name"].as_str()).filter(|n| !n.is_empty()).collect::<Vec<_>>()}))),
        "github.project.listAssignableUsersBySlug" => Ok(ask(t, pr::REVIEWER_CANDIDATES, json!({"repo": repo, "limit": 200}), host)?.map(|a| {
            let mut users: Vec<Value> = a["users"].as_array().into_iter().flatten().filter_map(|u| u["login"].as_str().filter(|l| !l.is_empty()).map(|login| json!({"login": login, "name": null, "avatarUrl": u["avatarUrl"].as_str().unwrap_or("")}))).collect();
            for seed in params.get("seedLogins").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                if !users.iter().any(|u| u["login"] == seed) {
                    users.push(json!({"login": seed, "name": null, "avatarUrl": ""}));
                }
            }
            json!({"users": users})
        })),
        "github.project.listIssueTypesBySlug" => {
            Ok(match raw(t, project::ISSUE_TYPES, with_host(json!({"repo": repo}), host))? {
                Ok(a) => Ok(json!({"types": a["types"].as_array().into_iter().flatten().map(|ty| json!({"id": ty["id"], "name": ty["name"], "color": ty["color"], "description": ty["description"]})).collect::<Vec<_>>()})),
                // A host without issue types has no such field, which the host answers as none
                Err((_, message)) if message.to_ascii_lowercase().contains("doesn't exist") || message.to_ascii_lowercase().contains("issuetypes") => Ok(json!({"types": []})),
                Err((code, message)) => Err(classify(&code, &message, host)),
            })
        }
        "github.project.updateIssueTypeBySlug" => {
            let mut args = json!({"repo": repo, "number": number()?});
            if let Some(id) = params.get("issueTypeId").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                args["issue_type"] = json!(id);
            }
            Ok(match raw(t, project::SET_ISSUE_TYPE, with_host(args, host))? {
                Ok(_) => Ok(json!({})),
                Err((_, message)) if message.starts_with("issue not found") => Err(json!({"type": "not_found", "message": "Issue not found.", "reason": "issue_not_found"})),
                Err((code, message)) => Err(classify(&code, &message, host)),
            })
        }
        "github.project.updateIssueBySlug" => update_issue(t, params, repo, host, number()?),
        "github.project.updatePullRequestBySlug" => update_pull_request(t, params, repo, host, number()?),
        "github.project.addIssueCommentBySlug" => {
            if params["body"].as_str().is_some_and(|b| b.trim().is_empty()) {
                return Ok(Err(validation("Comment body required.", "comment_body_required")));
            }
            let args = json!({"repo": repo, "number": number()?, "body": params["body"]});
            Ok(ask(t, project::COMMENT_ADD, args, host)?.and_then(|a| {
                let c = &a["comment"];
                let id = c["id"].as_u64().ok_or_else(|| json!({"type": "unknown", "message": "GitHub request failed.", "reason": "request_failed"}))?;
                Ok(json!({"comment": {"id": id, "author": c["author"].as_str().unwrap_or("You"), "authorAvatarUrl": c["avatar_url"].as_str().unwrap_or(""), "body": c["body"].as_str().unwrap_or(params["body"].as_str().unwrap_or("")), "createdAt": c["created_at"].as_str().unwrap_or(""), "url": c["url"].as_str().unwrap_or(""), "isBot": c["is_bot"] == true}}))
            }))
        }
        "github.project.updateIssueCommentBySlug" => {
            if params["body"].as_str().is_some_and(|b| b.trim().is_empty()) {
                return Ok(Err(validation("Comment body required.", "comment_body_required")));
            }
            let args = json!({"repo": repo, "comment_id": int_positive(params.get("commentId"))?, "body": params["body"]});
            Ok(ask(t, project::COMMENT_EDIT, args, host)?.map(|_| json!({})))
        }
        "github.project.deleteIssueCommentBySlug" => Ok(ask(t, project::COMMENT_DELETE, json!({"repo": repo, "comment_id": int_positive(params.get("commentId"))?}), host)?.map(|_| json!({}))),
        _ => work_item_details(t, params, repo, host, number()?),
    }
}

fn update_issue(t: &Target, params: &Value, repo: &str, host: &str, number: u64) -> Result<Answer, Failure> {
    let u = &params["updates"];
    let mut args = json!({"repo": repo, "number": number});
    let mut any = false;
    if let Some(state) = u.get("state").and_then(Value::as_str) {
        args["state"] = json!(state);
        any = true;
    }
    for key in ["title", "body"] {
        if let Some(v) = optional(u, key) {
            args[key] = json!(v);
            any = true;
        }
    }
    for (from, to) in [("addLabels", "add_labels"), ("removeLabels", "remove_labels"), ("addAssignees", "add_assignees"), ("removeAssignees", "remove_assignees")] {
        let names = strings(u.get(from))?;
        if !names.is_empty() {
            args[to] = json!(names);
            any = true;
        }
    }
    if !any {
        return Ok(Ok(json!({})));
    }
    Ok(ask(t, project::UPDATE_ISSUE, args, host)?.map(|_| json!({})))
}

fn update_pull_request(t: &Target, params: &Value, repo: &str, host: &str, number: u64) -> Result<Answer, Failure> {
    let u = &params["updates"];
    let mut edit = json!({"repo": repo, "number": number});
    let mut edits = false;
    for key in ["title", "body"] {
        if let Some(v) = optional(u, key) {
            edit[key] = json!(v);
            edits = true;
        }
    }
    if edits {
        if let Err(e) = ask(t, pr::EDIT, edit, host)? {
            return Ok(Err(e));
        }
    }
    if let Some(state) = u.get("state").and_then(Value::as_str) {
        if let Err(e) = ask(t, pr::SET_STATE, json!({"repo": repo, "number": number, "state": state}), host)? {
            return Ok(Err(e));
        }
    }
    Ok(Ok(json!({})))
}

/// A work item's address says whether GitHub answered a pull request as an issue.
fn pull_address(issue: &Value) -> bool {
    issue["url"].as_str().is_some_and(|u| u.split('/').nth(5) == Some("pull"))
}

fn names(v: &Value) -> Vec<&str> {
    v.as_array().into_iter().flatten().filter_map(Value::as_str).collect()
}

fn work_item_details(t: &Target, params: &Value, repo: &str, host: &str, number: u64) -> Result<Answer, Failure> {
    let change = params["type"] == "pr";
    let not_found = || json!({"type": "not_found", "message": "Item not found.", "reason": "item_not_found"});
    let args = json!({"repo": repo, "number": number});
    let view = match raw(t, if change { pr::VIEW } else { issue::VIEW }, with_host(args.clone(), host))? {
        Ok(v) => v,
        Err((code, _)) if code == "NOT_FOUND" => return Ok(Err(not_found())),
        Err((code, message)) => return Ok(Err(classify(&code, &message, host))),
    };
    let item = &view[if change { "pr" } else { "issue" }];
    if !item.is_object() || (!change && pull_address(item)) {
        return Ok(Err(not_found()));
    }
    let comments = match ask(t, if change { pr::COMMENTS } else { issue::COMMENTS }, args, host)? {
        Ok(a) => a,
        Err(e) => return Ok(Err(e)),
    };
    let mut rows: Vec<(u64, Value)> = comments["comments"].as_array().into_iter().flatten().filter_map(|c| review::comment_row(c, "created_at_ms")).collect();
    rows.sort_by_key(|(at, _)| *at);
    let state = match (change, item["state"].as_str(), item["draft"] == true) {
        (true, _, true) => "draft",
        (true, Some("merged"), _) => "merged",
        (_, Some("closed"), _) => "closed",
        _ => "open",
    };
    let mut details_item = json!({
        "id": "",
        "type": if change { "pr" } else { "issue" },
        "number": item["number"].as_u64().unwrap_or(number),
        "title": item["title"].as_str().unwrap_or(""),
        "state": state,
        "url": item["url"].as_str().unwrap_or(""),
        "labels": item["labels"].clone(),
        "updatedAt": item["updated_at_ms"].as_u64().map(time::iso).unwrap_or_default(),
        "author": item["author"].as_str().map_or(Value::Null, |a| json!(a)),
    });
    if change {
        details_item["branchName"] = json!(item["head_ref"].as_str().unwrap_or(""));
        details_item["baseRefName"] = json!(item["base_ref"].as_str().unwrap_or(""));
    }
    Ok(Ok(json!({"details": {"item": details_item, "body": item["body"].as_str().unwrap_or(""), "comments": rows.into_iter().map(|(_, r)| r).collect::<Vec<_>>(), "participants": [], "assignees": names(&item["assignees"])}})))
}

/// What a pasted project reference names.
enum Pasted {
    Org { owner: String, number: u64, host: String, view: Option<u64> },
    User { owner: String, number: u64, host: String, view: Option<u64> },
    Bare { owner: String, number: u64 },
}

fn number_of(part: &str) -> Option<u64> {
    (!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())).then(|| part.parse::<u64>().ok()).flatten().filter(|n| (1..(1u64 << 53)).contains(n))
}

/// The project a pasted address or `owner/number` names: an exact project URL on github.com (or the named host) with no credentials, never a prefix match.
fn parse_paste(input: &str, host: Option<&str>) -> Option<Pasted> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.len() > INPUT_MAX_BYTES {
        return None;
    }
    if let Some((scheme, rest)) = trimmed.split_once("://") {
        if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
            return None;
        }
        let (authority, path) = rest.split_once('/').map_or((rest, ""), |(a, p)| (a, p));
        let path = path.split(['?', '#']).next().unwrap_or("");
        if authority.contains('@') {
            return None;
        }
        let url_host = authority.to_ascii_lowercase();
        let named = host.map(|h| h.trim().to_ascii_lowercase());
        if url_host != DEFAULT_HOST && named.as_deref() != Some(url_host.as_str()) {
            return None;
        }
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        let with_view = parts.len() == 6 && parts[4] == "views";
        if !(matches!(parts.first(), Some(&"orgs" | &"users")) && parts.len() >= 4 && owner_slug_ok(parts[1]) && parts[2] == "projects" && (parts.len() == 4 || with_view)) {
            return None;
        }
        let number = number_of(parts[3])?;
        let view = if with_view { Some(number_of(parts[5])?) } else { None };
        let owner = parts[1].to_string();
        return Some(if parts[0] == "orgs" { Pasted::Org { owner, number, host: url_host, view } } else { Pasted::User { owner, number, host: url_host, view } });
    }
    let (owner, number) = trimmed.split_once('/')?;
    owner_slug_ok(owner).then_some(())?;
    Some(Pasted::Bare { owner: owner.to_string(), number: number_of(number).filter(|_| number.bytes().all(|b| b.is_ascii_digit()))? })
}

fn resolve_ref(t: &Target, params: &Value) -> Result<Value, Failure> {
    let input = text(params, "input").unwrap_or("").trim().to_string();
    if input.is_empty() {
        return Ok(fail(validation("Input required.", "input_required")));
    }
    if input.len() > INPUT_MAX_BYTES {
        return Ok(fail(validation("Project reference is too large to resolve.", "input_too_large")));
    }
    let named = optional(params, "host");
    let Some(parsed) = parse_paste(&input, named.as_deref()) else { return Ok(fail(validation("Could not parse input. Expected a GitHub project URL or `owner/number`.", "input_unparseable"))) };
    let (owner, number, preferred, host, view) = match parsed {
        Pasted::Org { owner, number, host, view } => (owner, number, Some("organization"), host, view),
        Pasted::User { owner, number, host, view } => (owner, number, Some("user"), host, view),
        Pasted::Bare { owner, number } => (owner, number, None, named.unwrap_or_else(|| DEFAULT_HOST.to_string()), None),
    };
    let candidates: Vec<&str> = preferred.map_or(vec!["organization", "user"], |p| vec![p]);
    let mut owner_type = None;
    let mut last = json!({"type": "not_found", "message": "Owner not found.", "reason": "owner_not_found"});
    for candidate in candidates {
        match raw(t, project::RESOLVE, with_host(json!({"owner": owner, "owner_type": candidate}), &host))? {
            Ok(_) => {
                owner_type = Some(candidate);
                break;
            }
            Err((code, _)) if code == "NOT_FOUND" => last = json!({"type": "not_found", "message": "Owner not found.", "reason": "owner_not_found"}),
            Err((code, message)) => return Ok(fail(classify(&code, &message, &host))),
        }
    }
    let Some(owner_type) = owner_type else { return Ok(fail(last)) };
    match raw(t, project::RESOLVE, with_host(json!({"owner": owner, "owner_type": owner_type, "project_number": number}), &host))? {
        Ok(a) => {
            let mut out = json!({"ok": true, "owner": owner, "ownerType": owner_type, "number": number, "title": a["title"].as_str().unwrap_or(""), "host": host});
            if let Some(view) = view {
                out["viewNumber"] = json!(view);
            }
            Ok(out)
        }
        Err((code, _)) if code == "NOT_FOUND" => Ok(fail(json!({"type": "not_found", "message": "Project not found.", "reason": "project_not_found"}))),
        Err((code, message)) => Ok(fail(classify(&code, &message, &host))),
    }
}
