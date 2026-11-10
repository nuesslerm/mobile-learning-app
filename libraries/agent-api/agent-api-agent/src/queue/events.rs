//! The events of `agent.queue`, on the `thread/<id>` topic beside the thirteen of `agent.threads` (which are unchanged).
use super::results::QueueItem;
use crate::wire;
use agent_api_proto::{Event, ParseError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum QueueEvent {
    /// A prompt joined the queue; `position` 1 is next.
    #[serde(rename = "queue.added")]
    Added { item: QueueItem, position: u32 },
    #[serde(rename = "queue.edited")]
    Edited { item: String, prompt: String },
    /// `order` is the queue's ids in the order they now start.
    #[serde(rename = "queue.moved")]
    Moved { order: Vec<String> },
    #[serde(rename = "queue.deleted")]
    Deleted { item: String },
    /// The queue was held, so nothing starts by itself; `reason` is `force_stop`.
    #[serde(rename = "queue.held")]
    Held { reason: String },
    /// A forced stop's hold was released; `item` is the one sent now, when one was.
    #[serde(rename = "queue.resumed")]
    Resumed {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        item: Option<String>,
    },
    /// The item left the queue to run as the turn of the same id (the envelope's `turn`).
    #[serde(rename = "queue.started")]
    Started { item: String },
    /// The running turn goes on with another prompt; `item` is the queued prompt it took, when it took one.
    #[serde(rename = "turn.steered")]
    Steered {
        prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        item: Option<String>,
    },
}

pub const KINDS: [&str; 8] = ["queue.added", "queue.edited", "queue.moved", "queue.deleted", "queue.held", "queue.resumed", "queue.started", "turn.steered"];

impl QueueEvent {
    pub fn into_event(&self, thread: &str, turn: Option<&str>, seq: u64) -> Event {
        wire::encode(self, thread, turn, seq)
    }

    pub fn from_event(event: &Event) -> Result<QueueEvent, ParseError> {
        wire::decode(event, &KINDS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_of_each() -> Vec<QueueEvent> {
        vec![
            QueueEvent::Added { item: QueueItem { item: "q1".into(), prompt: "next".into(), created_at_ms: 1_700_000_000_000, ..QueueItem::default() }, position: 1 },
            QueueEvent::Edited { item: "q1".into(), prompt: "better".into() },
            QueueEvent::Moved { order: vec!["q2".into(), "q1".into()] },
            QueueEvent::Deleted { item: "q2".into() },
            QueueEvent::Held { reason: "force_stop".into() },
            QueueEvent::Resumed { item: Some("q1".into()) },
            QueueEvent::Started { item: "q1".into() },
            QueueEvent::Steered { prompt: "go left".into(), item: None },
        ]
    }

    #[test]
    fn given_one_event_of_every_kind_when_put_on_the_envelope_and_read_back_then_each_is_the_same_and_its_kind_is_listed() {
        let events = one_of_each();
        assert_eq!(events.len(), KINDS.len());
        for (at, event) in events.iter().enumerate() {
            // Given each on the envelope of thread t1, When written as a frame and read back
            let wire = event.into_event("t1", Some("u1"), at as u64 + 1);
            let back = match agent_api_proto::Incoming::parse(&wire.encode()) {
                Ok(agent_api_proto::Incoming::Event(parsed)) => QueueEvent::from_event(&parsed),
                other => panic!("{other:?}"),
            };
            // Then it is the same, its kind is the nth of the group's, and its topic is the thread's
            assert_eq!(back.as_ref(), Ok(event));
            assert_eq!((wire.kind.as_str(), wire.topic()), (KINDS[at], Some("thread/t1".to_string())));
        }
    }

    #[test]
    fn given_a_resume_that_sent_no_item_and_a_steer_of_a_prompt_when_written_then_the_fields_that_are_none_are_left_out() {
        // Given a resume with no item and a steer with no item
        let resumed = QueueEvent::Resumed { item: None }.into_event("t1", None, 4);
        let steered = QueueEvent::Steered { prompt: "p".into(), item: None }.into_event("t1", Some("u1"), 5);
        // When written, Then the frames carry only what is set
        assert_eq!(resumed.encode(), r#"{"event":{"seq":4,"thread":"t1","type":"queue.resumed"}}"#);
        assert_eq!(steered.encode(), r#"{"event":{"seq":5,"thread":"t1","turn":"u1","type":"turn.steered","prompt":"p"}}"#);
    }
}
