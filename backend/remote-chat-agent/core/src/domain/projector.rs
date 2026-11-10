//! The pure fold of an event into the read model (`orchestration/projector.ts`): it advances the snapshot sequence, caps what a
//! thread keeps, settles the latest turn when its session leaves running, and keeps the latest diff per turn on the side.

use super::lifecycle::{self, Lifecycle, Lineage, OpenRequest, TurnRecord};
use super::types::*;
use serde_json::Value;
use std::collections::HashMap;

/// What the agent API keeps of a thread it created (`Body::NeutralCreate`): the defaults of its turns and the sequence of its newest neutral event.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NeutralThread {
    pub workspace: Option<String>,
    pub driver: Option<String>,
    pub model: Option<String>,
    /// 0 for a thread with no neutral event.
    pub last_seq: i64,
    /// The turns the thread's events tell of, oldest first, and what each said and how it stands (`domain::lifecycle`).
    pub turns: Vec<TurnRecord>,
    /// The approvals and questions a turn waits on.
    pub requests: Vec<OpenRequest>,
    /// The turns a fork copied from its source: they are not its own work.
    pub inherited: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ReadModel {
    pub snapshot_sequence: i64,
    pub threads: HashMap<String, Thread>,
    pub turn_diffs: HashMap<String, String>,
    pub neutral: HashMap<String, NeutralThread>,
    pub updated_at: String,
}

impl ReadModel {
    pub fn empty(now: &str) -> Self {
        Self { snapshot_sequence: 0, threads: HashMap::new(), turn_diffs: HashMap::new(), neutral: HashMap::new(), updated_at: now.to_string() }
    }
}

/// The turn state a neutral `turn.finished` reason settles a running turn into.
fn finished_state(reason: Option<&str>) -> &'static str {
    match reason {
        Some("interrupted") => "interrupted",
        Some("failed") => "error",
        _ => "completed",
    }
}

/// The turn state a session status settles a running turn into; None while the session is still going.
fn settled_state(status: &str) -> Option<&'static str> {
    match status {
        "idle" | "ready" => Some("completed"),
        "error" => Some("error"),
        "interrupted" | "stopped" => Some("interrupted"),
        _ => None,
    }
}

fn compare_activities(left: &Activity, right: &Activity) -> std::cmp::Ordering {
    match (left.sequence, right.sequence) {
        (Some(l), Some(r)) if l != r => return l.cmp(&r),
        (Some(_), None) => return std::cmp::Ordering::Greater,
        (None, Some(_)) => return std::cmp::Ordering::Less,
        _ => {}
    }
    left.created_at.cmp(&right.created_at).then_with(|| left.id.cmp(&right.id))
}

fn field<T: serde::de::DeserializeOwned>(payload: &Value, name: &str) -> Option<T> {
    payload.get(name).and_then(|v| serde_json::from_value(v.clone()).ok())
}

fn text(payload: &Value, name: &str) -> Option<String> {
    payload.get(name).and_then(Value::as_str).map(str::to_string)
}

pub fn project(model: &mut ReadModel, event: &Event) {
    model.snapshot_sequence = event.sequence;
    model.updated_at = event.created_at.clone();
    let payload = &event.payload;
    match event.kind.as_str() {
        "thread.created" => {
            if model.threads.contains_key(&event.thread_id) {
                return;
            }
            let (Some(title), Some(cwd), Some(selection), Some(runtime), Some(interaction), Some(created)) = (
                text(payload, "title"),
                text(payload, "cwd"),
                field::<ModelSelection>(payload, "modelSelection"),
                text(payload, "runtimeMode"),
                text(payload, "interactionMode"),
                text(payload, "createdAt"),
            ) else {
                return;
            };
            model.threads.insert(
                event.thread_id.clone(),
                Thread {
                    id: event.thread_id.clone(),
                    title,
                    cwd,
                    model_selection: selection,
                    runtime_mode: runtime,
                    interaction_mode: interaction,
                    latest_turn: None,
                    created_at: created.clone(),
                    updated_at: created,
                    archived_at: None,
                    deleted_at: None,
                    messages: Vec::new(),
                    proposed_plans: Vec::new(),
                    activities: Vec::new(),
                    session: None,
                    token_usage: None,
                    branch: text(payload, "branch"),
                    worktree_path: text(payload, "worktreePath"),
                    lifecycle: Lifecycle { lineage: field::<Lineage>(payload, "lineage"), ..Lifecycle::default() },
                },
            );
            if let Some(neutral) = payload.get("neutral").and_then(Value::as_object) {
                let own = |name: &str| neutral.get(name).and_then(Value::as_str).map(str::to_string);
                model.neutral.insert(event.thread_id.clone(), NeutralThread { workspace: own("workspace"), driver: own("driver"), model: own("model"), ..NeutralThread::default() });
            }
        }
        "thread.neutral-event" => {
            {
                let (Some(thread), Some(neutral)) = (model.threads.get_mut(&event.thread_id), model.neutral.get_mut(&event.thread_id)) else { return };
                neutral.last_seq = event.sequence;
                thread.updated_at = event.created_at.clone();
                let turn = text(payload, "turnId");
                let (kind, reason) = (payload["event"]["type"].as_str(), payload["event"]["reason"].as_str());
                match (kind, turn) {
                    (Some("turn.started"), Some(turn_id)) => {
                        thread.latest_turn = Some(LatestTurn { turn_id, state: "running".into(), requested_at: event.created_at.clone(), started_at: Some(event.created_at.clone()), completed_at: None });
                    }
                    (Some("turn.finished"), Some(turn_id)) => {
                        if let Some(latest) = thread.latest_turn.as_mut().filter(|t| t.turn_id == turn_id && t.state == "running") {
                            latest.state = finished_state(reason).into();
                            latest.completed_at = Some(event.created_at.clone());
                        }
                    }
                    _ => {}
                }
            }
            lifecycle::fold(model, &event.thread_id, text(payload, "turnId").as_deref(), &payload["event"], &event.created_at);
        }
        "thread.deleted" => {
            if let Some(thread) = model.threads.get_mut(&event.thread_id) {
                let at = text(payload, "deletedAt");
                thread.deleted_at = at.clone();
                if let Some(at) = at {
                    thread.updated_at = at;
                }
            }
        }
        "thread.archived" => {
            if let Some(thread) = model.threads.get_mut(&event.thread_id) {
                thread.archived_at = text(payload, "archivedAt");
                if let Some(at) = text(payload, "updatedAt") {
                    thread.updated_at = at;
                }
            }
        }
        "thread.unarchived" => {
            if let Some(thread) = model.threads.get_mut(&event.thread_id) {
                thread.archived_at = None;
                if let Some(at) = text(payload, "updatedAt") {
                    thread.updated_at = at;
                }
            }
        }
        "thread.meta-updated" => {
            if let Some(thread) = model.threads.get_mut(&event.thread_id) {
                if let Some(at) = text(payload, "updatedAt") {
                    thread.updated_at = at;
                }
                if let Some(title) = text(payload, "title") {
                    thread.title = title;
                }
                if let Some(cwd) = text(payload, "cwd") {
                    thread.cwd = cwd;
                }
                if let Some(selection) = field::<ModelSelection>(payload, "modelSelection") {
                    thread.model_selection = selection;
                }
                if let Some(branch) = text(payload, "branch") {
                    thread.branch = Some(branch);
                }
                if let Some(path) = text(payload, "worktreePath") {
                    thread.worktree_path = Some(path);
                }
            }
        }
        "thread.runtime-mode-set" => {
            if let Some(thread) = model.threads.get_mut(&event.thread_id) {
                if let Some(mode) = text(payload, "runtimeMode") {
                    thread.runtime_mode = mode;
                }
                if let Some(at) = text(payload, "updatedAt") {
                    thread.updated_at = at;
                }
            }
        }
        "thread.interaction-mode-set" => {
            if let Some(thread) = model.threads.get_mut(&event.thread_id) {
                if let Some(mode) = text(payload, "interactionMode") {
                    thread.interaction_mode = mode;
                }
                if let Some(at) = text(payload, "updatedAt") {
                    thread.updated_at = at;
                }
            }
        }
        "thread.message-sent" => {
            let Some(thread) = model.threads.get_mut(&event.thread_id) else { return };
            let Some(incoming) = field::<Message>(payload, "message") else { return };
            match thread.messages.iter_mut().find(|m| m.id == incoming.id) {
                Some(existing) => *existing = incoming,
                None => thread.messages.push(incoming),
            }
            if thread.messages.len() > MAX_THREAD_MESSAGES {
                let excess = thread.messages.len() - MAX_THREAD_MESSAGES;
                thread.messages.drain(..excess);
            }
            thread.updated_at = event.created_at.clone();
        }
        "thread.session-set" => {
            let Some(thread) = model.threads.get_mut(&event.thread_id) else { return };
            let Some(session) = field::<Session>(payload, "session") else { return };
            let settled = settled_state(&session.status);
            let mut latest = thread.latest_turn.clone();
            if session.status == "running" && session.active_turn_id.is_some() {
                let turn_id = session.active_turn_id.clone().unwrap_or_default();
                let same = thread.latest_turn.as_ref().filter(|t| t.turn_id == turn_id);
                latest = Some(LatestTurn {
                    turn_id,
                    state: "running".into(),
                    requested_at: same.map_or_else(|| event.created_at.clone(), |t| t.requested_at.clone()),
                    started_at: Some(same.and_then(|t| t.started_at.clone()).unwrap_or_else(|| event.created_at.clone())),
                    completed_at: None,
                });
            } else if let (Some(turn), Some(state)) = (thread.latest_turn.as_ref().filter(|t| t.state == "running"), settled) {
                latest = Some(LatestTurn { state: state.into(), completed_at: Some(event.created_at.clone()), ..turn.clone() });
            }
            thread.session = Some(session);
            thread.latest_turn = latest;
            thread.updated_at = event.created_at.clone();
        }
        "thread.proposed-plan-upserted" => {
            let Some(thread) = model.threads.get_mut(&event.thread_id) else { return };
            let Some(incoming) = field::<ProposedPlan>(payload, "proposedPlan") else { return };
            thread.proposed_plans.retain(|p| p.id != incoming.id);
            thread.proposed_plans.push(incoming);
            thread.proposed_plans.sort_by(|l, r| l.created_at.cmp(&r.created_at).then_with(|| l.id.cmp(&r.id)));
            if thread.proposed_plans.len() > MAX_THREAD_PROPOSED_PLANS {
                let excess = thread.proposed_plans.len() - MAX_THREAD_PROPOSED_PLANS;
                thread.proposed_plans.drain(..excess);
            }
            thread.updated_at = event.created_at.clone();
        }
        "thread.activity-appended" => {
            let Some(thread) = model.threads.get_mut(&event.thread_id) else { return };
            let Some(incoming) = field::<Activity>(payload, "activity") else { return };
            thread.activities.retain(|a| a.id != incoming.id);
            let at = thread.activities.partition_point(|probe| compare_activities(probe, &incoming) != std::cmp::Ordering::Greater);
            thread.activities.insert(at, incoming);
            if thread.activities.len() > MAX_THREAD_ACTIVITIES {
                let excess = thread.activities.len() - MAX_THREAD_ACTIVITIES;
                thread.activities.drain(..excess);
            }
            thread.updated_at = event.created_at.clone();
        }
        "thread.turn-diff-updated" => {
            if let (Some(turn), Some(diff)) = (text(payload, "turnId"), text(payload, "unifiedDiff")) {
                model.turn_diffs.insert(turn, diff);
            }
        }
        _ => {}
    }
}
