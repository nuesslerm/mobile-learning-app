//! The thirteen neutral thread events (`docs/plans/rca-client/architecture.md`), whatever agent CLI produced them. On the wire each is an
//! `agent_api_proto::Event`: the envelope names the thread, the turn and the thread's `seq`, `type` is the event's kind and the fields below
//! sit beside it. A client that meets a field it does not know ignores it; a kind it does not know is an error to report, not to guess at.
use agent_api_proto::{Event, ParseError};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// What a tool did to a file, as the executor's write and edit results tell it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileOp {
    Create,
    Write,
    Edit,
}

/// Why a turn ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Completed,
    Interrupted,
    Failed,
}

/// Tokens a turn has used so far; the counts the CLI gave, none invented.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ThreadEvent {
    /// A turn began; the envelope's `turn` is its id.
    #[serde(rename = "turn.started")]
    TurnStarted {},
    #[serde(rename = "message.delta")]
    MessageDelta { role: Role, text: String },
    #[serde(rename = "reasoning.delta")]
    ReasoningDelta { text: String },
    /// `call` ties this to the `tool.output` and `tool.finished` of the same call.
    #[serde(rename = "tool.started")]
    ToolStarted { call: String, tool: String, args: Value },
    #[serde(rename = "tool.output")]
    ToolOutput { call: String, stream: OutputStream, data: String },
    #[serde(rename = "tool.finished")]
    ToolFinished { call: String, ok: bool, result: Value },
    #[serde(rename = "file.changed")]
    FileChanged { path: String, op: FileOp },
    /// The turn waits for `approval.answer` before it runs `tool`.
    #[serde(rename = "approval.requested")]
    ApprovalRequested { approval: String, tool: String, args: Value },
    /// The turn waits for `input.answer`.
    #[serde(rename = "input.requested")]
    InputRequested {
        request: String,
        prompt: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        options: Vec<String>,
    },
    /// The plan so far, as markdown.
    #[serde(rename = "plan.updated")]
    PlanUpdated { plan: String },
    #[serde(rename = "usage")]
    Usage(Usage),
    #[serde(rename = "error")]
    Error { code: String, message: String },
    #[serde(rename = "turn.finished")]
    TurnFinished { reason: FinishReason },
}

/// The `type` of every event, in the order the owner lists them.
pub const KINDS: [&str; 13] = [
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

impl ThreadEvent {
    /// The event's `type`.
    pub fn kind(&self) -> &'static str {
        match self {
            ThreadEvent::TurnStarted {} => KINDS[0],
            ThreadEvent::MessageDelta { .. } => KINDS[1],
            ThreadEvent::ReasoningDelta { .. } => KINDS[2],
            ThreadEvent::ToolStarted { .. } => KINDS[3],
            ThreadEvent::ToolOutput { .. } => KINDS[4],
            ThreadEvent::ToolFinished { .. } => KINDS[5],
            ThreadEvent::FileChanged { .. } => KINDS[6],
            ThreadEvent::ApprovalRequested { .. } => KINDS[7],
            ThreadEvent::InputRequested { .. } => KINDS[8],
            ThreadEvent::PlanUpdated { .. } => KINDS[9],
            ThreadEvent::Usage(_) => KINDS[10],
            ThreadEvent::Error { .. } => KINDS[11],
            ThreadEvent::TurnFinished { .. } => KINDS[12],
        }
    }

    /// The event as it goes on the wire: `seq` is the thread's engine sequence, `turn` the turn it belongs to (none for an event of no turn).
    pub fn into_event(&self, thread: &str, turn: Option<&str>, seq: u64) -> Event {
        // Not `remove`: it swaps the last field into the place of the one it takes out, and the fields keep the order they are declared in
        let data: Map<String, Value> = match serde_json::to_value(self) {
            Ok(Value::Object(fields)) => fields.into_iter().filter(|(name, _)| name != "type").collect(),
            _ => Map::new(),
        };
        Event { seq, thread: Some(thread.to_string()), turn: turn.map(str::to_string), kind: self.kind().to_string(), data }
    }

    /// The thread event a wire event is, or why it is none: a kind outside the thirteen, a missing field, a field of the wrong kind, no thread.
    pub fn from_event(event: &Event) -> Result<ThreadEvent, ParseError> {
        if event.thread.is_none() {
            return Err(ParseError("a thread event needs a thread".into()));
        }
        if !KINDS.contains(&event.kind.as_str()) {
            return Err(ParseError(format!("{} is not a thread event", event.kind)));
        }
        let mut fields = Map::new();
        fields.insert("type".into(), Value::String(event.kind.clone()));
        fields.extend(event.data.clone());
        serde_json::from_value(Value::Object(fields)).map_err(|e| ParseError(format!("{}: {e}", event.kind)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn one_of_each() -> Vec<ThreadEvent> {
        vec![
            ThreadEvent::TurnStarted {},
            ThreadEvent::MessageDelta { role: Role::Assistant, text: "Hi".into() },
            ThreadEvent::ReasoningDelta { text: "think".into() },
            ThreadEvent::ToolStarted { call: "c1".into(), tool: "read_file".into(), args: json!({"path": "a.txt"}) },
            ThreadEvent::ToolOutput { call: "c1".into(), stream: OutputStream::Stdout, data: "x".into() },
            ThreadEvent::ToolFinished { call: "c1".into(), ok: true, result: json!({"content": "x"}) },
            ThreadEvent::FileChanged { path: "a.txt".into(), op: FileOp::Edit },
            ThreadEvent::ApprovalRequested { approval: "ap1".into(), tool: "run_command".into(), args: json!({"command": "ls"}) },
            ThreadEvent::InputRequested { request: "rq1".into(), prompt: "Which?".into(), options: vec!["a".into(), "b".into()] },
            ThreadEvent::PlanUpdated { plan: "1. look".into() },
            ThreadEvent::Usage(Usage { input_tokens: 10, output_tokens: 4, cache_read_tokens: None }),
            ThreadEvent::Error { code: "RATE_LIMITED".into(), message: "slow down".into() },
            ThreadEvent::TurnFinished { reason: FinishReason::Failed },
        ]
    }

    #[test]
    fn given_one_event_of_every_kind_when_put_on_the_envelope_and_read_back_then_each_is_the_same_and_its_kind_is_the_owners() {
        // Given the thirteen events
        let events = one_of_each();
        assert_eq!(events.iter().map(ThreadEvent::kind).collect::<Vec<_>>(), KINDS);
        for (at, event) in events.iter().enumerate() {
            // When each is put on the envelope of thread t1, turn u1, seq at+1, written as a frame, parsed and read as a thread event
            let wire = event.into_event("t1", Some("u1"), at as u64 + 1);
            let frame = wire.encode();
            let back = match agent_api_proto::Incoming::parse(&frame) {
                Ok(agent_api_proto::Incoming::Event(parsed)) => ThreadEvent::from_event(&parsed),
                other => panic!("{frame}: {other:?}"),
            };
            // Then it is unchanged, the envelope carries the thread, turn and seq, and the topic is the thread's
            assert_eq!(back.as_ref(), Ok(event), "{frame}");
            assert_eq!((wire.thread.as_deref(), wire.turn.as_deref(), wire.seq, wire.topic()), (Some("t1"), Some("u1"), at as u64 + 1, Some("thread/t1".to_string())));
        }
    }

    #[test]
    fn given_an_event_when_written_then_its_fields_sit_beside_the_envelope_fields_and_a_field_that_is_none_is_left_out() {
        // Given a text delta and a usage with no cache count
        let delta = ThreadEvent::MessageDelta { role: Role::Assistant, text: "hi".into() }.into_event("t1", Some("u1"), 12);
        let usage = ThreadEvent::Usage(Usage { input_tokens: 1, output_tokens: 2, cache_read_tokens: None }).into_event("t1", None, 13);
        // When written, Then the frames are the owner's shape
        assert_eq!(delta.encode(), r#"{"event":{"seq":12,"thread":"t1","turn":"u1","type":"message.delta","role":"assistant","text":"hi"}}"#);
        assert_eq!(usage.encode(), r#"{"event":{"seq":13,"thread":"t1","type":"usage","input_tokens":1,"output_tokens":2}}"#);
    }

    #[test]
    fn given_events_that_are_no_thread_event_when_read_then_each_is_refused_saying_why() {
        let mut data = Map::new();
        data.insert("text".into(), json!("x"));
        let base = Event { seq: 1, thread: Some("t".into()), turn: None, kind: "message.delta".into(), data };
        // Given a kind outside the thirteen, an event with no thread, a delta missing its role, and a delta whose role is not one
        let unknown = Event { kind: "item.completed".into(), ..base.clone() };
        let no_thread = Event { thread: None, ..base.clone() };
        let no_role = base.clone();
        let mut bad = base;
        bad.data.insert("role".into(), json!("robot"));
        // When read, Then each is an error naming what is wrong
        assert_eq!(ThreadEvent::from_event(&unknown).unwrap_err().0, "item.completed is not a thread event");
        assert_eq!(ThreadEvent::from_event(&no_thread).unwrap_err().0, "a thread event needs a thread");
        assert!(ThreadEvent::from_event(&no_role).unwrap_err().0.contains("missing field `role`"));
        assert!(ThreadEvent::from_event(&bad).unwrap_err().0.contains("unknown variant `robot`"));
    }

    #[test]
    fn given_an_event_with_a_field_this_version_does_not_know_when_read_then_it_is_read_and_the_field_ignored() {
        // Given a finished turn whose frame carries a field added after this version
        let mut data = Map::new();
        data.insert("reason".into(), json!("completed"));
        data.insert("cost_usd".into(), json!(0.01));
        let event = Event { seq: 9, thread: Some("t".into()), turn: Some("u".into()), kind: "turn.finished".into(), data };
        // When read, Then it is the finished turn
        assert_eq!(ThreadEvent::from_event(&event), Ok(ThreadEvent::TurnFinished { reason: FinishReason::Completed }));
    }
}
