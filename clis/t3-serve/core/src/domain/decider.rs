//! Commands in, events out, decided against the current state; pure. A thread event carries the thread row after the change (the
//! recorded shape), so folding an event is a merge. Run steps (what a provider turn reports) are decided here too.
use super::command::{Command, Family, ACCEPTED_ONLY};
use super::state::{is_archived, State};
use crate::ports::{AgentEventKind, EventDraft, Finish, Millis};
use serde_json::{json, Map, Value};
use t3_contracts::rows::{project_defaults, thread_defaults, thread_event_row};
use t3_contracts::time::iso;

/// What the engine does after the events are stored.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Launch(Launch),
    Interrupt { thread_id: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Launch {
    pub thread_id: String,
    pub run_id: String,
    pub instance_id: String,
    pub model: Option<String>,
    pub prompt: String,
    pub runtime_mode: String,
    pub interaction_mode: String,
    /// Ids of the attachments the message carries.
    pub attachments: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Decision {
    pub drafts: Vec<EventDraft>,
    pub effect: Option<Effect>,
}

/// Why a command was not decided: a refusal in words, or the one the wire names on its own.
#[derive(Debug, Clone, PartialEq)]
pub enum Rejection {
    Refused(String),
    /// `project.delete` without `force` of a project that still holds threads (`ProjectNotEmptyError`).
    ProjectNotEmpty(String),
}

impl From<String> for Rejection {
    fn from(message: String) -> Rejection {
        Rejection::Refused(message)
    }
}

impl Rejection {
    pub fn message(&self) -> String {
        match self {
            Rejection::Refused(message) => message.clone(),
            Rejection::ProjectNotEmpty(project) => t3_contracts::wire::project_not_empty(project),
        }
    }
}

/// What the decider needs of the outside, as values: the time and a fresh id source.
pub struct Ctx<'a> {
    pub now: Millis,
    pub new_id: &'a dyn Fn() -> String,
}

fn draft(kind: &str, thread: Option<&str>, project: Option<&str>, at: Millis, payload: Value) -> EventDraft {
    EventDraft { event_id: String::new(), kind: kind.into(), thread_id: thread.map(str::to_string), project_id: project.map(str::to_string), occurred_at: at, payload }
}

fn one(d: EventDraft) -> Result<Decision, String> {
    Ok(Decision { drafts: vec![d], effect: None })
}

/// The commands the thread decider builds; the rest of the decoder's union is refused with a typed error.
pub const SUPPORTED: [&str; 26] = [
    "thread.settle",
    "thread.unsettle",
    "thread.snooze",
    "thread.unsnooze",
    "thread.auto-settle.set",
    "thread.pin.reorder",
    "thread.mark-unread",
    "thread.fork",
    "thread.merge_back",
    "thread.user-input.dismiss",
    "thread.create",
    "thread.archive",
    "thread.unarchive",
    "thread.delete",
    "thread.metadata.update",
    "thread.runtime-mode.set",
    "thread.interaction-mode.set",
    "thread.model-selection.set",
    "thread.pin",
    "thread.unpin",
    "thread.visit",
    "thread.guest-share.close",
    "thread.guest-message.dismiss",
    "thread.stop",
    "message.dispatch",
    "run.interrupt",
];

fn row_event(kind: &str, shell: &Map<String, Value>, at: Millis) -> EventDraft {
    let id = shell.get("id").and_then(Value::as_str);
    let mut row = thread_event_row(shell);
    if kind == "thread.pinned" || kind == "thread.unpinned" {
        // the recorded row has no pin fields; a pin event is read for them (shape unverified)
        for key in ["pinnedAt", "pinOrderKey"] {
            row[key] = shell.get(key).cloned().unwrap_or(Value::Null);
        }
    }
    draft(kind, id, shell.get("projectId").and_then(Value::as_str), at, row)
}

/// A thread event whose row also carries `extra` keys the recorded event row leaves out.
fn marked(kind: &str, shell: &Map<String, Value>, extra: &[&str], at: Millis) -> Result<Decision, String> {
    let mut event = row_event(kind, shell, at);
    for key in extra {
        event.payload[*key] = shell.get(*key).cloned().unwrap_or(Value::Null);
    }
    one(event)
}

/// `thread.fork` makes `targetThreadId` out of the source's row, in the source's family; `thread.merge_back` only marks the target.
fn fork_or_merge(state: &State, cmd: &Command, ctx: &Ctx) -> Result<Decision, String> {
    let (source, target) = (cmd.require("sourceThreadId")?, cmd.require("targetThreadId")?);
    let from = state.threads.get(source).ok_or_else(|| format!("thread {source} does not exist"))?;
    let (now, at) = (ctx.now, iso(ctx.now));
    if cmd.kind == "thread.merge_back" {
        let into = state.threads.get(target).ok_or_else(|| format!("thread {target} does not exist"))?;
        return one(draft("thread.command-accepted", Some(target), into["projectId"].as_str(), now, json!({"commandType": cmd.kind, "sourceThreadId": source})));
    }
    if state.threads.contains_key(target) {
        return Err(format!("thread {target} already exists"));
    }
    let mut shell = from.clone();
    let root = from["lineage"]["rootThreadId"].as_str().unwrap_or(source).to_string();
    let title = cmd.str("title").map_or_else(|| format!("{} fork", from["title"].as_str().unwrap_or("Thread")), str::to_string);
    let defaults = thread_defaults();
    for key in ["archivedAt", "settledOverride", "settledAt", "unsettledAt", "snoozedUntil", "snoozedAt", "pinnedAt", "pinOrderKey", "lastVisitedAt", "deletedAt", "latestRunId", "activeRunId", "status", "latestVisibleMessage"] {
        shell.insert(key.into(), defaults.get(key).cloned().unwrap_or(Value::Null));
    }
    touched(
        &mut shell,
        &at,
        vec![
            ("id", target.into()),
            ("title", title.into()),
            ("lineage", json!({"parentThreadId": source, "relationshipToParent": "fork", "rootThreadId": root})),
            ("forkedFrom", json!({"threadId": source})),
            ("createdAt", at.clone().into()),
        ],
    );
    one(row_event("thread.created", &shell, now))
}

fn touched(shell: &mut Map<String, Value>, at: &str, changes: Vec<(&str, Value)>) {
    for (k, v) in changes {
        shell.insert(k.into(), v);
    }
    shell.insert("updatedAt".into(), at.into());
}

pub fn decide_thread(state: &State, cmd: &Command, ctx: &Ctx) -> Result<Decision, String> {
    if !SUPPORTED.contains(&cmd.kind.as_str()) && !ACCEPTED_ONLY.contains(&cmd.kind.as_str()) {
        return Err(format!("command type {} is not supported by this server", cmd.kind));
    }
    let (now, at) = (ctx.now, iso(ctx.now));
    if cmd.kind == "thread.create" {
        let (id, project) = (cmd.require("threadId")?, cmd.require("projectId")?);
        if state.threads.contains_key(id) {
            return Err(format!("thread {id} already exists"));
        }
        if !state.active_project(project) {
            return Err(format!("project {project} does not exist"));
        }
        let mut shell = thread_defaults();
        let selection = cmd.body.get("modelSelection").filter(|m| m.is_object()).cloned();
        let instance = selection.as_ref().and_then(|m| m["instanceId"].as_str()).unwrap_or("opencode").to_string();
        for key in ["runtimeMode", "interactionMode", "branch", "worktreePath", "createdBy", "creationSource"] {
            if let Some(v) = cmd.body.get(key) {
                shell.insert(key.into(), v.clone());
            }
        }
        touched(
            &mut shell,
            &at,
            vec![
                ("id", id.into()),
                ("projectId", project.into()),
                ("title", cmd.str("title").unwrap_or("New thread").into()),
                ("providerInstanceId", instance.as_str().into()),
                ("modelSelection", selection.unwrap_or(Value::Null)),
                ("lineage", json!({"parentThreadId": null, "relationshipToParent": null, "rootThreadId": id})),
                ("createdAt", at.clone().into()),
            ],
        );
        return one(row_event("thread.created", &shell, now));
    }
    if cmd.kind == "thread.fork" || cmd.kind == "thread.merge_back" {
        return fork_or_merge(state, cmd, ctx);
    }
    let id = cmd.require("threadId")?;
    let mut shell = state.threads.get(id).cloned().ok_or_else(|| format!("thread {id} does not exist"))?;
    let archived = is_archived(&shell);
    let event = |kind: &str, shell: &Map<String, Value>| one(row_event(kind, shell, now));
    match cmd.kind.as_str() {
        "thread.archive" if archived => Err(format!("thread {id} is already archived")),
        "thread.archive" => {
            touched(&mut shell, &at, vec![("archivedAt", at.clone().into())]);
            event("thread.archived", &shell)
        }
        "thread.unarchive" if !archived => Err(format!("thread {id} is not archived")),
        "thread.unarchive" => {
            touched(&mut shell, &at, vec![("archivedAt", Value::Null)]);
            event("thread.unarchived", &shell)
        }
        "thread.delete" => {
            touched(&mut shell, &at, vec![("deletedAt", at.clone().into())]);
            event("thread.deleted", &shell)
        }
        "thread.metadata.update" => {
            let patch = cmd.rest(&["threadId", "id", "projectId", "createdAt", "updatedAt", "archivedAt"]);
            if patch.is_empty() {
                return Err("thread.metadata.update carries no field to change".into());
            }
            for (k, v) in patch {
                if shell.contains_key(&k) {
                    shell.insert(k, v);
                }
            }
            touched(&mut shell, &at, vec![]);
            event("thread.metadata-updated", &shell)
        }
        "thread.runtime-mode.set" => {
            let mode = cmd.require("runtimeMode")?;
            touched(&mut shell, &at, vec![("runtimeMode", mode.into())]);
            event("thread.runtime-mode-updated", &shell)
        }
        "thread.interaction-mode.set" => {
            let mode = cmd.require("interactionMode")?;
            touched(&mut shell, &at, vec![("interactionMode", mode.into())]);
            event("thread.interaction-mode-updated", &shell)
        }
        "thread.model-selection.set" => {
            let selection = cmd.body.get("modelSelection").filter(|m| m["instanceId"].is_string()).cloned().ok_or("thread.model-selection.set needs modelSelection")?;
            touched(&mut shell, &at, vec![("providerInstanceId", selection["instanceId"].clone()), ("modelSelection", selection)]);
            event("thread.model-selection-updated", &shell)
        }
        "thread.settle" => {
            let settled_at = cmd.str("settledAt").map_or(at.clone(), str::to_string);
            touched(&mut shell, &at, vec![("settledOverride", "settled".into()), ("settledAt", settled_at.into()), ("unsettledAt", Value::Null), ("snoozedUntil", Value::Null), ("snoozedAt", Value::Null), ("pinnedAt", Value::Null), ("pinOrderKey", Value::Null)]);
            marked("thread.settled", &shell, &["unsettledAt", "pinnedAt", "pinOrderKey"], now)
        }
        "thread.unsettle" => {
            touched(&mut shell, &at, vec![("settledOverride", "active".into()), ("settledAt", Value::Null), ("unsettledAt", at.clone().into())]);
            marked("thread.unsettled", &shell, &["unsettledAt"], now)
        }
        "thread.snooze" => {
            let until = cmd.require("snoozedUntil")?;
            touched(&mut shell, &at, vec![("snoozedUntil", until.into()), ("snoozedAt", at.clone().into())]);
            event("thread.snoozed", &shell)
        }
        "thread.unsnooze" => {
            touched(&mut shell, &at, vec![("snoozedUntil", Value::Null), ("snoozedAt", Value::Null)]);
            event("thread.unsnoozed", &shell)
        }
        "thread.auto-settle.set" => {
            let enabled = cmd.body.get("enabled").and_then(Value::as_bool).ok_or("thread.auto-settle.set needs enabled")?;
            touched(&mut shell, &at, vec![("autoSettleDisabledAt", if enabled { Value::Null } else { at.clone().into() })]);
            marked("thread.auto-settle-set", &shell, &["autoSettleDisabledAt"], now)
        }
        "thread.mark-unread" => {
            touched(&mut shell, &at, vec![("lastVisitedAt", Value::Null)]);
            event("thread.marked-unread", &shell)
        }
        "thread.pin.reorder" => {
            touched(&mut shell, &at, vec![("pinOrderKey", cmd.require("orderKey")?.into())]);
            marked("thread.pin-reordered", &shell, &["pinOrderKey"], now)
        }
        "thread.user-input.dismiss" => one(draft("thread.command-accepted", Some(id), shell["projectId"].as_str(), now, json!({"commandType": cmd.kind}))),
        kind if ACCEPTED_ONLY.contains(&kind) => one(draft("thread.command-accepted", Some(id), shell["projectId"].as_str(), now, json!({"commandType": kind}))),
        "thread.pin" => {
            let order = cmd.str("orderKey").map_or(format!("{now:013}"), str::to_string);
            touched(&mut shell, &at, vec![("pinnedAt", at.clone().into()), ("pinOrderKey", order.into())]);
            event("thread.pinned", &shell)
        }
        "thread.unpin" => {
            touched(&mut shell, &at, vec![("pinnedAt", Value::Null), ("pinOrderKey", Value::Null)]);
            event("thread.unpinned", &shell)
        }
        "thread.visit" => {
            touched(&mut shell, &at, vec![("lastVisitedAt", at.clone().into())]);
            event("thread.visited", &shell)
        }
        "thread.guest-share.close" if shell.get("guestShare").is_none_or(Value::is_null) => one(draft("thread.command-accepted", Some(id), shell["projectId"].as_str(), now, json!({"commandType": cmd.kind}))),
        "thread.guest-share.close" => {
            touched(&mut shell, &at, vec![("guestShare", Value::Null)]);
            event("thread.guest-share-closed", &shell)
        }
        "thread.guest-message.dismiss" => {
            let message = cmd.require("guestMessageId")?;
            let mut share = shell.get("guestShare").filter(|s| s.is_object()).cloned().ok_or("This thread has no guest link.")?;
            let pending = share["messages"].as_array_mut().and_then(|all| all.iter_mut().find(|m| m["id"] == message && m["status"] == "pending")).ok_or_else(|| format!("Guest message {message} is not pending."))?;
            pending["status"] = "dismissed".into();
            touched(&mut shell, &at, vec![("guestShare", share)]);
            event("thread.guest-message-dismissed", &shell)
        }
        "thread.stop" | "run.interrupt" => interrupt(state, id, ctx),
        "message.dispatch" => dispatch_message(state, cmd, &shell, ctx),
        other => Err(format!("command type {other} is not supported by this server")),
    }
}

fn active_run(state: &State, thread: &str) -> Option<Value> {
    state.entities.get(thread)?.runs.iter().rev().find(|r| r["status"] == "running").cloned()
}

fn interrupt(state: &State, thread: &str, ctx: &Ctx) -> Result<Decision, String> {
    let mut run = active_run(state, thread).ok_or_else(|| format!("thread {thread} has no active run"))?;
    run["stopRequested"] = Value::Bool(true);
    run["updatedAt"] = iso(ctx.now).into();
    Ok(Decision { drafts: vec![draft("run.updated", Some(thread), None, ctx.now, run)], effect: Some(Effect::Interrupt { thread_id: thread.to_string() }) })
}

fn dispatch_message(state: &State, cmd: &Command, shell: &Map<String, Value>, ctx: &Ctx) -> Result<Decision, String> {
    let thread = shell["id"].as_str().unwrap_or("");
    if is_archived(shell) {
        return Err(format!("thread {thread} is archived"));
    }
    let mode = cmd.str("dispatchMode").unwrap_or("start_immediately");
    if mode != "start_immediately" {
        return Err(format!("dispatch mode {mode} is not supported by this server"));
    }
    if active_run(state, thread).is_some() {
        return Err(format!("thread {thread} already has an active run"));
    }
    let message = cmd.body.get("message").filter(|m| m.is_object());
    let text = message.and_then(|m| m["text"].as_str()).or_else(|| cmd.str("text")).or_else(|| cmd.str("content")).filter(|t| !t.is_empty()).ok_or("message.dispatch needs the message text")?;
    let attachments = message_attachments(message)?;
    let message_id = message.and_then(|m| m["messageId"].as_str()).or_else(|| cmd.str("messageId")).map(str::to_string).unwrap_or_else(|| (ctx.new_id)());
    let run_id = (ctx.new_id)();
    let at = iso(ctx.now);
    let instance = shell["providerInstanceId"].as_str().unwrap_or("opencode").to_string();
    let model = shell["modelSelection"]["model"].as_str().map(str::to_string);
    let ordinal = state.entities.get(thread).map_or(0, |e| e.turn_items.len()) + 1;
    let user_item = json!({"id": format!("{run_id}:user"), "threadId": thread, "runId": run_id, "type": "user_message", "ordinal": ordinal, "status": "completed", "messageId": message_id, "text": text, "providerInstanceId": instance, "createdAt": at, "updatedAt": at});
    let mut message_entity = json!({"id": message_id, "threadId": thread, "runId": run_id, "role": "user", "text": text, "status": "completed", "providerInstanceId": instance, "createdAt": at, "updatedAt": at});
    if !attachments.is_empty() {
        message_entity["attachments"] = Value::Array(attachments.clone());
    }
    let run = json!({"id": run_id, "threadId": thread, "status": "running", "stopRequested": false, "error": null, "providerInstanceId": instance, "model": model, "requestedAt": at, "startedAt": at, "completedAt": null, "createdAt": at, "updatedAt": at});
    let t = Some(thread);
    let drafts = vec![draft("message.updated", t, None, ctx.now, message_entity), draft("turn-item.updated", t, None, ctx.now, user_item), draft("run.created", t, None, ctx.now, run)];
    let launch = Launch {
        thread_id: thread.to_string(),
        run_id,
        instance_id: instance,
        model,
        prompt: text.to_string(),
        runtime_mode: shell["runtimeMode"].as_str().unwrap_or("full-access").to_string(),
        interaction_mode: shell["interactionMode"].as_str().unwrap_or("default").to_string(),
        attachments: attachments.iter().filter_map(|a| a["id"].as_str().map(str::to_string)).collect(),
    };
    Ok(Decision { drafts, effect: Some(Effect::Launch(launch)) })
}

/// The most attachments one message carries (`PROVIDER_SEND_TURN_MAX_ATTACHMENTS`).
const MAX_ATTACHMENTS: usize = 100;

/// The attachments a message names, each as the client sent it: an object with an id of the characters the pin allows (`^[a-z0-9_-]+$`, at most 128).
pub fn message_attachments(message: Option<&Value>) -> Result<Vec<Value>, String> {
    let Some(list) = message.and_then(|m| m.get("attachments")).filter(|a| !a.is_null()) else { return Ok(vec![]) };
    let list = list.as_array().ok_or("message.attachments must be an array")?;
    check_attachments(list)?;
    Ok(list.clone())
}

pub fn check_attachments(list: &[Value]) -> Result<(), String> {
    if list.len() > MAX_ATTACHMENTS {
        return Err(format!("You can attach up to {MAX_ATTACHMENTS} files per message or question response."));
    }
    for item in list {
        let id = item["id"].as_str().unwrap_or("");
        if id.is_empty() || id.len() > 128 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
            return Err("every attachment needs an id of letters, digits, dashes and underscores".into());
        }
    }
    Ok(())
}

/// A thread moved into another project: the folder it works in became one. The event is the thread's row after the change, as a metadata
/// update carries it.
pub fn decide_adopt(state: &State, thread: &str, project: &str, ctx: &Ctx) -> Result<Decision, String> {
    let mut shell = state.threads.get(thread).cloned().ok_or_else(|| format!("thread {thread} does not exist"))?;
    if !state.active_project(project) {
        return Err(format!("project {project} does not exist"));
    }
    touched(&mut shell, &iso(ctx.now), vec![("projectId", project.into())]);
    one(row_event("thread.metadata-updated", &shell, ctx.now))
}

/// `projects.mutate` of type `project.create|update|archive`; `project.delete` is `delete_project`.
pub fn decide_project(state: &State, cmd: &Command, ctx: &Ctx) -> Result<Decision, String> {
    let (now, at) = (ctx.now, iso(ctx.now));
    let id = cmd.require("projectId")?;
    match cmd.kind.trim_start_matches("project.") {
        "create" => {
            if state.projects.contains_key(id) {
                return Err(format!("project {id} already exists"));
            }
            let mut project = project_defaults();
            for (k, v) in cmd.rest(&["projectId", "title", "createdAt"]) {
                if project.contains_key(&k) {
                    project.insert(k, v);
                }
            }
            for (k, v) in [("id", json!(id)), ("title", json!(cmd.require("title")?)), ("createdAt", json!(at)), ("updatedAt", json!(at))] {
                project.insert(k.into(), v);
            }
            one(draft("project.created", None, Some(id), now, Value::Object(project)))
        }
        _ if !state.project_exists(id) => Err(format!("project {id} does not exist")),
        "update" => {
            let patch = cmd.rest(&["projectId", "id", "createdAt", "updatedAt", "archivedAt"]);
            if patch.is_empty() {
                return Err("project.update carries no field to change".into());
            }
            one(draft("project.updated", None, Some(id), now, json!({"projectId": id, "patch": patch})))
        }
        "archive" => one(draft("project.archived", None, Some(id), now, json!({"projectId": id, "archivedAt": at}))),
        other => Err(format!("project mutation {other} is not supported")),
    }
}

/// A project with threads, active or archived, is deleted only with `force`, and then its threads go first, in one decision.
pub fn delete_project(state: &State, cmd: &Command, ctx: &Ctx) -> Result<Decision, Rejection> {
    let (id, at) = (cmd.require("projectId")?, iso(ctx.now));
    if !state.project_exists(id) {
        return Err(format!("project {id} does not exist").into());
    }
    let threads: Vec<&Map<String, Value>> = state.threads.values().filter(|t| t["projectId"] == id).collect();
    if !threads.is_empty() && cmd.body.get("force").and_then(Value::as_bool) != Some(true) {
        return Err(Rejection::ProjectNotEmpty(id.to_string()));
    }
    let mut drafts: Vec<EventDraft> = threads
        .into_iter()
        .map(|thread| {
            let mut shell = thread.clone();
            touched(&mut shell, &at, vec![("deletedAt", at.clone().into())]);
            row_event("thread.deleted", &shell, ctx.now)
        })
        .collect();
    drafts.push(draft("project.deleted", None, Some(id), ctx.now, json!({"projectId": id, "deletedAt": at})));
    Ok(Decision { drafts, effect: None })
}

pub fn decide(state: &State, family: Family, cmd: &Command, ctx: &Ctx) -> Result<Decision, Rejection> {
    match family {
        Family::Thread => decide_thread(state, cmd, ctx).map_err(Rejection::Refused),
        Family::Project if cmd.kind == "project.delete" => delete_project(state, cmd, ctx),
        Family::Project => decide_project(state, cmd, ctx).map_err(Rejection::Refused),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Completed,
    Interrupted,
    Failed(String),
}

/// What a running turn reports. Text, reasoning and tool output arrive as the pieces the agent API streams, and the thread's rows grow by them.
#[derive(Debug, Clone, PartialEq)]
pub enum RunStep {
    Text(String),
    Reasoning(String),
    ToolStarted { id: String, name: String, input: Value },
    ToolOutput { id: String, data: String },
    ToolFinished { id: String, ok: bool },
    /// The turn logged a failure; the first one is the run's error.
    Failure(String),
    Finished(Outcome),
}

impl RunStep {
    /// The step an agent event stands for; none for what t3 shows nothing of (the user's own message is the dispatch's).
    pub fn of(kind: &AgentEventKind) -> Option<RunStep> {
        Some(match kind {
            AgentEventKind::Text(delta) => RunStep::Text(delta.clone()),
            AgentEventKind::Reasoning(delta) => RunStep::Reasoning(delta.clone()),
            AgentEventKind::ToolStarted { id, name, input } => RunStep::ToolStarted { id: id.clone(), name: name.clone(), input: input.clone() },
            AgentEventKind::ToolOutput { id, data } => RunStep::ToolOutput { id: id.clone(), data: data.clone() },
            AgentEventKind::ToolFinished { id, ok } => RunStep::ToolFinished { id: id.clone(), ok: *ok },
            AgentEventKind::Failure(message) => RunStep::Failure(message.clone()),
            AgentEventKind::TurnFinished(Finish::Completed) => RunStep::Finished(Outcome::Completed),
            AgentEventKind::TurnFinished(Finish::Interrupted) => RunStep::Finished(Outcome::Interrupted),
            AgentEventKind::TurnFinished(Finish::Failed) => RunStep::Finished(Outcome::Failed(TURN_FAILED.into())),
            AgentEventKind::TurnStarted | AgentEventKind::UserText(_) | AgentEventKind::Other => return None,
        })
    }
}

/// The error of a failed turn that logged none.
pub const TURN_FAILED: &str = "the turn failed";

/// The events one report of a turn stands for. A turn that already ended records nothing more.
pub fn decide_run_step(state: &State, thread: &str, run_id: &str, step: &RunStep, ctx: &Ctx) -> Vec<EventDraft> {
    let entities = state.entities.get(thread);
    let Some(run) = entities.and_then(|e| e.runs.iter().find(|r| r["id"] == run_id)).cloned().filter(|r| r["status"] == "running") else {
        return vec![];
    };
    let at = iso(ctx.now);
    let t = Some(thread);
    let instance = run["providerInstanceId"].clone();
    let items = entities.map(|e| &e.turn_items);
    let existing = |id: &str| items.and_then(|i| i.iter().find(|x| x["id"] == id)).cloned();
    let next_ordinal = items.map_or(0, Vec::len) + 1;
    let item = |id: String, kind: &str, status: &str, extra: Value| {
        let old = existing(&id);
        let ordinal = old.as_ref().map_or(next_ordinal, |o| o["ordinal"].as_u64().unwrap_or(0) as usize);
        let created = old.as_ref().map_or(at.clone(), |o| o["createdAt"].as_str().unwrap_or("").to_string());
        let mut v = json!({"id": id, "threadId": thread, "runId": run_id, "type": kind, "ordinal": ordinal, "status": status, "providerInstanceId": instance, "createdAt": created, "updatedAt": at});
        v.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap_or_default());
        v
    };
    let grown = |id: &str, delta: &str| format!("{}{delta}", existing(id).and_then(|o| o["text"].as_str().map(str::to_string)).unwrap_or_default());
    let assistant_id = format!("{run_id}:assistant");
    let message_id = format!("{run_id}:assistant-message");
    let error_id = format!("{run_id}:error");
    let error = |message: &str| draft("turn-item.updated", t, None, ctx.now, item(error_id.clone(), "error", "completed", json!({"message": message})));
    let tool = |id: &str, status: &str, output: Value| {
        let key = format!("{run_id}:tool:{id}");
        let old = existing(&key)?;
        Some(draft("turn-item.updated", t, None, ctx.now, item(key, "command_execution", status, json!({"name": old["name"], "input": old["input"], "output": output}))))
    };
    let printed = |id: &str| existing(&format!("{run_id}:tool:{id}")).and_then(|o| o["output"].as_str().map(str::to_string));
    match step {
        RunStep::Text(delta) => vec![draft("turn-item.updated", t, None, ctx.now, item(assistant_id.clone(), "assistant_message", "streaming", json!({"text": grown(&assistant_id, delta), "messageId": message_id})))],
        RunStep::Reasoning(delta) => {
            let id = format!("{run_id}:reasoning");
            vec![draft("turn-item.updated", t, None, ctx.now, item(id.clone(), "reasoning", "streaming", json!({"text": grown(&id, delta)})))]
        }
        RunStep::ToolStarted { id, name, input } => vec![draft("turn-item.updated", t, None, ctx.now, item(format!("{run_id}:tool:{id}"), "command_execution", "running", json!({"name": name, "input": input, "output": null})))],
        RunStep::ToolOutput { id, data } => tool(id, "running", json!(format!("{}{data}", printed(id).unwrap_or_default()))).into_iter().collect(),
        RunStep::ToolFinished { id, ok } => tool(id, if *ok { "completed" } else { "failed" }, json!(printed(id).unwrap_or_default())).into_iter().collect(),
        RunStep::Failure(_) if existing(&error_id).is_some() => vec![],
        RunStep::Failure(message) => vec![error(message)],
        RunStep::Finished(outcome) => {
            let logged = existing(&error_id).and_then(|e| e["message"].as_str().map(str::to_string));
            let (status, error_text) = match outcome {
                Outcome::Completed => ("completed", Value::Null),
                Outcome::Interrupted => ("interrupted", Value::Null),
                Outcome::Failed(m) => ("failed", json!(logged.clone().unwrap_or_else(|| m.clone()))),
            };
            let mut out = vec![];
            if let Some(streaming) = existing(&assistant_id) {
                let text = streaming["text"].clone();
                out.push(draft("turn-item.updated", t, None, ctx.now, item(assistant_id, "assistant_message", "completed", json!({"text": text, "messageId": message_id}))));
                out.push(draft("message.updated", t, None, ctx.now, json!({"id": message_id, "threadId": thread, "runId": run_id, "role": "assistant", "text": text, "status": "completed", "providerInstanceId": instance, "createdAt": at, "updatedAt": at})));
            }
            if let (Outcome::Failed(m), None) = (outcome, &logged) {
                out.push(error(m));
            }
            let mut ended = run.clone();
            let fields = ended.as_object_mut().unwrap();
            for (k, v) in [("status", json!(status)), ("completedAt", json!(at)), ("updatedAt", json!(at)), ("error", error_text)] {
                fields.insert(k.into(), v);
            }
            out.push(draft("run.updated", t, None, ctx.now, ended));
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::StoredEvent;

    fn cmd(value: Value, family: Family) -> Command {
        Command::parse(&value, family).unwrap()
    }

    fn fold(state: &mut State, drafts: Vec<EventDraft>) {
        for d in drafts {
            let sequence = state.last_sequence + 1;
            state.apply(&StoredEvent { sequence, event_id: format!("e{sequence}"), kind: d.kind, thread_id: d.thread_id, project_id: d.project_id, occurred_at: d.occurred_at, payload: d.payload });
        }
    }

    fn with(state: &mut State, family: Family, value: Value) -> Result<Decision, String> {
        let counter = std::cell::Cell::new(0);
        let new_id = || {
            counter.set(counter.get() + 1);
            format!("id{}", counter.get())
        };
        let decision = decide(state, family, &cmd(value, family), &Ctx { now: 0, new_id: &new_id }).map_err(|r| r.message())?;
        fold(state, decision.drafts.clone());
        Ok(decision)
    }

    fn with_project() -> State {
        let mut state = State::default();
        with(&mut state, Family::Project, json!({"type": "project.create", "commandId": "c", "projectId": "p", "title": "P", "workspaceRoot": "/w"})).unwrap();
        state
    }

    fn shared_thread() -> State {
        let mut state = with_project();
        with(&mut state, Family::Thread, json!({"type": "thread.create", "commandId": "c", "threadId": "t", "projectId": "p"})).unwrap();
        let share = json!({"id": "s1", "host": "owner", "createdAt": "2026-11-01T00:00:00.000Z", "expiresAt": "2026-11-02T00:00:00.000Z", "guestName": "Ada", "messages": [
            {"id": "g1", "text": "hi", "status": "pending", "submittedAt": "2026-11-01T00:01:00.000Z"},
            {"id": "g2", "text": "again", "status": "sent", "submittedAt": "2026-11-01T00:02:00.000Z"}]});
        let shell = state.threads.get_mut("t").unwrap();
        shell.insert("guestShare".into(), share);
        state
    }

    #[test]
    fn dismissing_a_pending_guest_message_marks_only_it_and_one_that_is_not_pending_or_a_thread_with_no_link_is_refused() {
        // Given a thread shared with a guest who has one message waiting and one already sent
        let mut state = shared_thread();
        let dismiss = |id: &str| json!({"type": "thread.guest-message.dismiss", "commandId": format!("d-{id}"), "threadId": "t", "guestMessageId": id});
        // When the waiting message is dismissed
        with(&mut state, Family::Thread, dismiss("g1")).unwrap();
        // Then it is dismissed, the sent one is untouched and the share stays open
        let share = state.threads["t"]["guestShare"].clone();
        assert_eq!((share["messages"][0]["status"].clone(), share["messages"][1]["status"].clone(), share["guestName"].clone()), (json!("dismissed"), json!("sent"), json!("Ada")));
        // And a second dismissal, a sent message and an unknown one are each refused with the reference's message, changing nothing
        for id in ["g1", "g2", "nope"] {
            assert_eq!(with(&mut state, Family::Thread, dismiss(id)).unwrap_err(), format!("Guest message {id} is not pending."));
        }
        assert_eq!(state.threads["t"]["guestShare"], share);
        let mut plain = with_project();
        with(&mut plain, Family::Thread, json!({"type": "thread.create", "commandId": "c", "threadId": "t", "projectId": "p"})).unwrap();
        assert_eq!(with(&mut plain, Family::Thread, dismiss("g1")).unwrap_err(), "This thread has no guest link.");
    }

    #[test]
    fn closing_a_guest_share_drops_it_and_closing_a_thread_with_none_succeeds_without_a_change() {
        // Given a shared thread and one that is not shared
        let mut state = shared_thread();
        let close = |n: u32| json!({"type": "thread.guest-share.close", "commandId": format!("x{n}"), "threadId": "t"});
        // When the share is closed, and closed again
        let first = with(&mut state, Family::Thread, close(1)).unwrap();
        let again = with(&mut state, Family::Thread, close(2)).unwrap();
        // Then the first records the change and the share is gone; the second is accepted and records no row change
        assert_eq!((first.drafts[0].kind.as_str(), state.threads["t"].get("guestShare").cloned()), ("thread.guest-share-closed", None));
        assert_eq!(again.drafts[0].kind, "thread.command-accepted");
    }

    #[test]
    fn a_thread_needs_a_live_project_and_a_new_id() {
        // Given a state with one project
        let mut state = with_project();
        let create = |id: &str, project: &str| json!({"type": "thread.create", "commandId": "c", "threadId": id, "projectId": project});
        // When threads are created for a missing project, then properly, then again with the same id
        assert!(with(&mut state, Family::Thread, create("t", "missing")).is_err());
        with(&mut state, Family::Thread, create("t", "p")).unwrap();
        // Then the repeat is refused and the thread has the T3 defaults
        assert!(with(&mut state, Family::Thread, create("t", "p")).is_err());
        assert_eq!((state.threads["t"]["runtimeMode"].as_str(), state.threads["t"]["title"].as_str(), state.threads["t"]["providerInstanceId"].as_str()), (Some("full-access"), Some("New thread"), Some("opencode")));
    }

    #[test]
    fn archiving_twice_unarchiving_a_live_thread_and_unsupported_types_are_refused() {
        let mut state = with_project();
        with(&mut state, Family::Thread, json!({"type": "thread.create", "commandId": "c", "threadId": "t", "projectId": "p"})).unwrap();
        let thread = |kind: &str| json!({"type": kind, "commandId": "c", "threadId": "t"});
        assert!(with(&mut state, Family::Thread, thread("thread.unarchive")).is_err());
        with(&mut state, Family::Thread, thread("thread.archive")).unwrap();
        assert!(with(&mut state, Family::Thread, thread("thread.archive")).is_err());
        assert!(with(&mut state, Family::Thread, thread("thread.pull-request.sync")).unwrap_err().contains("not supported"));
        assert!(is_archived(&state.threads["t"]));
    }

    #[test]
    fn a_message_starts_one_run_and_a_second_waits_for_it_to_end() {
        // Given a thread with a message dispatched
        let mut state = with_project();
        with(&mut state, Family::Thread, json!({"type": "thread.create", "commandId": "c", "threadId": "t", "projectId": "p"})).unwrap();
        let send = |id: &str| json!({"type": "message.dispatch", "commandId": id, "threadId": "t", "message": {"messageId": id, "text": "hi"}});
        let first = with(&mut state, Family::Thread, send("m1")).unwrap();
        // When a second message is dispatched while the run is active
        let second = with(&mut state, Family::Thread, send("m2"));
        // Then it is refused, the first asked for a launch of the message, and the shell shows the run
        assert!(second.unwrap_err().contains("active run"));
        let Some(Effect::Launch(launch)) = first.effect else { panic!("no launch") };
        assert_eq!((launch.prompt.as_str(), launch.runtime_mode.as_str()), ("hi", "full-access"));
        assert_eq!((state.threads["t"]["status"].as_str(), state.threads["t"]["activeRunId"].as_str()), (Some("running"), Some(launch.run_id.as_str())));
    }

    /// A thread with one running run `r`, and the steps of a turn applied to it.
    fn running() -> (State, String) {
        let mut state = with_project();
        with(&mut state, Family::Thread, json!({"type": "thread.create", "commandId": "c", "threadId": "t", "projectId": "p"})).unwrap();
        let sent = with(&mut state, Family::Thread, json!({"type": "message.dispatch", "commandId": "m", "threadId": "t", "message": {"messageId": "m", "text": "hi"}})).unwrap();
        let Some(Effect::Launch(launch)) = sent.effect else { panic!("no launch") };
        (state, launch.run_id)
    }

    fn step(state: &mut State, run: &str, step: RunStep) -> usize {
        let drafts = decide_run_step(state, "t", run, &step, &Ctx { now: 0, new_id: &|| "id".to_string() });
        let written = drafts.len();
        fold(state, drafts);
        written
    }

    fn item(state: &State, run: &str, suffix: &str) -> Value {
        let id = format!("{run}:{suffix}");
        state.entities["t"].turn_items.iter().find(|i| i["id"] == id.as_str()).cloned().unwrap_or(Value::Null)
    }

    #[test]
    fn streamed_text_and_a_tools_output_grow_their_rows_and_a_tool_that_finishes_keeps_what_it_printed() {
        // Given a running run
        let (mut state, run) = running();
        // When the assistant says two things around a tool that prints twice, and the tool finishes
        for s in [
            RunStep::Text("Hel".into()),
            RunStep::ToolStarted { id: "k".into(), name: "bash".into(), input: json!({"command": "ls"}) },
            RunStep::ToolOutput { id: "k".into(), data: "a\n".into() },
            RunStep::ToolOutput { id: "k".into(), data: "b\n".into() },
            RunStep::ToolFinished { id: "k".into(), ok: false },
            RunStep::Text("lo".into()),
        ] {
            step(&mut state, &run, s);
        }
        // Then the text is one row of both pieces, and the tool row says what it ran, printed and how it ended
        assert_eq!((item(&state, &run, "assistant")["text"].as_str(), item(&state, &run, "assistant")["status"].as_str()), (Some("Hello"), Some("streaming")));
        let tool = item(&state, &run, "tool:k");
        assert_eq!((tool["name"].as_str(), tool["output"].as_str(), tool["status"].as_str()), (Some("bash"), Some("a\nb\n"), Some("failed")));
        assert_eq!(tool["input"], json!({"command": "ls"}));
        // And the rows keep the order their first piece came in
        assert_eq!((item(&state, &run, "assistant")["ordinal"].as_u64(), tool["ordinal"].as_u64()), (Some(2), Some(3)));
    }

    #[test]
    fn the_first_failure_a_turn_logs_is_the_runs_error_and_a_failed_turn_that_logged_none_gets_a_plain_one() {
        // Given two running runs
        let (mut logged, run) = running();
        let (mut bare, bare_run) = running();
        // When the first logs two failures and fails, and the second fails with none
        for s in [RunStep::Failure("rate limited".into()), RunStep::Failure("second".into()), RunStep::Finished(Outcome::Failed("the turn failed".into()))] {
            step(&mut logged, &run, s);
        }
        step(&mut bare, &bare_run, RunStep::Finished(Outcome::Failed("the turn failed".into())));
        // Then the run's error is the first failure, which is the only error row, and the bare one says only that the turn failed
        let error = |state: &State, run: &str| state.entities["t"].runs.iter().find(|r| r["id"] == run).unwrap()["error"].as_str().map(str::to_string);
        assert_eq!(error(&logged, &run).as_deref(), Some("rate limited"));
        assert_eq!(logged.entities["t"].turn_items.iter().filter(|i| i["type"] == "error").count(), 1);
        assert_eq!((error(&bare, &bare_run).as_deref(), item(&bare, &bare_run, "error")["message"].as_str()), (Some("the turn failed"), Some("the turn failed")));
    }

    #[test]
    fn a_step_of_a_run_that_ended_or_for_a_tool_that_never_started_records_nothing() {
        // Given a run that completed, and another that is running
        let (mut ended, run) = running();
        step(&mut ended, &run, RunStep::Finished(Outcome::Completed));
        let (mut open, open_run) = running();
        // When more of the ended run arrives, and output and an end for a tool that never started arrive for the other
        let late = [step(&mut ended, &run, RunStep::Text("late".into())), step(&mut ended, &run, RunStep::Finished(Outcome::Interrupted))];
        let orphan = [step(&mut open, &open_run, RunStep::ToolOutput { id: "x".into(), data: "?".into() }), step(&mut open, &open_run, RunStep::ToolFinished { id: "x".into(), ok: true })];
        // Then nothing was written for any, and the ended run is still completed
        assert_eq!((late, orphan), ([0, 0], [0, 0]));
        assert_eq!(ended.entities["t"].runs[0]["status"], "completed");
    }

    #[test]
    fn what_an_agent_event_stands_for_is_its_text_tool_and_end_and_not_the_users_own_message_or_what_t3_shows_nothing_of() {
        // Given one agent event of each kind
        use crate::ports::AgentEventKind as K;
        let steps = |kind: K| RunStep::of(&kind);
        // When each is read as a step, Then the shown ones are steps and the others are none, and each end says how the turn ended
        assert_eq!(steps(K::Text("a".into())), Some(RunStep::Text("a".into())));
        assert_eq!(steps(K::ToolFinished { id: "k".into(), ok: true }), Some(RunStep::ToolFinished { id: "k".into(), ok: true }));
        assert_eq!(steps(K::TurnFinished(Finish::Interrupted)), Some(RunStep::Finished(Outcome::Interrupted)));
        assert_eq!(steps(K::TurnFinished(Finish::Failed)), Some(RunStep::Finished(Outcome::Failed(TURN_FAILED.into()))));
        assert!([K::TurnStarted, K::UserText("hi".into()), K::Other].into_iter().all(|k| steps(k).is_none()));
    }

    fn thread_with(state: &mut State, value: Value) -> Result<Decision, String> {
        with(state, Family::Thread, value)
    }

    fn live() -> State {
        let mut state = with_project();
        thread_with(&mut state, json!({"type": "thread.create", "commandId": "c", "threadId": "t", "projectId": "p"})).unwrap();
        state
    }

    #[test]
    fn settling_snoozing_and_bringing_back_move_the_rows_the_way_t3_reads_them() {
        // Given a pinned thread
        let mut state = live();
        let send = |state: &mut State, kind: &str, extra: Value| {
            let mut c = json!({"type": kind, "commandId": "c", "threadId": "t"});
            c.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap());
            thread_with(state, c).unwrap();
        };
        send(&mut state, "thread.pin", json!({}));
        // When it is settled, snoozed until a time, woken, and then unsettled
        send(&mut state, "thread.settle", json!({}));
        let settled = state.threads["t"].clone();
        send(&mut state, "thread.snooze", json!({"snoozedUntil": "2030-01-01T00:00:00.000Z"}));
        let snoozed = state.threads["t"].clone();
        send(&mut state, "thread.unsnooze", json!({"reason": "user"}));
        send(&mut state, "thread.unsettle", json!({"reason": "user"}));
        // Then settling unpinned it and set the override, the snooze names its time, and unsettling kept the thread out of automatic settling
        assert_eq!((settled["settledOverride"].as_str(), settled["pinnedAt"].is_null(), settled["settledAt"].is_null()), (Some("settled"), true, false));
        assert_eq!(snoozed["snoozedUntil"], "2030-01-01T00:00:00.000Z");
        let end = &state.threads["t"];
        assert_eq!((end["settledOverride"].as_str(), end["settledAt"].is_null(), end["snoozedUntil"].is_null(), end["unsettledAt"].is_null()), (Some("active"), true, true, false));
    }

    #[test]
    fn a_fork_is_a_new_thread_in_its_sources_family_and_a_second_fork_to_the_same_id_is_refused() {
        // Given a thread that is settled
        let mut state = live();
        thread_with(&mut state, json!({"type": "thread.settle", "commandId": "c", "threadId": "t"})).unwrap();
        let fork = json!({"type": "thread.fork", "commandId": "c", "sourceThreadId": "t", "targetThreadId": "f", "sourcePoint": {}});
        // When it is forked and forked again to the same id
        thread_with(&mut state, fork.clone()).unwrap();
        let again = thread_with(&mut state, fork);
        // Then the fork is a fresh thread titled after its source with the source as parent and root, and the repeat is refused
        let f = &state.threads["f"];
        assert_eq!((f["title"].as_str(), f["lineage"]["parentThreadId"].as_str(), f["lineage"]["rootThreadId"].as_str(), f["settledOverride"].is_null()), (Some("New thread fork"), Some("t"), Some("t"), true));
        assert!(again.unwrap_err().contains("already exists"));
    }

    #[test]
    fn a_command_the_agent_api_does_the_work_of_leaves_only_its_acceptance_and_the_rows_unchanged() {
        // Given a thread
        let mut state = live();
        let before = state.threads["t"].clone();
        // When each of the accepted-only commands is decided
        for kind in ACCEPTED_ONLY {
            let decision = thread_with(&mut state, json!({"type": kind, "commandId": "c", "threadId": "t"})).unwrap();
            // Then one acceptance event is recorded and the shell row is as it was
            assert_eq!(decision.drafts.iter().map(|d| d.kind.as_str()).collect::<Vec<_>>(), ["thread.command-accepted"], "{kind}");
        }
        assert_eq!(state.threads["t"], before);
    }
}
