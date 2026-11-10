//! The lifecycle of a thread of the agent API, as T3 rules it (`references/t3code`, `orchestration-v2`: `Orchestrator.ts` `dispatchThreadFork`,
//! `dispatchThreadMergeBack`, `dispatchQueued*`, `dispatchCheckpointRollback`, `dispatchPreparedRunRetry`, `dispatchProviderSessionDetach`,
//! `dispatchThreadUserInputDismiss` and the thread mutations of `dispatchThreadMutation`): the requests a turn waits on, the queue, settling and
//! snoozing, archive and delete, fork and merge back, a forced stop, a rollback, a retry and a detach.
//!
//! Pure. A command is decided into neutral events of the thread (`{type, ...}` in the payload of a `thread.neutral-event`, the shapes of
//! `agent-api-agent`'s `requests`, `queue` and `lifecycle` modules), and the same events are folded back into the thread's state, so the state is
//! whatever its log says. The refusals carry the contract's codes and messages (`rca-service-core` checks the two against each other).

use super::decider::Context;
use super::ids::iso_from_ms;
use super::projector::{NeutralThread, ReadModel};
use super::types::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// The kinds of the thirteen neutral events of `agent.threads`, which the lifecycle never changes.
pub const THREAD_EVENT_KINDS: [&str; 13] = [
    "turn.started",
    "message.delta",
    "reasoning.delta",
    "tool.started",
    "tool.output",
    "tool.finished",
    "file.changed",
    "approval.requested",
    "input.requested",
    "plan.updated",
    "usage",
    "error",
    "turn.finished",
];

/// What a fork keeps of the turns it copies: those events, and the others that belong to a turn's own history.
const TURN_HISTORY_KINDS: [&str; 6] = ["approval.answered", "input.answered", "input.dismissed", "turn.steered", "turn.retried", "workflow.stopped"];

fn is_false(b: &bool) -> bool {
    !*b
}

/// A thread's settle and snooze state. `state` is the person's choice, `settled` or `active`; a thread that never settled has none of it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settle {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settled_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsettled_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snoozed_until_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snoozed_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub woke_at_ms: Option<u64>,
}

/// Where a thread came from. A thread with none is the root of its own family.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lineage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relationship: Option<String>,
    pub root: String,
}

/// A prompt that waits in the queue.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedPrompt {
    pub item: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction_mode: Option<String>,
    pub created_at_ms: u64,
}

/// The context of a fork that waits for the thread's next prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingMergeBack {
    pub source: String,
    pub context: String,
}

/// What the lifecycle keeps in a thread. Every field is left out of the thread's JSON while it is empty, so a thread that has none of it
/// serialises to the bytes it always did.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Lifecycle {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settle: Option<Settle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineage: Option<Lineage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queue: Vec<QueuedPrompt>,
    #[serde(rename = "queueHeld", default, skip_serializing_if = "is_false")]
    pub queue_held: bool,
    #[serde(rename = "pendingMergeBack", default, skip_serializing_if = "Option::is_none")]
    pub pending_merge_back: Option<PendingMergeBack>,
}

/// A turn as the thread's events tell it: what the person asked, what the agent said, how it stands.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnRecord {
    pub id: String,
    /// `running`, `completed`, `interrupted` or `error`.
    pub state: String,
    pub prompt: String,
    pub reply: String,
    pub started_at: String,
    pub completed_at: Option<String>,
}

impl TurnRecord {
    pub fn running(&self) -> bool {
        self.state == "running"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
    Approval,
    Input,
}

/// An approval or a question the turn waits on.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenRequest {
    pub id: String,
    pub kind: RequestKind,
    /// The turn that asked.
    pub turn: Option<String>,
}

/// What `SettleSet` puts the thread in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettleTo {
    Settled,
    Active,
    Snoozed,
    Awake,
}

impl SettleTo {
    pub fn name(self) -> &'static str {
        match self {
            SettleTo::Settled => "settled",
            SettleTo::Active => "active",
            SettleTo::Snoozed => "snoozed",
            SettleTo::Awake => "awake",
        }
    }
}

/// An event a command decided: the engine event it becomes is `kind` of `thread_id`.
pub struct Draft {
    pub kind: &'static str,
    pub thread_id: String,
    pub payload: Value,
}

/// A command the thread's state refuses: the contract's `code` and its `message`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
}

fn refuse<T>(code: &'static str, message: String) -> Result<T, Refusal> {
    Err(Refusal { code, message })
}

/// The messages the contract gives each refusal (`agent-api-agent`'s `message` modules).
pub mod msg {
    pub fn thread_not_found(thread: &str) -> String {
        format!("no thread has the id {thread}")
    }

    pub fn turn_running(thread: &str) -> String {
        format!("thread {thread} already has a running turn")
    }

    pub fn no_turn(thread: &str, turn: Option<&str>) -> String {
        match turn {
            Some(turn) => format!("thread {thread} is not running turn {turn}"),
            None => format!("thread {thread} has no running turn"),
        }
    }

    pub fn request_not_open(thread: &str, request: &str) -> String {
        format!("request {request} is not open on thread {thread}")
    }

    pub fn wrong_kind(method: &str, request: &str, kind: &str) -> String {
        match (method, kind) {
            ("input.dismiss", "approval") => format!("approval {request} needs an answer: answer it or stop the turn"),
            _ => format!("request {request} is {kind}, which {method} does not answer"),
        }
    }

    pub fn item_not_queued(item: &str) -> String {
        format!("Run {item} is not queued.")
    }

    pub fn target_not_queued(item: &str) -> String {
        format!("Queue target {item} is not queued.")
    }

    pub fn thread_not_active(thread: &str) -> String {
        format!("Thread {thread} is not active.")
    }

    pub const NOT_STOPPED: &str = "A queued user message can only be sent now while the thread is stopped.";

    pub fn queue_held(thread: &str) -> String {
        format!("the queue of thread {thread} is held by a forced stop: resume it first")
    }

    pub fn no_defaults(thread: &str) -> String {
        format!("thread {thread} has no workspace or driver and the queued prompt names none")
    }

    pub fn already_archived(thread: &str) -> String {
        format!("thread {thread} is already archived")
    }

    pub fn not_archived(thread: &str) -> String {
        format!("thread {thread} is not archived")
    }

    pub fn thread_exists(thread: &str) -> String {
        format!("thread {thread} already exists")
    }

    pub fn fork_source_running(turn: &str) -> String {
        format!("Fork source run {turn} is running; in-progress and rolled-back runs cannot be forked.")
    }

    pub fn no_stable_fork(point: &str) -> String {
        format!("No stable source run was found for fork source {point}.")
    }

    pub fn no_stable_merge(point: &str) -> String {
        format!("No stable source run was found for merge-back source {point}.")
    }

    pub fn snooze_not_future(thread: &str, until_ms: u64) -> String {
        format!("Thread {thread} snooze wake time {until_ms} is not in the future.")
    }

    pub fn snooze_request_open(thread: &str) -> String {
        format!("Thread {thread} has a pending approval or user-input request and cannot be snoozed.")
    }

    pub fn snooze_queued(thread: &str) -> String {
        format!("Thread {thread} has a queued run and cannot be snoozed.")
    }

    pub fn snooze_archived(thread: &str) -> String {
        format!("Thread {thread} is archived and cannot be snoozed.")
    }

    pub fn superseded(thread: &str, turn: &str) -> String {
        format!("Thread {thread} has started a newer turn than {turn}; nothing was stopped.")
    }

    pub fn not_a_fork(source: &str, target: &str) -> String {
        format!("Thread {source} is not a fork of {target}.")
    }

    pub const TARGET_BUSY: &str = "Finish or stop the target thread's active work and clear its queue before merging back.";

    pub fn merge_pending(target: &str) -> String {
        format!("Thread {target} already has pending context from another side chat or fork. Send a message there first.")
    }

    pub fn source_not_finished(turn: &str, state: &str) -> String {
        format!("Merge-back source run {turn} is {state}; only provider-finished runs are supported.")
    }

    pub fn turn_not_found(thread: &str, turn: &str) -> String {
        format!("thread {thread} has no turn {turn}")
    }

    pub fn session_not_owned(thread: &str, session: &str) -> String {
        format!("Provider session {session} does not belong to thread {thread}.")
    }

    pub fn not_retryable(turn: &str, why: &str) -> String {
        format!("Turn {turn} cannot be retried: {why}")
    }
}

fn text(value: &Value, name: &str) -> Option<String> {
    value.get(name).and_then(Value::as_str).map(str::to_string)
}

fn strings(value: &Value, name: &str) -> Vec<String> {
    value.get(name).and_then(Value::as_array).map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
}

fn at(iso: &str) -> u64 {
    super::ids::ms_from_iso(iso).unwrap_or(0)
}

// ---------------------------------------------------------------------------------------------------------------- folding

/// Folds one neutral event (`event` is `{type, ...}`, `turn` the turn it belongs to) of an agent API thread into its state. The thread and its
/// neutral part must exist; the projector checks. `at_iso` is when the engine committed the event.
pub fn fold(model: &mut ReadModel, thread_id: &str, turn: Option<&str>, event: &Value, at_iso: &str) {
    let Some(kind) = event.get("type").and_then(Value::as_str) else { return };
    let mut dropped_turns: Vec<String> = Vec::new();
    {
        let (Some(thread), Some(neutral)) = (model.threads.get_mut(thread_id), model.neutral.get_mut(thread_id)) else { return };
        match kind {
            "turn.started" => {
                if let Some(id) = turn {
                    match neutral.turns.iter_mut().find(|t| t.id == id) {
                        Some(known) => {
                            known.state = "running".into();
                            known.completed_at = None;
                        }
                        None => neutral.turns.push(TurnRecord { id: id.into(), state: "running".into(), prompt: String::new(), reply: String::new(), started_at: at_iso.into(), completed_at: None }),
                    }
                }
            }
            "message.delta" => {
                if let (Some(id), Some(delta)) = (turn, event.get("text").and_then(Value::as_str)) {
                    if let Some(record) = neutral.turns.iter_mut().find(|t| t.id == id) {
                        match event.get("role").and_then(Value::as_str) {
                            Some("user") => record.prompt.push_str(delta),
                            _ => record.reply.push_str(delta),
                        }
                    }
                }
            }
            "turn.finished" => {
                if let Some(id) = turn {
                    if let Some(record) = neutral.turns.iter_mut().find(|t| t.id == id && t.running()) {
                        record.state = match event.get("reason").and_then(Value::as_str) {
                            Some("interrupted") => "interrupted",
                            Some("failed") => "error",
                            _ => "completed",
                        }
                        .into();
                        record.completed_at = Some(at_iso.into());
                    }
                    neutral.requests.retain(|r| r.turn.as_deref() != Some(id));
                }
            }
            "approval.requested" | "input.requested" => {
                let (key, kind) = if kind == "approval.requested" { ("approval", RequestKind::Approval) } else { ("request", RequestKind::Input) };
                if let Some(id) = text(event, key) {
                    if !neutral.requests.iter().any(|r| r.id == id) {
                        neutral.requests.push(OpenRequest { id, kind, turn: turn.map(str::to_string) });
                    }
                }
            }
            "approval.answered" => neutral.requests.retain(|r| Some(&r.id) != text(event, "approval").as_ref()),
            "input.answered" | "input.dismissed" => neutral.requests.retain(|r| Some(&r.id) != text(event, "request").as_ref()),
            "queue.added" => {
                if let Some(item) = event.get("item").and_then(|v| serde_json::from_value::<QueuedPrompt>(v.clone()).ok()) {
                    if !thread.lifecycle.queue.iter().any(|q| q.item == item.item) {
                        thread.lifecycle.queue.push(item);
                    }
                }
            }
            "queue.edited" => {
                if let (Some(item), Some(prompt)) = (text(event, "item"), text(event, "prompt")) {
                    if let Some(queued) = thread.lifecycle.queue.iter_mut().find(|q| q.item == item) {
                        queued.prompt = prompt;
                    }
                }
            }
            "queue.moved" => {
                let order = strings(event, "order");
                let queue = std::mem::take(&mut thread.lifecycle.queue);
                let (mut sorted, rest): (Vec<_>, Vec<_>) = queue.into_iter().partition(|q| order.contains(&q.item));
                sorted.sort_by_key(|q| order.iter().position(|id| *id == q.item));
                sorted.extend(rest);
                thread.lifecycle.queue = sorted;
            }
            "queue.deleted" | "queue.started" => {
                if let Some(item) = text(event, "item") {
                    thread.lifecycle.queue.retain(|q| q.item != item);
                }
            }
            "turn.steered" => {
                if let Some(item) = text(event, "item") {
                    thread.lifecycle.queue.retain(|q| q.item != item);
                }
            }
            "queue.held" => thread.lifecycle.queue_held = true,
            "queue.resumed" => {
                thread.lifecycle.queue_held = false;
                if let Some(item) = text(event, "item") {
                    if let Some(at) = thread.lifecycle.queue.iter().position(|q| q.item == item) {
                        let first = thread.lifecycle.queue.remove(at);
                        thread.lifecycle.queue.insert(0, first);
                    }
                }
            }
            "thread.archived" => thread.archived_at = Some(iso_from_ms(event.get("at_ms").and_then(Value::as_u64).unwrap_or_else(|| at(at_iso)))),
            "thread.unarchived" => thread.archived_at = None,
            "thread.deleted" => thread.deleted_at = Some(iso_from_ms(event.get("at_ms").and_then(Value::as_u64).unwrap_or_else(|| at(at_iso)))),
            "thread.settle_changed" => {
                thread.lifecycle.settle = event.get("settle").and_then(|v| serde_json::from_value::<Settle>(v.clone()).ok()).filter(|s| *s != Settle::default());
            }
            "thread.forked" => neutral.inherited = strings(event, "turns"),
            "thread.merged_back" => {
                if let (Some(source), Some(context)) = (text(event, "source"), text(event, "context")) {
                    thread.lifecycle.pending_merge_back = Some(PendingMergeBack { source, context });
                }
            }
            "thread.merge_consumed" => thread.lifecycle.pending_merge_back = None,
            "thread.rolled_back" => {
                dropped_turns = strings(event, "dropped");
                neutral.turns.retain(|t| !dropped_turns.contains(&t.id));
                neutral.inherited.retain(|t| !dropped_turns.contains(t));
                neutral.requests.retain(|r| r.turn.as_ref().is_none_or(|t| !dropped_turns.contains(t)));
                thread.messages.retain(|m| m.turn_id.as_ref().is_none_or(|t| !dropped_turns.contains(t)));
                thread.activities.retain(|a| a.turn_id.as_ref().is_none_or(|t| !dropped_turns.contains(t)));
                thread.proposed_plans.retain(|p| p.turn_id.as_ref().is_none_or(|t| !dropped_turns.contains(t)));
                thread.latest_turn = text(event, "to_turn").and_then(|to| neutral.turns.iter().find(|t| t.id == to)).map(|t| LatestTurn { turn_id: t.id.clone(), state: t.state.clone(), requested_at: t.started_at.clone(), started_at: Some(t.started_at.clone()), completed_at: t.completed_at.clone() });
            }
            "session.detached" => {
                if let Some(session) = thread.session.as_mut() {
                    session.provider_session_id = None;
                }
            }
            _ => {}
        }
    }
    for turn in dropped_turns {
        model.turn_diffs.remove(&turn);
    }
}

// --------------------------------------------------------------------------------------------------------------- deciding

fn neutral_draft(thread_id: &str, turn: Option<&str>, event: Value) -> Draft {
    let mut payload = Map::new();
    if let Some(turn) = turn {
        payload.insert("turnId".into(), json!(turn));
    }
    payload.insert("event".into(), event);
    Draft { kind: super::decider::NEUTRAL_KIND, thread_id: thread_id.to_string(), payload: Value::Object(payload) }
}

fn running_turn(neutral: &NeutralThread) -> Option<&TurnRecord> {
    neutral.turns.iter().rev().find(|t| t.running())
}

fn open_request<'a>(neutral: &'a NeutralThread, id: &str) -> Option<&'a OpenRequest> {
    neutral.requests.iter().find(|r| r.id == id)
}

fn active(thread: &Thread) -> bool {
    thread.archived_at.is_none() && thread.deleted_at.is_none()
}

/// The thread a command is about and its agent API part, or the refusal of a thread that is none.
fn target<'a>(model: &'a ReadModel, thread_id: &str) -> Result<(&'a Thread, &'a NeutralThread), Refusal> {
    match (model.threads.get(thread_id), model.neutral.get(thread_id)) {
        (Some(thread), Some(neutral)) => Ok((thread, neutral)),
        _ => refuse("THREAD_NOT_FOUND", msg::thread_not_found(thread_id)),
    }
}

/// The turn a request was opened by, for the envelope of the event that closes it.
fn request_turn(neutral: &NeutralThread, id: &str) -> Option<String> {
    open_request(neutral, id).and_then(|r| r.turn.clone())
}

/// Decides a lifecycle command against the thread's state. `Ok` with no drafts is a command that changes nothing (a retried add, a delete of a
/// thread already gone).
pub fn decide(model: &ReadModel, thread_id: &str, body: &Body, ctx: &Context) -> Result<Vec<Draft>, Refusal> {
    match body {
        Body::Fork { source, at_turn, title } => return fork(model, thread_id, source, at_turn.as_deref(), title.as_deref(), ctx),
        Body::MergeBack { source, turn } => return merge_back(model, thread_id, source, turn.as_deref()),
        _ => {}
    }
    let (thread, neutral) = target(model, thread_id)?;
    let one = |turn: Option<&str>, event: Value| Ok(vec![neutral_draft(thread_id, turn, event)]);
    match body {
        Body::ApprovalAnswer { approval, decision } => match open_request(neutral, approval) {
            None => refuse("REQUEST_NOT_OPEN", msg::request_not_open(thread_id, approval)),
            Some(r) if r.kind != RequestKind::Approval => refuse("WRONG_REQUEST_KIND", msg::wrong_kind("approval.answer", approval, "a question")),
            Some(_) => one(request_turn(neutral, approval).as_deref(), json!({"type": "approval.answered", "approval": approval, "decision": decision})),
        },
        Body::InputAnswer { request, answer } => match open_request(neutral, request) {
            None => refuse("REQUEST_NOT_OPEN", msg::request_not_open(thread_id, request)),
            Some(r) if r.kind != RequestKind::Input => refuse("WRONG_REQUEST_KIND", msg::wrong_kind("input.answer", request, "an approval")),
            Some(_) => one(request_turn(neutral, request).as_deref(), json!({"type": "input.answered", "request": request, "answer": answer})),
        },
        Body::InputDismiss { request } => match open_request(neutral, request) {
            None => refuse("REQUEST_NOT_OPEN", msg::request_not_open(thread_id, request)),
            Some(r) if r.kind != RequestKind::Input => refuse("WRONG_REQUEST_KIND", msg::wrong_kind("input.dismiss", request, "approval")),
            Some(_) => one(request_turn(neutral, request).as_deref(), json!({"type": "input.dismissed", "request": request})),
        },
        Body::QueueAdd { item, prompt, attachments, workspace, driver, model: model_name, runtime_mode, interaction_mode } => {
            let id = item.clone().unwrap_or_else(|| (ctx.new_id)());
            if thread.lifecycle.queue.iter().any(|q| q.item == id) {
                return Ok(Vec::new());
            }
            if workspace.is_none() && neutral.workspace.is_none() || driver.is_none() && neutral.driver.is_none() {
                return refuse("NO_DEFAULTS", msg::no_defaults(thread_id));
            }
            let queued = QueuedPrompt {
                item: id,
                prompt: prompt.clone(),
                attachments: attachments.clone(),
                workspace: workspace.clone(),
                driver: driver.clone(),
                model: model_name.clone(),
                runtime_mode: runtime_mode.clone(),
                interaction_mode: interaction_mode.clone(),
                created_at_ms: ctx.now_ms,
            };
            let position = thread.lifecycle.queue.len() + 1;
            one(None, json!({"type": "queue.added", "item": queued, "position": position}))
        }
        Body::QueueEdit { item, prompt } => match thread.lifecycle.queue.iter().any(|q| q.item == *item) {
            true => one(None, json!({"type": "queue.edited", "item": item, "prompt": prompt})),
            false => refuse("ITEM_NOT_QUEUED", msg::item_not_queued(item)),
        },
        Body::QueueMove { item, before } => {
            let queue = &thread.lifecycle.queue;
            if !queue.iter().any(|q| q.item == *item) {
                return refuse("ITEM_NOT_QUEUED", msg::item_not_queued(item));
            }
            let mut rest: Vec<&str> = queue.iter().map(|q| q.item.as_str()).filter(|id| id != item).collect();
            let at = match before {
                None => rest.len(),
                Some(before) => match rest.iter().position(|id| id == before) {
                    Some(at) => at,
                    None => return refuse("ITEM_NOT_QUEUED", msg::target_not_queued(before)),
                },
            };
            rest.insert(at, item);
            if rest.iter().copied().eq(queue.iter().map(|q| q.item.as_str())) {
                return Ok(Vec::new());
            }
            one(None, json!({"type": "queue.moved", "order": rest}))
        }
        Body::QueueDelete { item } => match thread.lifecycle.queue.iter().any(|q| q.item == *item) {
            true => one(None, json!({"type": "queue.deleted", "item": item})),
            false => refuse("ITEM_NOT_QUEUED", msg::item_not_queued(item)),
        },
        Body::QueueResume { item } => {
            if !active(thread) {
                return refuse("THREAD_NOT_ACTIVE", msg::thread_not_active(thread_id));
            }
            if let Some(item) = item {
                if running_turn(neutral).is_some() {
                    return refuse("NOT_STOPPED", msg::NOT_STOPPED.into());
                }
                if !thread.lifecycle.queue.iter().any(|q| q.item == *item) {
                    return refuse("ITEM_NOT_QUEUED", msg::item_not_queued(item));
                }
            }
            let mut event = json!({"type": "queue.resumed"});
            if let Some(item) = item {
                event["item"] = json!(item);
            }
            one(None, event)
        }
        Body::QueueStart { item } => {
            if !active(thread) {
                return refuse("THREAD_NOT_ACTIVE", msg::thread_not_active(thread_id));
            }
            if running_turn(neutral).is_some() {
                return refuse("NOT_STOPPED", msg::NOT_STOPPED.into());
            }
            if thread.lifecycle.queue_held {
                return refuse("QUEUE_HELD", msg::queue_held(thread_id));
            }
            if thread.lifecycle.queue.first().map(|q| q.item.as_str()) != Some(item.as_str()) {
                return refuse("ITEM_NOT_QUEUED", msg::item_not_queued(item));
            }
            one(Some(item), json!({"type": "queue.started", "item": item}))
        }
        Body::Steer { turn, prompt, item } => {
            if !active(thread) {
                return refuse("THREAD_NOT_ACTIVE", msg::thread_not_active(thread_id));
            }
            let Some(running) = running_turn(neutral).filter(|r| turn.as_ref().is_none_or(|t| *t == r.id)) else {
                return refuse("NO_TURN", msg::no_turn(thread_id, turn.as_deref()));
            };
            let steer = match (prompt, item) {
                (Some(prompt), _) => (prompt.clone(), None),
                (None, Some(item)) => match thread.lifecycle.queue.iter().find(|q| q.item == *item) {
                    Some(queued) => (queued.prompt.clone(), Some(item.clone())),
                    None => return refuse("ITEM_NOT_QUEUED", msg::item_not_queued(item)),
                },
                (None, None) => return refuse("INVALID_PARAMS", "name exactly one of prompt and item".into()),
            };
            let mut event = json!({"type": "turn.steered", "prompt": steer.0});
            if let Some(item) = steer.1 {
                event["item"] = json!(item);
            }
            one(Some(&running.id), event)
        }
        Body::LifecycleArchive { archive } => match (archive, thread.archived_at.is_some()) {
            (true, true) => refuse("ALREADY_ARCHIVED", msg::already_archived(thread_id)),
            (false, false) => refuse("NOT_ARCHIVED", msg::not_archived(thread_id)),
            (true, false) => one(None, json!({"type": "thread.archived", "at_ms": ctx.now_ms})),
            (false, true) => one(None, json!({"type": "thread.unarchived", "at_ms": ctx.now_ms})),
        },
        Body::LifecycleDelete { only_if_unused } => {
            if thread.deleted_at.is_some() || (*only_if_unused && !neutral.turns.is_empty()) {
                return Ok(Vec::new());
            }
            one(None, json!({"type": "thread.deleted", "at_ms": ctx.now_ms}))
        }
        Body::SettleSet { to, until_ms, at_ms } => settle(thread_id, thread, neutral, *to, *until_ms, *at_ms, ctx),
        Body::ForceStop { turn } => match running_turn(neutral) {
            Some(running) if turn.as_ref().is_none_or(|t| *t == running.id) => {
                let id = running.id.clone();
                Ok(vec![neutral_draft(thread_id, Some(&id), json!({"type": "thread.force_stopped"})), neutral_draft(thread_id, Some(&id), json!({"type": "queue.held", "reason": "force_stop"}))])
            }
            Some(running) => refuse("SUPERSEDED", msg::superseded(thread_id, turn.as_deref().unwrap_or(&running.id))),
            None => refuse("NO_TURN", msg::no_turn(thread_id, turn.as_deref())),
        },
        Body::WorkflowStop { turn, workflow } => match running_turn(neutral) {
            Some(running) if running.id == *turn => {
                let mut event = json!({"type": "workflow.stopped"});
                if let Some(workflow) = workflow {
                    event["workflow"] = json!(workflow);
                }
                one(Some(turn), event)
            }
            _ => refuse("NO_TURN", msg::no_turn(thread_id, Some(turn))),
        },
        Body::Rollback { turn, restore_files } => {
            if running_turn(neutral).is_some() {
                return refuse("TURN_RUNNING", msg::turn_running(thread_id));
            }
            let Some(at) = neutral.turns.iter().position(|t| t.id == *turn) else { return refuse("TURN_NOT_FOUND", msg::turn_not_found(thread_id, turn)) };
            let dropped: Vec<&str> = neutral.turns[at + 1..].iter().map(|t| t.id.as_str()).collect();
            one(None, json!({"type": "thread.rolled_back", "to_turn": turn, "dropped": dropped, "restored_files": restore_files}))
        }
        Body::Detach { session, reason } => match thread.session.as_ref().and_then(|s| s.provider_session_id.as_ref()) {
            Some(own) if own == session => {
                let mut event = json!({"type": "session.detached", "session": session});
                if let Some(reason) = reason {
                    event["reason"] = json!(reason);
                }
                one(None, event)
            }
            _ => refuse("SESSION_NOT_OWNED", msg::session_not_owned(thread_id, session)),
        },
        Body::Retry { turn, new_turn } => {
            let Some(record) = neutral.turns.iter().find(|t| t.id == *turn) else { return refuse("TURN_NOT_FOUND", msg::turn_not_found(thread_id, turn)) };
            if running_turn(neutral).is_some() {
                return refuse("TURN_RUNNING", msg::turn_running(thread_id));
            }
            if neutral.turns.last().map(|t| t.id.as_str()) != Some(turn.as_str()) {
                return refuse("NOT_RETRYABLE", msg::not_retryable(turn, "it is not the newest turn"));
            }
            if record.state != "error" {
                return refuse("NOT_RETRYABLE", msg::not_retryable(turn, "it did not fail"));
            }
            one(Some(new_turn), json!({"type": "turn.retried", "retry_of": turn, "prompt": record.prompt}))
        }
        _ => refuse("INTERNAL", "not a lifecycle command".into()),
    }
}

fn settle(thread_id: &str, thread: &Thread, neutral: &NeutralThread, to: SettleTo, until_ms: Option<u64>, at_ms: Option<u64>, ctx: &Context) -> Result<Vec<Draft>, Refusal> {
    let mut next = thread.lifecycle.settle.clone().unwrap_or_default();
    let now = ctx.now_ms;
    match to {
        SettleTo::Settled => {
            let already = next.state.as_deref() == Some("settled") && next.settled_at_ms.is_some();
            if !already {
                next.settled_at_ms = Some(at_ms.unwrap_or(now));
            }
            next.state = Some("settled".into());
            next.unsettled_at_ms = None;
            next.snoozed_until_ms = None;
            next.snoozed_at_ms = None;
        }
        SettleTo::Active => {
            let already = next.state.as_deref() == Some("active");
            next.state = Some("active".into());
            next.settled_at_ms = None;
            if !already {
                next.unsettled_at_ms = Some(now);
            }
        }
        SettleTo::Snoozed => {
            let until = until_ms.unwrap_or(0);
            if until <= now {
                return refuse("SNOOZE_NOT_FUTURE", msg::snooze_not_future(thread_id, until));
            }
            if !neutral.requests.is_empty() {
                return refuse("SNOOZE_REQUEST_OPEN", msg::snooze_request_open(thread_id));
            }
            if !thread.lifecycle.queue.is_empty() {
                return refuse("SNOOZE_QUEUED", msg::snooze_queued(thread_id));
            }
            if thread.archived_at.is_some() {
                return refuse("SNOOZE_ARCHIVED", msg::snooze_archived(thread_id));
            }
            next.snoozed_until_ms = Some(until);
            next.snoozed_at_ms = Some(now);
        }
        SettleTo::Awake => {
            if next.snoozed_until_ms.is_some() {
                next.woke_at_ms = Some(now);
            }
            next.snoozed_until_ms = None;
            next.snoozed_at_ms = None;
        }
    }
    Ok(vec![neutral_draft(thread_id, None, json!({"type": "thread.settle_changed", "state": to.name(), "settle": next}))])
}

/// The turn of `neutral` a fork or merge back takes as its end: the one named (which must exist, and `point` is `run`), else the latest one that
/// completed (`latest_stable`).
fn source_turn<'a>(neutral: &'a NeutralThread, named: Option<&str>) -> Option<&'a TurnRecord> {
    match named {
        Some(id) => neutral.turns.iter().find(|t| t.id == id),
        None => neutral.turns.iter().rev().find(|t| t.state == "completed"),
    }
}

fn fork(model: &ReadModel, new_id: &str, source: &str, at_turn: Option<&str>, title: Option<&str>, ctx: &Context) -> Result<Vec<Draft>, Refusal> {
    if model.threads.contains_key(new_id) {
        return refuse("THREAD_EXISTS", msg::thread_exists(new_id));
    }
    let (origin, neutral) = target(model, source)?;
    let point = if at_turn.is_some() { "run" } else { "latest_stable" };
    let Some(end) = source_turn(neutral, at_turn) else { return refuse("NO_STABLE_TURN", msg::no_stable_fork(point)) };
    if end.running() {
        return refuse("FORK_SOURCE_RUNNING", msg::fork_source_running(&end.id));
    }
    let end_at = neutral.turns.iter().position(|t| t.id == end.id).unwrap_or(0);
    let copied: Vec<String> = neutral.turns[..=end_at].iter().filter(|t| !t.running()).map(|t| t.id.clone()).collect();
    let root = origin.lifecycle.lineage.as_ref().map_or_else(|| source.to_string(), |l| l.root.clone());
    let title = title.map_or_else(|| format!("{} fork", origin.title), str::to_string);
    let mut agent = Map::new();
    for (name, value) in [("workspace", &neutral.workspace), ("driver", &neutral.driver), ("model", &neutral.model)] {
        if let Some(value) = value {
            agent.insert(name.into(), json!(value));
        }
    }
    let created_at = (ctx.now)();
    let payload = json!({
        "title": title,
        "cwd": origin.cwd,
        "modelSelection": origin.model_selection,
        "runtimeMode": origin.runtime_mode,
        "interactionMode": origin.interaction_mode,
        "createdAt": created_at,
        "neutral": agent,
        "lineage": {"parent": source, "relationship": "fork", "root": root},
    });
    let mut drafts = vec![Draft { kind: "thread.created", thread_id: new_id.into(), payload }];
    drafts.push(neutral_draft(new_id, None, json!({"type": "thread.forked", "source": source, "turns": copied})));
    for event in ctx.source_events.iter().filter(|e| e.kind == super::decider::NEUTRAL_KIND) {
        let Some(turn) = text(&event.payload, "turnId").filter(|t| copied.contains(t)) else { continue };
        let Some(inner) = event.payload.get("event").filter(|inner| inner.get("type").and_then(Value::as_str).is_some_and(|k| THREAD_EVENT_KINDS.contains(&k) || TURN_HISTORY_KINDS.contains(&k))) else { continue };
        drafts.push(neutral_draft(new_id, Some(&turn), inner.clone()));
    }
    Ok(drafts)
}

/// What a fork did, as text for the parent's next prompt: each of its own turns (not those it inherited) up to `end`, the prompt and the reply.
fn merge_context(neutral: &NeutralThread, end: usize) -> String {
    neutral.turns[..=end]
        .iter()
        .filter(|t| !neutral.inherited.contains(&t.id))
        .map(|t| format!("User: {}\nAssistant: {}", t.prompt.trim(), t.reply.trim()))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn merge_back(model: &ReadModel, target_id: &str, source: &str, named: Option<&str>) -> Result<Vec<Draft>, Refusal> {
    let (parent, parent_neutral) = target(model, target_id)?;
    let (child, child_neutral) = target(model, source)?;
    let is_fork = child.lifecycle.lineage.as_ref().is_some_and(|l| l.relationship.as_deref() == Some("fork") && l.parent.as_deref() == Some(target_id));
    if !is_fork {
        return refuse("NOT_A_FORK", msg::not_a_fork(source, target_id));
    }
    if running_turn(parent_neutral).is_some() || !parent.lifecycle.queue.is_empty() {
        return refuse("TARGET_BUSY", msg::TARGET_BUSY.into());
    }
    if parent.lifecycle.pending_merge_back.as_ref().is_some_and(|p| p.source != source) {
        return refuse("MERGE_PENDING", msg::merge_pending(target_id));
    }
    let point = if named.is_some() { "run" } else { "latest_stable" };
    let Some(end) = source_turn(child_neutral, named) else { return refuse("NO_STABLE_TURN", msg::no_stable_merge(point)) };
    if end.state != "completed" {
        return refuse("SOURCE_NOT_FINISHED", msg::source_not_finished(&end.id, &end.state));
    }
    let end_at = child_neutral.turns.iter().position(|t| t.id == end.id).unwrap_or(0);
    let context = merge_context(child_neutral, end_at);
    Ok(vec![neutral_draft(target_id, None, json!({"type": "thread.merged_back", "source": source, "context": context}))])
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::domain::decider::Context;
    use crate::domain::projector::project;
    use serde_json::json;

    /// A model with threads folded from the events the tests record, the way the engine does: a thread, then neutral events one by one.
    pub(crate) struct Rig {
        pub(crate) model: ReadModel,
        seq: i64,
        pub(crate) now_ms: u64,
        /// Every event the rig projected, as a store holds them.
        log: Vec<Event>,
    }

    impl Rig {
        pub(crate) fn new() -> Rig {
            Rig { model: ReadModel::empty("2023-11-14T22:13:20.000Z"), seq: 0, now_ms: 1_700_000_000_000, log: Vec::new() }
        }

        pub(crate) fn event(&mut self, thread: &str, kind: &str, payload: Value) {
            self.seq += 1;
            let event = Event { event_id: format!("e{}", self.seq), sequence: self.seq, created_at: iso_from_ms(self.now_ms), thread_id: thread.into(), command_id: None, actor: "t".into(), kind: kind.into(), payload };
            project(&mut self.model, &event);
            self.log.push(event);
        }

        pub(crate) fn thread(&mut self, id: &str, workspace: Option<&str>, driver: Option<&str>) {
            let mut agent = Map::new();
            for (name, value) in [("workspace", workspace), ("driver", driver)] {
                if let Some(value) = value {
                    agent.insert(name.into(), json!(value));
                }
            }
            self.event(id, "thread.created", json!({"title": format!("title of {id}"), "cwd": "/work", "modelSelection": {"cli": "claudeAgent"}, "runtimeMode": "full-access", "interactionMode": "default", "createdAt": iso_from_ms(self.now_ms), "neutral": agent}));
        }

        pub(crate) fn say(&mut self, thread: &str, turn: Option<&str>, event: Value) {
            let mut payload = Map::new();
            if let Some(turn) = turn {
                payload.insert("turnId".into(), json!(turn));
            }
            payload.insert("event".into(), event);
            self.event(thread, "thread.neutral-event", Value::Object(payload));
        }

        /// A turn that ran: its prompt, its reply and how it ended (`running` leaves it open).
        pub(crate) fn turn(&mut self, thread: &str, id: &str, prompt: &str, reply: &str, ended: &str) {
            self.say(thread, Some(id), json!({"type": "turn.started"}));
            self.say(thread, Some(id), json!({"type": "message.delta", "role": "user", "text": prompt}));
            self.say(thread, Some(id), json!({"type": "message.delta", "role": "assistant", "text": reply}));
            if ended != "running" {
                self.say(thread, Some(id), json!({"type": "turn.finished", "reason": ended}));
            }
        }

        /// Decides `body`, then folds what it decided, and answers the events' payloads. A fork is given its source's log, as the engine does.
        pub(crate) fn run(&mut self, thread: &str, body: Body) -> Result<Vec<Value>, Refusal> {
            let source_events: Vec<Event> = match &body {
                Body::Fork { source, .. } => self.log.iter().filter(|e| e.thread_id == *source).cloned().collect(),
                _ => Vec::new(),
            };
            let now = || iso_from_ms(self.now_ms);
            let ids = std::cell::Cell::new(0);
            let new_id = || {
                ids.set(ids.get() + 1);
                format!("id{}", ids.get())
            };
            let ctx = Context { now: &now, new_id: &new_id, default_cwd: "/work", actor: "t", now_ms: self.now_ms, source_events: &source_events };
            let drafts = decide(&self.model, thread, &body, &ctx)?;
            let payloads = drafts.iter().map(|d| d.payload.clone()).collect();
            for draft in drafts {
                self.event(&draft.thread_id.clone(), draft.kind, draft.payload);
            }
            Ok(payloads)
        }

        pub(crate) fn lifecycle(&self, thread: &str) -> &Lifecycle {
            &self.model.threads[thread].lifecycle
        }
    }

    fn add(item: &str, prompt: &str) -> Body {
        Body::QueueAdd { item: Some(item.into()), prompt: prompt.into(), attachments: vec![], workspace: None, driver: None, model: None, runtime_mode: None, interaction_mode: None }
    }

    fn code<T>(result: Result<T, Refusal>) -> &'static str {
        result.err().expect("refused").code
    }

    #[test]
    fn given_a_thread_with_a_queue_when_items_are_added_edited_moved_and_deleted_then_the_queue_is_what_the_events_say_and_each_refusal_names_the_item() {
        // Given a thread that has a workspace and a driver, and three prompts queued, one of them added twice
        let mut rig = Rig::new();
        rig.thread("t", Some("w1"), Some("claudeAgent"));
        for (item, prompt) in [("q1", "one"), ("q2", "two"), ("q3", "three"), ("q1", "one again")] {
            rig.run("t", add(item, prompt)).unwrap();
        }
        // Then the repeat changed nothing, and the queue is in the order of the adds
        let ids = |rig: &Rig| rig.lifecycle("t").queue.iter().map(|q| q.item.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&rig), ["q1", "q2", "q3"]);
        assert_eq!(rig.lifecycle("t").queue[0].prompt, "one");
        // When q3 goes before q1, q1 is edited, q2 is deleted, and q1 is moved last
        rig.run("t", Body::QueueMove { item: "q3".into(), before: Some("q1".into()) }).unwrap();
        rig.run("t", Body::QueueEdit { item: "q1".into(), prompt: "better".into() }).unwrap();
        rig.run("t", Body::QueueDelete { item: "q2".into() }).unwrap();
        rig.run("t", Body::QueueMove { item: "q3".into(), before: None }).unwrap();
        // Then the queue is q1 (edited), q3
        assert_eq!((ids(&rig), rig.lifecycle("t").queue[0].prompt.as_str()), (vec!["q1".to_string(), "q3".to_string()], "better"));
        // And a move that changes nothing records nothing, and the refusals name the item or the target
        assert!(rig.run("t", Body::QueueMove { item: "q1".into(), before: Some("q3".into()) }).unwrap().is_empty());
        let gone = rig.run("t", Body::QueueDelete { item: "q2".into() }).unwrap_err();
        let target = rig.run("t", Body::QueueMove { item: "q1".into(), before: Some("ghost".into()) }).unwrap_err();
        assert_eq!((gone.code, gone.message), ("ITEM_NOT_QUEUED", "Run q2 is not queued.".to_string()));
        assert_eq!((target.code, target.message), ("ITEM_NOT_QUEUED", "Queue target ghost is not queued.".to_string()));
    }

    #[test]
    fn given_a_thread_with_no_workspace_or_driver_when_a_prompt_is_added_then_it_is_refused_unless_the_item_names_both() {
        // Given a thread that has a workspace and no driver
        let mut rig = Rig::new();
        rig.thread("t", Some("w1"), None);
        // When a prompt names nothing, then one names the driver
        let bare = rig.run("t", add("q1", "one"));
        let named = rig.run("t", Body::QueueAdd { item: Some("q2".into()), prompt: "two".into(), attachments: vec![], workspace: None, driver: Some("codex".into()), model: None, runtime_mode: None, interaction_mode: None });
        // Then the first is refused, and the second is queued
        assert_eq!(code(bare), "NO_DEFAULTS");
        assert!(named.is_ok());
        assert_eq!(rig.lifecycle("t").queue.len(), 1);
    }

    #[test]
    fn given_a_forced_stop_when_the_queue_is_started_or_resumed_then_it_waits_for_the_resume_and_starts_in_order_and_never_while_a_turn_runs() {
        // Given a thread with a running turn and two prompts queued
        let mut rig = Rig::new();
        rig.thread("t", Some("w1"), Some("claudeAgent"));
        rig.turn("t", "u1", "work", "…", "running");
        rig.run("t", add("q1", "one")).unwrap();
        rig.run("t", add("q2", "two")).unwrap();
        // When the turn is force stopped, naming a turn that is not the running one first
        let late = rig.run("t", Body::ForceStop { turn: Some("u0".into()) }).unwrap_err();
        let stopped = rig.run("t", Body::ForceStop { turn: Some("u1".into()) }).unwrap();
        rig.say("t", Some("u1"), json!({"type": "turn.finished", "reason": "interrupted"}));
        // Then the late stop was refused as superseded, the stop recorded the stop and the hold, and the queue is held
        assert_eq!(late.code, "SUPERSEDED");
        assert_eq!(stopped.iter().map(|p| p["event"]["type"].as_str().unwrap()).collect::<Vec<_>>(), ["thread.force_stopped", "queue.held"]);
        assert!(rig.lifecycle("t").queue_held);
        // And the held queue does not start, until it is resumed
        assert_eq!(code(rig.run("t", Body::QueueStart { item: "q1".into() })), "QUEUE_HELD");
        rig.run("t", Body::QueueResume { item: Some("q2".into()) }).unwrap();
        assert_eq!((rig.lifecycle("t").queue_held, rig.lifecycle("t").queue[0].item.as_str()), (false, "q2"));
        // And only the first of the queue starts, as a turn of the item's own id, and then it leaves the queue
        assert_eq!(code(rig.run("t", Body::QueueStart { item: "q1".into() })), "ITEM_NOT_QUEUED");
        let started = rig.run("t", Body::QueueStart { item: "q2".into() }).unwrap();
        assert_eq!(started[0]["turnId"], json!("q2"));
        assert_eq!(rig.lifecycle("t").queue.iter().map(|q| q.item.as_str()).collect::<Vec<_>>(), ["q1"]);
        // And while a turn runs nothing starts and nothing is sent now
        rig.turn("t", "q2", "two", "…", "running");
        assert_eq!(code(rig.run("t", Body::QueueStart { item: "q1".into() })), "NOT_STOPPED");
        assert_eq!(code(rig.run("t", Body::QueueResume { item: Some("q1".into()) })), "NOT_STOPPED");
    }

    #[test]
    fn given_an_archived_or_deleted_thread_when_its_queue_is_resumed_or_started_then_it_is_refused_as_not_active() {
        // Given a thread with a queued prompt, archived, and another deleted
        let mut rig = Rig::new();
        for id in ["a", "d"] {
            rig.thread(id, Some("w1"), Some("claudeAgent"));
            rig.run(id, add("q1", "one")).unwrap();
        }
        rig.run("a", Body::LifecycleArchive { archive: true }).unwrap();
        rig.run("d", Body::LifecycleDelete { only_if_unused: false }).unwrap();
        // When each is resumed and started, Then each is refused as not active, with T3's words
        for id in ["a", "d"] {
            let resumed = rig.run(id, Body::QueueResume { item: None }).unwrap_err();
            assert_eq!((resumed.code, resumed.message), ("THREAD_NOT_ACTIVE", format!("Thread {id} is not active.")));
            assert_eq!(code(rig.run(id, Body::QueueStart { item: "q1".into() })), "THREAD_NOT_ACTIVE");
        }
    }

    #[test]
    fn given_a_running_turn_when_it_is_steered_with_a_prompt_or_a_queued_item_then_the_turn_goes_on_and_a_taken_item_leaves_the_queue() {
        // Given a running turn u1 and a queued prompt
        let mut rig = Rig::new();
        rig.thread("t", Some("w1"), Some("claudeAgent"));
        rig.run("t", add("q1", "do this instead")).unwrap();
        assert_eq!(code(rig.run("t", Body::Steer { turn: None, prompt: Some("x".into()), item: None })), "NO_TURN");
        rig.turn("t", "u1", "work", "…", "running");
        // When it is steered with a prompt, naming the wrong turn first, then with the queued item, then with an item that is not queued
        let wrong = rig.run("t", Body::Steer { turn: Some("u0".into()), prompt: Some("x".into()), item: None });
        let prompt = rig.run("t", Body::Steer { turn: Some("u1".into()), prompt: Some("go left".into()), item: None }).unwrap();
        let item = rig.run("t", Body::Steer { turn: None, prompt: None, item: Some("q1".into()) }).unwrap();
        let missing = rig.run("t", Body::Steer { turn: None, prompt: None, item: Some("q1".into()) });
        // Then the wrong turn has none running, the steers are events of the running turn, the item's prompt is the steer and the item left the queue
        assert_eq!(code(wrong), "NO_TURN");
        assert_eq!((prompt[0]["turnId"].clone(), prompt[0]["event"]["prompt"].clone()), (json!("u1"), json!("go left")));
        assert_eq!((item[0]["event"]["prompt"].clone(), item[0]["event"]["item"].clone()), (json!("do this instead"), json!("q1")));
        assert!(rig.lifecycle("t").queue.is_empty());
        assert_eq!(code(missing), "ITEM_NOT_QUEUED");
    }

    #[test]
    fn given_open_requests_when_they_are_answered_or_dismissed_then_only_the_right_kind_closes_and_a_dismissed_approval_is_refused() {
        // Given a running turn that asks for an approval and a question
        let mut rig = Rig::new();
        rig.thread("t", Some("w1"), Some("claudeAgent"));
        rig.turn("t", "u1", "work", "…", "running");
        rig.say("t", Some("u1"), json!({"type": "approval.requested", "approval": "ap1", "tool": "run_command", "args": {}}));
        rig.say("t", Some("u1"), json!({"type": "input.requested", "request": "rq1", "prompt": "Which?"}));
        // When a question is answered as an approval, an approval as a question, an approval is dismissed, and an unknown request is answered
        let as_approval = rig.run("t", Body::ApprovalAnswer { approval: "rq1".into(), decision: "accept".into() }).unwrap_err();
        let as_question = rig.run("t", Body::InputAnswer { request: "ap1".into(), answer: "yes".into() });
        let dismissed_approval = rig.run("t", Body::InputDismiss { request: "ap1".into() }).unwrap_err();
        let unknown = rig.run("t", Body::InputAnswer { request: "nope".into(), answer: "x".into() }).unwrap_err();
        // Then each is refused with the contract's code and words
        assert_eq!(as_approval.code, "WRONG_REQUEST_KIND");
        assert_eq!(code(as_question), "WRONG_REQUEST_KIND");
        assert_eq!((dismissed_approval.code, dismissed_approval.message), ("WRONG_REQUEST_KIND", "approval ap1 needs an answer: answer it or stop the turn".to_string()));
        assert_eq!((unknown.code, unknown.message), ("REQUEST_NOT_OPEN", "request nope is not open on thread t".to_string()));
        // And the right answers close them, as events of the turn that asked, and an answered request is no longer open
        let approved = rig.run("t", Body::ApprovalAnswer { approval: "ap1".into(), decision: "accept_for_session".into() }).unwrap();
        rig.run("t", Body::InputDismiss { request: "rq1".into() }).unwrap();
        assert_eq!((approved[0]["turnId"].clone(), approved[0]["event"]["decision"].clone()), (json!("u1"), json!("accept_for_session")));
        assert!(rig.model.neutral["t"].requests.is_empty());
        assert_eq!(code(rig.run("t", Body::ApprovalAnswer { approval: "ap1".into(), decision: "accept".into() })), "REQUEST_NOT_OPEN");
    }

    #[test]
    fn given_a_turn_that_ends_with_a_request_open_when_it_finishes_then_the_request_is_no_longer_open() {
        // Given a turn that asked for an approval and was interrupted
        let mut rig = Rig::new();
        rig.thread("t", Some("w1"), Some("claudeAgent"));
        rig.turn("t", "u1", "work", "…", "running");
        rig.say("t", Some("u1"), json!({"type": "approval.requested", "approval": "ap1", "tool": "run_command", "args": {}}));
        assert_eq!(rig.model.neutral["t"].requests.len(), 1);
        // When the turn finishes, Then nothing waits on the person
        rig.say("t", Some("u1"), json!({"type": "turn.finished", "reason": "interrupted"}));
        assert!(rig.model.neutral["t"].requests.is_empty());
    }

    #[test]
    fn given_settle_and_snooze_when_they_are_asked_for_then_the_first_settle_time_is_kept_a_snooze_ends_a_snooze_and_each_refusal_names_why() {
        // Given a thread
        let mut rig = Rig::new();
        rig.thread("t", Some("w1"), Some("claudeAgent"));
        let now = rig.now_ms;
        let snooze = |until: u64| Body::SettleSet { to: SettleTo::Snoozed, until_ms: Some(until), at_ms: None };
        // When it is snoozed until a time that is not in the future, and until one that is
        let past = rig.run("t", snooze(now)).unwrap_err();
        rig.run("t", snooze(now + 60_000)).unwrap();
        // Then the first is refused, and the second is kept with when it began
        assert_eq!((past.code, past.message), ("SNOOZE_NOT_FUTURE", format!("Thread t snooze wake time {now} is not in the future.")));
        assert_eq!(rig.lifecycle("t").settle, Some(Settle { snoozed_until_ms: Some(now + 60_000), snoozed_at_ms: Some(now), ..Settle::default() }));
        // When time passes and the thread is settled, then settled again later
        rig.now_ms += 1_000;
        rig.run("t", Body::SettleSet { to: SettleTo::Settled, until_ms: None, at_ms: None }).unwrap();
        let first = rig.lifecycle("t").settle.clone().unwrap();
        rig.now_ms += 1_000;
        rig.run("t", Body::SettleSet { to: SettleTo::Settled, until_ms: None, at_ms: None }).unwrap();
        // Then settling ended the snooze, and the second settle kept the first time
        assert_eq!((first.state.as_deref(), first.snoozed_until_ms, first.settled_at_ms), (Some("settled"), None, Some(now + 1_000)));
        assert_eq!(rig.lifecycle("t").settle.as_ref().unwrap().settled_at_ms, Some(now + 1_000));
        // When it is made active, Then it is no longer settled and says when it stopped being so
        rig.run("t", Body::SettleSet { to: SettleTo::Active, until_ms: None, at_ms: None }).unwrap();
        assert_eq!(rig.lifecycle("t").settle.as_ref().map(|s| (s.state.clone(), s.settled_at_ms, s.unsettled_at_ms)), Some((Some("active".to_string()), None, Some(now + 2_000))));
    }

    #[test]
    fn given_a_snooze_that_is_asked_for_when_a_request_is_open_something_is_queued_or_the_thread_is_archived_then_each_is_refused_with_its_own_code() {
        // Given three threads: one with a question open, one with a prompt queued, one archived
        let mut rig = Rig::new();
        for id in ["asking", "queued", "archived"] {
            rig.thread(id, Some("w1"), Some("claudeAgent"));
        }
        rig.turn("asking", "u1", "work", "…", "running");
        rig.say("asking", Some("u1"), json!({"type": "input.requested", "request": "rq1", "prompt": "Which?"}));
        rig.run("queued", add("q1", "one")).unwrap();
        rig.run("archived", Body::LifecycleArchive { archive: true }).unwrap();
        // When each is snoozed until a time in the future, Then each is refused
        let until = rig.now_ms + 60_000;
        let snooze = Body::SettleSet { to: SettleTo::Snoozed, until_ms: Some(until), at_ms: None };
        let codes: Vec<&str> = ["asking", "queued", "archived"].iter().map(|id| code(rig.run(id, snooze.clone()))).collect();
        assert_eq!(codes, ["SNOOZE_REQUEST_OPEN", "SNOOZE_QUEUED", "SNOOZE_ARCHIVED"]);
        // And waking a thread that sleeps records when, and waking one that does not changes nothing
        rig.run("queued", Body::QueueDelete { item: "q1".into() }).unwrap();
        rig.run("queued", snooze.clone()).unwrap();
        rig.now_ms += 5;
        rig.run("queued", Body::SettleSet { to: SettleTo::Awake, until_ms: None, at_ms: None }).unwrap();
        rig.run("queued", Body::SettleSet { to: SettleTo::Awake, until_ms: None, at_ms: None }).unwrap();
        let settle = rig.lifecycle("queued").settle.clone().unwrap();
        assert_eq!((settle.snoozed_until_ms, settle.woke_at_ms), (None, Some(until - 60_000 + 5)));
    }

    #[test]
    fn given_threads_when_archived_and_unarchived_and_deleted_then_each_refuses_what_is_already_so_and_a_delete_only_if_unused_keeps_a_used_thread() {
        // Given a thread that ran a turn and one that did not
        let mut rig = Rig::new();
        rig.thread("used", Some("w1"), Some("claudeAgent"));
        rig.turn("used", "u1", "work", "done", "completed");
        rig.thread("idle", Some("w1"), Some("claudeAgent"));
        // When the used one is archived twice and unarchived twice
        rig.run("used", Body::LifecycleArchive { archive: true }).unwrap();
        let again = rig.run("used", Body::LifecycleArchive { archive: true }).unwrap_err();
        assert!(rig.model.threads["used"].archived_at.is_some());
        rig.run("used", Body::LifecycleArchive { archive: false }).unwrap();
        let back = rig.run("used", Body::LifecycleArchive { archive: false }).unwrap_err();
        // Then the repeats are refused with T3's words, and the thread is back
        assert_eq!((again.message, back.message), ("thread used is already archived".to_string(), "thread used is not archived".to_string()));
        assert!(rig.model.threads["used"].archived_at.is_none());
        // And a delete that asks for an unused thread leaves the used one and removes the idle one
        assert!(rig.run("used", Body::LifecycleDelete { only_if_unused: true }).unwrap().is_empty());
        assert_eq!(rig.run("idle", Body::LifecycleDelete { only_if_unused: true }).unwrap().len(), 1);
        assert!(rig.model.threads["idle"].deleted_at.is_some() && rig.model.threads["used"].deleted_at.is_none());
        assert!(rig.run("idle", Body::LifecycleDelete { only_if_unused: false }).unwrap().is_empty());
    }

    #[test]
    fn given_a_thread_with_finished_turns_and_a_running_one_when_it_is_forked_then_the_fork_holds_only_the_finished_turns_with_their_events_and_a_lineage() {
        // Given a source with a completed turn, a failed one and a running one, and a queue event in the first's log that is no part of its history
        let mut rig = Rig::new();
        rig.thread("src", Some("w1"), Some("claudeAgent"));
        rig.turn("src", "u1", "one", "first", "completed");
        rig.say("src", Some("u1"), json!({"type": "queue.started", "item": "q-old"}));
        rig.turn("src", "u2", "two", "second", "failed");
        rig.turn("src", "u3", "three", "partial", "running");
        let fork = |source: &str, at_turn: Option<&str>| Body::Fork { source: source.into(), at_turn: at_turn.map(String::from), title: None };
        // When it is forked, naming the running turn, then not naming a turn
        let running = rig.run("fork", fork("src", Some("u3"))).unwrap_err();
        let payloads = rig.run("fork", fork("src", None)).unwrap();
        // Then the running turn is refused, and the fork takes the latest completed turn (u1: u2 failed), not the later ones
        assert_eq!((running.code, running.message), ("FORK_SOURCE_RUNNING", "Fork source run u3 is running; in-progress and rolled-back runs cannot be forked.".to_string()));
        assert_eq!(payloads[1]["event"], json!({"type": "thread.forked", "source": "src", "turns": ["u1"]}));
        let copied: Vec<(Value, Value)> = payloads[2..].iter().map(|p| (p["turnId"].clone(), p["event"]["type"].clone())).collect();
        let kinds = ["turn.started", "message.delta", "message.delta", "turn.finished"];
        assert_eq!(copied, kinds.iter().map(|k| (json!("u1"), json!(k))).collect::<Vec<_>>());
        // And the fork is titled for its source, has its lineage, its defaults and the copied turn's prompt and reply
        let made = &rig.model.threads["fork"];
        assert_eq!(made.title, "title of src fork");
        assert_eq!(made.lifecycle.lineage, Some(Lineage { parent: Some("src".into()), relationship: Some("fork".into()), root: "src".into() }));
        assert_eq!((rig.model.neutral["fork"].workspace.as_deref(), rig.model.neutral["fork"].driver.as_deref()), (Some("w1"), Some("claudeAgent")));
        let record = &rig.model.neutral["fork"].turns[0];
        assert_eq!((record.id.as_str(), record.state.as_str(), record.prompt.as_str(), record.reply.as_str()), ("u1", "completed", "one", "first"));
        assert_eq!(rig.model.neutral["fork"].turns.len(), 1);
        // And a fork of that fork keeps the root, and a fork onto a thread that exists, of a thread nobody has, or of one with no finished turn, is refused
        rig.run("grandchild", fork("fork", None)).unwrap();
        assert_eq!(rig.model.threads["grandchild"].lifecycle.lineage.as_ref().map(|l| (l.parent.as_deref(), l.root.as_str())), Some((Some("fork"), "src")));
        assert_eq!(code(rig.run("fork", fork("src", None))), "THREAD_EXISTS");
        assert_eq!(code(rig.run("f0", fork("nobody", None))), "THREAD_NOT_FOUND");
        rig.thread("fresh", Some("w1"), None);
        let none = rig.run("f2", fork("fresh", None)).unwrap_err();
        assert_eq!((none.code, none.message), ("NO_STABLE_TURN", "No stable source run was found for fork source latest_stable.".to_string()));
        let named = rig.run("f3", fork("src", Some("ghost"))).unwrap_err();
        assert_eq!(named.message, "No stable source run was found for fork source run.");
    }

    #[test]
    fn given_a_fork_when_it_is_merged_back_then_the_parent_holds_the_context_of_the_forks_own_turns_and_each_refusal_says_why() {
        // Given a parent with a finished turn, and a fork of it that ran its own turn
        let mut rig = Rig::new();
        rig.thread("parent", Some("w1"), Some("claudeAgent"));
        rig.turn("parent", "p1", "start", "started", "completed");
        rig.run("kid", Body::Fork { source: "parent".into(), at_turn: None, title: Some("a side road".into()) }).unwrap();
        rig.thread("stranger", Some("w1"), Some("claudeAgent"));
        // When the stranger is merged into the parent, then the fork before it has finished a turn of its own
        let not_fork = rig.run("parent", Body::MergeBack { source: "stranger".into(), turn: None }).unwrap_err();
        let nothing = rig.run("parent", Body::MergeBack { source: "kid".into(), turn: Some("ghost".into()) }).unwrap_err();
        rig.turn("kid", "k1", "explore", "found it", "completed");
        let merged = rig.run("parent", Body::MergeBack { source: "kid".into(), turn: None }).unwrap();
        // Then the stranger is no fork of the parent, an unknown turn is no stable run, and the context is the fork's own turn and not the inherited one
        assert_eq!((not_fork.code, not_fork.message), ("NOT_A_FORK", "Thread stranger is not a fork of parent.".to_string()));
        assert_eq!(nothing.code, "NO_STABLE_TURN");
        assert_eq!(merged[0]["event"]["context"], json!("User: explore\nAssistant: found it"));
        assert_eq!(rig.lifecycle("parent").pending_merge_back, Some(PendingMergeBack { source: "kid".into(), context: "User: explore\nAssistant: found it".into() }));
        // And a second fork cannot merge while the first waits, the parent that runs or has a queue refuses, and a consumed merge frees it
        rig.run("kid2", Body::Fork { source: "parent".into(), at_turn: None, title: None }).unwrap();
        rig.turn("kid2", "k2", "x", "y", "completed");
        assert_eq!(code(rig.run("parent", Body::MergeBack { source: "kid2".into(), turn: None })), "MERGE_PENDING");
        rig.say("parent", None, json!({"type": "thread.merge_consumed", "source": "kid"}));
        rig.run("parent", add("q1", "one")).unwrap();
        assert_eq!(code(rig.run("parent", Body::MergeBack { source: "kid2".into(), turn: None })), "TARGET_BUSY");
        rig.run("parent", Body::QueueDelete { item: "q1".into() }).unwrap();
        assert!(rig.run("parent", Body::MergeBack { source: "kid2".into(), turn: None }).is_ok());
        // And a fork whose turn has not completed is refused saying how the turn stands
        rig.turn("kid2", "k3", "x", "y", "running");
        rig.say("parent", None, json!({"type": "thread.merge_consumed", "source": "kid2"}));
        let open = rig.run("parent", Body::MergeBack { source: "kid2".into(), turn: Some("k3".into()) }).unwrap_err();
        assert_eq!((open.code, open.message), ("SOURCE_NOT_FINISHED", "Merge-back source run k3 is running; only provider-finished runs are supported.".to_string()));
        rig.say("parent", None, json!({"type": "thread.merge_consumed", "source": "kid"}));
        rig.turn("parent", "p2", "busy", "…", "running");
        assert_eq!(code(rig.run("parent", Body::MergeBack { source: "kid2".into(), turn: Some("k2".into()) })), "TARGET_BUSY");
    }

    #[test]
    fn given_turns_when_one_is_rolled_back_to_then_the_later_ones_go_with_their_messages_activities_and_diffs_and_a_running_or_unknown_turn_is_refused() {
        // Given a thread with three turns, each with a message, an activity and a diff, the third still running
        let mut rig = Rig::new();
        rig.thread("t", Some("w1"), Some("claudeAgent"));
        for (id, end) in [("u1", "completed"), ("u2", "completed"), ("u3", "running")] {
            rig.turn("t", id, id, "reply", end);
            let message = json!({"message": {"id": format!("m-{id}"), "role": "assistant", "text": "x", "turnId": id, "createdAt": "2023-11-14T22:13:20.000Z", "updatedAt": "2023-11-14T22:13:20.000Z"}});
            rig.event("t", "thread.message-sent", message);
            let activity = json!({"activity": {"id": format!("a-{id}"), "tone": "info", "kind": "k", "summary": "s", "payload": {}, "turnId": id, "createdAt": "2023-11-14T22:13:20.000Z"}});
            rig.event("t", "thread.activity-appended", activity);
            rig.event("t", "thread.turn-diff-updated", json!({"turnId": id, "unifiedDiff": "diff"}));
        }
        // When it is rolled back while a turn runs, to a turn it never ran, and then once the running turn finished, to the first
        let running = rig.run("t", Body::Rollback { turn: "u1".into(), restore_files: false }).unwrap_err();
        rig.say("t", Some("u3"), json!({"type": "turn.finished", "reason": "interrupted"}));
        let unknown = rig.run("t", Body::Rollback { turn: "u9".into(), restore_files: false }).unwrap_err();
        let events = rig.run("t", Body::Rollback { turn: "u1".into(), restore_files: false }).unwrap();
        // Then the first two were refused with the shared words, and the rollback names the turns it dropped
        assert_eq!((running.code, running.message), ("TURN_RUNNING", "thread t already has a running turn".to_string()));
        assert_eq!((unknown.code, unknown.message), ("TURN_NOT_FOUND", "thread t has no turn u9".to_string()));
        assert_eq!(events[0]["event"], json!({"type": "thread.rolled_back", "to_turn": "u1", "dropped": ["u2", "u3"], "restored_files": false}));
        // And the later turns' records, messages, activities and diffs are gone, the first's stay, and the thread's latest turn is the first
        let thread = &rig.model.threads["t"];
        assert_eq!(rig.model.neutral["t"].turns.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["u1"]);
        assert_eq!((thread.messages.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), thread.activities.iter().map(|a| a.id.as_str()).collect::<Vec<_>>()), (vec!["m-u1"], vec!["a-u1"]));
        assert_eq!(rig.model.turn_diffs.keys().cloned().collect::<Vec<_>>(), ["u1"]);
        assert_eq!(thread.latest_turn.as_ref().map(|t| (t.turn_id.as_str(), t.state.as_str())), Some(("u1", "completed")));
    }

    #[test]
    fn given_failed_and_completed_turns_when_a_retry_is_asked_then_only_the_newest_failed_turn_is_retried_and_the_retry_carries_its_prompt() {
        // Given a thread whose first turn failed and second failed too, and a third that completed
        let mut rig = Rig::new();
        rig.thread("t", Some("w1"), Some("claudeAgent"));
        rig.turn("t", "u1", "first", "…", "failed");
        rig.turn("t", "u2", "second", "…", "failed");
        let retry = |turn: &str| Body::Retry { turn: turn.into(), new_turn: "u3".into() };
        // When the older failure is retried, then the newest
        let older = rig.run("t", retry("u1")).unwrap_err();
        let newest = rig.run("t", retry("u2")).unwrap();
        // Then the older is not the newest turn, and the retry is an event of the new turn carrying the failed turn's prompt
        assert_eq!((older.code, older.message), ("NOT_RETRYABLE", "Turn u1 cannot be retried: it is not the newest turn".to_string()));
        assert_eq!((newest[0]["turnId"].clone(), newest[0]["event"].clone()), (json!("u3"), json!({"type": "turn.retried", "retry_of": "u2", "prompt": "second"})));
        // And a turn that completed is not retried, one that runs blocks it, and one never run is unknown
        rig.turn("t", "u3", "second", "ok", "completed");
        assert_eq!(rig.run("t", retry("u3")).unwrap_err().message, "Turn u3 cannot be retried: it did not fail");
        rig.turn("t", "u4", "x", "…", "running");
        assert_eq!(code(rig.run("t", retry("u4"))), "TURN_RUNNING");
        assert_eq!(code(rig.run("t", retry("u9"))), "TURN_NOT_FOUND");
    }

    #[test]
    fn given_a_thread_with_a_session_when_it_is_detached_then_only_its_own_session_is_and_the_next_turn_starts_without_it() {
        // Given a thread whose CLI session is s1
        let mut rig = Rig::new();
        rig.thread("t", Some("w1"), Some("claudeAgent"));
        rig.event("t", "thread.session-set", json!({"session": {"cli": "claudeAgent", "providerSessionId": "s1", "status": "ready"}}));
        // When another session is detached, then its own
        let other = rig.run("t", Body::Detach { session: "s2".into(), reason: None }).unwrap_err();
        rig.run("t", Body::Detach { session: "s1".into(), reason: Some("stale".into()) }).unwrap();
        // Then the other is refused with T3's words, and the thread has no provider session to resume
        assert_eq!((other.code, other.message), ("SESSION_NOT_OWNED", "Provider session s2 does not belong to thread t.".to_string()));
        assert_eq!(rig.model.threads["t"].session.as_ref().and_then(|s| s.provider_session_id.clone()), None);
        assert_eq!(code(rig.run("t", Body::Detach { session: "s1".into(), reason: None })), "SESSION_NOT_OWNED");
    }

    #[test]
    fn given_a_workflow_stop_when_the_turn_runs_or_does_not_then_only_the_running_turn_is_stopped() {
        // Given a thread that runs u1
        let mut rig = Rig::new();
        rig.thread("t", Some("w1"), Some("claudeAgent"));
        rig.turn("t", "u1", "work", "…", "running");
        // When a workflow of another turn is stopped, then one of the running turn naming the workflow
        let other = rig.run("t", Body::WorkflowStop { turn: "u0".into(), workflow: None }).unwrap_err();
        let stopped = rig.run("t", Body::WorkflowStop { turn: "u1".into(), workflow: Some("agent-7".into()) }).unwrap();
        // Then the other turn is not running, and the stop is an event of u1 naming the workflow
        assert_eq!((other.code, other.message), ("NO_TURN", "thread t is not running turn u0".to_string()));
        assert_eq!((stopped[0]["turnId"].clone(), stopped[0]["event"].clone()), (json!("u1"), json!({"type": "workflow.stopped", "workflow": "agent-7"})));
    }

    #[test]
    fn given_a_thread_with_no_lifecycle_state_when_serialised_then_the_json_has_no_lifecycle_field_and_with_state_it_has_exactly_those() {
        // Given a thread nothing has happened to
        let mut rig = Rig::new();
        rig.thread("t", Some("w1"), Some("claudeAgent"));
        let bare = serde_json::to_value(&rig.model.threads["t"]).unwrap();
        // Then its keys are those the thread always had
        let mut keys: Vec<&str> = bare.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["activities", "archivedAt", "createdAt", "cwd", "deletedAt", "id", "interactionMode", "latestTurn", "messages", "modelSelection", "proposedPlans", "runtimeMode", "session", "title", "updatedAt"]);
        // When it is given a queue, a hold, a settle and a lineage
        rig.run("t", add("q1", "one")).unwrap();
        rig.run("t", Body::SettleSet { to: SettleTo::Settled, until_ms: None, at_ms: None }).unwrap();
        rig.say("t", None, json!({"type": "queue.held", "reason": "force_stop"}));
        let with = serde_json::to_value(&rig.model.threads["t"]).unwrap();
        // Then those fields are in it, camel-cased as the thread's other fields, and it reads back the same
        let mut new: Vec<&str> = with.as_object().unwrap().keys().map(String::as_str).filter(|k| !keys.contains(k)).collect();
        new.sort_unstable();
        assert_eq!(new, ["queue", "queueHeld", "settle"]);
        assert_eq!(serde_json::from_value::<Thread>(with).unwrap(), rig.model.threads["t"]);
    }
}
