//! The terminal quick commands: the list kept as Orca normalises it (trimmed, bounded, a global scope when none is named), changed by one
//! upsert or delete at a time so two phones' edits of different commands both stand.
use crate::store::{self, QUICK_COMMANDS};
use orca_core::wire::Failure;
use orca_core::AgentApi;
use serde_json::{json, Map, Value};

const MAX_ID: usize = 80;
const MAX_LABEL: usize = 80;
const MAX_REPO_ID: usize = 200;
const MAX_COMMAND: usize = 4000;

pub(crate) fn list(agent: &dyn AgentApi) -> Result<Vec<Value>, Failure> {
    store::read_list(agent, QUICK_COMMANDS)
}

fn text<'a>(item: &'a Map<String, Value>, key: &str) -> Result<&'a str, Failure> {
    item.get(key).and_then(Value::as_str).ok_or_else(|| Failure::invalid(format!("Invalid quick command: {key} must be a string")))
}

fn scope(item: &Map<String, Value>) -> Result<Value, Failure> {
    match item.get("scope") {
        None | Some(Value::Null) => Ok(json!({"type": "global"})),
        Some(s) => match (s["type"].as_str(), s["repoId"].as_str()) {
            (Some("global"), None) => Ok(json!({"type": "global"})),
            (Some("repo"), Some(repo)) if !repo.trim().is_empty() => Ok(json!({"type": "repo", "repoId": repo.trim().chars().take(MAX_REPO_ID).collect::<String>()})),
            _ => Err(Failure::invalid("Invalid quick command: scope is global or a repo with its id")),
        },
    }
}

/// The command an upsert names, as it is kept.
fn normalized(command: &Value) -> Result<Value, Failure> {
    let item = command.as_object().ok_or_else(|| Failure::invalid("Invalid quick command"))?;
    let id = text(item, "id")?.trim();
    if id.is_empty() || id.chars().count() > MAX_ID {
        return Err(Failure::invalid("Invalid quick command: id is 1 to 80 characters"));
    }
    let label = text(item, "label")?.trim();
    if label.chars().count() > MAX_LABEL {
        return Err(Failure::invalid("Invalid quick command: label is at most 80 characters"));
    }
    let mut kept = Map::new();
    kept.extend([("id".to_string(), json!(id)), ("label".into(), json!(label)), ("scope".into(), scope(item)?)]);
    match item.get("action").and_then(Value::as_str) {
        Some("agent-prompt") => {
            let agent = text(item, "agent")?;
            if agent.is_empty() {
                return Err(Failure::invalid("Invalid quick command: agent is empty"));
            }
            kept.extend([("action".to_string(), json!("agent-prompt")), ("agent".into(), json!(agent)), ("prompt".into(), json!(text(item, "prompt")?.trim_end()))]);
        }
        None | Some("terminal-command") => {
            let body = text(item, "command")?.trim_end();
            if body.chars().count() > MAX_COMMAND {
                return Err(Failure::invalid("Invalid quick command: command is at most 4000 characters"));
            }
            let enter = item.get("appendEnter").and_then(Value::as_bool).ok_or_else(|| Failure::invalid("Invalid quick command: appendEnter must be a boolean"))?;
            kept.extend([("action".to_string(), json!("terminal-command")), ("command".into(), json!(body)), ("appendEnter".into(), json!(enter))]);
        }
        Some(_) => return Err(Failure::invalid("Invalid quick command: action is terminal-command or agent-prompt")),
    }
    Ok(Value::Object(kept))
}

pub(crate) fn update(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let mutation = &params["mutation"];
    let mut commands = list(agent)?;
    match mutation["type"].as_str() {
        Some("upsert") => {
            let command = normalized(&mutation["command"])?;
            match commands.iter().position(|c| c["id"] == command["id"]) {
                Some(at) => commands[at] = command,
                None => commands.push(command),
            }
        }
        Some("delete") => {
            let id = mutation["id"].as_str().filter(|i| !i.is_empty()).ok_or_else(|| Failure::invalid("Missing quick command id"))?;
            commands.retain(|c| c["id"] != id);
        }
        _ => return Err(Failure::invalid("A quick command mutation is an upsert or a delete")),
    }
    store::write(agent, QUICK_COMMANDS, json!(commands))?;
    Ok(json!({"terminalQuickCommands": commands}))
}
