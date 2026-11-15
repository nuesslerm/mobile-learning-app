//! `agentSession.rewind`: the chat goes back to before a user message, by dropping the turns from the one that message opened on (`thread.rollback`,
//! files left as they are: the phone's rewind is the conversation's). The target is the message that opens a turn; the turn before it is the last
//! one kept, so a chat's first turn cannot be rewound to (rca keeps the thread as it was after a turn). A rewound journal is a new epoch.
use crate::agent::ask;
use crate::fold::Folded;
use crate::journal::{cursor_at, epoch_of, load, FENCE};
use crate::ops::{lookup, refused_with, Envelope};
use crate::session::finish;
use agent_api_agent::lifecycle::THREAD_ROLLBACK;
use orca_core::wire::Failure;
use orca_core::AgentApi;
use serde_json::{json, Value};

fn refusal(reason: &str) -> Value {
    refused_with("agent_session_operation_invalid", &format!("agent_session_rewind:{reason}"), json!({"reason": "rewindRefused", "rewindReason": reason}))
}

fn identifier(params: &Value, key: &str, message: &str) -> Result<String, Failure> {
    match orca_core::wire::string_field(params, key)? {
        None => Err(Failure::invalid("Invalid input: expected string, received undefined")),
        Some(s) if s.is_empty() || s.len() > 4096 => Err(Failure::invalid(message.to_string())),
        Some(s) => Ok(s.to_string()),
    }
}

/// The user message that opens a turn, and the turn before it (the last turn the chat keeps).
fn cut(folded: &Folded, item: &str) -> Result<Option<String>, &'static str> {
    let Some(target) = folded.items.iter().find(|i| i.id == item && i.body["kind"] == "message" && i.body["role"] == "user") else { return Err("invalid-target") };
    let Some(turn) = target.turn.as_deref() else { return Err("invalid-target") };
    let opener = folded.items.iter().find(|i| i.id == format!("turn:{turn}")).and_then(|t| t.body["userItemId"].as_str());
    if opener != Some(item) {
        return Err("invalid-target");
    }
    let turns: Vec<&str> = folded.items.iter().filter(|i| i.body["kind"] == "turn").filter_map(|i| i.body["turnId"].as_str()).collect();
    let at = turns.iter().position(|t| *t == turn).ok_or("invalid-target")?;
    Ok(at.checked_sub(1).map(|k| turns[k].to_string()))
}

pub(crate) fn rewind(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    match params.get("envelope") {
        Some(e) if e.is_object() => {}
        other => return Err(Failure::invalid(format!("Invalid input: expected object, received {}", other.map_or("undefined", orca_core::wire::received)))),
    }
    let env = Envelope::read(params)?;
    let item = identifier(params, "itemId", "Invalid item id")?;
    let expected = identifier(params, "expectedEpoch", "Invalid journal epoch")?;
    if let Some(done) = lookup(agent, &env)? {
        return Ok(done);
    }
    let loaded = load(agent, &env.session)?;
    let thread = crate::journal::view(agent, &env.session)?;
    // a running turn, or a prompt waiting on the phone, is work the rewind would cut under
    if thread["status"] == "running" || !loaded.folded.pending_prompt_ids().is_empty() {
        return Ok(refusal("busy"));
    }
    if expected != epoch_of(&loaded.folded) {
        return Ok(refusal("stale-epoch"));
    }
    let kept = match cut(&loaded.folded, &item) {
        Ok(Some(turn)) => turn,
        Ok(None) => return Ok(refusal("unsupported")),
        Err(reason) => return Ok(refusal(reason)),
    };
    match ask(agent, THREAD_ROLLBACK.name(), json!({"thread": env.session, "turn": kept, "restore_files": false})) {
        Ok(_) => {}
        Err(f) if f.message.contains("TURN_RUNNING") => return Ok(refusal("busy")),
        Err(f) => return Err(f),
    }
    let after = load(agent, &env.session)?;
    let epoch = epoch_of(&after.folded);
    let answer = json!({"ok": true, "replayed": false, "fence": FENCE, "cursor": cursor_at(&epoch, after.folded.last_seq), "value": {"itemId": item, "epoch": epoch}});
    finish(agent, &env, answer)
}
