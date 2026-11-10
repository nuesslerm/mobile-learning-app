//! A command as the client sends it, decoded the way the real decoder refuses: an undecodable payload is a defect, never a typed error.
use serde_json::{Map, Value};
use t3_contracts::rows::{command_types, command_union_defect};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Family {
    Thread,
    Project,
}

#[derive(Debug, Clone)]
pub struct Command {
    pub kind: String,
    pub command_id: String,
    pub body: Map<String, Value>,
}

/// The commands whose effect is the agent API's and whose only mark on the thread's rows is that they were accepted.
pub const ACCEPTED_ONLY: [&str; 15] = [
    "queue.resume",
    "queued-run.start",
    "queued-run.reorder",
    "queued-run.cancel",
    "queued-run.edit",
    "queued-message.promote-to-steer",
    "runtime-request.respond",
    "checkpoint.rollback",
    "prepared-run.retry",
    "subagent.stop",
    "provider-session.detach",
    "thread.active.reorder",
    "thread.pull-request.link",
    "thread.pull-request.unlink",
    "thread.pull-request.watch",
];

const PROJECT_TYPES: [&str; 4] = ["project.create", "project.update", "project.archive", "project.delete"];

/// The keys a type needs before it is decodable, in the order the decoder reports them.
fn required(kind: &str) -> &'static [&'static str] {
    match kind {
        "thread.create" => &["commandId", "threadId", "projectId"],
        "project.create" => &["commandId", "projectId", "title", "workspaceRoot"],
        "project.update" | "project.archive" | "project.delete" => &["commandId", "projectId"],
        "thread.fork" | "thread.merge_back" => &["commandId", "sourceThreadId", "targetThreadId"],
        "thread.pin.reorder" | "thread.active.reorder" => &["commandId"],
        k if ACCEPTED_ONLY.contains(&k) => &["commandId", "threadId"],
        k if k.starts_with("thread.") || k.starts_with("run.") || k == "message.dispatch" => &["commandId", "threadId"],
        _ => &["commandId"],
    }
}

impl Command {
    /// `Err` is the defect text of an undecodable payload.
    pub fn parse(value: &Value, family: Family) -> Result<Command, String> {
        let body = value.as_object().ok_or("Expected an object")?.clone();
        let kind = body.get("type").and_then(Value::as_str).unwrap_or("");
        let known = match family {
            Family::Thread => command_types().contains(&kind),
            Family::Project => PROJECT_TYPES.contains(&kind),
        };
        if !known {
            return Err(match family {
                Family::Thread => command_union_defect().to_string(),
                Family::Project => format!("Expected one of {} but got {kind:?}", PROJECT_TYPES.join(" | ")),
            });
        }
        if let Some(missing) = required(kind).iter().find(|k| !body.contains_key(**k)) {
            return Err(format!("Missing key\n  at [\"{missing}\"]"));
        }
        let command_id = body.get("commandId").and_then(Value::as_str).unwrap_or("").to_string();
        Ok(Command { kind: kind.to_string(), command_id, body })
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        self.body.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
    }

    pub fn require(&self, key: &str) -> Result<&str, String> {
        self.str(key).ok_or_else(|| format!("{} needs {key}", self.kind))
    }

    /// Every field except the envelope's and the ones named in `except`.
    pub fn rest(&self, except: &[&str]) -> Map<String, Value> {
        self.body.iter().filter(|(k, _)| !["type", "commandId"].contains(&k.as_str()) && !except.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect()
    }
}
