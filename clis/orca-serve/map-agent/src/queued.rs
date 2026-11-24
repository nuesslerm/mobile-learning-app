//! The queued-message rows over `agent.queue`: a queued card is a queued rca prompt (its message id is the queue item id), the card list rides
//! beside the history pages and batches, and the body fingerprint the phone compares with is the hash the host computes of the card's body.
use crate::agent::{ask, refused_code};
use crate::journal::events_after;
use crate::ops::{lookup, refused, Envelope};
use crate::session::{accepted, finish, submission, text};
use agent_api_agent::queue::{code, QUEUE_DELETE, QUEUE_EDIT, QUEUE_EDIT_HOLD, QUEUE_LIST, QUEUE_MOVE, QUEUE_RESUME, TURN_STEER};
use orca_core::wire::Failure;
use orca_core::AgentApi;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub(crate) const MESSAGES: &str = "agent-session.queued-messages.v2";
pub(crate) const REORDER: &str = "agent-session.queued-message-reorder.v1";
pub(crate) const EDIT: &str = "agent-session.queued-message-edit.v1";

fn canonical(v: &Value) -> String {
    match v {
        Value::Array(a) => format!("[{}]", a.iter().map(canonical).collect::<Vec<_>>().join(",")),
        Value::Object(o) => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort();
            format!("{{{}}}", keys.iter().map(|k| format!("{}:{}", Value::String((*k).clone()), canonical(&o[*k]))).collect::<Vec<_>>().join(","))
        }
        other => other.to_string(),
    }
}

/// The body of a queued card: the prompt as the user's message.
pub(crate) fn body_of(prompt: &str) -> Value {
    json!({"kind": "message", "role": "user", "blocks": [{"type": "text", "text": prompt}]})
}

/// `agentSessionSendBodyFingerprint`: the hash of the canonical `agentSession.send` payload of the body.
pub(crate) fn fingerprint(session: &str, body: &Value) -> String {
    let payload = json!({"method": "agentSession.send", "sessionId": session, "fields": {"body": body}});
    Sha256::digest(canonical(&payload).as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn handoff(session: &str, item: &str, prompt: &str) -> Value {
    let id = format!(
        "queue-send-{:x}",
        Sha256::digest(json!([session, item]).to_string().as_bytes())
    );
    let mut value = submission(&id, &fingerprint(session, &body_of(prompt)));
    value["submittedAt"] = 0.into();
    value["queuedMessageId"] = item.into();
    value
}

struct Queue {
    held: bool,
    editing: Vec<String>,
    items: Vec<(String, String)>,
}

fn read(agent: &dyn AgentApi, session: &str) -> Result<Queue, Failure> {
    let list = ask(agent, QUEUE_LIST.name(), json!({"thread": session}))?;
    let items = list["items"].as_array().map(|a| a.iter().map(|i| (i["item"].as_str().unwrap_or_default().to_string(), i["prompt"].as_str().unwrap_or_default().to_string())).collect()).unwrap_or_default();
    let editing = list["editing"].as_array().into_iter().flatten().filter_map(|id| id.as_str().map(str::to_string)).collect();
    Ok(Queue { held: list["held"] == true, editing, items })
}

/// Adds `queuedMessages`, `queuePause` and `nextQueuedMessageId` to a history page or a stream frame, where the phone reads them.
pub(crate) fn publish(agent: &dyn AgentApi, session: &str, frame: &mut Value) -> Result<(), Failure> {
    let queue = read(agent, session)?;
    let cards: Vec<Value> = queue.items.iter().enumerate().map(|(n, (id, prompt))| json!({"messageId": id, "position": n + 1, "body": body_of(prompt), "state": "waiting"})).collect();
    let next = queue.items.first().filter(|(id, _)| !queue.held && !queue.editing.contains(id)).map(|(id, _)| id.clone());
    if let Some(h) = frame.as_object_mut() {
        h.insert("queuedMessages".into(), Value::Array(cards));
        h.insert("queuePause".into(), if queue.held { json!({"reason": "stopped"}) } else { Value::Null });
        h.insert("nextQueuedMessageId".into(), next.map_or(Value::Null, Value::String));
    }
    Ok(())
}

fn id(params: &Value) -> Result<&str, Failure> {
    text(params, "messageId").ok_or_else(|| Failure::invalid("Invalid queued message id"))
}

/// What became of an item that is no longer queued: it started as a turn, or it was deleted, or the host never had it.
fn gone(agent: &dyn AgentApi, session: &str, item: &str) -> Result<&'static str, Failure> {
    let (events, _) = events_after(agent, session, 0)?;
    for e in events.iter().rev().filter(|e| e["item"].as_str() == Some(item)) {
        match e["type"].as_str() {
            Some("queue.started" | "turn.steered") => return Ok("dispatched"),
            Some("queue.deleted") => return Ok("withdrawn"),
            _ => {}
        }
    }
    Ok("missing")
}

fn invalid(message: &str) -> Value {
    refused("agent_session_operation_invalid", message)
}

pub(crate) fn delete(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let env = Envelope::read(params)?;
    if let Some(done) = lookup(agent, &env)? {
        return Ok(done);
    }
    let message = id(params)?;
    let value = match agent.request(QUEUE_DELETE.name(), json!({"thread": env.session, "item": message})) {
        Ok(_) => json!({"deleted": true, "messageId": message}),
        Err(e) if refused_code(&e) == Some(code::ITEM_NOT_QUEUED) => json!({"deleted": false, "messageId": message, "disposition": gone(agent, &env.session, message)?}),
        Err(e) => return Err(crate::agent::refusal(e)),
    };
    let answer = accepted(agent, &env.session, value)?;
    finish(agent, &env, answer)
}

pub(crate) fn resume(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let env = Envelope::read(params)?;
    if let Some(done) = lookup(agent, &env)? {
        return Ok(done);
    }
    let was_held = read(agent, &env.session)?.held;
    ask(agent, QUEUE_RESUME.name(), json!({"thread": env.session}))?;
    let answer = accepted(agent, &env.session, json!({"resumed": was_held}))?;
    finish(agent, &env, answer)
}

/// Send now: into the running turn when one runs (the queued prompt leaves the queue), as the next turn when the thread is stopped.
pub(crate) fn send(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let env = Envelope::read(params)?;
    if let Some(done) = lookup(agent, &env)? {
        return Ok(done);
    }
    let message = id(params)?;
    let prompt = read(agent, &env.session)?.items.into_iter().find(|(item, _)| item == message).map(|(_, prompt)| prompt);
    let running = crate::journal::view(agent, &env.session)?["status"] == "running";
    let sent = if running { agent.request(TURN_STEER.name(), json!({"thread": env.session, "item": message})) } else { agent.request(QUEUE_RESUME.name(), json!({"thread": env.session, "item": message})) };
    match sent {
        Ok(_) => {}
        Err(e) if refused_code(&e) == Some(code::ITEM_EDITING) => return Ok(invalid("This queued message is being edited.")),
        Err(e) if refused_code(&e) == Some(code::ITEM_NOT_QUEUED) => {
            return Ok(match gone(agent, &env.session, message)? {
                "dispatched" => invalid("This queued message was already sent."),
                "withdrawn" => invalid("This queued message was withdrawn."),
                _ => invalid("No queued message by that id."),
            })
        }
        Err(e) => return Err(crate::agent::refusal(e)),
    }
    let submission = handoff(&env.session, message, prompt.as_deref().unwrap_or_default());
    let answer = accepted(agent, &env.session, json!({"clientMessageId": submission["clientMessageId"], "submission": submission}))?;
    finish(agent, &env, answer)
}

pub(crate) fn update(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let env = Envelope::read(params)?;
    let message = id(params)?;
    let expected = text(params, "expectedBodyFingerprint").ok_or_else(|| Failure::invalid("Invalid body fingerprint"))?;
    let new_text = params["text"].as_str().ok_or_else(|| Failure::invalid("Invalid text"))?;
    let queue = read(agent, &env.session)?;
    let Some((_, prompt)) = queue.items.iter().find(|(i, _)| i == message) else {
        let disposition = gone(agent, &env.session, message)?;
        return plain(agent, &env, json!({"status": "gone", "messageId": message, "disposition": disposition}));
    };
    let images = params["images"].as_array().is_some_and(|a| !a.is_empty());
    let skills = params["skills"].as_array().is_some_and(|a| !a.is_empty());
    if new_text.is_empty() || images || skills {
        return plain(agent, &env, json!({"status": "not-editable", "messageId": message}));
    }
    let current = fingerprint(&env.session, &body_of(prompt));
    let wanted = fingerprint(&env.session, &body_of(new_text));
    let result = if current == wanted {
        json!({"status": "unchanged", "messageId": message, "fingerprint": wanted})
    } else if current != expected {
        json!({"status": "changed", "messageId": message})
    } else {
        match agent.request(QUEUE_EDIT.name(), json!({"thread": env.session, "item": message, "prompt": new_text})) {
            Ok(_) => json!({"status": "updated", "messageId": message, "fingerprint": wanted}),
            Err(e) if refused_code(&e) == Some(code::ITEM_NOT_QUEUED) => json!({"status": "gone", "messageId": message, "disposition": gone(agent, &env.session, message)?}),
            Err(e) => return Err(crate::agent::refusal(e)),
        }
    };
    plain(agent, &env, result)
}

/// An answer that is the result itself and is read again when the phone retries (a save compares fingerprints, so a repeat is safe).
fn plain(agent: &dyn AgentApi, env: &Envelope, value: Value) -> Result<Value, Failure> {
    accepted(agent, &env.session, value)
}

pub(crate) fn move_to(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let env = Envelope::read_union(params)?;
    if let Some(done) = lookup(agent, &env)? {
        return Ok(done);
    }
    let message = id(params)?;
    let status = |s: &str| json!({"messageId": message, "status": s});
    let order: Vec<String> = read(agent, &env.session)?.items.into_iter().map(|(i, _)| i).collect();
    let value = (|| -> Result<Option<Value>, Failure> {
        let Some(at) = order.iter().position(|i| i == message) else { return Ok(Some(status("gone"))) };
        let rest: Vec<&String> = order.iter().filter(|i| *i != message).collect();
        // `before` is the card the moved one goes ahead of; none is the end.
        let before: Option<&String> = if let Some(a) = params.get("afterMessageId").and_then(Value::as_str) {
            let Some(p) = rest.iter().position(|i| *i == a) else { return Ok(Some(status("stale"))) };
            rest.get(p + 1).copied()
        } else {
            match params["beforeMessageId"].as_str() {
                Some(b) => {
                    if !rest.iter().any(|i| *i == b) {
                        return Ok(Some(status("stale")));
                    }
                    rest.iter().find(|i| *i == &b).copied()
                }
                None => None,
            }
        };
        let target = before.map_or(rest.len(), |b| rest.iter().position(|i| *i == b).unwrap_or(rest.len()));
        if target == at {
            return Ok(Some(status("unchanged")));
        }
        let mut p = json!({"thread": env.session, "item": message});
        if let Some(b) = before {
            p["before"] = b.as_str().into();
        }
        match agent.request(QUEUE_MOVE.name(), p) {
            Ok(_) => Ok(Some(status("moved"))),
            Err(e) if refused_code(&e) == Some(code::ITEM_NOT_QUEUED) => Ok(Some(status("stale"))),
            Err(e) => Err(crate::agent::refusal(e)),
        }
    })()?
    .unwrap_or_else(|| status("unchanged"));
    let answer = accepted(agent, &env.session, value)?;
    finish(agent, &env, answer)
}

pub(crate) fn edit_hold(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    if !params.is_object() && !params.is_null() {
        return Err(Failure::invalid(format!("Invalid input: expected object, received {}", orca_core::wire::received(params))));
    }
    let action = params["action"].as_str().filter(|a| matches!(*a, "acquire" | "renew" | "release")).ok_or_else(|| Failure::invalid("Invalid discriminator value. Expected 'acquire' | 'renew' | 'release'"))?;
    let session = crate::session::agent_session_id(params)?;
    let message = id(params)?;
    let edit = text(params, "editId").ok_or_else(|| Failure::invalid("Invalid edit id"))?;
    let mut request = json!({"thread": session, "item": message, "edit": edit, "action": action});
    if action == "acquire" {
        let queue = read(agent, session)?;
        let Some((_, prompt)) = queue.items.iter().find(|(id, _)| id == message) else {
            return Ok(json!({"status": "gone", "disposition": gone(agent, session, message)?}));
        };
        let fp = fingerprint(session, &body_of(prompt));
        if params["expectedBodyFingerprint"] != fp {
            return Ok(json!({"status": "changed"}));
        }
        request["expected_prompt"] = prompt.as_str().into();
    }
    let value = ask(agent, QUEUE_EDIT_HOLD.name(), request)?;
    match value["status"].as_str() {
        Some("held") => {
            let prompt = value["prompt"].as_str().ok_or_else(|| Failure::invalid("Queued message lease did not return text"))?;
            Ok(json!({
                "status": "held", "fingerprint": fingerprint(session, &body_of(prompt)),
                "leaseDurationMs": value["lease_duration_ms"], "remainingMs": value["lease_duration_ms"],
            }))
        }
        Some("gone") => Ok(json!({"status": "gone", "disposition": gone(agent, session, message)?})),
        _ => Ok(json!({"status": value["status"]})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The recorded set `fixtures/orca-agent-session`: the fingerprints the pinned Orca code gave sessions and texts.
    fn recorded() -> Vec<Value> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/orca-agent-session/interactions.jsonl");
        std::fs::read_to_string(path).expect("the recorded set").lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }

    #[test]
    fn given_the_fingerprints_the_pinned_orca_code_recorded_when_the_host_hashes_the_same_bodies_then_each_is_the_same_hash() {
        // Given the recorded interactions: session, the texts of the body's blocks, and the fingerprint Orca gave
        let set = recorded();
        assert!(set.len() >= 5);
        for i in &set {
            let texts: Vec<Value> = i["input"]["texts"].as_array().unwrap().iter().map(|t| json!({"type": "text", "text": t})).collect();
            let body = json!({"kind": "message", "role": "user", "blocks": texts});
            // When the host hashes the same body
            let mine = fingerprint(i["input"]["sessionId"].as_str().unwrap(), &body);
            // Then it is the hash Orca recorded
            assert_eq!(mine, i["steps"][0]["payload"]["fingerprint"].as_str().unwrap(), "{}", i["id"]);
        }
    }
}
