//! The issue rows of `github.*` and `gitlab.*` over `issue.*`: create, update (state, title and description) and comment. rca sets no assignees and
//! changes no labels of an issue, so a request that asks for one is the host's `{ok: false}` with those words, answered before anything is changed.
use crate::github::{int_positive, required_string};
use crate::{time, with_repo, Target};
use agent_api_hosted::{issue, pr};
use orca_core::wire::{received, Failure};
use orca_core::AgentError;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, PartialEq)]
pub enum Host {
    Github,
    Gitlab,
}

impl Host {
    fn provider(self) -> &'static str {
        match self {
            Host::Github => "github",
            Host::Gitlab => "gitlab",
        }
    }

    fn failed(self, message: &str) -> Value {
        match self {
            Host::Github => crate::failure::gh(message),
            Host::Gitlab => crate::failure::glab(message),
        }
    }

    /// The key of the request that names another repository than the workspace's.
    fn named(self) -> &'static str {
        match self {
            Host::Github => "ownerRepo",
            Host::Gitlab => "projectRef",
        }
    }
}

fn unsupported(what: &str) -> Value {
    json!({"ok": false, "error": format!("rca does not {what}")})
}

fn names(v: Option<&Value>, key: &str) -> Result<Vec<String>, Failure> {
    match v {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items.iter().map(|i| i.as_str().map(str::to_string).ok_or_else(|| Failure::invalid(format!("Invalid input: expected string, received {}", received(i))))).collect(),
        Some(other) => Err(Failure::invalid(format!("Invalid input: expected array, received {} at \"{key}\"", received(other)))),
    }
}

fn args(host: Host, params: &Value, mut args: Value) -> Result<Value, Failure> {
    args["provider"] = json!(host.provider());
    with_repo(args, params, host.named())
}

/// What a refusal of rca's is as the host answers it: its `{ok: false}`, except for a request rca itself could not make.
fn refused(host: Host, e: AgentError) -> Result<Value, Failure> {
    match e {
        AgentError::Refused { code, message } if code != "UNKNOWN_WORKSPACE" && code != "INVALID_PARAMS" => Ok(host.failed(&message)),
        other => Err(crate::refusal(other)),
    }
}

/// `github.createIssue` and `gitlab.createIssue`: the new issue's number and address.
pub fn create(host: Host, t: &Target, params: &Value) -> Result<Value, Failure> {
    required_string(params, "repo", "Missing repo selector")?;
    let title = required_string(params, "title", "Missing title")?;
    let body = params.get("body").and_then(Value::as_str).ok_or_else(|| Failure::invalid("Invalid input: expected string, received undefined"))?;
    let labels = names(params.get("labels"), "labels")?;
    let assignees = names(params.get("assignees"), "assignees")?;
    if !assignees.is_empty() {
        return Ok(unsupported("set assignees when it opens an issue"));
    }
    let mut body_args = json!({"title": title, "body": body});
    if !labels.is_empty() {
        body_args["labels"] = json!(labels);
    }
    match t.try_ask(issue::CREATE, args(host, params, body_args)?) {
        Ok(v) => match (v["number"].as_u64(), v["url"].as_str()) {
            (Some(number), url) => Ok(json!({"ok": true, "number": number, "url": url.unwrap_or("")})),
            _ => Ok(crate::failure::code_host(if host == Host::Github { "github.unexpectedResponse" } else { "gitlab.unexpectedResponse" }, None)),
        },
        Err(e) => refused(host, e),
    }
}

/// `github.updateIssue` and `gitlab.updateIssue`: the state, title and description, each one request, stopping at the first the executor refuses.
pub fn update(host: Host, t: &Target, params: &Value) -> Result<Value, Failure> {
    required_string(params, "repo", "Missing repo selector")?;
    let number = int_positive(params.get("number"))?;
    let updates = match params.get("updates") {
        Some(u @ Value::Object(_)) => u,
        Some(other) => return Err(Failure::invalid(format!("Invalid input: expected object, received {}", received(other)))),
        None => return Err(Failure::invalid("Invalid input: expected object, received undefined")),
    };
    let state = match updates.get("state") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s == "open" || s == "closed" => Some(s.as_str()),
        Some(_) => return Err(Failure::invalid("Invalid option: expected one of \"open\"|\"closed\"")),
    };
    let text_of = |key: &str| updates.get(key).and_then(Value::as_str).filter(|s| !s.is_empty());
    if ["addLabels", "removeLabels"].iter().any(|k| !names(updates.get(*k), k).unwrap_or_default().is_empty()) {
        return Ok(unsupported("change the labels of an issue"));
    }
    if ["addAssignees", "removeAssignees"].iter().any(|k| !names(updates.get(*k), k).unwrap_or_default().is_empty()) {
        return Ok(unsupported("change the assignees of an issue"));
    }
    let mut asked = false;
    if let Some(state) = state {
        asked = true;
        if let Err(e) = t.try_ask(issue::SET_STATE, args(host, params, json!({"number": number, "state": state}))?) {
            return refused(host, e);
        }
    }
    if text_of("title").is_some() || text_of("body").is_some() {
        asked = true;
        let mut edit = json!({"number": number});
        for key in ["title", "body"] {
            if let Some(v) = text_of(key) {
                edit[key] = json!(v);
            }
        }
        if let Err(e) = t.try_ask(issue::EDIT, args(host, params, edit)?) {
            return refused(host, e);
        }
    }
    if !asked {
        // The host resolves the repository before it sees that nothing changes, so an unresolved one is still said
        if let Err(e) = t.try_ask(issue::LIST, args(host, params, json!({"limit": 1}))?) {
            return refused(host, e);
        }
    }
    Ok(json!({"ok": true}))
}

/// `github.addIssueComment` (an issue or, with `type: "pr"`, a pull request) and `gitlab.addIssueComment`: the new comment as the phone renders it.
pub fn comment(host: Host, t: &Target, params: &Value) -> Result<Value, Failure> {
    required_string(params, "repo", "Missing repo selector")?;
    let number = int_positive(params.get("number"))?;
    let body = required_string(params, "body", "Comment body required")?;
    let on_pr = host == Host::Github && params.get("type").and_then(Value::as_str) == Some("pr");
    let (method, mut call) = if on_pr { (pr::COMMENT, json!({"number": number, "body": body})) } else { (issue::COMMENT, json!({"number": number, "body": body})) };
    call["provider"] = json!(host.provider());
    let call = with_repo(call, params, if on_pr { "prRepo" } else { host.named() })?;
    let c = match t.try_ask(method, call) {
        Ok(c) => c,
        Err(e) => return refused(host, e),
    };
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let url = c["url"].as_str().unwrap_or("");
    let id = c["database_id"].as_u64().or_else(|| url.rsplit_once('-').and_then(|(_, n)| n.parse().ok())).or_else(|| c["id"].as_str().and_then(|n| n.parse().ok())).unwrap_or(now);
    Ok(json!({"ok": true, "comment": {
        "id": id,
        "author": c["author"].as_str().unwrap_or("You"),
        "authorAvatarUrl": c["avatar_url"].as_str().unwrap_or(""),
        "body": c["body"].as_str().unwrap_or(body),
        "createdAt": c["created_at"].as_str().map_or_else(|| time::iso(now), str::to_string),
        "url": url,
        "isBot": c["is_bot"] == true,
    }}))
}
