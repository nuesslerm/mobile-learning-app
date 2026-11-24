//! Commands in, events out, decided against the current state; pure. A thread event carries the thread row after the change (the
//! recorded shape), so folding an event is a merge. Run steps (what a provider turn reports) are decided here too.
use super::command::{Command, Family, ACCEPTED_ONLY};
use super::state::{is_archived, State};
use crate::ports::{AgentEventKind, EventDraft, Finish, Millis, RuntimeRequestEvent, RuntimeResponse};
use serde_json::{json, Map, Value};
use t3_contracts::rows::{project_defaults, thread_defaults, thread_event_row};
use t3_contracts::runs;
use t3_contracts::time::iso;

/// What the engine does after the events are stored.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Launch(Launch),
    /// A message that waits in the agent API's queue behind the running turn.
    Queue(Launch),
    /// `hold` keeps the queue from starting by itself after the stop.
    Interrupt { thread_id: String, hold: bool },
    /// Follows the queued turn the agent API starts: the one of `run_id`, else whichever starts next.
    Follow { thread_id: String, run_id: Option<String> },
    /// Prepares the workspace the preparing run `run_id` waits for (its worktree and setup), then releases or fails the run.
    Prepare { thread_id: String, run_id: String },
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
    /// The worktree the thread works in, when it has one.
    pub worktree: Option<String>,
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
    /// A command of a thread the read model does not hold.
    ThreadMissing(String),
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
            Rejection::ThreadMissing(thread) => format!("No orchestration projection exists for thread {thread}."),
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
pub const SUPPORTED: [&str; 33] = [
    "thread.guest-share.open",
    "thread.guest-share.redeem",
    "thread.guest-message.submit",
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
    "prepared-run.fail",
    "prepared-run.progress",
    "prepared-run.release",
    "prepared-run.retry",
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

/// What a guest is told (`GUEST_FAILURE` of the reference) and the most messages waiting at once.
pub const GUEST_INACTIVE: &str = "This guest link is no longer active.";
pub const GUEST_USED: &str = "This guest link was already used.";
pub const GUEST_BUSY: &str = "Wait for the owner to answer your earlier messages.";
pub const GUEST_PENDING_LIMIT: usize = 3;

/// The thread's share when it is the one the command names and has not expired.
fn live_share(shell: &Map<String, Value>, cmd: &Command, now: Millis) -> Result<Value, String> {
    let share = shell.get("guestShare").filter(|s| s.is_object()).ok_or(GUEST_INACTIVE)?;
    let expired = share["expiresAt"].as_str().and_then(t3_contracts::time::parse_iso).is_none_or(|at| now >= at);
    if share["id"] != cmd.require("shareId")? || expired {
        return Err(GUEST_INACTIVE.into());
    }
    Ok(share.clone())
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
                return one(draft("thread.command-accepted", Some(id), shell["projectId"].as_str(), now, json!({"commandType": cmd.kind})));
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
        "thread.mark-unread" if shell.get("latestRunCompletedAt").is_none_or(Value::is_null) => Err(format!("Thread {id} has no completed run to mark unread.")),
        "thread.mark-unread" => {
            touched(&mut shell, &at, vec![("lastVisitedAt", Value::Null)]);
            event("thread.marked-unread", &shell)
        }
        "thread.pin.reorder" => {
            touched(&mut shell, &at, vec![("pinOrderKey", cmd.require("orderKey")?.into())]);
            marked("thread.pin-reordered", &shell, &["pinOrderKey"], now)
        }
        "thread.user-input.dismiss" => one(draft("thread.command-accepted", Some(id), shell["projectId"].as_str(), now, json!({"commandType": cmd.kind}))),
        "thread.stop" if active_run(state, id).is_none() => stop_unopened(state, id, cmd.str("reason"), ctx).map_or_else(|| one(draft("thread.command-accepted", Some(id), shell["projectId"].as_str(), now, json!({"commandType": cmd.kind}))), Ok),
        "thread.stop" => interrupt(state, id, None, cmd.str("reason"), true, ctx),
        "run.interrupt" => interrupt(state, id, cmd.str("runId"), cmd.str("reason"), cmd.body.get("holdQueue") == Some(&json!(true)), ctx),
        "queued-run.start" if active_run(state, id).is_some() || !queued_may_start(state, id) => Err("A queued user message can only be sent now while the thread is stopped.".into()),
        "queued-run.edit" | "queued-run.cancel" | "queued-run.reorder" | "queued-run.start" if queued_run(state, id, cmd.str("runId")).is_some() => queue_command(state, id, cmd, ctx),
        "prepared-run.fail" | "prepared-run.progress" | "prepared-run.release" | "prepared-run.retry" => prepared_run(state, id, &shell, cmd, ctx),
        "queue.resume" => Ok(Decision { drafts: vec![draft("thread.command-accepted", Some(id), shell["projectId"].as_str(), now, json!({"commandType": cmd.kind}))], effect: Some(Effect::Follow { thread_id: id.to_string(), run_id: None }) }),
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
            let visited = match cmd.str("visitedAt") {
                Some(text) if t3_contracts::time::parse_iso(text).is_none() => return Err(format!("Thread {id} visit time {text} is not a valid timestamp.")),
                Some(text) => text.to_string(),
                None => at.clone(),
            };
            touched(&mut shell, &at, vec![("lastVisitedAt", visited.into())]);
            event("thread.visited", &shell)
        }
        "thread.guest-share.open" if archived => Err("Archived threads cannot be shared.".into()),
        "thread.guest-share.open" => {
            let share = json!({"id": cmd.require("shareId")?, "host": cmd.require("host")?, "createdAt": at, "expiresAt": cmd.require("expiresAt")?, "guestName": null, "messages": []});
            touched(&mut shell, &at, vec![("guestShare", share)]);
            event("thread.guest-share-opened", &shell)
        }
        "thread.guest-share.redeem" => {
            let mut share = live_share(&shell, cmd, now)?;
            if !share["guestName"].is_null() {
                return Err(GUEST_USED.into());
            }
            share["guestName"] = cmd.require("guestName")?.into();
            touched(&mut shell, &at, vec![("guestShare", share)]);
            event("thread.guest-share-redeemed", &shell)
        }
        "thread.guest-message.submit" => {
            let mut share = live_share(&shell, cmd, now)?;
            if share["guestName"].is_null() {
                return Err(GUEST_INACTIVE.into());
            }
            let messages = share["messages"].as_array_mut().ok_or(GUEST_INACTIVE)?;
            if messages.iter().filter(|m| m["status"] == "pending").count() >= GUEST_PENDING_LIMIT {
                return Err(GUEST_BUSY.into());
            }
            messages.push(json!({"id": cmd.require("guestMessageId")?, "text": cmd.str("text").unwrap_or_default(), "status": "pending", "submittedAt": at}));
            touched(&mut shell, &at, vec![("guestShare", share)]);
            event("thread.guest-message-submitted", &shell)
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
        "message.dispatch" => dispatch_message(state, cmd, &shell, ctx),
        other => Err(format!("command type {other} is not supported by this server")),
    }
}

/// A queued message can be sent now only while the thread is stopped: a thread that has run ended by a stop, which a failed or finished run is not.
/// Whether a running turn refuses it is the agent API's to say, in the same words, so a running thread passes here.
pub fn queued_may_start(state: &State, thread: &str) -> bool {
    let last = state.entities.get(thread).and_then(|e| e.runs.iter().rev().find(|r| r["status"] != "queued"));
    active_run(state, thread).is_some() || last.is_none_or(|r| r["status"] == "interrupted")
}

fn active_run(state: &State, thread: &str) -> Option<Value> {
    state.entities.get(thread)?.runs.iter().rev().find(|r| r["status"] == "running").cloned()
}

fn run_named(state: &State, thread: &str, run: &str) -> Option<Value> {
    state.entities.get(thread)?.runs.iter().find(|r| r["id"] == run).cloned()
}

fn queued_run(state: &State, thread: &str, run: Option<&str>) -> Option<Value> {
    run_named(state, thread, run?).filter(|r| r["status"] == "queued")
}

/// The queued runs, first to start first.
pub fn queued_runs(state: &State, thread: &str) -> Vec<Value> {
    let mut queued: Vec<Value> = state.entities.get(thread).map(|e| e.runs.iter().filter(|r| r["status"] == "queued").cloned().collect()).unwrap_or_default();
    queued.sort_by(|a, b| a["queueRank"].as_f64().partial_cmp(&b["queueRank"].as_f64()).unwrap_or(std::cmp::Ordering::Equal));
    queued
}

pub fn queue_hold_events(state: &State, thread: &str, held: bool, now: Millis) -> Vec<EventDraft> {
    queued_runs(state, thread).into_iter().filter(|run| run["queueHeld"].as_bool().unwrap_or(false) != held).map(|mut run| {
        run["queueHeld"] = held.into();
        run["updatedAt"] = iso(now).into();
        draft("run.updated", Some(thread), None, now, run)
    }).collect()
}

/// Whether the provider has begun the run's turn: it has shown something of its own, which a hung or not yet answering turn has not.
pub fn provider_turn_running(state: &State, thread: &str, run: &str) -> bool {
    state.entities.get(thread).is_some_and(|e| e.turn_items.iter().any(|i| i["runId"] == run && !matches!(i["type"].as_str(), Some("user_message" | "run_interrupt_request" | "run_interrupt_result"))))
}

fn thread_provider_thread(state: &State, thread: &str, instance: &str, fallback_run: &str) -> String {
    let first = state.entities.get(thread).and_then(|e| e.runs.first()).and_then(|r| r["id"].as_str().map(str::to_string)).unwrap_or_else(|| fallback_run.to_string());
    runs::provider_thread_id(instance, &first)
}

/// A turn item of a run that the run's own state writes: the shape every such row shares.
#[allow(clippy::too_many_arguments)]
fn signal_item(thread: &str, run: &Value, provider_thread: &str, id: String, ordinal: u64, parent: Value, status: &str, title: &str, kind: &str, message: &str, at: &str) -> Value {
    let run_id = run["id"].as_str().unwrap_or("");
    json!({"completedAt": at, "id": id, "message": message, "nativeItemRef": null, "nodeId": runs::node_id(run_id), "ordinal": ordinal, "parentItemId": parent, "providerThreadId": provider_thread, "providerTurnId": null, "runId": run_id, "startedAt": at, "status": status, "threadId": thread, "title": title, "type": kind, "updatedAt": at})
}

fn items_of(state: &State, thread: &str, run: &str) -> u64 {
    state.entities.get(thread).map_or(0, |e| e.turn_items.iter().filter(|i| i["runId"] == run).count() as u64)
}

/// `thread.stop` of a thread whose last run never opened its provider: the pin still records the stop request on that run, which stays failed.
fn stop_unopened(state: &State, thread: &str, reason: Option<&str>, ctx: &Ctx) -> Option<Decision> {
    let run = state.entities.get(thread)?.runs.last().filter(|r| r["openFailed"] == true)?.clone();
    let run_id = run["id"].as_str().unwrap_or("");
    let pt = thread_provider_thread(state, thread, run["providerInstanceId"].as_str().unwrap_or(""), run_id);
    let nth = items_of(state, thread, run_id) + 1;
    let request = signal_item(thread, &run, &pt, runs::signal_item_id(run_id, "interrupt-request"), runs::item_ordinal(run["ordinal"].as_u64().unwrap_or(0), nth), Value::Null, "completed", "Interrupt requested", "run_interrupt_request", reason.unwrap_or("Stopped by user"), &iso(ctx.now));
    Some(Decision { drafts: vec![draft("turn-item.updated", Some(thread), None, ctx.now, request)], effect: Some(Effect::Interrupt { thread_id: thread.to_string(), hold: true }) })
}

/// `run.interrupt` and `thread.stop` (which holds the queue). A run whose provider turn has not begun ends at once, with the two signal rows the pin
/// writes; one that has is told to stop and ends when the turn does.
fn interrupt(state: &State, thread: &str, named: Option<&str>, reason: Option<&str>, hold: bool, ctx: &Ctx) -> Result<Decision, String> {
    let mut run = match named {
        Some(id) if run_named(state, thread, id).is_some() => run_named(state, thread, id).filter(|r| r["status"] == "running").ok_or_else(|| format!("Run {id} is not interruptible."))?,
        _ => active_run(state, thread).ok_or_else(|| format!("thread {thread} has no active run"))?,
    };
    let (at, t) = (iso(ctx.now), Some(thread));
    let run_id = run["id"].as_str().unwrap_or("").to_string();
    let instance = run["providerInstanceId"].as_str().unwrap_or("").to_string();
    let pt = thread_provider_thread(state, thread, &instance, &run_id);
    let nth = items_of(state, thread, &run_id);
    let ordinal = run["ordinal"].as_u64().unwrap_or(0);
    let request_id = runs::signal_item_id(&run_id, "interrupt-request");
    let request = signal_item(thread, &run, &pt, request_id.clone(), runs::item_ordinal(ordinal, nth + 1), Value::Null, "completed", "Interrupt requested", "run_interrupt_request", reason.unwrap_or("Stopped by user"), &at);
    let mut drafts = vec![draft("turn-item.updated", t, None, ctx.now, request)];
    run["stopRequested"] = Value::Bool(true);
    run["updatedAt"] = at.clone().into();
    if !provider_turn_running(state, thread, &run_id) {
        let result = signal_item(thread, &run, &pt, runs::signal_item_id(&run_id, "interrupt-result"), runs::item_ordinal(ordinal, nth + 2), request_id.into(), "interrupted", "Interrupted", "run_interrupt_result", "Run interrupted before provider start", &at);
        drafts.push(draft("turn-item.updated", t, None, ctx.now, result));
        run["status"] = "interrupted".into();
        run["completedAt"] = at.into();
    }
    drafts.push(draft("run.updated", t, None, ctx.now, run));
    Ok(Decision { drafts, effect: Some(Effect::Interrupt { thread_id: thread.to_string(), hold }) })
}

fn dispatch_mode(cmd: &Command) -> &str {
    match cmd.body.get("dispatchMode") {
        Some(Value::String(mode)) => mode,
        Some(mode) => mode["type"].as_str().unwrap_or("start_immediately"),
        None => "start_immediately",
    }
}

fn dispatch_message(state: &State, cmd: &Command, shell: &Map<String, Value>, ctx: &Ctx) -> Result<Decision, String> {
    let thread = shell["id"].as_str().unwrap_or("");
    if is_archived(shell) {
        return Err(format!("thread {thread} is archived"));
    }
    let active = active_run(state, thread);
    let requested = dispatch_mode(cmd);
    let mode = if requested == "start_immediately" && cmd.str("deliveryIntent") == Some("auto") && active.as_ref().is_some_and(|run| !provider_turn_running(state, thread, run["id"].as_str().unwrap_or(""))) {
        "queue_after_active"
    } else {
        requested
    };
    if !["start_immediately", "queue_after_active", "defer_start"].contains(&mode) {
        return Err(format!("dispatch mode {mode} is not supported by this server"));
    }
    let active = active.is_some();
    if active && mode != "queue_after_active" {
        return Err(format!("thread {thread} already has an active run"));
    }
    let message = cmd.body.get("message").filter(|m| m.is_object());
    let text = message.and_then(|m| m["text"].as_str()).or_else(|| cmd.str("text")).or_else(|| cmd.str("content")).filter(|t| !t.is_empty()).ok_or("message.dispatch needs the message text")?;
    let attachments = message_attachments(message.or(Some(&Value::Object(cmd.body.clone()))))?;
    let message_id = message.and_then(|m| m["messageId"].as_str()).or_else(|| cmd.str("messageId")).map(str::to_string).unwrap_or_else(|| (ctx.new_id)());
    let existing = state.entities.get(thread).map(|e| e.runs.as_slice()).unwrap_or(&[]);
    let ordinal = existing.len() as u64 + 1;
    let run_id = runs::run_id(thread, ordinal);
    let at = iso(ctx.now);
    let instance = shell["providerInstanceId"].as_str().unwrap_or("opencode").to_string();
    let model = shell["modelSelection"]["model"].as_str().map(str::to_string);
    let (created_by, source) = (cmd.str("createdBy").unwrap_or("user"), cmd.str("creationSource").unwrap_or("web"));
    let mut message_entity = json!({"attachments": attachments, "createdAt": at, "createdBy": created_by, "creationSource": source, "id": message_id, "nodeId": runs::node_id(&run_id), "role": "user", "runId": run_id, "streaming": false, "text": text, "threadId": thread, "updatedAt": at});
    if let Some(context) = message.and_then(|m| m.get("context")).or_else(|| cmd.body.get("context")).and_then(composer_context) {
        message_entity["context"] = context;
    }
    let (native, session) = existing.first().map_or_else(|| ((ctx.new_id)(), (ctx.new_id)()), |first| (first["nativeThreadId"].clone().as_str().unwrap_or("").to_string(), first["sessionUuid"].as_str().unwrap_or("").to_string()));
    let queued = active;
    let preparing = mode == "defer_start";
    let rank = existing.iter().filter_map(|r| r["queueRank"].as_f64()).fold(0.0, f64::max) + 1.0;
    let run = json!({
        "id": run_id, "threadId": thread, "ordinal": ordinal, "status": if queued { "queued" } else if preparing { "preparing" } else { "running" }, "userMessageId": message_id, "stopRequested": false, "error": null,
        "providerInstanceId": instance, "modelSelection": shell["modelSelection"], "requestedAt": at, "startedAt": if queued || preparing { Value::Null } else { at.clone().into() }, "completedAt": null,
        "nativeThreadId": native, "sessionUuid": session, "queueRank": if queued { json!(rank) } else { Value::Null }, "updatedAt": at,
    });
    let mut run = run;
    // A launch that makes its own worktree hands the strategy to the run: the workspace is prepared for it, and again when a retry asks
    let preparation = cmd.body.get("dispatchMode").and_then(|m| m.get("workspaceStrategy")).filter(|w| preparing && w["type"] == "worktree").cloned();
    if let Some(strategy) = &preparation {
        run["workspacePreparation"] = strategy.clone();
    }
    let t = Some(thread);
    let mut drafts = Vec::new();
    if !queued {
        let pt = thread_provider_thread(state, thread, &instance, &run_id);
        let mut item = user_item(thread, &run, &pt, &message_entity, "turn_start", &at);
        let earlier = state.entities.get(thread).and_then(|e| e.turn_items.iter().find(|i| i["id"] == item["id"])).map(|i| i["ordinal"].clone());
        if let Some(ordinal) = &earlier {
            item["ordinal"] = ordinal.clone();
        }
        drafts.push(draft("turn-item.updated", t, None, ctx.now, item));
        if preparing {
            let nth = items_of(state, thread, &run_id) + u64::from(earlier.is_none()) + 1;
            let id = format!("turn-item:provider:{instance}:native-item:workspace-preparation{}", runs::enc(&format!(":{run_id}")));
            let row = json!({"completedAt": null, "id": id, "input": "Preparing workspace", "nativeItemRef": null, "nodeId": runs::node_id(&run_id), "ordinal": runs::item_ordinal(ordinal, nth), "parentItemId": null, "providerThreadId": pt, "providerTurnId": null, "runId": run_id, "startedAt": at, "status": "running", "threadId": thread, "title": "Preparing workspace", "type": "command_execution", "updatedAt": at});
            drafts.push(draft("turn-item.updated", t, None, ctx.now, row));
        }
    }
    drafts.insert(0, draft("message.updated", t, None, ctx.now, message_entity));
    drafts.push(draft("run.created", t, None, ctx.now, run));
    let launch = Launch {
        thread_id: thread.to_string(),
        run_id: run_id.clone(),
        instance_id: instance,
        model,
        prompt: text.to_string(),
        runtime_mode: shell["runtimeMode"].as_str().unwrap_or("full-access").to_string(),
        interaction_mode: shell["interactionMode"].as_str().unwrap_or("default").to_string(),
        attachments: attachments.iter().filter_map(|a| a["id"].as_str().map(str::to_string)).collect(),
        worktree: shell["worktreePath"].as_str().filter(|p| !p.is_empty()).map(str::to_string),
    };
    let effect = if queued {
        Some(Effect::Queue(launch))
    } else if preparing {
        preparation.map(|_| Effect::Prepare { thread_id: thread.to_string(), run_id: run_id.clone() })
    } else {
        Some(Effect::Launch(launch))
    };
    Ok(Decision { drafts, effect })
}

fn composer_context(value: &Value) -> Option<Value> {
    let records = value["records"].as_array()?;
    // Guest attribution is assigned by the host.
    (value["version"] == 1 && records.len() <= 200).then(|| json!({"version": 1, "records": records}))
}

fn user_item(thread: &str, run: &Value, provider_thread: &str, message: &Value, intent: &str, at: &str) -> Value {
    let run_id = run["id"].as_str().unwrap_or("");
    let message_id = message["id"].as_str().unwrap_or("");
    let attachments = message["attachments"].as_array().map(Vec::as_slice).unwrap_or_default();
    let mut item = json!({
        "attachments": attachments, "completedAt": at, "createdBy": message["createdBy"].as_str().unwrap_or("user"), "creationSource": message["creationSource"].as_str().unwrap_or("web"),
        "id": runs::message_item_id(message_id), "inputIntent": intent, "messageId": message_id,
        "nativeItemRef": null, "nodeId": runs::node_id(run_id), "ordinal": runs::item_ordinal(run["ordinal"].as_u64().unwrap_or(0), 1), "parentItemId": null, "providerThreadId": provider_thread,
        "providerTurnId": null, "runId": run_id, "startedAt": at, "status": "completed", "text": message["text"].as_str().unwrap_or(""), "threadId": thread, "title": null, "type": "user_message", "updatedAt": at,
    });
    if let Some(context) = message.get("context") {
        item["context"] = context.clone();
    }
    item
}

/// The title the workspace row of a preparing run carries; it is also how the row is found again.
const PREPARING: &str = "Preparing workspace";
/// The code of the failure a workspace preparation leaves on its run: a retry looks for it.
pub const PREPARATION_FAILED: &str = "workspace_preparation_failed";

fn preparation_item(state: &State, thread: &str, run: &str) -> Option<Value> {
    state.entities.get(thread)?.turn_items.iter().find(|i| i["runId"] == run && i["type"] == "command_execution" && i["input"] == PREPARING).cloned()
}

fn failure_item(state: &State, thread: &str, run: &str) -> Option<Value> {
    state.entities.get(thread)?.turn_items.iter().find(|i| i["runId"] == run && i["type"] == "error" && i["failure"]["code"] == PREPARATION_FAILED && i["status"] == "failed").cloned()
}

/// `prepared-run.progress|release|fail|retry`, after the pin's `Orchestrator`: the run waits for its workspace (`preparing`), the worktree and the setup
/// script are the service's work, and each of them reports here. A release starts the run's turn; a failure leaves the run failed with a workspace row and
/// an error row, which a retry takes back before it asks for the preparation again.
fn prepared_run(state: &State, thread: &str, shell: &Map<String, Value>, cmd: &Command, ctx: &Ctx) -> Result<Decision, String> {
    let run_id = cmd.require("runId")?;
    let wanted = if cmd.kind == "prepared-run.retry" { "failed" } else { "preparing" };
    let not_waiting = || format!("Run {run_id} is not awaiting workspace preparation.");
    let run = run_named(state, thread, run_id).filter(|r| r["status"] == wanted).ok_or_else(|| if cmd.kind == "prepared-run.retry" { format!("Run {run_id} has no failed workspace preparation to retry.") } else { not_waiting() })?;
    let item = preparation_item(state, thread, run_id).ok_or_else(|| if cmd.kind == "prepared-run.retry" { format!("Run {run_id} has no failed workspace preparation to retry.") } else { not_waiting() })?;
    let (at, t) = (iso(ctx.now), Some(thread));
    let changed = |mut row: Value, patch: Vec<(&str, Value)>| {
        for (k, v) in patch {
            row[k] = v;
        }
        row["updatedAt"] = at.clone().into();
        row
    };
    match cmd.kind.as_str() {
        "prepared-run.progress" => {
            let title = match cmd.str("phase") {
                Some("worktree") => "Preparing worktree",
                Some("setup") => "Starting setup script",
                _ => return Err("prepared-run.progress needs a phase of worktree or setup".into()),
            };
            Ok(Decision { drafts: vec![draft("turn-item.updated", t, None, ctx.now, changed(item, vec![("title", title.into())]))], effect: None })
        }
        "prepared-run.release" => {
            let done = changed(item, vec![("status", "completed".into()), ("title", "Workspace ready".into()), ("output", "Workspace preparation completed.".into()), ("exitCode", 0.into()), ("completedAt", at.clone().into())]);
            let started = changed(run.clone(), vec![("status", "running".into()), ("startedAt", at.clone().into())]);
            let message = state.entities.get(thread).and_then(|e| e.messages.iter().find(|m| m["id"] == run["userMessageId"])).cloned().unwrap_or(Value::Null);
            let launch = Launch {
                thread_id: thread.to_string(),
                run_id: run_id.to_string(),
                instance_id: run["providerInstanceId"].as_str().unwrap_or_default().to_string(),
                model: run["modelSelection"]["model"].as_str().map(str::to_string),
                prompt: message["text"].as_str().unwrap_or_default().to_string(),
                runtime_mode: shell["runtimeMode"].as_str().unwrap_or("full-access").to_string(),
                interaction_mode: shell["interactionMode"].as_str().unwrap_or("default").to_string(),
                attachments: message["attachments"].as_array().into_iter().flatten().filter_map(|a| a["id"].as_str().map(str::to_string)).collect(),
                worktree: shell["worktreePath"].as_str().filter(|p| !p.is_empty()).map(str::to_string),
            };
            Ok(Decision { drafts: vec![draft("turn-item.updated", t, None, ctx.now, done), draft("run.updated", t, None, ctx.now, started)], effect: Some(Effect::Launch(launch)) })
        }
        "prepared-run.fail" => {
            let failure = cmd.body.get("failure").filter(|f| f.is_object()).ok_or("prepared-run.fail needs a failure")?;
            let message = failure["message"].as_str().unwrap_or("Workspace preparation failed.");
            let instance = run["providerInstanceId"].as_str().unwrap_or_default();
            let ended = changed(item.clone(), vec![("status", "failed".into()), ("title", "Workspace preparation failed".into()), ("output", message.into()), ("exitCode", 1.into()), ("completedAt", at.clone().into())]);
            let nth = items_of(state, thread, run_id) + 1;
            let mut error = failure.clone();
            error["code"] = PREPARATION_FAILED.into();
            let row = json!({
                "completedAt": at, "failure": error, "id": format!("turn-item:provider:{instance}:native-item:workspace-preparation-failure{}", runs::enc(&format!(":{run_id}"))), "nativeItemRef": null,
                "nodeId": item["nodeId"], "ordinal": runs::item_ordinal(run["ordinal"].as_u64().unwrap_or(0), nth), "parentItemId": null, "providerThreadId": item["providerThreadId"], "providerTurnId": null,
                "runId": run_id, "startedAt": at, "status": "failed", "threadId": thread, "title": "Workspace preparation failed", "type": "error", "updatedAt": at,
            });
            let failed = changed(run, vec![("status", "failed".into()), ("completedAt", at.clone().into())]);
            Ok(Decision { drafts: vec![draft("turn-item.updated", t, None, ctx.now, ended), draft("turn-item.updated", t, None, ctx.now, row), draft("run.updated", t, None, ctx.now, failed)], effect: None })
        }
        _ => {
            let failed_item = failure_item(state, thread, run_id).filter(|_| run["workspacePreparation"].is_object() && !is_archived(shell)).ok_or_else(|| format!("Run {run_id} has no failed workspace preparation to retry."))?;
            if active_run(state, thread).is_some() || state.entities.get(thread).is_some_and(|e| e.runs.iter().any(|r| r["id"] != run["id"] && matches!(r["status"].as_str(), Some("preparing" | "starting")))) {
                return Err("Another run is active on this thread.".into());
            }
            let mut again = changed(item, vec![("status", "running".into()), ("title", PREPARING.into()), ("completedAt", Value::Null)]);
            if let Some(row) = again.as_object_mut() {
                row.remove("output");
                row.remove("exitCode");
            }
            let drafts = vec![
                draft("turn-item.updated", t, None, ctx.now, changed(failed_item, vec![("status", "cancelled".into())])),
                draft("turn-item.updated", t, None, ctx.now, again),
                draft("run.updated", t, None, ctx.now, changed(run, vec![("status", "preparing".into()), ("completedAt", Value::Null)])),
            ];
            Ok(Decision { drafts, effect: Some(Effect::Prepare { thread_id: thread.to_string(), run_id: run_id.to_string() }) })
        }
    }
}

/// A queued run that left the queue: its turn began, so its user message is now a row of the timeline. A run that follows an interrupted one has the
/// earlier runs handed over to it.
fn start_queued(state: &State, thread: &str, run: &Value, held: Option<bool>, ctx: &Ctx) -> Vec<EventDraft> {
    let at = iso(ctx.now);
    let run_id = run["id"].as_str().unwrap_or("");
    let ordinal = run["ordinal"].as_u64().unwrap_or(0);
    let entities = state.entities.get(thread);
    let message = entities.and_then(|e| e.messages.iter().find(|m| m["id"] == run["userMessageId"])).cloned().unwrap_or(Value::Null);
    let mut started = run.clone();
    started["status"] = "running".into();
    started["startedAt"] = at.clone().into();
    started["queueRank"] = Value::Null;
    started["updatedAt"] = at.clone().into();
    if let Some(held) = held {
        started["queueHeld"] = held.into();
    }
    let previous = entities.map_or(&[][..], |e| e.runs.as_slice()).iter().rfind(|r| runs::is_started(r) && r["ordinal"].as_u64() < Some(ordinal));
    if previous.is_some_and(|p| p["status"] == "interrupted") {
        started["handoff"] = json!({"uuid": (ctx.new_id)(), "from": 1, "to": ordinal - 1});
    }
    let instance = run["providerInstanceId"].as_str().unwrap_or("");
    let pt = thread_provider_thread(state, thread, instance, run_id);
    let item = user_item(thread, run, &pt, &message, "queued_turn", &at);
    vec![draft("turn-item.updated", Some(thread), None, ctx.now, item), draft("run.updated", Some(thread), None, ctx.now, started)]
}

/// The events that end a queued run unstarted: it is cancelled and its message is taken back.
pub fn cancelled(run: &Value, now: Millis) -> Vec<EventDraft> {
    let (at, thread) = (iso(now), run["threadId"].as_str());
    let mut ended = run.clone();
    for (k, v) in [("status", json!("cancelled")), ("completedAt", json!(at)), ("queueRank", Value::Null), ("updatedAt", json!(at))] {
        ended[k] = v;
    }
    vec![draft("run.updated", thread, None, now, ended), draft("message.removed", thread, None, now, json!({"id": run["userMessageId"]}))]
}

/// `queued-run.edit|cancel|reorder|start` of a run the rows hold queued; the agent API has accepted the command already.
fn queue_command(state: &State, thread: &str, cmd: &Command, ctx: &Ctx) -> Result<Decision, String> {
    let run_id = cmd.str("runId").unwrap_or("");
    let run = queued_run(state, thread, Some(run_id)).ok_or_else(|| format!("Run {run_id} is not queued."))?;
    let (at, t) = (iso(ctx.now), Some(thread));
    let changed = |mut run: Value, patch: Vec<(&str, Value)>| {
        for (k, v) in patch {
            run[k] = v;
        }
        run["updatedAt"] = at.clone().into();
        draft("run.updated", t, None, ctx.now, run)
    };
    let message = || state.entities.get(thread).and_then(|e| e.messages.iter().find(|m| m["id"] == run["userMessageId"])).cloned();
    match cmd.kind.as_str() {
        "queued-run.edit" => {
            let mut message = message().ok_or_else(|| format!("Run {run_id} has no message."))?;
            message["text"] = cmd.str("text").unwrap_or("").into();
            message["updatedAt"] = at.clone().into();
            Ok(Decision { drafts: vec![draft("message.updated", t, None, ctx.now, message)], effect: None })
        }
        "queued-run.cancel" => {
            Ok(Decision { drafts: cancelled(&run, ctx.now), effect: None })
        }
        "queued-run.reorder" => {
            let mut order: Vec<Value> = queued_runs(state, thread);
            order.retain(|r| r["id"] != run["id"]);
            let at_index = cmd.str("beforeRunId").and_then(|before| order.iter().position(|r| r["id"] == before)).unwrap_or(order.len());
            order.insert(at_index, run.clone());
            let drafts = order.into_iter().enumerate().filter(|(i, r)| r["queueRank"].as_f64() != Some(*i as f64 + 1.0)).map(|(i, r)| changed(r, vec![("queueRank", json!(i as f64 + 1.0))])).collect();
            Ok(Decision { drafts, effect: None })
        }
        _ => {
            Ok(Decision { drafts: start_queued(state, thread, &run, Some(false), ctx), effect: Some(Effect::Follow { thread_id: thread.to_string(), run_id: Some(run_id.to_string()) }) })
        }
    }
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
        "update" if cmd.body.get("archived") == Some(&json!(true)) => one(draft("project.archived", None, Some(id), now, json!({"projectId": id, "archivedAt": at}))),
        "update" => {
            let patch = cmd.rest(&["projectId", "id", "createdAt", "updatedAt", "archivedAt"]);
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
    let subject = match cmd.kind.as_str() {
        "thread.create" => None,
        "thread.fork" | "thread.merge_back" => cmd.str("sourceThreadId"),
        _ => cmd.str("threadId"),
    };
    if let (Family::Thread, Some(thread)) = (family, subject) {
        if !state.threads.contains_key(thread) {
            return Err(Rejection::ThreadMissing(thread.to_string()));
        }
    }
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
    /// The provider's session could not be opened, so the turn never began.
    OpenFailed(String),
}

/// What a running turn reports. Text, reasoning and tool output arrive as the pieces the agent API streams, and the thread's rows grow by them.
#[derive(Debug, Clone, PartialEq)]
pub enum RunStep {
    /// The agent API began the turn: a queued run leaves the queue.
    Started,
    Text(String),
    Reasoning(String),
    ToolStarted { id: String, name: String, input: Value },
    ToolOutput { id: String, data: String },
    ToolFinished { id: String, ok: bool },
    Request(RuntimeRequestEvent),
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
            AgentEventKind::Request(request) => RunStep::Request(request.clone()),
            AgentEventKind::Failure(message) => RunStep::Failure(message.clone()),
            AgentEventKind::TurnFinished(Finish::Completed) => RunStep::Finished(Outcome::Completed),
            AgentEventKind::TurnFinished(Finish::Interrupted) => RunStep::Finished(Outcome::Interrupted),
            AgentEventKind::TurnFinished(Finish::Failed) => RunStep::Finished(Outcome::Failed(TURN_FAILED.into())),
            AgentEventKind::TurnStarted => RunStep::Started,
            AgentEventKind::UserText(_) | AgentEventKind::Other => return None,
        })
    }
}

/// The error of a failed turn that logged none.
pub const TURN_FAILED: &str = "the turn failed";

fn approval_kind(tool: &str) -> &'static str {
    match tool.to_ascii_lowercase().as_str() {
        "bash" | "command" | "exec_command" | "run_command" | "shell" => "command",
        "read" | "read_file" => "file-read",
        "edit" | "write" | "write_file" | "apply_patch" => "file-change",
        _ => "permission",
    }
}

/// The events one report of a turn stands for. A turn that already ended records nothing more.
pub fn decide_run_step(state: &State, thread: &str, run_id: &str, step: &RunStep, ctx: &Ctx) -> Vec<EventDraft> {
    let entities = state.entities.get(thread);
    let Some(run) = entities.and_then(|e| e.runs.iter().find(|r| r["id"] == run_id)).cloned() else {
        return vec![];
    };
    if run["status"] == "queued" {
        return if matches!(step, RunStep::Started) { start_queued(state, thread, &run, None, ctx) } else { vec![] };
    }
    if run["status"] != "running" || matches!(step, RunStep::Started) {
        return vec![];
    }
    let at = iso(ctx.now);
    let t = Some(thread);
    let instance = run["providerInstanceId"].clone();
    let items = entities.map(|e| &e.turn_items);
    let existing = |id: &str| items.and_then(|i| i.iter().find(|x| x["id"] == id)).cloned();
    let run_ordinal = run["ordinal"].as_u64().unwrap_or(0);
    let next_ordinal = runs::item_ordinal(run_ordinal, items.map_or(0, |i| i.iter().filter(|x| x["runId"] == run_id).count() as u64) + 1);
    let item = |id: String, kind: &str, status: &str, extra: Value| {
        let old = existing(&id);
        let ordinal = old.as_ref().map_or(next_ordinal, |o| o["ordinal"].as_u64().unwrap_or(0));
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
    let attempt_id = runs::attempt_id(run_id);
    let provider_turn = entities.and_then(|e| e.provider_turns.iter().find(|turn| turn["runAttemptId"] == attempt_id));
    let provider_turn_id = format!("provider-turn:run:{}", runs::enc(run_id));
    let request_item_id = |id: &str| format!("{run_id}:request:{id}");
    let open_request = |id: &str, kind: &str, item_type: &str, fields: Value| {
        let session = state.threads.get(thread).zip(entities).and_then(|(shell, entities)| {
            state.run_rows(shell, thread, &entities.runs).provider_sessions.into_iter().next()
        });
        let Some(session) = session else { return vec![] };
        let mut card = item(request_item_id(id), item_type, "waiting", fields);
        card["requestId"] = json!(id);
        card["providerTurnId"] = json!(provider_turn_id);
        card["providerThreadId"] = json!(thread_provider_thread(state, thread, instance.as_str().unwrap_or(""), run_id));
        let request = json!({
            "id": id, "nodeId": runs::node_id(run_id), "providerTurnId": provider_turn_id,
            "nativeRequestRef": null, "kind": kind, "status": "pending",
            "responseCapability": {"type": "live", "providerSessionId": session["id"]},
            "createdAt": at, "resolvedAt": null,
        });
        vec![draft("turn-item.updated", t, None, ctx.now, card), draft("runtime-request.updated", t, None, ctx.now, request)]
    };
    let pending = || entities.into_iter().flat_map(|e| &e.runtime_requests).filter(|r| r["status"] == "pending" && r["nodeId"] == runs::node_id(run_id));
    let close_request = |old: &Value, response: &RuntimeResponse| {
        let mut request = old.clone();
        let id = old["id"].as_str().unwrap_or_default();
        let cancelled = matches!(response, RuntimeResponse::Dismissed);
        request["status"] = json!(if cancelled { "cancelled" } else { "resolved" });
        request["resolvedAt"] = json!(at);
        match response {
            RuntimeResponse::Approval(decision) => request["decision"] = json!(decision),
            RuntimeResponse::Answer(answer) => request["answers"] = json!({(id): answer}),
            RuntimeResponse::Dismissed => {}
        }
        let mut events = vec![draft("runtime-request.updated", t, None, ctx.now, request)];
        if let Some(mut card) = existing(&request_item_id(id)) {
            card["status"] = json!(if cancelled { "cancelled" } else { "completed" });
            card["updatedAt"] = json!(at);
            events.push(draft("turn-item.updated", t, None, ctx.now, card));
        }
        events
    };
    let mut events = match step {
        RunStep::Started => vec![],
        RunStep::Text(delta) => vec![draft("turn-item.updated", t, None, ctx.now, item(assistant_id.clone(), "assistant_message", "streaming", json!({"text": grown(&assistant_id, delta), "messageId": message_id})))],
        RunStep::Reasoning(delta) => {
            let id = format!("{run_id}:reasoning");
            vec![draft("turn-item.updated", t, None, ctx.now, item(id.clone(), "reasoning", "streaming", json!({"text": grown(&id, delta)})))]
        }
        RunStep::ToolStarted { id, name, input } => vec![draft("turn-item.updated", t, None, ctx.now, item(format!("{run_id}:tool:{id}"), "command_execution", "running", json!({"name": name, "input": input, "output": null})))],
        RunStep::ToolOutput { id, data } => tool(id, "running", json!(format!("{}{data}", printed(id).unwrap_or_default()))).into_iter().collect(),
        RunStep::ToolFinished { id, ok } => tool(id, if *ok { "completed" } else { "failed" }, json!(printed(id).unwrap_or_default())).into_iter().collect(),
        RunStep::Request(RuntimeRequestEvent::Approval { id, tool, args }) => {
            let kind = approval_kind(tool);
            let prompt = args["command"].as_str().map(str::to_string).unwrap_or_else(|| format!("{tool} {args}"));
            open_request(id, kind, "approval_request", json!({"requestKind": kind, "prompt": prompt}))
        }
        RunStep::Request(RuntimeRequestEvent::Question { id, prompt, options }) => {
            let prompt = if prompt.trim().is_empty() { "Input requested" } else { prompt.trim() };
            let choices: Vec<_> = options.iter().filter(|option| !option.trim().is_empty())
                .map(|option| json!({"label": option.trim(), "description": option.trim(), "value": option})).collect();
            open_request(id, "user_input", "user_input_request", json!({"questions": [{"id": id, "header": "Question", "question": prompt, "options": choices, "allowCustomAnswer": choices.is_empty()}]}))
        }
        RunStep::Request(RuntimeRequestEvent::Closed { id, response }) => pending().find(|request| request["id"] == id.as_str()).map_or_else(Vec::new, |request| close_request(request, response)),
        RunStep::Failure(_) if existing(&error_id).is_some() => vec![],
        RunStep::Failure(message) => vec![error(message)],
        RunStep::Finished(outcome) => {
            let logged = existing(&error_id).and_then(|e| e["message"].as_str().map(str::to_string));
            let (status, error_text) = match outcome {
                Outcome::Completed => ("completed", Value::Null),
                Outcome::Interrupted => ("interrupted", Value::Null),
                Outcome::Failed(m) | Outcome::OpenFailed(m) => ("failed", json!(logged.clone().unwrap_or_else(|| m.clone()))),
            };
            let mut out: Vec<_> = pending().flat_map(|request| close_request(request, &RuntimeResponse::Dismissed)).collect();
            if let Some(streaming) = existing(&assistant_id) {
                let text = streaming["text"].clone();
                out.push(draft("turn-item.updated", t, None, ctx.now, item(assistant_id, "assistant_message", "completed", json!({"text": text, "messageId": message_id}))));
                out.push(draft("message.updated", t, None, ctx.now, json!({"id": message_id, "threadId": thread, "runId": run_id, "role": "assistant", "text": text, "status": "completed", "providerInstanceId": instance, "createdAt": at, "updatedAt": at})));
            }
            if let (Outcome::Failed(m), None) = (outcome, &logged) {
                out.push(error(m));
            }
            let mut ended = run.clone();
            if matches!(outcome, Outcome::OpenFailed(_)) {
                let driver = instance.as_str().unwrap_or("");
                let pt = thread_provider_thread(state, thread, driver, run_id);
                let mut failed = signal_item(thread, &run, &pt, runs::signal_item_id(run_id, "provider-session-open-failure"), next_ordinal, Value::Null, "failed", "Provider session failed to open", "error", "", &at);
                let row = failed.as_object_mut().unwrap();
                row.remove("message");
                row.insert("failure".into(), json!({"class": "provider_error", "code": null, "message": format!("Failed to open {driver} provider session {}.", runs::shared_session_id(driver)), "retryable": null}));
                out.push(draft("turn-item.updated", t, None, ctx.now, failed));
                ended["startedAt"] = Value::Null;
                ended["openFailed"] = Value::Bool(true);
            }
            let fields = ended.as_object_mut().unwrap();
            for (k, v) in [("status", json!(status)), ("completedAt", json!(at)), ("updatedAt", json!(at)), ("error", error_text)] {
                fields.insert(k.into(), v);
            }
            if let Some(turn) = provider_turn {
                let mut turn = turn.clone();
                turn["status"] = json!(status);
                turn["completedAt"] = json!(at);
                out.push(draft("provider-turn.updated", t, None, ctx.now, turn));
            }
            out.push(draft("run.updated", t, None, ctx.now, ended));
            out
        }
    };
    if provider_turn.is_none() && matches!(step, RunStep::Text(_) | RunStep::Reasoning(_) | RunStep::ToolStarted { .. } | RunStep::Request(RuntimeRequestEvent::Approval { .. } | RuntimeRequestEvent::Question { .. })) {
        if let Some((shell, entities)) = state.threads.get(thread).zip(entities).filter(|(_, entities)| entities.provider_turns.is_empty()) {
            let rows = state.run_rows(shell, thread, &entities.runs);
            events.extend(rows.provider_sessions.into_iter().map(|row| draft("provider-session.attached", t, None, ctx.now, row)));
            events.extend(rows.provider_threads.into_iter().map(|row| draft("provider-thread.updated", t, None, ctx.now, row)));
        }
        let provider_thread = thread_provider_thread(state, thread, instance.as_str().unwrap_or(""), run_id);
        let turn = json!({
            "id": provider_turn_id,
            "providerThreadId": provider_thread,
            "nodeId": runs::node_id(run_id),
            "runAttemptId": attempt_id,
            "nativeTurnRef": null,
            "ordinal": run_ordinal,
            "status": "running",
            "startedAt": at,
            "completedAt": null,
        });
        events.push(draft("provider-turn.updated", t, None, ctx.now, turn));
    }
    events
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
        assert_eq!((item(&state, &run, "assistant")["ordinal"].as_u64(), tool["ordinal"].as_u64()), (Some(1_000_002), Some(1_000_003)));
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
        assert_eq!(steps(K::TurnStarted), Some(RunStep::Started));
        assert!([K::UserText("hi".into()), K::Other].into_iter().all(|k| steps(k).is_none()));
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
    fn the_projection_carries_the_optional_keys_only_after_a_command_set_them_as_the_real_server_does() {
        // Given a thread nobody pinned or settled
        let mut state = live();
        let keys = |state: &State| -> Vec<String> { state.thread_projection("t").unwrap()["thread"].as_object().unwrap().keys().filter(|k| ["activeOrderKey", "pinOrderKey", "pinnedAt", "unsettledAt", "autoSettleDisabledAt"].contains(&k.as_str())).cloned().collect() };
        let send = |state: &mut State, kind: &str| {
            thread_with(state, json!({"type": kind, "commandId": "c", "threadId": "t"})).unwrap();
        };
        let before = keys(&state);
        // When it is pinned, unpinned, settled and unsettled
        send(&mut state, "thread.pin");
        let pinned = keys(&state);
        send(&mut state, "thread.unpin");
        let unpinned = keys(&state);
        send(&mut state, "thread.settle");
        let settled = keys(&state);
        send(&mut state, "thread.unsettle");
        // Then each command added exactly the keys the recording shows for it, and unsettling stamped its time
        assert_eq!((before.len(), pinned, unpinned.len(), settled.len()), (0, vec!["pinnedAt".to_string()], 2, 4));
        assert!(state.thread_projection("t").unwrap()["thread"]["unsettledAt"].is_string());
    }

    #[test]
    fn a_visit_needs_a_valid_time_and_a_deleted_thread_still_has_its_projection_with_the_time_it_was_deleted() {
        // Given a thread
        let mut state = live();
        // When a visit names a time that is not one, a valid one, and the thread is then deleted
        let invalid = thread_with(&mut state, json!({"type": "thread.visit", "commandId": "c", "threadId": "t", "visitedAt": "a"}));
        thread_with(&mut state, json!({"type": "thread.visit", "commandId": "c", "threadId": "t", "visitedAt": "2030-01-01T00:00:00.000Z"})).unwrap();
        let visited = state.thread_projection("t").unwrap()["thread"]["lastVisitedAt"].clone();
        thread_with(&mut state, json!({"type": "thread.delete", "commandId": "c", "threadId": "t"})).unwrap();
        // Then the first is refused with the real server's words, the second is kept, and only the read of a deleted thread answers
        assert_eq!(invalid.unwrap_err(), "Thread t visit time a is not a valid timestamp.");
        assert_eq!(visited, "2030-01-01T00:00:00.000Z");
        assert!(state.thread_projection("t").is_none());
        let deleted = state.thread_projection_read("t").unwrap();
        assert!(deleted["thread"]["deletedAt"].is_string() && deleted["thread"]["titleRegeneration"].is_null() && deleted["thread"].get("titleRegeneration").is_some());
    }

    #[test]
    fn marking_unread_needs_a_completed_run_and_then_forgets_the_visit() {
        // Given a thread nobody ran and one visited after a run completed
        let mut state = live();
        let mark = json!({"type": "thread.mark-unread", "commandId": "c", "threadId": "t"});
        // When it is marked unread before and after a run completed
        let refused = thread_with(&mut state, mark.clone());
        state.threads.get_mut("t").unwrap().extend([("latestRunCompletedAt".to_string(), json!("2030-01-01T00:00:00.000Z")), ("lastVisitedAt".to_string(), json!("2030-01-02T00:00:00.000Z"))]);
        thread_with(&mut state, mark).unwrap();
        // Then the first is refused in T3's words and the second clears the visit
        assert_eq!(refused.err().as_deref(), Some("Thread t has no completed run to mark unread."));
        assert!(state.threads["t"]["lastVisitedAt"].is_null());
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

    fn queue_after(state: &mut State, id: &str) {
        let sent = with(state, Family::Thread, json!({"type": "message.dispatch", "commandId": format!("c-{id}"), "threadId": "t", "message": {"messageId": id, "text": format!("text {id}")}, "dispatchMode": {"type": "queue_after_active"}})).unwrap();
        assert!(matches!(sent.effect, Some(Effect::Queue(_))));
    }

    fn statuses(state: &State) -> Vec<(String, String, Value)> {
        let rows = t3_contracts::runs::derive("t", &state.threads["t"], &state.entities["t"].runs, None).runs;
        rows.iter().map(|r| (r["id"].as_str().unwrap().to_string(), r["status"].as_str().unwrap().to_string(), r["queuePosition"].clone())).collect()
    }

    #[test]
    fn a_message_sent_while_a_turn_runs_waits_as_a_queued_run_that_can_be_edited_reordered_and_cancelled_and_its_user_row_comes_when_it_starts() {
        // Given a running run and three messages queued behind it
        let (mut state, run) = running();
        assert_eq!(run, "run:thread:t:ordinal:1");
        for id in ["q1", "q2", "q3"] {
            queue_after(&mut state, id);
        }
        // Then they are queued runs 2 to 4 in order, with their messages in the thread and no row of the timeline yet
        assert_eq!(statuses(&state).iter().map(|(_, s, p)| (s.as_str(), p.clone())).collect::<Vec<_>>(), [("running", Value::Null), ("queued", json!(1)), ("queued", json!(2)), ("queued", json!(3))]);
        assert_eq!((state.entities["t"].messages.len(), state.entities["t"].turn_items.len()), (4, 1));
        // When the last is moved before the first, the second is edited and the first cancelled
        let q = |n: u64| format!("run:thread:t:ordinal:{n}");
        with(&mut state, Family::Thread, json!({"type": "queued-run.reorder", "commandId": "r", "threadId": "t", "runId": q(4), "beforeRunId": q(2)})).unwrap();
        with(&mut state, Family::Thread, json!({"type": "queued-run.edit", "commandId": "e", "threadId": "t", "runId": q(3), "text": "edited"})).unwrap();
        with(&mut state, Family::Thread, json!({"type": "queued-run.cancel", "commandId": "x", "threadId": "t", "runId": q(2)})).unwrap();
        // Then the queue is the last then the second, the edit is in the message, and the cancelled run is cancelled with its message taken back
        assert_eq!(statuses(&state).into_iter().skip(1).map(|(_, s, p)| (s, p)).collect::<Vec<_>>(), [("cancelled".to_string(), Value::Null), ("queued".into(), json!(2)), ("queued".into(), json!(1))]);
        let texts: Vec<&str> = state.entities["t"].messages.iter().map(|m| m["text"].as_str().unwrap()).collect();
        assert_eq!(texts, ["hi", "edited", "text q3"]);
    }

    #[test]
    fn a_run_interrupted_before_its_provider_turn_began_ends_at_once_with_the_two_signal_rows_and_one_that_began_is_only_asked_to_stop() {
        // Given two running runs, the second with an answer from the provider already
        let (mut waiting, run) = running();
        let (mut answering, answered) = running();
        step(&mut answering, &answered, RunStep::Text("Hel".into()));
        // When each is interrupted
        let first = with(&mut waiting, Family::Thread, json!({"type": "run.interrupt", "commandId": "i", "threadId": "t", "runId": run})).unwrap();
        let second = with(&mut answering, Family::Thread, json!({"type": "run.interrupt", "commandId": "i", "threadId": "t", "runId": answered})).unwrap();
        // Then the first is interrupted with the signal rows, and the second keeps running until its turn ends; neither holds the queue
        let kinds = |state: &State| state.entities["t"].turn_items.iter().map(|i| i["type"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!((waiting.entities["t"].runs[0]["status"].as_str(), kinds(&waiting)), (Some("interrupted"), vec!["user_message".to_string(), "run_interrupt_request".into(), "run_interrupt_result".into()]));
        assert_eq!(waiting.entities["t"].turn_items[2]["message"], "Run interrupted before provider start");
        assert_eq!((answering.entities["t"].runs[0]["status"].as_str(), kinds(&answering).last().cloned()), (Some("running"), Some("run_interrupt_request".to_string())));
        assert_eq!((first.effect, second.effect), (Some(Effect::Interrupt { thread_id: "t".into(), hold: false }), Some(Effect::Interrupt { thread_id: "t".into(), hold: false })));
    }

    #[test]
    fn a_thread_stop_holds_the_queue_and_a_queued_run_started_by_hand_follows_the_interrupted_one_with_a_handoff_of_its_turns() {
        // Given a running run with one message queued behind it
        let (mut state, _) = running();
        queue_after(&mut state, "q1");
        // When the thread is stopped and the queued run is started
        let stop = with(&mut state, Family::Thread, json!({"type": "thread.stop", "commandId": "s", "threadId": "t", "reason": "user"})).unwrap();
        let start = with(&mut state, Family::Thread, json!({"type": "queued-run.start", "commandId": "g", "threadId": "t", "runId": "run:thread:t:ordinal:2"})).unwrap();
        // Then the stop held the queue and said why, and the second run is running with its user row, a handoff of the first and the queue flag off
        assert_eq!(stop.effect, Some(Effect::Interrupt { thread_id: "t".into(), hold: true }));
        assert_eq!(start.effect, Some(Effect::Follow { thread_id: "t".into(), run_id: Some("run:thread:t:ordinal:2".into()) }));
        assert_eq!(state.entities["t"].turn_items[1]["message"], "user");
        let second = &state.entities["t"].runs[1];
        assert_eq!((second["status"].as_str(), second["queueHeld"].clone(), second["handoff"]["from"].clone(), second["handoff"]["to"].clone()), (Some("running"), json!(false), json!(1), json!(1)));
        let last = state.entities["t"].turn_items.last().unwrap();
        assert_eq!((last["inputIntent"].as_str(), last["ordinal"].as_u64()), (Some("queued_turn"), Some(2_000_001)));
    }

    #[test]
    fn a_queued_run_starts_by_itself_when_the_agent_api_begins_its_turn_and_not_before() {
        // Given a running run, a queued one behind it, and the first ended
        let (mut state, run) = running();
        queue_after(&mut state, "q1");
        let second = "run:thread:t:ordinal:2";
        step(&mut state, &run, RunStep::Finished(Outcome::Completed));
        // When a step other than the start arrives for the queued run, and then its start
        let early = step(&mut state, second, RunStep::Text("early".into()));
        let begun = step(&mut state, second, RunStep::Started);
        // Then the first changed nothing, and the second started the run with its user row
        assert_eq!((early, begun, state.entities["t"].runs[1]["status"].as_str()), (0, 2, Some("running")));
    }

    #[test]
    fn a_run_whose_provider_never_opened_fails_unstarted_with_the_open_failure_row_and_the_thread_stop_that_follows_marks_it_without_ending_anything() {
        // Given a running run on a provider that cannot open
        let (mut state, run) = running();
        // When the open fails and the thread is stopped
        step(&mut state, &run, RunStep::Finished(Outcome::OpenFailed("no driver".into())));
        let stop = with(&mut state, Family::Thread, json!({"type": "thread.stop", "commandId": "s", "threadId": "t"})).unwrap();
        // Then the run is failed with no start, the failure row is the pin's, and the stop adds only its request row and holds the queue
        let ended = &state.entities["t"].runs[0];
        assert_eq!((ended["status"].as_str(), ended["startedAt"].is_null(), ended["error"].as_str()), (Some("failed"), true, Some("no driver")));
        let failed = state.entities["t"].turn_items[1].clone();
        assert_eq!((failed["status"].as_str(), failed["failure"]["message"].as_str(), failed["title"].as_str()), (Some("failed"), Some("Failed to open opencode provider session provider-session:provider-instance:opencode:shared."), Some("Provider session failed to open")));
        let kinds: Vec<&str> = state.entities["t"].turn_items.iter().map(|i| i["type"].as_str().unwrap()).collect();
        assert_eq!((kinds, stop.effect), (vec!["user_message", "error", "run_interrupt_request"], Some(Effect::Interrupt { thread_id: "t".into(), hold: true })));
    }

    #[test]
    fn a_message_dispatched_to_defer_its_start_waits_preparing_with_its_workspace_row_and_the_prepared_run_commands_refuse_a_run_that_is_not_preparing() {
        // Given a thread with a failed run
        let (mut state, run) = running();
        step(&mut state, &run, RunStep::Finished(Outcome::Failed("x".into())));
        // When a message is dispatched with its start deferred, and a prepared-run command names each run
        let sent = with(&mut state, Family::Thread, json!({"type": "message.dispatch", "commandId": "d", "threadId": "t", "message": {"messageId": "m2", "text": "again"}, "dispatchMode": {"type": "defer_start"}})).unwrap();
        let on = |state: &mut State, kind: &str, run: &str| thread_with(state, json!({"type": kind, "commandId": format!("p-{kind}-{run}"), "threadId": "t", "runId": run})).unwrap_err();
        let (failed_run, preparing_run) = (on(&mut state, "prepared-run.release", &run), on(&mut state, "prepared-run.progress", "run:thread:t:ordinal:2"));
        // Then it starts nothing (it names no workspace to prepare), the run is preparing with a running workspace row, and only the run that is not preparing is refused in T3's words
        let second = &state.entities["t"].runs[1];
        assert_eq!((sent.effect, second["status"].as_str(), second["startedAt"].is_null()), (None, Some("preparing"), true));
        let workspace = state.entities["t"].turn_items.iter().find(|i| i["type"] == "command_execution").unwrap();
        assert_eq!((workspace["title"].as_str(), workspace["status"].as_str(), workspace["ordinal"].as_u64()), (Some("Preparing workspace"), Some("running"), Some(2_000_002)));
        assert_eq!(failed_run, format!("Run {run} is not awaiting workspace preparation."));
        assert!(preparing_run.contains("needs a phase"), "{preparing_run}");
    }

    /// A thread with a run that waits for a worktree to be made from `main`.
    fn preparing() -> (State, String) {
        let mut state = live();
        let sent = thread_with(&mut state, json!({"type": "message.dispatch", "commandId": "d", "threadId": "t", "message": {"messageId": "m1", "text": "build it"}, "dispatchMode": {"type": "defer_start", "workspaceStrategy": {"type": "worktree", "baseRef": "main", "branch": "feature/x"}}})).unwrap();
        let Some(Effect::Prepare { run_id, .. }) = sent.effect else { panic!("no preparation: {:?}", sent.effect) };
        (state, run_id)
    }

    fn prepared(state: &mut State, kind: &str, run: &str, extra: Value) -> Result<Decision, String> {
        let mut command = json!({"type": kind, "commandId": format!("c-{kind}-{}", state.last_sequence), "threadId": "t", "runId": run});
        command.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap_or_default());
        thread_with(state, command)
    }

    #[test]
    fn a_deferred_message_that_names_a_worktree_asks_for_its_preparation_and_progress_and_release_move_the_workspace_row_and_start_the_run() {
        // Given a run that waits for a worktree
        let (mut state, run) = preparing();
        let row = |state: &State| state.entities["t"].turn_items.iter().find(|i| i["type"] == "command_execution").cloned().unwrap();
        // Then the run keeps the strategy it will be prepared by, and is preparing
        assert_eq!((state.entities["t"].runs[0]["workspacePreparation"]["baseRef"].clone(), state.entities["t"].runs[0]["status"].clone()), (json!("main"), json!("preparing")));
        // When each phase is reported, and then the run is released
        prepared(&mut state, "prepared-run.progress", &run, json!({"phase": "worktree"})).unwrap();
        let worktree = row(&state)["title"].clone();
        prepared(&mut state, "prepared-run.progress", &run, json!({"phase": "setup"})).unwrap();
        let setup = row(&state)["title"].clone();
        let released = prepared(&mut state, "prepared-run.release", &run, json!({})).unwrap();
        // Then the row's title follows the phase, and the release completes it, starts the run and asks the engine to launch its turn with the message
        assert_eq!((worktree, setup), (json!("Preparing worktree"), json!("Starting setup script")));
        let done = row(&state);
        assert_eq!((done["status"].clone(), done["title"].clone(), done["exitCode"].clone(), done["completedAt"].is_null()), (json!("completed"), json!("Workspace ready"), json!(0), false));
        assert_eq!((state.entities["t"].runs[0]["status"].clone(), state.entities["t"].runs[0]["startedAt"].is_null()), (json!("running"), false));
        let Some(Effect::Launch(launch)) = released.effect else { panic!("no launch") };
        assert_eq!((launch.run_id, launch.prompt), (run.clone(), "build it".to_string()));
        // And a run that was released is no longer awaiting preparation
        assert_eq!(prepared(&mut state, "prepared-run.release", &run, json!({})).unwrap_err(), format!("Run {run} is not awaiting workspace preparation."));
    }

    #[test]
    fn a_failed_preparation_leaves_the_run_failed_with_an_error_row_and_a_retry_takes_it_back_and_asks_again_only_for_that_failure() {
        // Given a preparing run, and a retry before anything failed
        let (mut state, run) = preparing();
        let early = prepared(&mut state, "prepared-run.retry", &run, json!({})).unwrap_err();
        // When the preparation fails with a reason
        prepared(&mut state, "prepared-run.fail", &run, json!({"failure": {"class": "validation_error", "message": "base main is not a branch", "code": null, "retryable": false}})).unwrap();
        // Then the run is failed, the workspace row says why and the error row carries the preparation code
        let items = &state.entities["t"].turn_items;
        let work = items.iter().find(|i| i["type"] == "command_execution").unwrap();
        let error = items.iter().find(|i| i["type"] == "error").unwrap();
        assert_eq!((state.entities["t"].runs[0]["status"].clone(), work["status"].clone(), work["output"].clone(), error["failure"]["code"].clone()), (json!("failed"), json!("failed"), json!("base main is not a branch"), json!("workspace_preparation_failed")));
        assert_eq!(early, format!("Run {run} has no failed workspace preparation to retry."));
        // And a retry cancels the error row, puts the workspace row and the run back to preparing and asks for the preparation again
        let retried = prepared(&mut state, "prepared-run.retry", &run, json!({})).unwrap();
        let items = &state.entities["t"].turn_items;
        let work = items.iter().find(|i| i["type"] == "command_execution").unwrap();
        assert_eq!((state.entities["t"].runs[0]["status"].clone(), state.entities["t"].runs[0]["completedAt"].clone(), work["status"].clone(), work["title"].clone(), work.get("output").is_none()), (json!("preparing"), Value::Null, json!("running"), json!("Preparing workspace"), true));
        assert_eq!(items.iter().find(|i| i["type"] == "error").unwrap()["status"], json!("cancelled"));
        assert_eq!(retried.effect, Some(Effect::Prepare { thread_id: "t".into(), run_id: run.clone() }));
        // And a second retry has no failure to take back
        assert_eq!(prepared(&mut state, "prepared-run.retry", &run, json!({})).unwrap_err(), format!("Run {run} has no failed workspace preparation to retry."));
    }

    #[test]
    fn a_queued_message_is_sent_now_only_after_a_stop_and_an_interrupt_of_a_run_that_is_not_running_is_not_interruptible() {
        // Given a failed run
        let (mut state, run) = running();
        step(&mut state, &run, RunStep::Finished(Outcome::Failed("x".into())));
        // When the run is interrupted
        let interrupt = thread_with(&mut state, json!({"type": "run.interrupt", "commandId": "i", "threadId": "t", "runId": run})).unwrap_err();
        // Then T3 says it is not interruptible, and a thread that failed is not stopped but one that was interrupted is
        assert_eq!(interrupt, format!("Run {run} is not interruptible."));
        assert!(!queued_may_start(&state, "t"));
        let (mut stopped, run) = running();
        with(&mut stopped, Family::Thread, json!({"type": "run.interrupt", "commandId": "i", "threadId": "t", "runId": run})).unwrap();
        assert!(queued_may_start(&stopped, "t"));
    }
}
