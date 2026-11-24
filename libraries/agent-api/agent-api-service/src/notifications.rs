//! The `service.notifications` sub-group: the journal of what happened that a person who was not looking would want to hear about (a turn that
//! finished or failed, a turn that waits for an answer). The service numbers each entry from 1 within an epoch, a fresh one for each run of the
//! service, and keeps the last `KEEP`: `notification.since` answers what came after a `seq` (all of what is kept when the caller's epoch is not the
//! current one, because its `seq` counts something else), and `notification.subscribe` pushes each new entry as it is made. Neutral of any app:
//! snake_case fields, times in milliseconds since the epoch.
use agent_api_proto::{Event, GroupName, Method, ParamError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub const GROUP: GroupName = GroupName::new("service.notifications");

pub const SINCE: Method = Method::new(GROUP, "notification.since");
pub const SUBSCRIBE: Method = Method::new(GROUP, "notification.subscribe");

/// Every method of the group.
pub const METHODS: &[Method] = &[SINCE, SUBSCRIBE];

/// The stream `notification.subscribe` opens.
pub const TOPIC: &str = "notifications";

/// Entries kept.
pub const KEEP: usize = 256;

/// The `type` of the stream's event.
pub mod kind {
    pub const POSTED: &str = "notification.posted";
}

/// What happened, in `NotificationView::cause`.
pub mod cause {
    pub const TURN_COMPLETED: &str = "turn.completed";
    pub const TURN_FAILED: &str = "turn.failed";
    pub const APPROVAL_REQUESTED: &str = "approval.requested";
    pub const INPUT_REQUESTED: &str = "input.requested";

    pub const ALL: [&str; 4] = [TURN_COMPLETED, TURN_FAILED, APPROVAL_REQUESTED, INPUT_REQUESTED];
}

/// One entry of the journal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotificationView {
    pub seq: u64,
    /// Unique across epochs.
    pub id: String,
    pub cause: String,
    pub title: String,
    pub body: String,
    /// Named `thread_id` and not `thread`, so the stream's event does not read it as the thread its envelope belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    pub at_ms: u64,
}

/// The params of `notification.since`: the last `seq` the caller saw (0 for none) and the epoch it saw it in.
#[derive(Debug, Clone, PartialEq)]
pub struct SinceParams {
    pub after_seq: u64,
    pub epoch: Option<String>,
}

fn epoch_param(params: &Value) -> Result<Option<String>, ParamError> {
    match params.get("epoch") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if !s.is_empty() && s.len() <= 128 => Ok(Some(s.clone())),
        Some(_) => Err(ParamError("epoch must be a string of 1 to 128 characters".into())),
    }
}

fn after_seq_param(params: &Value) -> Result<Option<u64>, ParamError> {
    match params.get("after_seq") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().map(Some).ok_or_else(|| ParamError("after_seq must be a non-negative integer".into())),
    }
}

fn only(params: &Value, known: &[&str]) -> Result<(), ParamError> {
    match params.as_object().and_then(|o| o.keys().find(|k| !known.contains(&k.as_str()))) {
        Some(extra) => Err(ParamError(format!("unknown field `{extra}`"))),
        None => Ok(()),
    }
}

impl SinceParams {
    pub fn parse(params: &Value) -> Result<SinceParams, ParamError> {
        only(params, &["after_seq", "epoch"])?;
        Ok(SinceParams { after_seq: after_seq_param(params)?.unwrap_or(0), epoch: epoch_param(params)? })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SinceResult {
    pub epoch: String,
    pub notifications: Vec<NotificationView>,
}

/// The params of `notification.subscribe`: with `after_seq` (and the `epoch` it was seen in) the stream starts from there, without it from now.
#[derive(Debug, Clone, PartialEq)]
pub struct SubscribeParams {
    pub after_seq: Option<u64>,
    pub epoch: Option<String>,
}

impl SubscribeParams {
    pub fn parse(params: &Value) -> Result<SubscribeParams, ParamError> {
        only(params, &["after_seq", "epoch"])?;
        Ok(SubscribeParams { after_seq: after_seq_param(params)?, epoch: epoch_param(params)? })
    }
}

/// The answer to `notification.subscribe`: the epoch the `seq`s count in, the `seq` the stream stands at, and whether `after_seq` could not be
/// resumed (another epoch, or older than the entries kept) so the stream starts from now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubscribeResult {
    pub topic: String,
    pub epoch: String,
    pub seq: u64,
    pub reset: bool,
}

/// The stream's event for an entry; its `seq` is the entry's.
pub fn posted_event(n: &NotificationView) -> Event {
    let mut data = Map::new();
    for (name, value) in [("id", json!(n.id)), ("cause", json!(n.cause)), ("title", json!(n.title)), ("body", json!(n.body)), ("at_ms", json!(n.at_ms))] {
        data.insert(name.into(), value);
    }
    for (name, value) in [("thread_id", &n.thread_id), ("workspace", &n.workspace)] {
        if let Some(value) = value {
            data.insert(name.into(), json!(value));
        }
    }
    Event { seq: n.seq, thread: None, turn: None, kind: kind::POSTED.to_string(), data }
}

/// The entry an event of the stream carries.
pub fn from_event(event: &Event) -> Option<NotificationView> {
    if event.kind != kind::POSTED {
        return None;
    }
    let mut fields = event.data.clone();
    fields.insert("seq".into(), json!(event.seq));
    serde_json::from_value(Value::Object(fields)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> NotificationView {
        NotificationView { seq: 4, id: "n-1-4".into(), cause: cause::TURN_COMPLETED.into(), title: "Finished".into(), body: "t1".into(), thread_id: Some("t1".into()), workspace: Some("w1".into()), at_ms: 9 }
    }

    #[test]
    fn given_the_params_of_since_and_subscribe_when_read_then_the_seq_defaults_for_since_and_is_optional_for_subscribe_and_nothing_else_is_accepted() {
        assert_eq!(SinceParams::parse(&json!({})).unwrap(), SinceParams { after_seq: 0, epoch: None });
        assert_eq!(SinceParams::parse(&json!({"after_seq": 3, "epoch": "e"})).unwrap(), SinceParams { after_seq: 3, epoch: Some("e".into()) });
        assert_eq!(SinceParams::parse(&json!({"after_seq": -1})).unwrap_err().0, "after_seq must be a non-negative integer");
        assert_eq!(SinceParams::parse(&json!({"epoch": ""})).unwrap_err().0, "epoch must be a string of 1 to 128 characters");
        assert_eq!(SinceParams::parse(&json!({"scope": 1})).unwrap_err().0, "unknown field `scope`");
        assert_eq!(SubscribeParams::parse(&json!({})).unwrap(), SubscribeParams { after_seq: None, epoch: None });
        assert_eq!(SubscribeParams::parse(&json!({"after_seq": 0})).unwrap().after_seq, Some(0));
        assert_eq!(METHODS, [SINCE, SUBSCRIBE]);
    }

    #[test]
    fn given_an_entry_when_it_becomes_an_event_and_back_then_the_seq_is_the_entrys_and_the_thread_is_data_not_the_envelope() {
        let event = posted_event(&entry());
        assert_eq!((event.seq, event.thread.clone(), event.topic()), (4, None, None));
        assert_eq!(event.to_value(), json!({"event": {"seq": 4, "type": "notification.posted", "id": "n-1-4", "cause": "turn.completed", "title": "Finished", "body": "t1", "at_ms": 9, "thread_id": "t1", "workspace": "w1"}}));
        assert_eq!(from_event(&event), Some(entry()));
        let other = Event { kind: "schedule.run".into(), ..event };
        assert_eq!(from_event(&other), None);
    }
}
