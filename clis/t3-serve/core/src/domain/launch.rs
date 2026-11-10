//! `orchestration.launchThread` decided without I/O, after the pin's `ThreadLaunchService`: the payload decoded the way the real
//! decoder refuses it, the launches this server does not build, the checks against the read model, and the two commands a launch
//! is made of (the thread claim under the launch's command id, then the first message under `<commandId>:initial-message`).
use super::auth::uuid_v4;
use super::decider::check_attachments;
use super::state::{is_archived, State};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq)]
pub enum Workspace {
    Root { branch: Option<String> },
    ExistingWorktree { path: String, branch: Option<String> },
    /// A worktree the server would create from `baseRef`.
    NewWorktree,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InitialMessage {
    pub message_id: Option<String>,
    pub text: String,
    pub attachments: Vec<Value>,
}

/// The fields this server reads; `generateTitle` and the message's `context` are accepted and not used.
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchRequest {
    pub command_id: String,
    pub project_id: String,
    pub thread_id: Option<String>,
    pub reuse_existing_thread: bool,
    pub title: String,
    pub model_selection: Value,
    pub runtime_mode: String,
    pub interaction_mode: String,
    pub creation_source: String,
    pub workspace: Workspace,
    pub initial_message: Option<InitialMessage>,
}

/// The required keys, in the order the decoder reports the first one missing.
const REQUIRED: [&str; 7] = ["commandId", "projectId", "title", "modelSelection", "runtimeMode", "interactionMode", "workspaceStrategy"];

fn at(path: &[&str]) -> String {
    path.iter().map(|k| format!("[\"{k}\"]")).collect()
}

fn missing(path: &[&str]) -> String {
    format!("Missing key\n  at {}", at(path))
}

fn expected(what: &str, path: &[&str]) -> String {
    format!("Expected {what}\n  at {}", at(path))
}

fn text(body: &Map<String, Value>, path: &[&str]) -> Result<String, String> {
    let key = path[path.len() - 1];
    body.get(key).ok_or_else(|| missing(path))?.as_str().map(str::to_string).ok_or_else(|| expected("string", path))
}

fn optional_text(body: &Map<String, Value>, path: &[&str]) -> Result<Option<String>, String> {
    match body.get(path[path.len() - 1]) {
        None => Ok(None),
        Some(_) => text(body, path).map(Some),
    }
}

fn object<'a>(value: &'a Value, path: &[&str]) -> Result<&'a Map<String, Value>, String> {
    value.as_object().ok_or_else(|| expected("object", path))
}

impl LaunchRequest {
    /// `Err` is the defect text of an undecodable payload.
    pub fn parse(payload: &Value) -> Result<LaunchRequest, String> {
        let body = payload.as_object().ok_or("Expected an object")?;
        if let Some(key) = REQUIRED.iter().find(|k| !body.contains_key(**k)) {
            return Err(missing(&[key]));
        }
        object(&body["modelSelection"], &["modelSelection"])?;
        let strategy = object(&body["workspaceStrategy"], &["workspaceStrategy"])?;
        let branch = optional_text(strategy, &["workspaceStrategy", "branch"])?;
        let workspace = match strategy.get("type").and_then(Value::as_str) {
            Some("root") => Workspace::Root { branch },
            Some("existing_worktree") => Workspace::ExistingWorktree { path: text(strategy, &["workspaceStrategy", "worktreePath"])?, branch },
            Some("worktree") => {
                text(strategy, &["workspaceStrategy", "baseRef"])?;
                Workspace::NewWorktree
            }
            _ => return Err(expected("\"root\" | \"existing_worktree\" | \"worktree\"", &["workspaceStrategy", "type"])),
        };
        let initial_message = match body.get("initialMessage") {
            None => None,
            Some(value) => {
                let message = object(value, &["initialMessage"])?;
                let attachments = message.get("attachments").ok_or_else(|| missing(&["initialMessage", "attachments"]))?;
                Some(InitialMessage {
                    message_id: optional_text(message, &["initialMessage", "messageId"])?,
                    text: text(message, &["initialMessage", "text"])?,
                    attachments: attachments.as_array().ok_or_else(|| expected("array", &["initialMessage", "attachments"]))?.clone(),
                })
            }
        };
        let reuse_existing_thread = match body.get("reuseExistingThread") {
            None => false,
            Some(flag) => flag.as_bool().ok_or_else(|| expected("boolean", &["reuseExistingThread"]))?,
        };
        Ok(LaunchRequest {
            command_id: text(body, &["commandId"])?,
            project_id: text(body, &["projectId"])?,
            thread_id: optional_text(body, &["threadId"])?,
            reuse_existing_thread,
            title: text(body, &["title"])?,
            model_selection: body["modelSelection"].clone(),
            runtime_mode: text(body, &["runtimeMode"])?,
            interaction_mode: text(body, &["interactionMode"])?,
            creation_source: optional_text(body, &["creationSource"])?.unwrap_or_else(|| "web".into()),
            workspace,
            initial_message,
        })
    }
}

/// A launch this server does not build, refused before anything is written: the pin creates the worktree and stores the files.
pub fn unsupported(req: &LaunchRequest) -> Option<&'static str> {
    if req.workspace == Workspace::NewWorktree {
        return Some("worktree launches are not supported by this server; launch on the project root or an existing worktree");
    }
    None
}

/// The client's thread id, else one derived from the command id so that a retry lands on the thread the first attempt made.
pub fn thread_id(req: &LaunchRequest) -> String {
    req.thread_id.clone().unwrap_or_else(|| uuid_v4(&Sha256::digest(format!("launch:{}", req.command_id))[..16]))
}

/// The checks against the read model before anything is written; `claimed` says the launch's own command was accepted before.
pub fn check(state: &State, req: &LaunchRequest, thread_id: &str, claimed: bool) -> Result<(), String> {
    if !state.active_project(&req.project_id) {
        return Err("Project not found.".into());
    }
    if req.reuse_existing_thread && req.thread_id.is_none() {
        return Err("Reusing an existing thread requires a thread id.".into());
    }
    if req.initial_message.as_ref().is_some_and(|m| m.text.is_empty()) {
        return Err("The initial message has no text.".into());
    }
    if let Some(m) = &req.initial_message {
        check_attachments(&m.attachments)?;
    }
    let thread = state.threads.get(thread_id);
    if claimed {
        return match thread {
            Some(t) if t["projectId"] != req.project_id.as_str() => Err("Project identity changed.".into()),
            Some(_) => Ok(()),
            None => Err("Thread not found.".into()),
        };
    }
    let empty = || state.entities.get(thread_id).is_none_or(|e| e.messages.is_empty() && e.runs.is_empty());
    if req.reuse_existing_thread && !thread.is_some_and(|t| t["projectId"] == req.project_id.as_str() && !is_archived(t) && empty()) {
        return Err("Only an empty active thread in the target project can change workspace during launch.".into());
    }
    Ok(())
}

/// The claim, under the launch's command id: `thread.create`, or for a reused thread the change to the launch's workspace and model.
pub fn claim(req: &LaunchRequest, thread_id: &str) -> Value {
    let (branch, worktree) = match &req.workspace {
        Workspace::Root { branch } => (branch.clone(), None),
        Workspace::ExistingWorktree { path, branch } => (branch.clone(), Some(path.clone())),
        Workspace::NewWorktree => (None, None),
    };
    let mut command = json!({
        "commandId": req.command_id,
        "threadId": thread_id,
        "modelSelection": req.model_selection,
        "runtimeMode": req.runtime_mode,
        "interactionMode": req.interaction_mode,
        "branch": branch,
        "worktreePath": worktree,
    });
    if req.reuse_existing_thread {
        command["type"] = json!("thread.metadata.update");
        command["providerInstanceId"] = req.model_selection["instanceId"].clone();
    } else {
        for (key, value) in [("type", "thread.create"), ("projectId", req.project_id.as_str()), ("title", req.title.as_str()), ("createdBy", "user"), ("creationSource", req.creation_source.as_str())] {
            command[key] = json!(value);
        }
    }
    command
}

pub fn initial_message(req: &LaunchRequest, thread_id: &str) -> Option<Value> {
    let m = req.initial_message.as_ref()?;
    let mut message = json!({"text": m.text});
    if let Some(id) = &m.message_id {
        message["messageId"] = json!(id);
    }
    if !m.attachments.is_empty() {
        message["attachments"] = json!(m.attachments);
    }
    Some(json!({
        "type": "message.dispatch",
        "commandId": format!("{}:initial-message", req.command_id),
        "threadId": thread_id,
        "message": message,
        "createdBy": "user",
        "creationSource": req.creation_source,
    }))
}

