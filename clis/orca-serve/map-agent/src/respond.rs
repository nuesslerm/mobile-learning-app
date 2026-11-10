//! `agentSession.respondToApproval` and `respondToQuestion` over `agent.requests`: the prompt is checked as the host checks it (it exists, it is
//! the kind asked for, the phone saw its current revision, it is still open), then answered, and the answer is read back as the item.
use crate::agent::refused_code;
use crate::journal::load;
use crate::ops::{lookup, refused_with, Envelope};
use crate::session::{accepted, finish, text};
use agent_api_agent::requests::{code, APPROVAL_ANSWER, INPUT_ANSWER};
use orca_core::wire::Failure;
use orca_core::AgentApi;
use serde_json::{json, Value};

fn invalid(reason: &str, message: &str) -> Value {
    refused_with("agent_session_operation_invalid", message, json!({"reason": reason}))
}

pub(crate) fn respond(agent: &dyn AgentApi, params: &Value, kind: &str) -> Result<Value, Failure> {
    let env = Envelope::read(params)?;
    if let Some(done) = lookup(agent, &env)? {
        return Ok(done);
    }
    let item_id = text(params, "itemId").ok_or_else(|| Failure::invalid("Invalid item id"))?;
    let expected = params["expectedRevision"].as_u64().filter(|r| *r > 0).ok_or_else(|| Failure::invalid("Invalid expected revision"))?;
    let loaded = load(agent, &env.session)?;
    let item = loaded.folded.items.iter().find(|i| i.id == item_id && i.body["kind"] == kind);
    let Some(item) = item else {
        let what = if loaded.folded.items.iter().any(|i| i.id == item_id) { format!("Item {item_id} is not a pending {kind}.") } else { format!("No item {item_id} in session {}.", env.session) };
        return Ok(invalid("promptGone", &what));
    };
    if item.revision != expected {
        return Ok(refused_with("agent_session_item_revision_stale", &format!("Item {item_id} has moved on."), json!({"reason": "promptMoved", "currentRevision": item.revision, "resolution": item.body["resolution"]})));
    }
    if !item.is_pending_prompt() {
        let state = item.body["resolution"]["state"].as_str().unwrap_or("resolved").to_string();
        return Ok(refused_with("agent_session_already_resolved", &format!("Item {item_id} was already {state}."), json!({"reason": "promptAlreadyResolved", "currentRevision": item.revision, "resolution": item.body["resolution"]})));
    }
    let (call, selected) = match (kind, item_id.split_once(':')) {
        ("approval", Some((_, approval))) => {
            let option = text(params, "optionId").filter(|o| item.body["options"].as_array().is_some_and(|os| os.iter().any(|x| x["id"] == *o)));
            let Some(option) = option else { return Ok(invalid("optionRejected", &format!("Option {} is not offered by item {item_id}.", text(params, "optionId").unwrap_or_default()))) };
            (agent.request(APPROVAL_ANSWER.name(), json!({"thread": env.session, "approval": approval, "decision": option})), option.to_string())
        }
        ("question", Some((_, request))) => {
            let Some((answer, selected)) = question_answer(params, item) else { return Ok(invalid("optionRejected", &format!("The answers do not match the questions on item {item_id}."))) };
            (agent.request(INPUT_ANSWER.name(), json!({"thread": env.session, "request": request, "answer": answer})), selected)
        }
        _ => return Ok(invalid("requestMalformed", &format!("Item id {item_id} is not a well-formed item key."))),
    };
    match call {
        Ok(_) => {}
        Err(e) if matches!(refused_code(&e), Some(c) if c == code::REQUEST_NOT_OPEN) => return Ok(invalid("promptGone", &format!("Item {item_id} is not a pending {kind}."))),
        Err(e) => return Err(crate::agent::refusal(e)),
    }
    let after = load(agent, &env.session)?;
    let answered = after.folded.items.iter().find(|i| i.id == item_id);
    let (revision, mut resolution) = answered.map_or((expected, json!({"state": "resolved"})), |i| (i.revision, i.body["resolution"].clone()));
    resolution["selectedOptionId"] = selected.into();
    let answer = accepted(agent, &env.session, json!({"itemId": item_id, "revision": revision, "resolution": resolution}))?;
    finish(agent, &env, answer)
}

/// The text rca's agent is given, and the option id the item is shown as answered with. Exactly one of `optionId` and `answers`.
fn question_answer(params: &Value, item: &crate::fold::Item) -> Option<(String, String)> {
    let offered = |id: &str| item.body["options"].as_array().is_some_and(|os| os.iter().any(|o| o["id"] == id));
    match (text(params, "optionId"), params["answers"].as_array()) {
        (Some(option), None) if offered(option) => Some((option.to_string(), option.to_string())),
        (None, Some(answers)) if !answers.is_empty() => {
            let first = &answers[0];
            let ids: Vec<&str> = first["optionIds"].as_array()?.iter().filter_map(Value::as_str).collect();
            if !ids.iter().all(|i| offered(i)) {
                return None;
            }
            let other = first["other"].as_str().filter(|o| !o.is_empty());
            let answer = match other {
                Some(o) => o.to_string(),
                None if ids.is_empty() => return None,
                None => ids.join(", "),
            };
            Some((answer, ids.first().map_or_else(|| "other".to_string(), |i| i.to_string())))
        }
        _ => None,
    }
}
