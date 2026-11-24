//! The read model: projects and threads folded from the event log, and the frames they are sent as.
use crate::ports::StoredEvent;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use t3_contracts::history::projected_rows;
use t3_contracts::rows::{project_defaults, thread_defaults, thread_projection_row};
use t3_contracts::runs;
use t3_contracts::time::iso;

#[derive(Debug, Clone, Default)]
pub struct Entities {
    pub runs: Vec<Value>,
    pub messages: Vec<Value>,
    pub turn_items: Vec<Value>,
}

#[derive(Debug, Clone, Default)]
pub struct State {
    pub last_sequence: u64,
    pub projects: BTreeMap<String, Map<String, Value>>,
    pub threads: BTreeMap<String, Map<String, Value>>,
    pub entities: BTreeMap<String, Entities>,
    /// Per thread, the projection keys an event has set that the base row leaves out.
    pub projection_extras: BTreeMap<String, Map<String, Value>>,
    /// Deleted threads: the real server still answers their projection, with `deletedAt` set.
    pub deleted: BTreeMap<String, Map<String, Value>>,
}

/// What folding one event did to the shell.
#[derive(Debug, Default, PartialEq)]
pub struct Applied {
    pub shell_changed: bool,
    /// Set when the event removed a thread: the list it left.
    pub removed_from: Option<&'static str>,
}

/// The event as the thread stream sends it.
pub fn event_json(e: &StoredEvent) -> Value {
    let payload = match e.kind.as_str() {
        "turn-item.updated" => turn_item_wire(&e.payload),
        "message.updated" => message_wire(&e.payload),
        _ => e.payload.clone(),
    };
    json!({
        "id": e.event_id,
        "threadId": e.thread_id,
        "providerInstanceId": e.payload.get("providerInstanceId").cloned().unwrap_or(Value::Null),
        "occurredAt": iso(e.occurred_at),
        "type": e.kind,
        "payload": payload,
    })
}

fn message_wire(value: &Value) -> Value {
    let mut message = value.clone();
    if let Some(fields) = message.as_object_mut() {
        fields.entry("nodeId").or_insert_with(|| value["runId"].as_str().map_or(Value::Null, |run| json!(runs::node_id(run))));
        fields.entry("attachments").or_insert_with(|| json!([]));
        fields.entry("streaming").or_insert_with(|| json!(false));
    }
    message
}

pub(crate) fn turn_item_wire(value: &Value) -> Value {
    let mut item = value.clone();
    let Some(fields) = item.as_object_mut() else { return item };
    let streaming = value["status"] == "streaming";
    if streaming {
        fields.insert("status".into(), json!("running"));
    }
    fields.entry("nodeId").or_insert_with(|| value["runId"].as_str().map_or(Value::Null, |run| json!(runs::node_id(run))));
    for field in ["providerThreadId", "providerTurnId", "nativeItemRef", "parentItemId"] {
        fields.entry(field).or_insert(Value::Null);
    }
    fields.entry("title").or_insert_with(|| value["name"].clone());
    fields.entry("startedAt").or_insert_with(|| value["createdAt"].clone());
    fields.entry("completedAt").or_insert_with(|| {
        if matches!(value["status"].as_str(), Some("completed" | "failed" | "cancelled" | "interrupted")) {
            value["updatedAt"].clone()
        } else {
            Value::Null
        }
    });
    match value["type"].as_str() {
        Some("assistant_message" | "reasoning" | "proposed_plan") => {
            fields.entry("streaming").or_insert_with(|| json!(streaming));
        }
        Some("command_execution") => {
            let input = &value["input"];
            if !input.is_string() {
                let command = input["command"].as_str().map(str::to_string)
                    .unwrap_or_else(|| if input.is_null() { String::new() } else { input.to_string() });
                fields.insert("input".into(), json!(command));
            }
            if value["output"].is_null() {
                fields.remove("output");
            }
        }
        Some("error") if fields.get("failure").is_none() => {
            fields.insert("failure".into(), json!({
                "class": "provider_error",
                "message": value["message"].as_str().unwrap_or("The provider failed"),
                "code": null,
                "retryable": null,
            }));
        }
        _ => {}
    }
    item
}

/// The keys of a thread row that the real projection carries only after an event of that kind set them: the recorded pin leaves `pinOrderKey` out, the unpin and the settle carry it as null.
fn projection_extras(kind: &str) -> &'static [&'static str] {
    match kind {
        "thread.settled" => &["activeOrderKey", "pinOrderKey", "pinnedAt", "unsettledAt"],
        "thread.unsettled" => &["unsettledAt"],
        "thread.pinned" => &["pinnedAt"],
        "thread.unpinned" => &["pinOrderKey", "pinnedAt"],
        "thread.auto-settle-set" => &["autoSettleDisabledAt"],
        "thread.snoozed" => &["limitRecovery"],
        "thread.unarchived" => &["guestShare", "titleRegeneration"],
        "thread.metadata-updated" => &["titleRegeneration"],
        _ => &[],
    }
}

fn merge(into: &mut Map<String, Value>, patch: &Value) {
    if let Some(patch) = patch.as_object() {
        for (k, v) in patch {
            into.insert(k.clone(), v.clone());
        }
    }
}

fn upsert(list: &mut Vec<Value>, entity: &Value) {
    match list.iter_mut().find(|e| e["id"] == entity["id"]) {
        Some(slot) => *slot = entity.clone(),
        None => list.push(entity.clone()),
    }
}

pub fn is_archived(shell: &Map<String, Value>) -> bool {
    shell.get("archivedAt").is_some_and(|v| !v.is_null())
}

/// A deleted project stays in the read model so that a retried delete answers its row; the shell and new threads skip it.
pub fn is_deleted(project: &Map<String, Value>) -> bool {
    project.get("deletedAt").is_some_and(|v| !v.is_null())
}

fn location(shell: &Map<String, Value>) -> &'static str {
    if is_archived(shell) {
        "archive"
    } else {
        "active"
    }
}

impl State {
    pub fn apply(&mut self, e: &StoredEvent) -> Applied {
        self.last_sequence = self.last_sequence.max(e.sequence);
        let updated_at = Value::String(iso(e.occurred_at));
        let mut applied = Applied::default();
        match e.kind.as_str() {
            "project.created" => {
                if let Some(id) = &e.project_id {
                    let mut project = project_defaults();
                    merge(&mut project, &e.payload);
                    self.projects.insert(id.clone(), project);
                }
            }
            "project.updated" | "project.archived" | "project.deleted" => {
                if let Some(project) = e.project_id.as_ref().and_then(|id| self.projects.get_mut(id)) {
                    match e.kind.as_str() {
                        "project.updated" => merge(project, &e.payload["patch"]),
                        "project.archived" => {
                            project.insert("archivedAt".into(), e.payload["archivedAt"].clone());
                        }
                        _ => {
                            project.insert("deletedAt".into(), e.payload["deletedAt"].clone());
                        }
                    }
                    project.insert("updatedAt".into(), updated_at);
                }
            }
            "thread.created" => {
                if let Some(id) = &e.thread_id {
                    let mut shell = thread_defaults();
                    for (k, v) in e.payload.as_object().into_iter().flatten() {
                        if shell.contains_key(k) {
                            shell.insert(k.clone(), v.clone());
                        }
                    }
                    self.threads.insert(id.clone(), shell);
                    applied.shell_changed = true;
                }
            }
            "thread.deleted" => {
                if let Some(id) = &e.thread_id {
                    let removed = self.threads.remove(id);
                    if let Some(mut shell) = removed.clone() {
                        shell.insert("deletedAt".into(), Value::String(iso(e.occurred_at)));
                        self.deleted.insert(id.clone(), shell);
                    }
                    applied.removed_from = removed.map(|s| location(&s));
                    self.entities.remove(id);
                    self.projection_extras.entry(id.clone()).or_default().insert("titleRegeneration".into(), e.payload.get("titleRegeneration").cloned().unwrap_or(Value::Null));
                }
            }
            kind if kind.starts_with("thread.") => {
                if let Some(shell) = e.thread_id.as_ref().and_then(|id| self.threads.get_mut(id)) {
                    // The recorded shell row has no guest share: it is kept only while one is open, and the event row (which has the key) carries it
                    if let Some(share) = e.payload.get("guestShare") {
                        applied.shell_changed |= if share.is_null() { shell.remove("guestShare").is_some() } else { shell.insert("guestShare".into(), share.clone()).as_ref() != Some(share) };
                    }
                    for (k, v) in e.payload.as_object().into_iter().flatten() {
                        if k != "id" && shell.contains_key(k) {
                            applied.shell_changed |= shell.insert(k.clone(), v.clone()).as_ref() != Some(v);
                        }
                    }
                    if let Some(id) = &e.thread_id {
                        let extras = self.projection_extras.entry(id.clone()).or_default();
                        for key in projection_extras(kind) {
                            extras.insert((*key).into(), e.payload.get(*key).or_else(|| shell.get(*key)).cloned().unwrap_or(Value::Null));
                        }
                    }
                }
            }
            "run.created" | "run.updated" => {
                if let Some(id) = &e.thread_id {
                    let entities = self.entities.entry(id.clone()).or_default();
                    upsert(&mut entities.runs, &e.payload);
                    let provider_thread = entities.runs.first().map(|first| runs::provider_thread_id(first["providerInstanceId"].as_str().unwrap_or(""), first["id"].as_str().unwrap_or("")));
                    let waiting = matches!(e.payload["status"].as_str(), Some("queued" | "cancelled"));
                    if let (false, Some(shell)) = (waiting, self.threads.get_mut(id)) {
                        let run = &e.payload;
                        let running = run["status"] == "running";
                        let mut set = vec![
                            ("status", json!(if running { "running" } else { "idle" })),
                            ("activeRunId", if running { run["id"].clone() } else { Value::Null }),
                            ("latestRunId", run["id"].clone()),
                            ("latestRunRequestedAt", run["requestedAt"].clone()),
                            ("latestRunStartedAt", run["startedAt"].clone()),
                            ("latestRunCompletedAt", run["completedAt"].clone()),
                            ("latestRunStopRequested", json!(run["stopRequested"] == true)),
                            ("lastError", run.get("error").cloned().unwrap_or(Value::Null)),
                            ("activeProviderThreadId", provider_thread.map_or(Value::Null, Value::from)),
                            ("updatedAt", updated_at),
                        ];
                        if e.kind == "run.created" {
                            set.push(("lastVisitedAt", run["requestedAt"].clone()));
                        }
                        set.into_iter().for_each(|(k, v)| {
                            shell.insert(k.into(), v);
                        });
                        applied.shell_changed = true;
                    }
                }
            }
            "message.removed" => {
                if let Some(entities) = e.thread_id.as_ref().and_then(|id| self.entities.get_mut(id)) {
                    entities.messages.retain(|m| m["id"] != e.payload["id"]);
                }
            }
            "message.updated" => {
                if let Some(id) = &e.thread_id {
                    let entities = self.entities.entry(id.clone()).or_default();
                    upsert(&mut entities.messages, &e.payload);
                    if let Some(shell) = self.threads.get_mut(id) {
                        if e.payload["role"] == "user" {
                            shell.insert("latestUserMessageAt".into(), e.payload["createdAt"].clone());
                            shell.insert("latestUserAuthoredMessageAt".into(), e.payload["createdAt"].clone());
                        }
                        shell.insert("latestVisibleMessage".into(), json!({
                            "id": e.payload["id"], "role": e.payload["role"], "text": e.payload["text"],
                            "updatedAt": e.payload.get("updatedAt").unwrap_or(&updated_at),
                        }));
                        shell.insert("updatedAt".into(), updated_at);
                        applied.shell_changed = true;
                    }
                }
            }
            "turn-item.updated" => {
                if let Some(id) = &e.thread_id {
                    let entities = self.entities.entry(id.clone()).or_default();
                    let before = entities.turn_items.len();
                    upsert(&mut entities.turn_items, &e.payload);
                    let count = entities.turn_items.len();
                    if let (true, Some(shell)) = (count != before, self.threads.get_mut(id)) {
                        shell.insert("itemCount".into(), count.into());
                        shell.insert("visibleItemCount".into(), count.into());
                        applied.shell_changed = true;
                    }
                }
            }
            _ => {}
        }
        applied
    }

    pub fn active_project(&self, id: &str) -> bool {
        self.projects.get(id).is_some_and(|p| p.get("archivedAt").is_none_or(Value::is_null) && !is_deleted(p))
    }

    /// A project that exists and is not deleted, archived or not.
    pub fn project_exists(&self, id: &str) -> bool {
        self.projects.get(id).is_some_and(|p| !is_deleted(p))
    }

    pub fn shell_snapshot(&self) -> Value {
        let (archived, active): (Vec<_>, Vec<_>) = self.threads.values().partition(|t| is_archived(t));
        let threads = |list: Vec<&Map<String, Value>>| -> Vec<Value> { list.into_iter().map(|t| Value::Object(t.clone())).collect() };
        let projects: Vec<Value> = self.projects.values().filter(|p| p.get("archivedAt").is_none_or(Value::is_null) && !is_deleted(p)).map(|p| Value::Object(p.clone())).collect();
        json!({"kind": "snapshot", "snapshot": {"schemaVersion": 2, "snapshotSequence": self.last_sequence, "threads": threads(active), "archivedThreads": threads(archived), "projects": projects}})
    }

    /// The shell frames one event stands for, read against the state right after it. An archived thread leaves the active list.
    pub fn shell_frames(&self, e: &StoredEvent, applied: &Applied) -> Vec<Value> {
        match e.kind.as_str() {
            "project.created" | "project.updated" => e.project_id.as_ref().and_then(|id| self.projects.get(id)).map(|p| vec![json!({"kind": "project.updated", "sequence": e.sequence, "project": p})]).unwrap_or_default(),
            "project.archived" | "project.deleted" => vec![json!({"kind": "project.removed", "sequence": e.sequence, "projectId": e.project_id})],
            "thread.archived" => vec![json!({"kind": "thread.removed", "sequence": e.sequence, "location": "active", "threadId": e.thread_id})],
            "thread.deleted" => applied.removed_from.map(|from| vec![json!({"kind": "thread.removed", "sequence": e.sequence, "location": from, "threadId": e.thread_id})]).unwrap_or_default(),
            _ if applied.shell_changed => e.thread_id.as_ref().and_then(|id| self.threads.get(id)).map(|shell| vec![json!({"kind": "thread.updated", "sequence": e.sequence, "location": location(shell), "thread": shell})]).unwrap_or_default(),
            _ => vec![],
        }
    }

    pub fn project_row(&self, id: &str) -> Option<Value> {
        let mut row = self.projects.get(id)?.clone();
        row.entry("deletedAt").or_insert(Value::Null);
        Some(Value::Object(row))
    }

    pub fn thread_projection(&self, id: &str) -> Option<Value> {
        self.projection_of(self.threads.get(id)?, id)
    }

    /// A projection read, which still answers for a deleted thread.
    pub fn thread_projection_read(&self, id: &str) -> Option<Value> {
        self.projection_of(self.threads.get(id).or_else(|| self.deleted.get(id))?, id)
    }

    fn projection_of(&self, shell: &Map<String, Value>, id: &str) -> Option<Value> {
        let empty = Entities::default();
        let entities = self.entities.get(id).unwrap_or(&empty);
        let workspace = shell["worktreePath"].as_str().or_else(|| shell["projectId"].as_str().and_then(|p| self.projects.get(p)).and_then(|p| p["workspaceRoot"].as_str()));
        let rows = runs::derive(id, shell, &entities.runs, workspace);
        let mut out = Map::new();
        let mut thread = thread_projection_row(shell);
        if let Some(row) = thread.as_object_mut() {
            row.insert("activeProviderThreadId".into(), rows.active_provider_thread.clone());
            if let Some(extras) = self.projection_extras.get(id) {
                row.extend(extras.clone());
            }
        }
        out.insert("thread".into(), thread);
        let items: Vec<Value> = entities.turn_items.iter().map(turn_item_wire).collect();
        let none = || json!([]);
        for (key, value) in [
            ("runs", Value::Array(rows.runs)),
            ("attempts", Value::Array(rows.attempts)),
            ("nodes", Value::Array(rows.nodes)),
            ("subagents", none()),
            ("providerSessions", Value::Array(rows.provider_sessions)),
            ("providerThreads", Value::Array(rows.provider_threads)),
            ("providerTurns", none()),
            ("runtimeRequests", none()),
            ("messages", Value::Array(entities.messages.iter().map(message_wire).collect())),
            ("plans", none()),
            ("visibleTurnItems", Value::Array(projected_rows(id, &items))),
            ("turnItems", Value::Array(items)),
            ("checkpointScopes", Value::Array(rows.checkpoint_scopes)),
            ("checkpoints", none()),
            ("contextHandoffs", Value::Array(rows.context_handoffs)),
            ("contextTransfers", none()),
        ] {
            out.insert(key.into(), value);
        }
        out.insert("updatedAt".into(), shell.get("updatedAt").cloned().unwrap_or(Value::Null));
        Some(Value::Object(out))
    }
}
