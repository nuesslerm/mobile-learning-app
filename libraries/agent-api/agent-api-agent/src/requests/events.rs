//! The events of `agent.requests`, on the `thread/<id>` topic beside the thirteen of `agent.threads` (which are unchanged). They close what
//! `approval.requested` and `input.requested` opened, and carry the turn the request belonged to on the envelope.
use super::params::Decision;
use crate::wire;
use agent_api_proto::{Event, ParseError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RequestEvent {
    #[serde(rename = "approval.answered")]
    ApprovalAnswered { approval: String, decision: Decision },
    #[serde(rename = "input.answered")]
    InputAnswered { request: String, answer: String },
    /// The question was closed with no answer.
    #[serde(rename = "input.dismissed")]
    InputDismissed { request: String },
}

pub const KINDS: [&str; 3] = ["approval.answered", "input.answered", "input.dismissed"];

impl RequestEvent {
    pub fn into_event(&self, thread: &str, turn: Option<&str>, seq: u64) -> Event {
        wire::encode(self, thread, turn, seq)
    }

    pub fn from_event(event: &Event) -> Result<RequestEvent, ParseError> {
        wire::decode(event, &KINDS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_one_event_of_every_kind_when_put_on_the_envelope_and_read_back_then_each_is_the_same_and_the_frame_is_the_owners_shape() {
        let events = [
            RequestEvent::ApprovalAnswered { approval: "ap1".into(), decision: Decision::AcceptForSession },
            RequestEvent::InputAnswered { request: "rq1".into(), answer: "the second".into() },
            RequestEvent::InputDismissed { request: "rq2".into() },
        ];
        for (at, event) in events.iter().enumerate() {
            // Given each on the envelope of thread t1, turn u1, When written as a frame and read back
            let wire = event.into_event("t1", Some("u1"), at as u64 + 1);
            let back = match agent_api_proto::Incoming::parse(&wire.encode()) {
                Ok(agent_api_proto::Incoming::Event(parsed)) => RequestEvent::from_event(&parsed),
                other => panic!("{other:?}"),
            };
            // Then it is the same, its kind is the group's and its topic the thread's
            assert_eq!(back.as_ref(), Ok(event));
            assert_eq!((wire.kind.as_str(), wire.topic()), (KINDS[at], Some("thread/t1".to_string())));
        }
        assert_eq!(events[0].into_event("t1", Some("u1"), 9).encode(), r#"{"event":{"seq":9,"thread":"t1","turn":"u1","type":"approval.answered","approval":"ap1","decision":"accept_for_session"}}"#);
    }

    #[test]
    fn given_an_event_of_another_group_or_one_missing_a_field_when_read_then_it_is_refused() {
        // Given a thread event, and an answer with no decision
        let thread_event = agent_api_proto::Event { seq: 1, thread: Some("t".into()), turn: None, kind: "turn.started".into(), data: Default::default() };
        let no_decision = agent_api_proto::Event { seq: 2, thread: Some("t".into()), turn: None, kind: "approval.answered".into(), data: serde_json::from_str(r#"{"approval":"a"}"#).unwrap() };
        // When read, Then each is refused naming what is wrong
        assert_eq!(RequestEvent::from_event(&thread_event).unwrap_err().0, "turn.started is not an event of this group");
        assert!(RequestEvent::from_event(&no_decision).unwrap_err().0.contains("missing field `decision`"));
    }
}
