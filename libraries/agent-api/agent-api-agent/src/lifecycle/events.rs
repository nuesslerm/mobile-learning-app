//! The events of `agent.lifecycle`, on the `thread/<id>` topic beside the thirteen of `agent.threads` (which are unchanged). Each carries what a client
//! needs to hold the thread's state without asking; `at_ms` is when it happened.
use super::params::SettleState;
use super::results::Settle;
use crate::wire;
use agent_api_proto::{Event, ParseError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LifecycleEvent {
    #[serde(rename = "thread.archived")]
    Archived { at_ms: u64 },
    #[serde(rename = "thread.unarchived")]
    Unarchived { at_ms: u64 },
    #[serde(rename = "thread.deleted")]
    Deleted { at_ms: u64 },
    /// The settle and snooze state after the change; `state` is what was asked for.
    #[serde(rename = "thread.settle_changed")]
    SettleChanged { state: SettleState, settle: Settle },
    /// First event of a fork: the thread it was forked from and the finished turns copied into it (their events follow, with the turn ids they had).
    #[serde(rename = "thread.forked")]
    Forked { source: String, turns: Vec<String> },
    /// The turn (the envelope's) was ended and its process killed, and the queue is held.
    #[serde(rename = "thread.force_stopped")]
    ForceStopped {},
    /// A fork's context now waits for this thread's next prompt.
    #[serde(rename = "thread.merged_back")]
    MergedBack { source: String, context: String },
    /// The next prompt took the context of the fork `source`.
    #[serde(rename = "thread.merge_consumed")]
    MergeConsumed { source: String },
    /// The turns after `to_turn` went from the thread. (Not `turn`: the envelope has that field.)
    #[serde(rename = "thread.rolled_back")]
    RolledBack { to_turn: String, dropped: Vec<String>, restored_files: bool },
    #[serde(rename = "session.detached")]
    Detached {
        session: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// The running turn (the envelope's) was asked to stop what it runs in the background.
    #[serde(rename = "workflow.stopped")]
    WorkflowStopped {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workflow: Option<String>,
    },
    /// The envelope's turn runs `retry_of`'s prompt again.
    #[serde(rename = "turn.retried")]
    Retried { retry_of: String, prompt: String },
}

pub const KINDS: [&str; 12] = [
    "thread.archived",
    "thread.unarchived",
    "thread.deleted",
    "thread.settle_changed",
    "thread.forked",
    "thread.force_stopped",
    "thread.merged_back",
    "thread.merge_consumed",
    "thread.rolled_back",
    "session.detached",
    "workflow.stopped",
    "turn.retried",
];

impl LifecycleEvent {
    pub fn into_event(&self, thread: &str, turn: Option<&str>, seq: u64) -> Event {
        wire::encode(self, thread, turn, seq)
    }

    pub fn from_event(event: &Event) -> Result<LifecycleEvent, ParseError> {
        wire::decode(event, &KINDS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::results::Resting;

    fn one_of_each() -> Vec<LifecycleEvent> {
        vec![
            LifecycleEvent::Archived { at_ms: 1_700_000_000_000 },
            LifecycleEvent::Unarchived { at_ms: 1_700_000_001_000 },
            LifecycleEvent::Deleted { at_ms: 1_700_000_002_000 },
            LifecycleEvent::SettleChanged { state: SettleState::Settled, settle: Settle { state: Some(Resting::Settled), settled_at_ms: Some(1_700_000_003_000), ..Settle::default() } },
            LifecycleEvent::Forked { source: "t0".into(), turns: vec!["u1".into(), "u2".into()] },
            LifecycleEvent::ForceStopped {},
            LifecycleEvent::MergedBack { source: "t2".into(), context: "the fork found the bug".into() },
            LifecycleEvent::MergeConsumed { source: "t2".into() },
            LifecycleEvent::RolledBack { to_turn: "u1".into(), dropped: vec!["u2".into()], restored_files: false },
            LifecycleEvent::Detached { session: "s1".into(), reason: Some("stale".into()) },
            LifecycleEvent::WorkflowStopped { workflow: None },
            LifecycleEvent::Retried { retry_of: "u2".into(), prompt: "again".into() },
        ]
    }

    #[test]
    fn given_one_event_of_every_kind_when_put_on_the_envelope_and_read_back_then_each_is_the_same_and_its_kind_is_listed_in_order() {
        let events = one_of_each();
        assert_eq!(events.len(), KINDS.len());
        for (at, event) in events.iter().enumerate() {
            // Given each on the envelope of thread t1, When written as a frame and read back
            let wire = event.into_event("t1", Some("u1"), at as u64 + 1);
            let back = match agent_api_proto::Incoming::parse(&wire.encode()) {
                Ok(agent_api_proto::Incoming::Event(parsed)) => LifecycleEvent::from_event(&parsed),
                other => panic!("{other:?}"),
            };
            // Then it is the same, its kind is the nth of the group's, and its topic is the thread's
            assert_eq!(back.as_ref(), Ok(event));
            assert_eq!((wire.kind.as_str(), wire.topic()), (KINDS[at], Some("thread/t1".to_string())));
        }
    }

    #[test]
    fn given_a_settle_change_and_an_event_with_no_fields_when_written_then_the_frames_hold_only_what_is_set() {
        // Given a snooze and a force stop
        let snoozed = LifecycleEvent::SettleChanged { state: SettleState::Snoozed, settle: Settle { snoozed_until_ms: Some(9), snoozed_at_ms: Some(5), ..Settle::default() } };
        // When written, Then the settle carries only its set fields, and the force stop only the envelope
        assert_eq!(snoozed.into_event("t1", None, 3).encode(), r#"{"event":{"seq":3,"thread":"t1","type":"thread.settle_changed","state":"snoozed","settle":{"snoozed_until_ms":9,"snoozed_at_ms":5}}}"#);
        assert_eq!(LifecycleEvent::ForceStopped {}.into_event("t1", Some("u1"), 4).encode(), r#"{"event":{"seq":4,"thread":"t1","turn":"u1","type":"thread.force_stopped"}}"#);
    }
}
