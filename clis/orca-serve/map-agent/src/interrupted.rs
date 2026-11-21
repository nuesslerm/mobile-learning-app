//! `agentSession.continueInterrupted`: the chat's newest turn was cut short by rca's own restart (the turn is interrupted and its events end with
//! the stop row that names the cause), and the phone asks to go on. The host's rule is kept: the cut is checked again at the call, and a cut that
//! another message or an earlier Continue already answered sends nothing. The turn the phone gets is a prompt in the chat, as the pinned host sends
//! one; the host's separate attribution note has no place in a journal folded from rca's events.
use crate::journal::{load, view};
use crate::session::{agent_session_id, start_turn, Started};
use orca_core::wire::{string_field, Failure};
use orca_core::AgentApi;
use serde_json::{json, Value};

pub(crate) const CONTINUE: &str = "agent-session.continue-interrupted.v1";
pub(crate) const CONTINUE_PHONE: &str = "agent-session.continue-interrupted.phone.v1";

/// The pinned host's wording for a turn it cut short, when no background work was lost (`AGENT_SESSION_RESTART_CONTINUATION_MESSAGE`).
const MESSAGE: &str = "Orca restarted, so your previous reply was cut off partway through. Before continuing, check whether your most recent action completed — don't repeat it if it did. Then carry on.";

fn turn_item_id(params: &Value) -> Result<&str, Failure> {
    match string_field(params, "turnItemId")? {
        None => Err(Failure::invalid("Invalid input: expected string, received undefined")),
        Some("") => Err(Failure::invalid("Too small: expected string to have >=1 characters")),
        Some(s) if s.chars().count() > 1024 => Err(Failure::invalid("Too big: expected string to have <=1024 characters")),
        Some(s) => Ok(s),
    }
}

fn outcome(session: &str, outcome: &str) -> Value {
    json!({"sessionId": session, "outcome": outcome})
}

pub(crate) fn continue_interrupted(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let session = agent_session_id(params)?;
    let turn = turn_item_id(params)?;
    if let Some(key) = params.as_object().and_then(|o| o.keys().find(|k| !matches!(k.as_str(), "sessionId" | "turnItemId"))) {
        return Err(Failure::invalid(format!("Unrecognized key: \"{key}\"")));
    }
    let thread = view(agent, session)?;
    if load(agent, session)?.folded.restart_cut() != Some(turn) {
        return Ok(outcome(session, "superseded"));
    }
    match start_turn(agent, session, MESSAGE, &[], None, &thread)? {
        Started::Turn => Ok(outcome(session, "pending")),
        Started::Refused(r) if r["refusal"]["code"] == "agent_session_conflict" => Ok(outcome(session, "superseded")),
        Started::Refused(r) => Ok(json!({"sessionId": session, "outcome": "refused", "reason": r["refusal"]["code"]})),
    }
}
