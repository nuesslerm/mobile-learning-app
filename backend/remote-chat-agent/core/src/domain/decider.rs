//! The pure decision of a command into events (`orchestration/decider.ts`): thread existence, a turn start that queues or is
//! refused, open requests that block new turns, and the single-event commands.

use super::lifecycle;
use super::projector::ReadModel;
use super::types::*;
use serde_json::{json, Map, Value};

/// The kind of the engine event that holds one neutral event of the agent API.
pub const NEUTRAL_KIND: &str = "thread.neutral-event";

pub struct Context<'a> {
    pub now: &'a dyn Fn() -> String,
    pub new_id: &'a dyn Fn() -> String,
    pub default_cwd: &'a str,
    pub actor: &'a str,
    /// The clock of this decision in milliseconds, the time the lifecycle's events carry.
    pub now_ms: u64,
    /// The log of the thread a `Fork` copies (empty for every other command), which the engine loads before it decides.
    pub source_events: &'a [Event],
}

pub enum Decision {
    Accepted(Vec<Event>),
    Rejected { code: String, message: String },
}

fn reject(code: &str, message: String) -> Decision {
    Decision::Rejected { code: code.into(), message }
}

struct Planned {
    kind: &'static str,
    thread_id: String,
    payload: Value,
}

fn request_id_of(activity: &Activity) -> Option<&str> {
    activity.payload.get("requestId").and_then(Value::as_str)
}

fn requested(kind: &str) -> bool {
    kind == "approval.requested" || kind == "user-input.requested"
}

fn resolved(kind: &str) -> bool {
    kind == "approval.resolved" || kind == "user-input.resolved"
}

/// Whether any approval or user-input request is still open: the last activity of each request id decides.
pub fn has_open_blocking_request(thread: &Thread) -> bool {
    let mut open: Vec<&str> = Vec::new();
    for activity in &thread.activities {
        let Some(id) = request_id_of(activity) else { continue };
        if requested(&activity.kind) {
            if !open.contains(&id) {
                open.push(id);
            }
        } else if resolved(&activity.kind) {
            open.retain(|o| *o != id);
        }
    }
    !open.is_empty()
}

pub fn thread_has_open_request(thread: &Thread, request_id: &str) -> bool {
    let mut open = false;
    for activity in &thread.activities {
        if request_id_of(activity) != Some(request_id) {
            continue;
        }
        if requested(&activity.kind) {
            open = true;
        } else if resolved(&activity.kind) {
            open = false;
        }
    }
    open
}

fn materialize(model: &ReadModel, planned: Vec<Planned>, command_id: &str, ctx: &Context) -> Vec<Event> {
    let mut sequence = model.snapshot_sequence;
    planned
        .into_iter()
        .map(|next| {
            sequence += 1;
            Event {
                event_id: (ctx.new_id)(),
                sequence,
                created_at: (ctx.now)(),
                thread_id: next.thread_id,
                command_id: Some(command_id.to_string()),
                actor: ctx.actor.to_string(),
                kind: next.kind.to_string(),
                payload: next.payload,
            }
        })
        .collect()
}

struct NewThread<'a> {
    title: &'a str,
    cwd: &'a str,
    selection: &'a ModelSelection,
    runtime: &'a str,
    interaction: &'a str,
    branch: &'a Option<String>,
    worktree_path: &'a Option<String>,
}

fn thread_created(thread_id: &str, new: NewThread, created_at: &str) -> Planned {
    let mut payload = Map::new();
    payload.insert("title".into(), json!(new.title));
    payload.insert("cwd".into(), json!(new.cwd));
    payload.insert("modelSelection".into(), new.selection.to_json());
    payload.insert("runtimeMode".into(), json!(new.runtime));
    payload.insert("interactionMode".into(), json!(new.interaction));
    if let Some(branch) = new.branch {
        payload.insert("branch".into(), json!(branch));
    }
    if let Some(path) = new.worktree_path {
        payload.insert("worktreePath".into(), json!(path));
    }
    payload.insert("createdAt".into(), json!(created_at));
    Planned { kind: "thread.created", thread_id: thread_id.into(), payload: Value::Object(payload) }
}

fn single(model: &ReadModel, command: &Command, kind: &'static str, payload: Value, ctx: &Context) -> Decision {
    Decision::Accepted(materialize(model, vec![Planned { kind, thread_id: command.thread_id.clone(), payload }], &command.command_id, ctx))
}

pub fn decide(model: &ReadModel, command: &Command, ctx: &Context) -> Decision {
    let thread_id = command.thread_id.as_str();
    match &command.body {
        Body::Create { title, cwd, model_selection, runtime_mode, interaction_mode, branch, worktree_path } => {
            if model.threads.contains_key(thread_id) {
                return reject("thread_exists", format!("thread {thread_id} already exists"));
            }
            let new = NewThread { title, cwd: cwd.as_deref().unwrap_or(ctx.default_cwd), selection: model_selection, runtime: runtime_mode, interaction: interaction_mode, branch, worktree_path };
            let planned = thread_created(thread_id, new, &(ctx.now)());
            Decision::Accepted(materialize(model, vec![planned], &command.command_id, ctx))
        }
        Body::NeutralCreate { title, workspace, driver, model: model_id } => {
            if model.threads.contains_key(thread_id) {
                return reject("thread_exists", format!("thread {thread_id} already exists"));
            }
            let selection = ModelSelection { cli: driver.clone().unwrap_or_else(|| "agent".into()), provider_id: None, model_id: model_id.clone() };
            let new = NewThread { title, cwd: ctx.default_cwd, selection: &selection, runtime: "full-access", interaction: "default", branch: &None, worktree_path: &None };
            let mut planned = thread_created(thread_id, new, &(ctx.now)());
            if let Value::Object(payload) = &mut planned.payload {
                let mut agent = Map::new();
                for (name, value) in [("workspace", workspace), ("driver", driver), ("model", model_id)] {
                    if let Some(value) = value {
                        agent.insert(name.into(), json!(value));
                    }
                }
                payload.insert("neutral".into(), Value::Object(agent));
            }
            Decision::Accepted(materialize(model, vec![planned], &command.command_id, ctx))
        }
        Body::Delete => {
            let at = (ctx.now)();
            single(model, command, "thread.deleted", json!({ "threadId": thread_id, "deletedAt": at }), ctx)
        }
        Body::Archive => {
            let at = (ctx.now)();
            single(model, command, "thread.archived", json!({ "threadId": thread_id, "archivedAt": at, "updatedAt": at }), ctx)
        }
        Body::Unarchive => {
            let at = (ctx.now)();
            single(model, command, "thread.unarchived", json!({ "threadId": thread_id, "updatedAt": at }), ctx)
        }
        Body::MetaUpdate { title, model_selection, cwd, branch, worktree_path } => {
            let mut patch = Map::new();
            patch.insert("threadId".into(), json!(thread_id));
            patch.insert("updatedAt".into(), json!((ctx.now)()));
            if let Some(v) = title {
                patch.insert("title".into(), json!(v));
            }
            if let Some(v) = model_selection {
                patch.insert("modelSelection".into(), v.to_json());
            }
            if let Some(v) = cwd {
                patch.insert("cwd".into(), json!(v));
            }
            if let Some(v) = branch {
                patch.insert("branch".into(), json!(v));
            }
            if let Some(v) = worktree_path {
                patch.insert("worktreePath".into(), json!(v));
            }
            single(model, command, "thread.meta-updated", Value::Object(patch), ctx)
        }
        Body::RuntimeModeSet { runtime_mode } => {
            let at = (ctx.now)();
            single(model, command, "thread.runtime-mode-set", json!({ "threadId": thread_id, "runtimeMode": runtime_mode, "updatedAt": at }), ctx)
        }
        Body::InteractionModeSet { interaction_mode } => {
            let at = (ctx.now)();
            single(model, command, "thread.interaction-mode-set", json!({ "threadId": thread_id, "interactionMode": interaction_mode, "updatedAt": at }), ctx)
        }
        Body::TurnStart { message_id, text, attachments, model_selection, runtime_mode, interaction_mode, source_proposed_plan, bootstrap } => {
            let existing = model.threads.get(thread_id);
            if existing.is_none() && bootstrap.is_none() {
                return reject("thread_not_found", format!("thread {thread_id} does not exist"));
            }
            if existing.is_some_and(has_open_blocking_request) {
                return reject("request_pending", format!("thread {thread_id} has a pending approval or user-input request"));
            }
            let created_at = (ctx.now)();
            let queued = existing.and_then(|t| t.latest_turn.as_ref()).is_some_and(|t| t.state == "running");
            let mut planned = Vec::new();
            if let (None, Some(b)) = (existing, bootstrap) {
                let new = NewThread { title: &b.title, cwd: b.cwd.as_deref().unwrap_or(ctx.default_cwd), selection: &b.model_selection, runtime: &b.runtime_mode, interaction: &b.interaction_mode, branch: &None, worktree_path: &None };
                planned.push(thread_created(thread_id, new, &created_at));
            }
            let mut message = Map::new();
            message.insert("id".into(), json!(message_id));
            message.insert("role".into(), json!("user"));
            message.insert("text".into(), json!(text));
            if let Some(a) = attachments {
                message.insert("attachments".into(), json!(a));
            }
            message.insert("createdAt".into(), json!(created_at));
            message.insert("updatedAt".into(), json!(created_at));
            planned.push(Planned { kind: "thread.message-sent", thread_id: thread_id.into(), payload: json!({ "message": message }) });

            let mut start = Map::new();
            start.insert("threadId".into(), json!(thread_id));
            start.insert("messageId".into(), json!(message_id));
            if let Some(selection) = model_selection {
                start.insert("modelSelection".into(), selection.to_json());
            }
            start.insert("runtimeMode".into(), json!(runtime_mode));
            start.insert("interactionMode".into(), json!(interaction_mode));
            if let Some(plan) = source_proposed_plan {
                start.insert("sourceProposedPlan".into(), plan.clone());
            }
            start.insert("createdAt".into(), json!(created_at));
            if queued {
                start.insert("queued".into(), json!(true));
            }
            planned.push(Planned { kind: "thread.turn-start-requested", thread_id: thread_id.into(), payload: Value::Object(start) });
            Decision::Accepted(materialize(model, planned, &command.command_id, ctx))
        }
        Body::TurnInterrupt { turn_id, created_at } => {
            let running = model.threads.get(thread_id).and_then(|t| t.latest_turn.as_ref()).is_some_and(|t| t.state == "running");
            if !running {
                return reject("no_active_turn", format!("thread {thread_id} has no running turn"));
            }
            let mut payload = Map::new();
            payload.insert("threadId".into(), json!(thread_id));
            if let Some(turn) = turn_id {
                payload.insert("turnId".into(), json!(turn));
            }
            payload.insert("createdAt".into(), json!(created_at));
            single(model, command, "thread.turn-interrupt-requested", Value::Object(payload), ctx)
        }
        Body::ApprovalRespond { request_id, decision, created_at } => match model.threads.get(thread_id) {
            Some(thread) if thread_has_open_request(thread, request_id) => {
                single(model, command, "thread.approval-response-requested", json!({ "threadId": thread_id, "requestId": request_id, "decision": decision, "createdAt": created_at }), ctx)
            }
            _ => reject("request_not_open", format!("request {request_id} is not open on thread {thread_id}")),
        },
        Body::UserInputRespond { request_id, answers, created_at } => match model.threads.get(thread_id) {
            Some(thread) if thread_has_open_request(thread, request_id) => {
                single(model, command, "thread.user-input-response-requested", json!({ "threadId": thread_id, "requestId": request_id, "answers": answers, "createdAt": created_at }), ctx)
            }
            _ => reject("request_not_open", format!("request {request_id} is not open on thread {thread_id}")),
        },
        Body::SessionStop { created_at } => single(model, command, "thread.session-stop-requested", json!({ "threadId": thread_id, "createdAt": created_at }), ctx),
        Body::SessionSet { .. } | Body::ActivityAppend { .. } | Body::ProposedPlanUpsert { .. } | Body::TurnDiffUpdate { .. } | Body::MessageAppend { .. } | Body::NeutralEvent { .. } => {
            reject("internal_command", "runtime commands are not decided".into())
        }
        lifecycle_command => match lifecycle::decide(model, thread_id, lifecycle_command, ctx) {
            Err(refusal) => reject(refusal.code, refusal.message),
            Ok(drafts) => {
                let planned = drafts.into_iter().map(|d| Planned { kind: d.kind, thread_id: d.thread_id, payload: d.payload }).collect();
                Decision::Accepted(materialize(model, planned, &command.command_id, ctx))
            }
        },
    }
}

/// The runtime-only commands never go through `decide`: each is one event, and a message append accumulates onto the message it
/// extends.
pub fn materialize_internal(model: &ReadModel, command: &Command, ctx: &Context) -> Vec<Event> {
    let thread_id = command.thread_id.clone();
    let planned = match &command.body {
        Body::SessionSet { session } => Planned { kind: "thread.session-set", thread_id, payload: json!({ "session": session }) },
        Body::ActivityAppend { activity } => Planned { kind: "thread.activity-appended", thread_id, payload: json!({ "activity": activity }) },
        Body::ProposedPlanUpsert { plan } => Planned { kind: "thread.proposed-plan-upserted", thread_id, payload: json!({ "proposedPlan": plan }) },
        Body::TurnDiffUpdate { turn_id, unified_diff } => Planned { kind: "thread.turn-diff-updated", thread_id, payload: json!({ "turnId": turn_id, "unifiedDiff": unified_diff }) },
        Body::NeutralEvent { turn_id, event } => {
            let mut payload = Map::new();
            if let Some(turn) = turn_id {
                payload.insert("turnId".into(), json!(turn));
            }
            payload.insert("event".into(), event.clone());
            Planned { kind: NEUTRAL_KIND, thread_id, payload: Value::Object(payload) }
        }
        Body::MessageAppend { message_id, delta, turn_id } => {
            let existing = model.threads.get(&thread_id).and_then(|t| t.messages.iter().find(|m| &m.id == message_id));
            let now = (ctx.now)();
            let mut message = Map::new();
            message.insert("id".into(), json!(message_id));
            message.insert("role".into(), json!("assistant"));
            message.insert("text".into(), json!(existing.map_or_else(|| delta.clone(), |m| format!("{}{delta}", m.text))));
            if let Some(turn) = turn_id {
                message.insert("turnId".into(), json!(turn));
            }
            message.insert("createdAt".into(), json!(existing.map_or_else(|| now.clone(), |m| m.created_at.clone())));
            message.insert("updatedAt".into(), json!(now));
            Planned { kind: "thread.message-sent", thread_id, payload: json!({ "message": message }) }
        }
        _ => return Vec::new(),
    };
    materialize(model, vec![planned], &command.command_id, ctx)
}
