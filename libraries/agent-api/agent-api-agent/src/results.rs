//! What each method answers. The service builds these; a mapper reads them. A field a reader does not know is ignored, so a service may add one.
use crate::events::{FileOp, Role, ThreadEvent};
use agent_api_proto::{thread_topic, Event, ParseError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Idle,
    Running,
}

/// A thread as `thread.create`, `thread.get` and `thread.list` show it. `turn` is the running turn, and `last_seq` the `seq` of its newest event
/// (0 for a thread with none), the `after_seq` to resume from. Times are RFC 3339.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThreadView {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub status: Status,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<String>,
    pub last_seq: u64,
    pub created_at: String,
    pub updated_at: String,
}

impl ThreadView {
    /// The result of `thread.create` and `thread.get`: `{thread: <view>}`.
    pub fn to_result(&self) -> Value {
        json!({ "thread": self })
    }

    pub fn from_result(result: &Value) -> Result<ThreadView, ParseError> {
        let thread = result.get("thread").cloned().ok_or_else(|| ParseError("a thread result needs a thread".into()))?;
        serde_json::from_value(thread).map_err(|e| ParseError(e.to_string()))
    }
}

/// The result of `thread.list`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListResult {
    pub threads: Vec<ThreadView>,
}

/// The result of `thread.subscribe`: the stream the events come on and the `seq` the replay starts after and ends at (`last_seq` when the
/// subscription was made; `after_seq == last_seq` means nothing was missed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubscribeResult {
    pub topic: String,
    pub after_seq: u64,
    pub last_seq: u64,
}

impl SubscribeResult {
    pub fn new(thread: &str, after_seq: u64, last_seq: u64) -> SubscribeResult {
        SubscribeResult { topic: thread_topic(thread), after_seq, last_seq }
    }
}

/// The result of `thread.history`: up to `limit` events after `after_seq`, each the `event` object of its frame, and whether more follow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryResult {
    pub events: Vec<Value>,
    pub last_seq: u64,
    pub more: bool,
}

impl HistoryResult {
    pub fn new(events: &[Event], last_seq: u64, more: bool) -> HistoryResult {
        HistoryResult { events: events.iter().map(|e| e.to_value()["event"].clone()).collect(), last_seq, more }
    }

    /// The events as wire events, or the first that is none.
    pub fn events(&self) -> Result<Vec<Event>, ParseError> {
        self.events.iter().map(|object| Event::from_value(&json!({ "event": object }))).collect()
    }
}

/// One row of a transcript: the settled item, with the `seq` of the event that opened it and the turn it belongs to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TranscriptItem {
    Message { seq: u64, turn: Option<String>, role: Role, text: String },
    Reasoning { seq: u64, turn: Option<String>, text: String },
    Tool { seq: u64, turn: Option<String>, call: String, tool: String, args: Value, output: String, ok: Option<bool>, result: Value },
    File { seq: u64, turn: Option<String>, path: String, op: FileOp },
    Plan { seq: u64, turn: Option<String>, plan: String },
    Error { seq: u64, turn: Option<String>, code: String, message: String },
}

/// The result of `thread.transcript`: the items of every event up to `last_seq`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptResult {
    pub thread: String,
    pub last_seq: u64,
    pub items: Vec<TranscriptItem>,
}

impl TranscriptResult {
    /// The transcript of a thread's events in `seq` order: consecutive deltas of one role in one turn are one message, a tool's start, output and
    /// end are one row, a later plan replaces its turn's earlier one. Waiting requests, usage and the turn's own start and end are not rows.
    pub fn fold(thread: &str, events: &[Event]) -> TranscriptResult {
        let mut items: Vec<TranscriptItem> = Vec::new();
        for event in events {
            let Ok(kind) = ThreadEvent::from_event(event) else { continue };
            let (seq, turn) = (event.seq, event.turn.clone());
            match kind {
                ThreadEvent::MessageDelta { role, text } => match items.last_mut() {
                    Some(TranscriptItem::Message { turn: t, role: r, text: body, .. }) if *t == turn && *r == role => body.push_str(&text),
                    _ => items.push(TranscriptItem::Message { seq, turn, role, text }),
                },
                ThreadEvent::ReasoningDelta { text } => match items.last_mut() {
                    Some(TranscriptItem::Reasoning { turn: t, text: body, .. }) if *t == turn => body.push_str(&text),
                    _ => items.push(TranscriptItem::Reasoning { seq, turn, text }),
                },
                ThreadEvent::ToolStarted { call, tool, args } => items.push(TranscriptItem::Tool { seq, turn, call, tool, args, output: String::new(), ok: None, result: Value::Null }),
                ThreadEvent::ToolOutput { call, data, .. } => {
                    if let Some(TranscriptItem::Tool { output, .. }) = items.iter_mut().rev().find(|i| matches!(i, TranscriptItem::Tool { call: c, .. } if *c == call)) {
                        output.push_str(&data);
                    }
                }
                ThreadEvent::ToolFinished { call, ok: done, result: value } => {
                    if let Some(TranscriptItem::Tool { ok, result, .. }) = items.iter_mut().rev().find(|i| matches!(i, TranscriptItem::Tool { call: c, .. } if *c == call)) {
                        (*ok, *result) = (Some(done), value);
                    }
                }
                ThreadEvent::FileChanged { path, op } => items.push(TranscriptItem::File { seq, turn, path, op }),
                ThreadEvent::PlanUpdated { plan } => match items.iter_mut().find(|i| matches!(i, TranscriptItem::Plan { turn: t, .. } if *t == turn)) {
                    Some(TranscriptItem::Plan { plan: body, .. }) => *body = plan,
                    _ => items.push(TranscriptItem::Plan { seq, turn, plan }),
                },
                ThreadEvent::Error { code, message } => items.push(TranscriptItem::Error { seq, turn, code, message }),
                ThreadEvent::TurnStarted {} | ThreadEvent::ApprovalRequested { .. } | ThreadEvent::InputRequested { .. } | ThreadEvent::Usage(_) | ThreadEvent::TurnFinished { .. } => {}
            }
        }
        TranscriptResult { thread: thread.to_string(), last_seq: events.last().map_or(0, |e| e.seq), items }
    }
}

/// The result of `turn.start`: the turn that began (its `turn.started` event is the first it emits).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartResult {
    pub thread: String,
    pub turn: String,
}

/// The result of `turn.interrupt`: the turn that was asked to stop (it ends with `turn.finished {reason: interrupted}`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InterruptResult {
    pub thread: String,
    pub turn: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{FinishReason, OutputStream};

    fn view() -> ThreadView {
        ThreadView { id: "t1".into(), title: "Fix login".into(), workspace: Some("w1".into()), driver: None, model: None, status: Status::Running, turn: Some("u1".into()), last_seq: 7, created_at: "2026-11-04T10:00:00Z".into(), updated_at: "2026-11-04T10:01:00Z".into() }
    }

    #[test]
    fn given_a_thread_view_when_made_a_result_and_read_back_then_it_is_the_same_and_unset_fields_are_absent() {
        // Given a running thread with a workspace and no driver or model
        // When it is made the result of thread.get and read back
        let result = view().to_result();
        // Then it is unchanged and the fields that are none are not in the frame
        assert_eq!(ThreadView::from_result(&result), Ok(view()));
        assert_eq!(result["thread"].as_object().unwrap().keys().collect::<Vec<_>>(), ["id", "title", "workspace", "status", "turn", "last_seq", "created_at", "updated_at"]);
        assert!(ThreadView::from_result(&json!({})).is_err());
    }

    fn at(seq: u64, turn: &str, event: ThreadEvent) -> Event {
        event.into_event("t1", Some(turn), seq)
    }

    #[test]
    fn given_the_events_of_two_turns_when_folded_then_deltas_and_a_tools_events_become_one_row_each_and_what_is_not_a_row_is_left_out() {
        // Given a turn with split reasoning and text, a tool with two outputs and a result, an edit, an approval, usage, a plan and its update, then a failed turn
        let events = vec![
            at(1, "u1", ThreadEvent::TurnStarted {}),
            at(2, "u1", ThreadEvent::ReasoningDelta { text: "look ".into() }),
            at(3, "u1", ThreadEvent::ReasoningDelta { text: "first".into() }),
            at(4, "u1", ThreadEvent::MessageDelta { role: Role::Assistant, text: "Rea".into() }),
            at(5, "u1", ThreadEvent::MessageDelta { role: Role::Assistant, text: "ding".into() }),
            at(6, "u1", ThreadEvent::ToolStarted { call: "c1".into(), tool: "run_command".into(), args: json!({"command": "ls"}) }),
            at(7, "u1", ThreadEvent::ToolOutput { call: "c1".into(), stream: OutputStream::Stdout, data: "a\n".into() }),
            at(8, "u1", ThreadEvent::ToolOutput { call: "c1".into(), stream: OutputStream::Stdout, data: "b\n".into() }),
            at(9, "u1", ThreadEvent::ToolFinished { call: "c1".into(), ok: true, result: json!({"exit_code": 0}) }),
            at(10, "u1", ThreadEvent::FileChanged { path: "a".into(), op: FileOp::Edit }),
            at(11, "u1", ThreadEvent::ApprovalRequested { approval: "ap".into(), tool: "run_command".into(), args: json!({}) }),
            at(12, "u1", ThreadEvent::PlanUpdated { plan: "1. a".into() }),
            at(13, "u1", ThreadEvent::PlanUpdated { plan: "1. a\n2. b".into() }),
            at(14, "u1", ThreadEvent::Usage(crate::events::Usage { input_tokens: 1, output_tokens: 1, cache_read_tokens: None })),
            at(15, "u1", ThreadEvent::TurnFinished { reason: FinishReason::Completed }),
            at(16, "u2", ThreadEvent::TurnStarted {}),
            at(17, "u2", ThreadEvent::MessageDelta { role: Role::Assistant, text: "Again".into() }),
            at(18, "u2", ThreadEvent::Error { code: "RATE_LIMITED".into(), message: "slow down".into() }),
        ];
        // When folded
        let transcript = TranscriptResult::fold("t1", &events);
        // Then the rows are the settled items in order, a message of the next turn is its own row, and last_seq is the newest event's
        let kinds: Vec<String> = transcript.items.iter().map(|i| serde_json::to_value(i).unwrap()["kind"].as_str().unwrap().to_string()).collect();
        assert_eq!(kinds, ["reasoning", "message", "tool", "file", "plan", "message", "error"]);
        assert_eq!(transcript.last_seq, 18);
        let turn = |t: &str| Some(t.to_string());
        assert_eq!(transcript.items[0], TranscriptItem::Reasoning { seq: 2, turn: turn("u1"), text: "look first".into() });
        assert_eq!(transcript.items[1], TranscriptItem::Message { seq: 4, turn: turn("u1"), role: Role::Assistant, text: "Reading".into() });
        assert_eq!(transcript.items[2], TranscriptItem::Tool { seq: 6, turn: turn("u1"), call: "c1".into(), tool: "run_command".into(), args: json!({"command": "ls"}), output: "a\nb\n".into(), ok: Some(true), result: json!({"exit_code": 0}) });
        assert_eq!(transcript.items[4], TranscriptItem::Plan { seq: 12, turn: turn("u1"), plan: "1. a\n2. b".into() });
        assert_eq!(transcript.items[5], TranscriptItem::Message { seq: 17, turn: turn("u2"), role: Role::Assistant, text: "Again".into() });
    }

    #[test]
    fn given_no_events_or_events_no_thread_event_when_folded_then_the_transcript_is_empty_and_a_tool_that_never_finished_has_no_verdict() {
        // Given no events, an event of a kind the contract does not know, and a tool that started and never finished
        let unknown = Event { seq: 1, thread: Some("t1".into()), turn: None, kind: "reset".into(), data: Default::default() };
        let open = vec![at(1, "u1", ThreadEvent::ToolStarted { call: "c".into(), tool: "search".into(), args: json!({}) })];
        // When folded, Then the first two are empty and the tool has no ok and a null result
        assert_eq!(TranscriptResult::fold("t1", &[]), TranscriptResult { thread: "t1".into(), last_seq: 0, items: vec![] });
        assert!(TranscriptResult::fold("t1", &[unknown]).items.is_empty());
        assert!(matches!(&TranscriptResult::fold("t1", &open).items[0], TranscriptItem::Tool { ok: None, result: Value::Null, .. }));
    }

    #[test]
    fn given_a_history_page_when_built_from_events_and_read_back_then_the_events_are_the_same_and_the_subscription_names_the_topic() {
        // Given two events
        let events = vec![at(1, "u1", ThreadEvent::TurnStarted {}), at(2, "u1", ThreadEvent::ReasoningDelta { text: "x".into() })];
        // When they are a history page and read back
        let page = HistoryResult::new(&events, 2, true);
        // Then each is the event object of its frame, they read back equal, and a subscribe result names thread/<id>
        assert_eq!(page.events[0], json!({"seq": 1, "thread": "t1", "turn": "u1", "type": "turn.started"}));
        assert_eq!(page.events(), Ok(events));
        assert_eq!(serde_json::to_value(SubscribeResult::new("t1", 2, 9)).unwrap(), json!({"topic": "thread/t1", "after_seq": 2, "last_seq": 9}));
        assert!(HistoryResult { events: vec![json!({"type": "x"})], last_seq: 0, more: false }.events().is_err());
    }
}
