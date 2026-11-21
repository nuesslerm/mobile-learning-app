//! `ssh.*`: an executor is an SSH target; its id is the target id and it reads connected while attached. rca never dials it, so a connect only
//! reports a target already there, and the summaries carry display identity only.
use crate::agent::ask;
use crate::files::plain;
use agent_api_workspace::REPO_LIST;
use orca_core::wire::{string_field, Failure};
use orca_core::AgentApi;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Every executor `repo.list` names, with whether any of its workspaces is attached.
fn executors(agent: &dyn AgentApi) -> Result<BTreeMap<String, bool>, Failure> {
    let mut all = BTreeMap::new();
    for row in ask(agent, REPO_LIST.name(), json!({}))?["repos"].as_array().into_iter().flatten() {
        if let Some(id) = row["executor"].as_str() {
            *all.entry(id.to_string()).or_insert(false) |= row["attached"] == true;
        }
    }
    Ok(all)
}

fn status(attached: bool) -> &'static str {
    if attached { "connected" } else { "disconnected" }
}

fn state(id: &str, attached: bool) -> Value {
    json!({"targetId": id, "status": status(attached), "error": null, "reconnectAttempt": 0})
}

fn target_id(params: &Value) -> Result<String, Failure> {
    string_field(params, "targetId")?.map(str::to_string).ok_or_else(|| Failure::invalid("Invalid input: expected string, received undefined"))
}

pub(crate) fn list_summaries(agent: &dyn AgentApi) -> Result<Value, Failure> {
    let targets: Vec<Value> = executors(agent)?.iter().map(|(id, attached)| json!({"id": id, "label": id, "connected": attached, "connectionStatus": status(*attached)})).collect();
    Ok(json!({"targets": targets}))
}

pub(crate) fn get_state(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let id = target_id(params)?;
    Ok(json!({"state": executors(agent)?.get(&id).map(|a| state(&id, *a))}))
}

pub(crate) fn connect(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let id = target_id(params)?;
    match executors(agent)?.get(&id) {
        Some(true) => Ok(json!({"state": state(&id, true)})),
        _ => Err(plain("SSH connection unavailable")),
    }
}
