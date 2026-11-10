//! The `gitlab.*` merge request rows over `mr.*`.
use crate::{number, outcome, text, with_repo, Target};
use agent_api_hosted::pr;
use orca_core::wire::Failure;
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
