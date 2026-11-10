//! A provider runtime event into the internal commands the engine decides (`orchestration/ingestion.ts`): the session lifecycle,
//! assistant text, reasoning, tool and approval activities, plans. State lives on the thread snapshot handed in;
//! nothing here does IO.

use super::types::*;
use serde_json::{json, Map, Value};

#[derive(Debug, Clone)]
pub struct Runtime {
    pub event_id: String,
    pub provider: String,
    pub thread_id: String,
    pub created_at: String,
    pub turn_id: Option<String>,
    pub item_id: Option<String>,
    pub request_id: Option<String>,
    pub kind: Kind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    Started,
    Updated,
    Completed,
}

#[derive(Debug, Clone, Default)]
pub struct Item {
    pub item_type: String,
    pub status: Option<String>,
    pub title: Option<String>,
    pub detail: Option<String>,
    pub data: Option<Value>,
    pub agent_id: Option<String>,
    pub parent_tool_use_id: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Kind {
    TurnStarted,
    TurnCompleted { state: String, error_message: Option<String> },
    TurnAborted,
    TurnPlanUpdated { plan: Value, explanation: Option<String> },
    TurnProposedDelta { delta: String },
    TurnProposedCompleted { plan_markdown: String },
    Item { phase: Phase, item: Item },
    ContentDelta { stream_kind: String, delta: String },
    RequestOpened { request_type: String, detail: Option<String> },
    RequestResolved { request_type: String, decision: Option<String> },
    UserInputRequested { questions: Value },
    RuntimeError { message: String },
}

const TOOL_ITEM_TYPES: [&str; 7] = ["command_execution", "file_change", "mcp_tool_call", "dynamic_tool_call", "collab_agent_tool_call", "web_search", "image_view"];

fn truncate(value: &str, limit: usize) -> String {
    if value.chars().count() > limit {
        format!("{}...", value.chars().take(limit - 3).collect::<String>())
    } else {
        value.to_string()
    }
}

fn truncate_detail(value: &str) -> String {
    truncate(value, 180)
}

fn as_record(value: &Value) -> Option<&Map<String, Value>> {
    value.as_object()
}

fn push_changed_file(target: &mut Vec<String>, seen: &mut Vec<String>, value: &Value) {
    let Some(record) = as_record(value) else { return };
    for key in ["path", "filePath", "relativePath", "filename", "newPath", "oldPath"] {
        if let Some(candidate) = record.get(key).and_then(Value::as_str) {
            if !candidate.trim().is_empty() && !seen.iter().any(|s| s == candidate) {
                seen.push(candidate.to_string());
                target.push(candidate.to_string());
            }
        }
    }
}

fn collect_changed_files(value: &Value, depth: usize) -> Vec<String> {
    if depth > 4 {
        return Vec::new();
    }
    if let Some(entries) = value.as_array() {
        let mut out: Vec<String> = Vec::new();
        if entries.iter().all(Value::is_string) {
            for entry in entries.iter().filter_map(Value::as_str) {
                if !entry.trim().is_empty() && !out.iter().any(|o| o == entry) {
                    out.push(entry.to_string());
                }
            }
            return out;
        }
        for entry in entries {
            for file in collect_changed_files(entry, depth + 1) {
                if !out.contains(&file) {
                    out.push(file);
                }
            }
            if out.len() >= 12 {
                break;
            }
        }
        return out;
    }
    let Some(record) = as_record(value) else { return Vec::new() };
    let mut files: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    push_changed_file(&mut files, &mut seen, value);
    if let Some(changed) = record.get("changedFiles") {
        for file in collect_changed_files(changed, depth + 1) {
            if !seen.contains(&file) {
                seen.push(file.clone());
                files.push(file);
            }
            if files.len() >= 12 {
                break;
            }
        }
    }
    for key in ["item", "result", "input", "data", "changes", "files", "edits", "patch", "patches", "operations"] {
        if let Some(nested) = record.get(key) {
            for file in collect_changed_files(nested, depth + 1) {
                if !seen.contains(&file) {
                    seen.push(file.clone());
                    files.push(file);
                }
            }
            if files.len() >= 12 {
                break;
            }
        }
    }
    files.truncate(12);
    files
}

/// The first meaningful line of an output, at most `limit` characters.
fn summarize_text(value: &str, limit: usize) -> String {
    let lines: Vec<String> = value.split('\n').map(|l| l.trim_end_matches('\r').split_whitespace().collect::<Vec<_>>().join(" ")).filter(|l| !l.is_empty()).collect();
    if let Some(first) = lines.iter().find(|l| l.as_str() != "```") {
        return if first.chars().count() <= limit { first.clone() } else { format!("{}…", first.chars().take(limit - 1).collect::<String>().trim_end()) };
    }
    if lines.len() > 1 {
        return format!("{} lines", lines.len());
    }
    value.chars().take(limit).collect()
}

fn summarize(value: &str) -> String {
    summarize_text(value, 240)
}

fn project_tool_data(data: &Value) -> Map<String, Value> {
    let mut projected = Map::new();
    let Some(record) = as_record(data) else { return projected };

    if let Some(item) = record.get("item").and_then(as_record) {
        let mut shown = Map::new();
        if let Some(command) = item.get("command") {
            shown.insert("command".into(), command.clone());
        }
        if let Some(aggregated) = item.get("aggregatedOutput").and_then(Value::as_str).filter(|a| !a.trim().is_empty()) {
            shown.insert("output".into(), json!(summarize(aggregated)));
        }
        if let Some(input) = item.get("input").and_then(as_record) {
            if let Some(command) = input.get("command") {
                shown.insert("input".into(), json!({ "command": command }));
            }
        }
        if let Some(result) = item.get("result").and_then(as_record) {
            let mut shown_result = Map::new();
            if let Some(command) = result.get("command") {
                shown_result.insert("command".into(), command.clone());
            }
            match result.get("content") {
                Some(Value::String(content)) => {
                    shown_result.insert("output".into(), json!(summarize(content)));
                }
                Some(Value::Array(parts)) => {
                    let joined = parts.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n");
                    if !joined.is_empty() {
                        shown_result.insert("output".into(), json!(summarize(&joined)));
                    }
                }
                _ => {}
            }
            if !shown_result.is_empty() {
                shown.insert("result".into(), Value::Object(shown_result));
            }
        }
        if !shown.is_empty() {
            projected.insert("item".into(), Value::Object(shown));
        }
    }

    if let Some(command) = record.get("command") {
        projected.insert("command".into(), command.clone());
    } else if let Some(input) = record.get("input").and_then(as_record) {
        if let Some(command) = input.get("command") {
            projected.insert("command".into(), command.clone());
        }
    }

    let files = collect_changed_files(data, 0);
    if !files.is_empty() {
        projected.insert("changedFiles".into(), json!(files));
    }
    if let Some(patch) = record.get("patch") {
        projected.insert("patch".into(), patch.clone());
    }

    match record.get("rawOutput") {
        Some(Value::String(raw)) if !raw.trim().is_empty() => {
            projected.insert("output".into(), json!(summarize(raw)));
        }
        Some(Value::Object(raw)) => {
            for key in ["content", "stdout", "stderr"] {
                if let Some(text) = raw.get(key).and_then(Value::as_str) {
                    projected.insert("output".into(), json!(summarize(text)));
                    break;
                }
            }
        }
        _ => {}
    }
    if !projected.contains_key("output") {
        if let Some(output) = record.get("output") {
            projected.insert("output".into(), output.clone());
        }
    }
    projected
}

fn request_kind(request_type: &str) -> Option<&'static str> {
    match request_type {
        "command_execution_approval" | "exec_command_approval" => Some("command"),
        "file_read_approval" => Some("file-read"),
        "file_change_approval" | "apply_patch_approval" => Some("file-change"),
        _ => None,
    }
}

fn approval_payload(event: &Runtime, request_type: &str, extra: (&str, &Option<String>)) -> Value {
    let mut payload = Map::new();
    if let Some(id) = &event.request_id {
        payload.insert("requestId".into(), json!(id));
    }
    if let Some(kind) = request_kind(request_type) {
        payload.insert("requestKind".into(), json!(kind));
    }
    payload.insert("requestType".into(), json!(request_type));
    if let Some(value) = extra.1 {
        payload.insert(extra.0.into(), json!(value));
    }
    Value::Object(payload)
}

fn approval_summary(opened: bool, request_type: &str) -> &'static str {
    if !opened {
        return "Approval resolved";
    }
    match request_kind(request_type) {
        Some("command") => "Command approval requested",
        Some("file-read") => "File-read approval requested",
        Some("file-change") => "File-change approval requested",
        _ => "Approval requested",
    }
}

struct Lifecycle {
    status: &'static str,
    active_turn_id: Option<String>,
    last_error: Option<String>,
}

fn derive_lifecycle(thread: Option<&Thread>, event: &Runtime) -> Option<Lifecycle> {
    let active = thread.and_then(|t| t.session.as_ref()).and_then(|s| s.active_turn_id.clone());
    let previous_error = thread.and_then(|t| t.session.as_ref()).and_then(|s| s.last_error.clone());
    match &event.kind {
        Kind::TurnStarted => Some(Lifecycle { status: "running", active_turn_id: event.turn_id.clone(), last_error: None }),
        Kind::TurnCompleted { state, error_message } => {
            let failed = state == "failed";
            Some(Lifecycle {
                status: if failed { "error" } else { "ready" },
                active_turn_id: None,
                last_error: failed.then(|| error_message.clone().or(previous_error).unwrap_or_else(|| "Turn failed".into())),
            })
        }
        Kind::TurnAborted => Some(Lifecycle { status: "interrupted", active_turn_id: None, last_error: None }),
        Kind::RuntimeError { message } => Some(Lifecycle { status: "error", active_turn_id: event.turn_id.clone().or(active), last_error: Some(message.clone()) }),
        _ => None,
    }
}

fn activity(event: &Runtime, tone: &str, kind: &str, summary: String, payload: Value) -> Activity {
    Activity { id: event.event_id.clone(), tone: tone.into(), kind: kind.into(), summary, payload, turn_id: event.turn_id.clone(), sequence: None, created_at: event.created_at.clone() }
}

fn command_id(event: &Runtime, tag: &str, new_id: &dyn Fn() -> String) -> String {
    format!("provider:{}:{tag}:{}", event.event_id, new_id())
}

fn new_command(event: &Runtime, tag: &str, new_id: &dyn Fn() -> String, body: Body) -> Command {
    Command { command_id: command_id(event, tag, new_id), thread_id: event.thread_id.clone(), body }
}

fn append(event: &Runtime, new_id: &dyn Fn() -> String, activity: Activity) -> Command {
    new_command(event, "thread-activity-append", new_id, Body::ActivityAppend { activity })
}

fn message_append(event: &Runtime, new_id: &dyn Fn() -> String, delta: &str, streaming: bool) -> Command {
    let key = event.item_id.clone().or_else(|| event.turn_id.clone()).unwrap_or_else(|| event.event_id.clone());
    new_command(event, if streaming { "assistant-delta" } else { "assistant-complete" }, new_id, Body::MessageAppend { message_id: format!("assistant:{key}"), delta: delta.into(), turn_id: event.turn_id.clone() })
}

fn proposed_plan_id(event: &Runtime) -> String {
    if let Some(turn) = &event.turn_id {
        return format!("plan:{}:turn:{turn}", event.thread_id);
    }
    if let Some(item) = &event.item_id {
        return format!("plan:{}:item:{item}", event.thread_id);
    }
    format!("plan:{}:event:{}", event.thread_id, event.event_id)
}

fn upsert_plan(event: &Runtime, new_id: &dyn Fn() -> String, plan: ProposedPlan) -> Command {
    new_command(event, "proposed-plan-upsert", new_id, Body::ProposedPlanUpsert { plan })
}

/// The commands one runtime event produces for the thread (None when the engine has not seen it).
pub fn ingest(thread: Option<&Thread>, event: &Runtime, new_id: &dyn Fn() -> String) -> Vec<Command> {
    let mut commands = Vec::new();
    let now = &event.created_at;

    if let Some(lifecycle) = derive_lifecycle(thread, event) {
        let session = Session {
            cli: event.provider.clone(),
            provider_session_id: thread.and_then(|t| t.session.as_ref()).and_then(|s| s.provider_session_id.clone()),
            status: lifecycle.status.into(),
            active_turn_id: lifecycle.active_turn_id,
            last_error: lifecycle.last_error,
        };
        commands.push(new_command(event, "thread-session-set", new_id, Body::SessionSet { session }));
    }

    match &event.kind {
        Kind::ContentDelta { stream_kind, delta } if !delta.is_empty() => {
            if stream_kind == "assistant_text" {
                commands.push(message_append(event, new_id, delta, true));
            } else if stream_kind == "reasoning_text" || stream_kind == "reasoning_summary_text" {
                commands.push(append(event, new_id, activity(event, "info", "reasoning.delta", "Reasoning".into(), json!({ "delta": delta }))));
            }
        }
        Kind::Item { phase, item } => {
            if item.item_type == "assistant_message" {
                if *phase == Phase::Completed {
                    if let Some(detail) = item.detail.as_deref().filter(|d| !d.is_empty()) {
                        commands.push(message_append(event, new_id, detail, false));
                    }
                }
            } else if TOOL_ITEM_TYPES.contains(&item.item_type.as_str()) {
                let status = item.status.as_deref();
                let kind = match phase {
                    Phase::Started => "tool.started",
                    Phase::Updated => "tool.updated",
                    Phase::Completed => match status {
                        Some("failed") => "tool.failed",
                        Some("declined") => "tool.declined",
                        _ => "tool.completed",
                    },
                };
                let failed = *phase == Phase::Completed && matches!(status, Some("failed" | "declined"));
                let tone = if failed { "error" } else { "tool" };
                let title = item.title.clone().filter(|t| !t.is_empty()).unwrap_or_else(|| item.item_type.clone());
                let summary = if *phase == Phase::Started && tone == "tool" { format!("{title} started") } else { title };
                let data = item.data.as_ref().map(project_tool_data).unwrap_or_default();
                let mut payload = Map::new();
                payload.insert("itemType".into(), json!(item.item_type));
                if let Some(id) = &event.item_id {
                    payload.insert("itemId".into(), json!(id));
                }
                if let Some(s) = &item.status {
                    payload.insert("status".into(), json!(s));
                }
                if let Some(t) = &item.title {
                    payload.insert("title".into(), json!(t));
                }
                if let Some(d) = &item.detail {
                    payload.insert("detail".into(), json!(truncate_detail(d)));
                }
                if let Some(a) = &item.agent_id {
                    payload.insert("agentId".into(), json!(a));
                }
                if let Some(p) = &item.parent_tool_use_id {
                    payload.insert("parentToolUseId".into(), json!(p));
                }
                if !data.is_empty() {
                    payload.insert("data".into(), Value::Object(data));
                }
                commands.push(append(event, new_id, activity(event, tone, kind, summary, Value::Object(payload))));
            }
        }
        Kind::RequestOpened { request_type, detail } if request_type != "tool_user_input" => {
            commands.push(append(event, new_id, activity(event, "approval", "approval.requested", approval_summary(true, request_type).into(), approval_payload(event, request_type, ("detail", detail)))));
        }
        Kind::RequestResolved { request_type, decision } if request_type != "tool_user_input" => {
            commands.push(append(event, new_id, activity(event, "approval", "approval.resolved", approval_summary(false, request_type).into(), approval_payload(event, request_type, ("decision", decision)))));
        }
        Kind::UserInputRequested { questions } => {
            let mut payload = Map::new();
            if let Some(id) = &event.request_id {
                payload.insert("requestId".into(), json!(id));
            }
            payload.insert("questions".into(), questions.clone());
            commands.push(append(event, new_id, activity(event, "info", "user-input.requested", "User input requested".into(), Value::Object(payload))));
        }
        Kind::TurnPlanUpdated { plan, explanation } => {
            let mut payload = Map::new();
            payload.insert("plan".into(), plan.clone());
            if let Some(e) = explanation {
                payload.insert("explanation".into(), json!(e));
            }
            commands.push(append(event, new_id, activity(event, "info", "plan.updated", "Plan updated".into(), Value::Object(payload))));
        }
        Kind::TurnProposedDelta { delta } => {
            let id = proposed_plan_id(event);
            let existing = thread.and_then(|t| t.proposed_plans.iter().find(|p| p.id == id));
            let merged = existing.map_or_else(|| delta.clone(), |p| format!("{}{delta}", p.plan_markdown));
            if !merged.trim().is_empty() {
                commands.push(upsert_plan(event, new_id, ProposedPlan { id, turn_id: event.turn_id.clone(), plan_markdown: merged, created_at: existing.map_or_else(|| now.clone(), |p| p.created_at.clone()), updated_at: now.clone() }));
            }
        }
        Kind::TurnProposedCompleted { plan_markdown } => {
            let id = proposed_plan_id(event);
            let existing = thread.and_then(|t| t.proposed_plans.iter().find(|p| p.id == id));
            if !plan_markdown.trim().is_empty() {
                commands.push(upsert_plan(event, new_id, ProposedPlan { id, turn_id: event.turn_id.clone(), plan_markdown: plan_markdown.clone(), created_at: existing.map_or_else(|| now.clone(), |p| p.created_at.clone()), updated_at: now.clone() }));
            }
        }
        _ => {}
    }
    commands
}
