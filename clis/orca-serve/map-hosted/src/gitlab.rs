//! The `gitlab.*` merge request rows over `mr.*`.
use crate::github::{int_positive, required_string};
use crate::{number, outcome, text, time, with_repo, Target};
use agent_api_hosted::pr;
use orca_core::wire::{received, Failure};
use std::time::{SystemTime, UNIX_EPOCH};
use serde_json::{json, Value};

fn iid(params: &Value) -> Result<u64, Failure> {
    number(params, "iid", "MR")
}

/// The merge method of `gitlab.mergeMR` when the phone names none: the executor's own default, sent by name so a double can refuse a missing one.
const DEFAULT_METHOD: &str = "merge";

/// `gitlab.mergeMR`: the host's `{ok}` of the merge, in the method the phone names, else `merge`.
pub fn merge(t: &Target, params: &Value) -> Result<Value, Failure> {
    let args = json!({"number": iid(params)?, "method": text(params, "method").unwrap_or(DEFAULT_METHOD)});
    outcome(t.try_ask(pr::MR_MERGE, with_repo(args, params, "projectRef")?))
}

/// `gitlab.updateMR`: title and description, labels added and removed and the ready mark, each one request, stopping at the first the executor refuses.
pub fn update(t: &Target, params: &Value) -> Result<Value, Failure> {
    let updates = params.get("updates").filter(|u| u.is_object()).ok_or_else(|| Failure::invalid("Missing updates"))?;
    let number = iid(params)?;
    let text_of = |key: &str| updates.get(key).and_then(Value::as_str);
    let names = |key: &str| updates.get(key).and_then(Value::as_array).map(|l| l.iter().filter_map(Value::as_str).collect::<Vec<_>>()).filter(|l| !l.is_empty());
    let (add, remove) = (names("addLabels"), names("removeLabels"));
    if text_of("title").is_none() && text_of("body").is_none() && add.is_none() && remove.is_none() && updates.get("readyForReview") != Some(&json!(true)) {
        return Err(Failure::invalid(crate::NO_UPDATES));
    }
    let mut changes = Vec::new();
    if text_of("title").is_some() || text_of("body").is_some() {
        let mut args = json!({"number": number});
        for key in ["title", "body"] {
            if let Some(v) = text_of(key) {
                args[key] = json!(v);
            }
        }
        changes.push((pr::MR_EDIT, args));
    }
    if add.is_some() || remove.is_some() {
        changes.push((pr::MR_SET_LABELS, json!({"number": number, "add": add.unwrap_or_default(), "remove": remove.unwrap_or_default()})));
    }
    if updates.get("readyForReview") == Some(&json!(true)) {
        changes.push((pr::MR_READY, json!({"number": number})));
    }
    for (method, args) in changes {
        let done = outcome(t.try_ask(method, with_repo(args, params, "projectRef")?))?;
        if done["ok"] != true {
            return Ok(done);
        }
    }
    Ok(json!({"ok": true}))
}

/// `gitlab.updateMRState`: close or reopen (`state` is `closed` or `opened`); `{ok: true}`, or the host's failure.
pub fn set_state(t: &Target, params: &Value) -> Result<Value, Failure> {
    let state = match params["state"].as_str() {
        Some("closed") => "closed",
        Some("opened") => "open",
        _ => return Err(Failure::invalid("Invalid option: expected one of \"opened\"|\"closed\"")),
    };
    match t.try_ask(pr::MR_SET_STATE, with_repo(json!({"number": iid(params)?, "state": state}), params, "projectRef")?) {
        Ok(_) => Ok(json!({"ok": true})),
        Err(orca_core::AgentError::Refused { code, message }) if code != "UNKNOWN_WORKSPACE" && code != "INVALID_PARAMS" => Ok(crate::failure::glab(&message)),
        Err(other) => Err(crate::refusal(other)),
    }
}

/// The schema of `gitlab.addMRComment` in its key order, answering zod's first issue: all of it is checked before the repo selector is resolved.
pub(crate) fn check_comment(params: &Value) -> Result<(), Failure> {
    required_string(params, "repo", "Missing repo selector")?;
    int_positive(params.get("iid"))?;
    required_string(params, "body", "Comment body is required")?;
    match params.get("projectRef") {
        None | Some(Value::Null) => Ok(()),
        Some(named @ Value::Object(_)) => {
            required_string(named, "host", "Missing GitLab host")?;
            required_string(named, "path", "Missing GitLab project path").map(drop)
        }
        Some(other) => Err(Failure::invalid(format!("Invalid input: expected object, received {}", received(other)))),
    }
}

/// `gitlab.addMRComment`: the new note as the host renders it from GitLab's answer, with the host's fallbacks for what GitLab left out, or the host's failure.
pub fn comment(t: &Target, params: &Value) -> Result<Value, Failure> {
    let body = required_string(params, "body", "Comment body is required")?;
    let args = with_repo(json!({"number": iid(params)?, "body": body}), params, "projectRef")?;
    let c = match t.try_ask(pr::MR_COMMENT, args) {
        Ok(c) => c,
        Err(orca_core::AgentError::Refused { code, message }) if code != "UNKNOWN_WORKSPACE" && code != "INVALID_PARAMS" => return Ok(crate::failure::glab(&message)),
        Err(other) => return Err(crate::refusal(other)),
    };
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    Ok(json!({"ok": true, "comment": {
        "id": c["database_id"].as_u64().unwrap_or(now),
        "author": c["author"].as_str().unwrap_or("You"),
        "authorAvatarUrl": c["avatar_url"].as_str().unwrap_or(""),
        "body": c["body"].as_str().unwrap_or(body),
        "createdAt": c["created_at"].as_str().map_or_else(|| time::iso(now), str::to_string),
        "url": "",
        "isBot": c["is_bot"] == true,
    }}))
}
