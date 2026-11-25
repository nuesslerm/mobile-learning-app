//! Structured launches reserve an operation before creating a chat or delivering its first prompt.
use crate::agent::{ask, state_get, state_set};
use crate::ids::{is_session_id_for, mint};
use crate::session::{driver_of, start_turn_text, text, unsupported};
use agent_api_agent::THREAD_CREATE;
use orca_core::wire::{Failure, RUNTIME_ERROR};
use orca_core::AgentApi;
use serde_json::{json, Value};

pub(crate) const ROUTING: &str = "agent.launch.v2";
pub(crate) const REPLAY_REQUIRED: &str = "agent.launch.replay-required.v1";
pub(crate) const REGISTERED_AGENTS: &str = "agent-session.structured.registered-agents.v1";
const UNKNOWN: &str = "agent_session_operation_unknown";

fn key(op: &str) -> String {
    format!("launch-op.{op}")
}

/// What the operation id is bound to: the same id with another agent, target or prompt is another request.
fn fingerprint(params: &Value) -> String {
    let mut fields = vec![&params["agent"], &params["target"], &params["prompt"]];
    if params["sessionId"].is_string() {
        fields.extend([&params["sessionId"], &params["sessionOptions"], &params["chatOptions"]]);
    }
    json!(fields).to_string()
}

fn recorded(agent: &dyn AgentApi, op: &str, fp: Option<&str>) -> Result<Option<Value>, Failure> {
    let Some(entry) = state_get(agent, &key(op))? else { return Ok(None) };
    match fp {
        Some(fp) if entry["fingerprint"] != fp => Err(Failure::new(RUNTIME_ERROR, "agent_session_operation_conflict")),
        _ => entry.get("result").cloned().map(Some).ok_or_else(|| Failure::new(UNKNOWN, UNKNOWN)),
    }
}

pub(crate) fn replay(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    text(params, "agent").ok_or_else(|| Failure::invalid("Unknown TUI agent"))?;
    let op = text(params, "operationId").ok_or_else(|| Failure::invalid("Malformed launch operation id"))?;
    if params["target"]["kind"] != "existing" || params["nativeChatDefault"] == false || params["surface"] == "terminal" || !params["agentArgs"].is_null() || !params["cwd"].is_null() {
        return Err(Failure::new("agent_launch_replay_unsupported", "This launch needs the legacy terminal or workspace route"));
    }
    if params["sessionId"].is_string() {
        return launch(agent, params);
    }
    recorded(agent, op, Some(&fingerprint(params)))?.ok_or_else(|| Failure::new(RUNTIME_ERROR, "agent_session_operation_unknown"))
}

pub(crate) fn launch(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let op = text(params, "operationId");
    let fp = fingerprint(params);
    if let Some(done) = op.map(|o| recorded(agent, o, Some(&fp))).transpose()?.flatten() {
        return Ok(done);
    }
    let agent_id = text(params, "agent").ok_or_else(|| Failure::invalid("Unknown TUI agent"))?;
    let driver = driver_of(agent_id).ok_or_else(unsupported)?;
    if params["target"]["kind"] != "existing" {
        return Err(Failure::new(RUNTIME_ERROR, "agent_launch_target_unsupported"));
    }
    let target = orca_map_workspace::resolve(agent, text(&params["target"], "worktree").ok_or_else(|| Failure::invalid("Missing worktree selector"))?)?;
    let session = match text(params, "sessionId") {
        Some(s) if is_session_id_for(agent_id, s) => s.to_string(),
        Some(_) => return Err(Failure::invalid("Launch session id must be named for its agent")),
        None => mint(agent_id),
    };
    let model = params["sessionOptions"]["model"].as_str().or_else(|| params["chatOptions"]["model"].as_str());
    let mut create = json!({"thread": session, "workspace": target.workspace, "driver": driver});
    if let Some(m) = model {
        create["model"] = m.into();
    }
    let after_reservation = |error: Failure| {
        if op.is_some() && error.code == "runtime_unavailable" {
            Failure::new(UNKNOWN, UNKNOWN)
        } else {
            error
        }
    };
    if let Some(op) = op {
        // A retry after a lost receipt must never create another chat or send another prompt.
        state_set(agent, &key(op), json!({"fingerprint": fp}))?;
    }
    ask(agent, THREAD_CREATE.name(), create).map_err(after_reservation)?;
    orca_map_workspace::layout::open_chat(agent, &target.workspace, &session, agent_id, text(params, "title").unwrap_or("New thread")).map_err(after_reservation)?;
    let mut result = json!({
        "outcome": {"kind": "structured", "sessionId": session, "tabId": session, "handle": session},
        "worktreeId": target.id,
        "receipt": {"mode": "structured", "preferred": "structured", "reason": if params.get("surface").is_some() { "requested" } else { "user_default" }, "detail": "the session runs as an rca thread"},
    });
    if let Some(prompt) = params.get("prompt").filter(|p| p.is_object()) {
        let delivery = prompt["delivery"].as_str().unwrap_or("submit");
        result["prompt"] = if delivery == "submit" {
            start_turn_text(agent, &session, prompt["text"].as_str().unwrap_or_default(), model).map_err(after_reservation)?;
            json!({"delivery": delivery, "outcome": "journaled", "messageId": format!("launch_{session}")})
        } else {
            json!({"delivery": delivery, "outcome": "not-delivered"})
        };
    }
    if let Some(op) = op {
        state_set(agent, &key(op), json!({"fingerprint": fp, "result": result})).map_err(|_| Failure::new(UNKNOWN, UNKNOWN))?;
    }
    Ok(result)
}
