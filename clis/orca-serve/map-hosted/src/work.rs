//! The work-item rows of `github.*` and `gitlab.*` over `issue.search`, `issue.count` and `issue.todos`: the list and the count of issues and pull or
//! merge requests, and GitLab's to-dos. A row's fields beyond what the search returns (reviewers, checks, merge state) are not filled.
use crate::issue::Host;
use crate::time;
use crate::{text, Target};
use agent_api_hosted::issue;
use orca_core::wire::Failure;
use orca_core::AgentError;
use serde_json::{json, Value};

const LIST_LIMIT: u64 = 24;
const PER_PAGE: u64 = 20;

#[derive(Default, Debug, PartialEq)]
pub struct Filters {
    kind: Option<&'static str>,
    state: Option<&'static str>,
    author: Option<String>,
    label: Option<String>,
    text: String,
}

fn words(query: &str) -> Vec<String> {
    let (mut out, mut word, mut quoted) = (Vec::new(), String::new(), false);
    for c in query.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                word.push(c);
            }
            c if c.is_whitespace() && !quoted => {
                if !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                }
            }
            c => word.push(c),
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

fn account(name: &str) -> bool {
    !name.is_empty() && name.len() <= 80 && !name.starts_with(['-', '/', '.']) && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

/// The task query of the phone as the filters rca's search takes (`is:issue`, `is:pr`, `is:open`, `is:closed`, `is:merged`, `author:`, `label:`); any other word stays the
/// search text, which GitHub reads as its own qualifiers.
pub fn parse_query(query: &str) -> Filters {
    let mut f = Filters::default();
    let mut rest = Vec::new();
    for word in words(query) {
        match word.to_ascii_lowercase().as_str() {
            "is:issue" => f.kind = Some("issue"),
            "is:pr" => f.kind = Some("pr"),
            "is:open" => f.state = Some("open"),
            "is:closed" => f.state = Some("closed"),
            "is:merged" => f.state = Some("merged"),
            _ => match word.split_once(':') {
                Some((key, value)) if key.eq_ignore_ascii_case("author") && f.author.is_none() && account(value) => f.author = Some(value.to_string()),
                Some((key, value)) if key.eq_ignore_ascii_case("label") && f.label.is_none() && !value.contains(',') && !value.starts_with('-') && value.trim_matches('"').chars().count().clamp(1, 50) == value.trim_matches('"').chars().count() && !value.trim_matches('"').contains('"') => f.label = Some(value.trim_matches('"').to_string()),
                _ => rest.push(word),
            },
        }
    }
    if f.state == Some("merged") {
        f.kind = Some("pr");
    }
    f.text = rest.join(" ");
    f
}

impl Filters {
    fn args(&self, host: Host) -> Value {
        let mut args = json!({"provider": host.provider(), "state": self.state.unwrap_or("open")});
        if let Some(kind) = self.kind {
            args["kind"] = json!(kind);
        }
        if let Some(a) = &self.author {
            args["author"] = json!(a);
        }
        if let Some(l) = &self.label {
            args["label"] = json!(l);
        }
        if !self.text.trim().is_empty() && self.text.chars().count() <= 256 {
            args["search"] = json!(self.text);
        }
        args
    }
}

fn query_of(params: &Value) -> &str {
    text(params, "query").unwrap_or("").trim()
}

fn failed_list(message: &str) -> Value {
    json!({"type": "unknown", "message": message})
}

fn github_time(ms: Option<u64>) -> String {
    ms.map_or_else(String::new, |t| time::iso(t).replace(".000Z", "Z"))
}

/// `github.countWorkItems`: the number the provider counts for the query, 0 when it cannot be read, as the host answers.
pub fn count(t: &Target, params: &Value) -> Result<Value, Failure> {
    let args = parse_query(query_of(params)).args(Host::Github);
    match t.try_ask(issue::COUNT, args) {
        Ok(v) => Ok(json!(v["count"].as_u64().unwrap_or(0))),
        Err(AgentError::Refused { code, .. }) if code != "UNKNOWN_WORKSPACE" && code != "INVALID_PARAMS" => Ok(json!(0)),
        Err(other) => Err(crate::refusal(other)),
    }
}

fn github_item(i: &Value) -> Value {
    let pr = i["kind"] == "pr";
    let state = match (i["state"].as_str(), i["draft"] == true) {
        (Some("merged"), _) => "merged",
        (Some("closed"), _) => "closed",
        (_, true) if pr => "draft",
        _ => "open",
    };
    let number = i["number"].as_u64().unwrap_or(0);
    let mut item = json!({
        "id": format!("{}:{number}", if pr { "pr" } else { "issue" }),
        "type": if pr { "pr" } else { "issue" },
        "number": number,
        "title": i["title"].as_str().unwrap_or(""),
        "state": state,
        "url": i["url"].as_str().unwrap_or(""),
        "labels": i["labels"].clone(),
        "updatedAt": github_time(i["updated_at_ms"].as_u64()),
    });
    item["author"] = i["author"].clone();
    if let Some(avatar) = i["author_avatar_url"].as_str() {
        item["authorAvatarUrl"] = json!(avatar);
    }
    if pr {
        item["branchName"] = json!(i["head_ref"].as_str().unwrap_or(""));
        item["baseRefName"] = json!(i["base_ref"].as_str().unwrap_or(""));
    } else if let Some(comments) = i["comments"].as_u64() {
        item["commentCount"] = json!(comments);
    }
    item
}

/// `github.listWorkItems`: the open issues and pull requests, or those the query asks for, as the list the phone shows. The sources are not read: a search names
/// the repository the workspace's remote does.
pub fn list_github(t: &Target, params: &Value) -> Result<Value, Failure> {
    let limit = params.get("limit").and_then(Value::as_f64).filter(|l| *l >= 1.0).map_or(LIST_LIMIT, |l| (l as u64).min(200));
    let page = params.get("page").and_then(Value::as_u64).filter(|p| (1..=100).contains(p)).unwrap_or(1);
    let mut args = parse_query(query_of(params)).args(Host::Github);
    args["limit"] = json!(limit);
    args["page"] = json!(page);
    let nothing = |errors: Option<Value>| {
        let mut out = json!({"items": [], "sources": {"issues": null, "prs": null, "originCandidate": null, "upstreamCandidate": null}});
        if let Some(e) = errors {
            out["errors"] = e;
        }
        out
    };
    match t.try_ask(issue::SEARCH, args) {
        Ok(v) => {
            let items: Vec<Value> = v["items"].as_array().map_or_else(Vec::new, |a| a.iter().map(github_item).collect());
            let mut out = nothing(None);
            out["items"] = json!(items);
            Ok(out)
        }
        Err(AgentError::Refused { code, message }) if code != "UNKNOWN_WORKSPACE" && code != "INVALID_PARAMS" => Ok(nothing(Some(json!({"issues": failed_list(&message), "prs": failed_list(&message)})))),
        Err(other) => Err(crate::refusal(other)),
    }
}

/// The host and project path a GitLab address names: `https://<host>/<group>/<project>/-/issues/3`.
fn project_of(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let (host, tail) = rest.split_once('/')?;
    let (path, _) = tail.split_once("/-/")?;
    Some((host.to_string(), path.to_string()))
}

fn gitlab_item(i: &Value) -> Value {
    let mr = i["kind"] == "pr";
    let number = i["number"].as_u64().unwrap_or(0);
    let url = i["url"].as_str().unwrap_or("");
    let (host, path) = project_of(url).unwrap_or_default();
    let state = match (i["state"].as_str(), i["draft"] == true) {
        (Some("merged"), _) => "merged",
        (Some("closed"), _) => "closed",
        (_, true) if mr => "draft",
        _ => "opened",
    };
    let mut item = json!({
        "id": format!("gitlab-{}-{path}-{number}", if mr { "mr" } else { "issue" }),
        "type": if mr { "mr" } else { "issue" },
        "number": number,
        "title": i["title"].as_str().unwrap_or(""),
        "state": state,
        "url": url,
        "labels": i["labels"].clone(),
        "updatedAt": i["updated_at_ms"].as_u64().map_or_else(String::new, time::iso),
        "author": i["author"].clone(),
        "repoId": path,
    });
    if mr {
        item["branchName"] = i["head_ref"].clone();
        item["baseRefName"] = i["base_ref"].clone();
        item["isCrossRepository"] = json!(false);
    }
    if !path.is_empty() {
        item["projectRef"] = json!({"host": host, "path": path});
    }
    item
}

/// `gitlab.listWorkItems`: merge requests and issues of the project by update time, a page of `perPage` of each kind as the host reads them. The totals are the rows
/// seen, with one more page when the executor says there are more (GitLab cannot count the union either).
pub fn list_gitlab(t: &Target, params: &Value) -> Result<Value, Failure> {
    let per_page = params.get("perPage").and_then(Value::as_f64).filter(|n| *n >= 1.0).map_or(PER_PAGE, |n| (n as u64).min(200));
    let page = params.get("page").and_then(Value::as_f64).filter(|n| *n >= 1.0).map_or(1, |n| n as u64).min(100);
    let state = match params.get("state").and_then(Value::as_str) {
        Some("closed") => "closed",
        Some("merged") => "merged",
        Some("all") => "all",
        _ => "open",
    };
    let mut args = json!({"provider": "gitlab", "state": state, "limit": per_page, "page": page});
    if state == "merged" {
        args["kind"] = json!("pr");
    }
    let q = query_of(params);
    if !q.is_empty() && q.chars().count() <= 256 {
        args["search"] = json!(q);
    }
    let empty = |error: Option<Value>| {
        let mut out = json!({"items": [], "page": page, "perPage": per_page, "totalCount": 0, "totalPages": 0});
        if let Some(e) = error {
            out["error"] = e;
        }
        out
    };
    match t.try_ask(issue::SEARCH, args) {
        Ok(v) => {
            let items: Vec<Value> = v["items"].as_array().map_or_else(Vec::new, |a| a.iter().map(gitlab_item).collect());
            let more = v["truncated"] == true;
            let seen = (page - 1) * per_page + items.len() as u64;
            Ok(json!({"items": items, "page": page, "perPage": per_page, "totalCount": seen + u64::from(more), "totalPages": if seen == 0 { 0 } else { page + u64::from(more) }}))
        }
        Err(AgentError::Refused { code, message }) if code == "HOSTED_FAILED" && message.contains("none of the git remotes") => Ok(empty(Some(json!({"type": "not_found", "message": "No GitLab project found for this repository."})))),
        Err(AgentError::Refused { code, message }) if code != "UNKNOWN_WORKSPACE" && code != "INVALID_PARAMS" => Ok(empty(Some(failed_list(&message)))),
        Err(other) => Err(crate::refusal(other)),
    }
}

/// `gitlab.todos`: the signed-in account's pending to-dos, the empty list when they cannot be read, as the host answers.
pub fn todos(t: &Target, _params: &Value) -> Result<Value, Failure> {
    match t.try_ask(issue::TODOS, json!({"provider": "gitlab", "limit": 50})) {
        Ok(v) => {
            let rows = v["todos"].as_array().map_or_else(Vec::new, |a| {
                a.iter()
                    .map(|d| {
                        json!({
                            "id": d["id"].as_str().and_then(|n| n.parse::<u64>().ok()).unwrap_or(0),
                            "actionName": d["action"].as_str().unwrap_or(""),
                            "targetType": d["target_type"].as_str().unwrap_or(""),
                            "targetIid": d["number"].clone(),
                            "targetTitle": d["title"].as_str().unwrap_or(""),
                            "targetUrl": d["url"].as_str().unwrap_or(""),
                            "projectPath": d["repo"].as_str().unwrap_or(""),
                            "authorUsername": d["author"].as_str().unwrap_or(""),
                            "authorAvatarUrl": d["author_avatar_url"].as_str().unwrap_or(""),
                            "updatedAt": d["updated_at_ms"].as_u64().map_or_else(String::new, time::iso),
                            "state": if d["state"] == "done" { "done" } else { "pending" },
                        })
                    })
                    .collect()
            });
            Ok(json!(rows))
        }
        Err(AgentError::Refused { code, .. }) if code != "UNKNOWN_WORKSPACE" && code != "INVALID_PARAMS" => Ok(json!([])),
        Err(other) => Err(crate::refusal(other)),
    }
}
