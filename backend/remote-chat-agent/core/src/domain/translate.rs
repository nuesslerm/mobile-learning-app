//! What an agent backend reports into the canonical runtime events the ingestion understands. One translator per turn stream; it
//! keeps what it has seen (message roles, how much of each part was already handed out, which tools are known) so a snapshot that
//! arrives after its increments adds nothing, and the user's own prompt echoed back is never taken for an answer.

use super::ingest::{Item, Kind, Phase, Runtime};
use super::accumulator::Accumulator;
use super::agent::AgentEvent;
use crate::domain::ids::{Clock, IdSource};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq)]
enum PartKind {
    Text,
    Reasoning,
}

pub struct Translator {
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdSource>,
    thread_id: String,
    provider: String,
    /// Whether tool activities carry their input and output (the thread view), or only their name and state (the chat stream).
    tool_data: bool,
    roles: HashMap<String, String>,
    accumulator: Accumulator,
    part_kinds: HashMap<String, PartKind>,
    tools_seen: HashSet<String>,
    tool_status: HashMap<String, String>,
    turn_completed_before_idle: bool,
}

fn tool_status(raw: Option<&str>) -> Option<&'static str> {
    match raw?.to_ascii_lowercase().as_str() {
        "running" | "pending" | "in_progress" | "inprogress" => Some("inProgress"),
        "completed" | "complete" | "success" | "succeeded" => Some("completed"),
        "failed" | "error" => Some("failed"),
        "declined" | "denied" | "cancelled" | "canceled" => Some("declined"),
        _ => None,
    }
}

fn terminal(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "declined")
}

fn plan_status(raw: &str) -> &'static str {
    match raw.to_ascii_lowercase().as_str() {
        "in_progress" | "inprogress" | "running" | "active" => "inProgress",
        "completed" | "done" | "finished" => "completed",
        _ => "pending",
    }
}

fn request_type(kind: &str) -> &'static str {
    match kind {
        "command" => "command_execution_approval",
        "file-read" => "file_read_approval",
        "file-change" => "file_change_approval",
        _ => "unknown",
    }
}

/// `toolNameToItemType`: what kind of tool a name is. Read tools map to a type the ingestion does not turn into an activity.
pub fn tool_item_type(name: &str) -> &'static str {
    let n = name.to_lowercase();
    if ["bash", "command", "shell", "exec"].contains(&n.as_str()) {
        "command_execution"
    } else if ["edit", "write", "apply_patch", "file_change", "multiedit", "notebookedit"].contains(&n.as_str()) {
        "file_change"
    } else if ["read", "glob", "grep"].contains(&n.as_str()) {
        "file_read"
    } else if n.starts_with("mcp_") || n.contains("__") {
        "mcp_tool_call"
    } else if n == "websearch" || n == "webfetch" {
        "web_search"
    } else {
        "dynamic_tool_call"
    }
}

fn nonempty(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string)
}

impl Translator {
    pub fn new(thread_id: &str, provider: &str, tool_data: bool, clock: Arc<dyn Clock>, ids: Arc<dyn IdSource>) -> Self {
        Self {
            clock,
            ids,
            thread_id: thread_id.into(),
            provider: provider.into(),
            tool_data,
            roles: HashMap::new(),
            accumulator: Accumulator::default(),
            part_kinds: HashMap::new(),
            tools_seen: HashSet::new(),
            tool_status: HashMap::new(),
            turn_completed_before_idle: false,
        }
    }

    fn event(&self, turn_id: Option<&str>, item_id: Option<&str>, request_id: Option<&str>, kind: Kind) -> Runtime {
        Runtime {
            event_id: self.ids.uuid(),
            provider: self.provider.clone(),
            thread_id: self.thread_id.clone(),
            created_at: self.clock.now_iso(),
            turn_id: turn_id.map(str::to_string),
            item_id: item_id.map(str::to_string),
            request_id: request_id.map(str::to_string),
            kind,
        }
    }

    /// The part of `text` not handed out yet for a part; `increment` is true for a delta and false for a snapshot.
    fn unseen(&mut self, part_id: &str, text: &str, increment: bool) -> Option<String> {
        if increment {
            self.accumulator.delta(part_id, text)
        } else {
            self.accumulator.snapshot(part_id, text)
        }
    }

    fn user_part(&self, message_id: Option<&str>) -> bool {
        message_id.is_some_and(|m| self.roles.get(m).is_some_and(|r| r == "user"))
    }

    fn content(&mut self, part_id: &str, kind: PartKind, text: &str, increment: bool) -> Vec<Runtime> {
        let Some(chunk) = self.unseen(part_id, text, increment) else { return Vec::new() };
        let stream_kind = if kind == PartKind::Reasoning { "reasoning_text" } else { "assistant_text" };
        vec![self.event(None, Some(part_id), None, Kind::ContentDelta { stream_kind: stream_kind.into(), delta: chunk })]
    }

    fn tool_part(&mut self, part: &Value) -> Vec<Runtime> {
        let Some(part_id) = nonempty(part.get("id")) else { return Vec::new() };
        let state = part.get("state");
        let status = tool_status(state.and_then(|s| s.get("status")).and_then(Value::as_str));
        let call_id = nonempty(part.get("callID"));
        let item_id = call_id.clone().unwrap_or_else(|| part_id.clone());
        let name = nonempty(part.get("toolName")).or_else(|| nonempty(part.get("tool")));
        let item_type = if self.tool_data { tool_item_type(name.as_deref().unwrap_or_default()) } else { "command_execution" };
        let title = name.clone().or_else(|| (!self.tool_data).then(|| "tool".to_string()));
        let input = state.and_then(|s| s.get("input")).cloned();
        let output = state.and_then(|s| s.get("output")).cloned();
        let patch = state.and_then(|s| s.get("metadata")).and_then(|m| m.get("diff")).cloned();

        let started_data = || -> Option<Value> { self.tool_data.then(|| input.clone().map(|i| json!({ "input": i }))).flatten() };
        let done_data = || -> Option<Value> {
            if !self.tool_data {
                return None;
            }
            let mut data = serde_json::Map::new();
            if let Some(o) = &output {
                data.insert("output".into(), o.clone());
            }
            if let Some(p) = &patch {
                data.insert("patch".into(), p.clone());
            }
            (!data.is_empty()).then_some(Value::Object(data))
        };
        let item = |status: &str, data: Option<Value>| Item { item_type: item_type.into(), status: Some(status.into()), title: title.clone(), data, agent_id: call_id.clone(), ..Item::default() };

        let mut out = Vec::new();
        if self.tools_seen.insert(part_id.clone()) {
            let first = status.unwrap_or("inProgress");
            if let Some(s) = status {
                self.tool_status.insert(part_id.clone(), s.into());
            }
            out.push(self.event(None, Some(&item_id), None, Kind::Item { phase: Phase::Started, item: item(if terminal(first) { "inProgress" } else { first }, started_data()) }));
            if terminal(first) {
                out.push(self.event(None, Some(&item_id), None, Kind::Item { phase: Phase::Completed, item: item(first, done_data()) }));
            }
            return out;
        }
        let Some(status) = status else { return out };
        if self.tool_status.get(&part_id).is_some_and(|p| p == status) {
            return out;
        }
        self.tool_status.insert(part_id, status.into());
        if terminal(status) {
            out.push(self.event(None, Some(&item_id), None, Kind::Item { phase: Phase::Completed, item: item(status, done_data()) }));
        } else {
            out.push(self.event(None, Some(&item_id), None, Kind::Item { phase: Phase::Updated, item: item(status, started_data()) }));
        }
        out
    }

    fn turn_pair(&self, turn_id: &str, state: &str, stop_reason: Option<&str>) -> Vec<Runtime> {
        let _ = stop_reason;
        vec![
            self.event(Some(turn_id), None, None, Kind::TurnStarted),
            self.event(Some(turn_id), None, None, Kind::TurnCompleted { state: state.into(), error_message: None }),
        ]
    }

    /// `active_turn` is the turn the thread's session says is running, which a completion settles rather than starting a new one.
    pub fn translate(&mut self, ev: &AgentEvent, active_turn: Option<&str>) -> Vec<Runtime> {
        let props = &ev.props;
        let ids = Arc::clone(&self.ids);
        let turn_id = || active_turn.map_or_else(|| format!("turn_{}", ids.hex(4)), str::to_string);
        match ev.kind.as_str() {
            "message.updated" => {
                let info = props.get("info");
                if let (Some(id), Some(role)) = (nonempty(info.and_then(|i| i.get("id"))), nonempty(info.and_then(|i| i.get("role")))) {
                    if role == "user" || role == "assistant" {
                        self.roles.insert(id, role);
                    }
                }
                Vec::new()
            }
            "message.part.updated" => {
                let Some(part) = props.get("part") else { return Vec::new() };
                if part.get("type").and_then(Value::as_str) == Some("tool") {
                    return self.tool_part(part);
                }
                let Some(part_id) = nonempty(part.get("id")) else { return Vec::new() };
                let part_type = part.get("type").and_then(Value::as_str);
                let field = part.get("field").and_then(Value::as_str);
                let kind = if field == Some("reasoning") || part_type == Some("reasoning") {
                    PartKind::Reasoning
                } else if part_type == Some("text") || field == Some("text") || part_type.is_none() {
                    PartKind::Text
                } else {
                    return Vec::new();
                };
                self.part_kinds.insert(part_id.clone(), kind);
                if self.user_part(part.get("messageID").and_then(Value::as_str)) {
                    return Vec::new();
                }
                match part.get("text").and_then(Value::as_str) {
                    Some(text) => self.content(&part_id, kind, text, false),
                    None => Vec::new(),
                }
            }
            "message.part.delta" => {
                let Some(part_id) = nonempty(props.get("partID")) else { return Vec::new() };
                let kind = match props.get("field").and_then(Value::as_str) {
                    Some("reasoning") => PartKind::Reasoning,
                    Some("text") => PartKind::Text,
                    _ => match self.part_kinds.get(&part_id) {
                        Some(k) => *k,
                        None => return Vec::new(),
                    },
                };
                self.part_kinds.insert(part_id.clone(), kind);
                if self.user_part(props.get("messageID").and_then(Value::as_str)) {
                    return Vec::new();
                }
                match props.get("delta").and_then(Value::as_str) {
                    Some(delta) => self.content(&part_id, kind, delta, true),
                    None => Vec::new(),
                }
            }
            "session.next.tool.called" => {
                let name = nonempty(props.get("tool")).unwrap_or_else(|| "tool".into());
                let item_id = format!("tool_{}", self.ids.hex(4));
                let item = Item { item_type: "command_execution".into(), status: Some("inProgress".into()), title: Some(name), ..Item::default() };
                vec![self.event(None, Some(&item_id), None, Kind::Item { phase: Phase::Started, item })]
            }
            "session.next.tool.success" | "session.next.tool.failed" => {
                let name = nonempty(props.get("tool")).unwrap_or_else(|| "tool".into());
                let status = if ev.kind.ends_with("failed") { "failed" } else { "completed" };
                let item = Item { item_type: "command_execution".into(), status: Some(status.into()), title: Some(name), ..Item::default() };
                vec![self.event(None, None, None, Kind::Item { phase: Phase::Completed, item })]
            }
            "session.turn.completed" => {
                self.turn_completed_before_idle = true;
                let state = match ev.str_prop("state") {
                    Some("failed") => "failed",
                    Some("interrupted") => "interrupted",
                    _ => "completed",
                };
                let id = nonempty(props.get("turnID")).unwrap_or_else(turn_id);
                self.turn_pair(&id, state, ev.str_prop("stopReason"))
            }
            "session.idle" => {
                if std::mem::take(&mut self.turn_completed_before_idle) {
                    return Vec::new();
                }
                let id = turn_id();
                self.turn_pair(&id, "completed", Some("session.idle"))
            }
            "session.error" => {
                let error = props.get("error");
                let message = nonempty(error.and_then(|e| e.get("data")).and_then(|d| d.get("message"))).or_else(|| nonempty(error.and_then(|e| e.get("name")))).unwrap_or_else(|| "session.error".into());
                vec![self.event(None, None, None, Kind::RuntimeError { message }), self.event(None, None, None, Kind::TurnAborted)]
            }
            "permission.asked" => {
                let Some(id) = nonempty(props.get("id")) else { return Vec::new() };
                let detail = nonempty(props.get("detail"));
                vec![self.event(None, None, Some(&id), Kind::RequestOpened { request_type: request_type(ev.str_prop("kind").unwrap_or("unknown")).into(), detail })]
            }
            "permission.replied" => {
                let Some(id) = nonempty(props.get("id")) else { return Vec::new() };
                let decision = nonempty(props.get("reply"));
                vec![self.event(None, None, Some(&id), Kind::RequestResolved { request_type: "unknown".into(), decision })]
            }
            "session.plan.updated" => {
                let plan: Vec<Value> = props.get("plan").and_then(Value::as_array).map(|steps| steps.iter().map(|s| json!({ "step": s.get("step").and_then(Value::as_str).unwrap_or_default(), "status": plan_status(s.get("status").and_then(Value::as_str).unwrap_or_default()) })).collect()).unwrap_or_default();
                vec![self.event(None, None, None, Kind::TurnPlanUpdated { plan: Value::Array(plan), explanation: nonempty(props.get("explanation")) })]
            }
            "session.plan.proposed" => {
                let markdown = ev.str_prop("planMarkdown").unwrap_or_default().to_string();
                if props.get("done").and_then(Value::as_bool) == Some(true) {
                    vec![self.event(None, None, None, Kind::TurnProposedCompleted { plan_markdown: markdown })]
                } else {
                    vec![self.event(None, None, None, Kind::TurnProposedDelta { delta: markdown })]
                }
            }
            "question.asked" => {
                let questions: Vec<Value> = props
                    .get("questions")
                    .and_then(Value::as_array)
                    .map(|qs| {
                        qs.iter()
                            .map(|q| {
                                let header = q.get("header").and_then(Value::as_str).unwrap_or_default();
                                json!({
                                    "id": header,
                                    "header": header,
                                    "question": q.get("question").and_then(Value::as_str).unwrap_or_default(),
                                    "options": q.get("options").filter(|o| o.is_array()).cloned().unwrap_or_else(|| json!([])),
                                    "multiSelect": q.get("multiple").and_then(Value::as_bool).unwrap_or(false),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let id = nonempty(props.get("id"));
                vec![self.event(None, None, id.as_deref(), Kind::UserInputRequested { questions: Value::Array(questions) })]
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{CountingIds, FixedClock};

    #[test]
    fn given_a_stopped_clock_and_counting_ids_when_a_session_goes_idle_then_the_turn_events_carry_those_ids_and_that_instant() {
        // Given a translator on a clock stopped at 14 Nov 2023 22:13:20 UTC and ids that count
        let mut translator = Translator::new("thr_1", "opencode", true, Arc::new(FixedClock(1_700_000_000_000)), Arc::new(CountingIds::default()));

        // When the agent reports that the session went idle with no turn in flight
        let events = translator.translate(&AgentEvent::new("session.idle", serde_json::json!({})), None);

        // Then the turn it settles is named from the first id, and each event has the next id and the stopped instant
        let ids: Vec<&str> = events.iter().map(|e| e.event_id.as_str()).collect();
        assert_eq!(ids, ["00000000-0000-4000-8000-000000000002", "00000000-0000-4000-8000-000000000003"]);
        assert!(events.iter().all(|e| e.created_at == "2023-11-14T22:13:20.000Z" && e.turn_id.as_deref() == Some("turn_00000001")), "{events:?}");
    }
}

