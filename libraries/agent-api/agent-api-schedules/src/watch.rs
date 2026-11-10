//! The `schedules.watch` sub-group: `schedule.subscribe` follows the `schedules` stream, which records every change to a schedule and every run.
//! The stream is numbered from a count of its own; a caller resumes by the last `seq` it saw, and one older than the tail the service keeps (or
//! ahead of the stream, as after a restart) is answered with `reset` true: `tasks` replaces what it held. `tasks` is the schedules as they are,
//! whichever way the caller came.
use crate::{RunView, TaskView};
use agent_api_proto::{Event, GroupName, Method, ParamError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub const GROUP: GroupName = GroupName::new("schedules.watch");

pub const SUBSCRIBE: Method = Method::new(GROUP, "schedule.subscribe");

/// Every method of the group.
pub const METHODS: &[Method] = &[SUBSCRIBE];

/// The stream `schedule.subscribe` opens.
pub const TOPIC: &str = "schedules";

/// What an inbound webhook is posted to: this prefix, the schedule's id, `/` and its token. It is a route of the service, outside the device bearers.
pub const WEBHOOK_PREFIX: &str = "/hooks/v1/schedules/";

/// The `type` of each event of the stream.
pub mod kind {
    /// A schedule was saved, enabled, disabled, deleted or given a new token.
    pub const CHANGED: &str = "schedule.changed";
    /// A run began, ended or was skipped.
    pub const RUN: &str = "schedule.run";
    /// A request reached a webhook: whether it was let in.
    pub const WEBHOOK: &str = "schedule.webhook";
}

/// What happened to a schedule in `schedule.changed`.
pub mod change {
    pub const SAVED: &str = "saved";
    pub const ENABLED: &str = "enabled";
    pub const DISABLED: &str = "disabled";
    pub const DELETED: &str = "deleted";
    pub const TOKEN_ROTATED: &str = "token_rotated";
}

/// What became of a request to a webhook in `schedule.webhook`.
pub mod outcome {
    pub const ACCEPTED: &str = "accepted";
    /// The token was not the schedule's (or the schedule is no webhook, or does not exist).
    pub const REJECTED: &str = "rejected_token";
    pub const DISABLED: &str = "disabled";
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubscribeParams {
    pub after_seq: Option<u64>,
}

impl SubscribeParams {
    pub fn parse(params: &Value) -> Result<SubscribeParams, ParamError> {
        if let Some(extra) = params.as_object().and_then(|o| o.keys().find(|k| *k != "after_seq")) {
            return Err(ParamError(format!("unknown field `{extra}`")));
        }
        let after_seq = match params.get("after_seq") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_u64().ok_or_else(|| ParamError("after_seq must be a non-negative integer".into()))?),
        };
        Ok(SubscribeParams { after_seq })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubscribeResult {
    pub topic: String,
    pub tasks: Vec<TaskView>,
    pub seq: u64,
    pub reset: bool,
}

fn event(seq: u64, kind: &str, fields: [(&str, Value); 3]) -> Event {
    let mut data = Map::new();
    for (name, value) in fields {
        data.insert(name.into(), value);
    }
    Event { seq, thread: None, turn: None, kind: kind.to_string(), data }
}

pub fn changed_event(seq: u64, schedule: &str, what: &str, task: Option<&TaskView>) -> Event {
    event(seq, kind::CHANGED, [("schedule", json!(schedule)), ("change", json!(what)), ("task", serde_json::to_value(task).unwrap_or(Value::Null))])
}

pub fn run_event(seq: u64, run: &RunView) -> Event {
    event(seq, kind::RUN, [("schedule", json!(run.schedule)), ("run", serde_json::to_value(run).unwrap_or(Value::Null)), ("at_ms", json!(run.at_ms))])
}

pub fn webhook_event(seq: u64, schedule: &str, what: &str, at_ms: u64) -> Event {
    event(seq, kind::WEBHOOK, [("schedule", json!(schedule)), ("outcome", json!(what)), ("at_ms", json!(at_ms))])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_subscribe_params_when_read_then_after_seq_is_optional_and_numeric_and_nothing_else_is_accepted() {
        assert_eq!(SubscribeParams::parse(&json!({})).unwrap().after_seq, None);
        assert_eq!(SubscribeParams::parse(&json!({"after_seq": 9})).unwrap().after_seq, Some(9));
        assert_eq!(SubscribeParams::parse(&json!({"after_seq": "9"})).unwrap_err().0, "after_seq must be a non-negative integer");
        assert_eq!(SubscribeParams::parse(&json!({"scope": "x"})).unwrap_err().0, "unknown field `scope`");
        assert_eq!(METHODS, [SUBSCRIBE]);
    }

    #[test]
    fn given_the_three_kinds_of_event_when_built_then_the_frame_names_the_schedule_and_what_happened() {
        assert_eq!(webhook_event(3, "s1", outcome::REJECTED, 1500).to_value(), json!({"event": {"seq": 3, "type": "schedule.webhook", "schedule": "s1", "outcome": "rejected_token", "at_ms": 1500}}));
        assert_eq!(changed_event(4, "s1", change::DELETED, None).to_value(), json!({"event": {"seq": 4, "type": "schedule.changed", "schedule": "s1", "change": "deleted", "task": null}}));
        assert_eq!(WEBHOOK_PREFIX, format!("/hooks/v1/{TOPIC}/"));
    }
}
