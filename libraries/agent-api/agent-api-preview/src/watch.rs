//! The `preview.watch` group: `preview.subscribe` follows the `preview` stream, which records every change to a tab. The stream is numbered from
//! one in each run of the service (`epoch` names the run); a caller resumes by the epoch and last `seq` it saw, and one from another run, or older
//! than the tail the service keeps, is answered with `reset` true: it asks `preview.list` again. `revision` of an event is its `seq`.
use crate::sessions::{Clear, SessionView};
use agent_api_proto::{Event, GroupName, Method, ParamError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub const GROUP: GroupName = GroupName::new("preview.watch");

pub const SUBSCRIBE: Method = Method::new(GROUP, "preview.subscribe");

/// Every method of the group.
pub const METHODS: &[Method] = &[SUBSCRIBE];

/// The stream `preview.subscribe` opens.
pub const TOPIC: &str = "preview";

/// The `type` of each event of the stream.
pub mod kind {
    pub const OPENED: &str = "preview.opened";
    pub const NAVIGATED: &str = "preview.navigated";
    /// The viewport, zoom or colour scheme changed, or the app was asked to reload or clear.
    pub const RESIZED: &str = "preview.resized";
    pub const FAILED: &str = "preview.failed";
    pub const CLOSED: &str = "preview.closed";
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubscribeParams {
    pub epoch: Option<String>,
    pub after_seq: Option<u64>,
}

impl SubscribeParams {
    pub fn parse(params: &Value) -> Result<SubscribeParams, ParamError> {
        if let Some(extra) = params.as_object().and_then(|o| o.keys().find(|k| *k != "after_seq" && *k != "epoch")) {
            return Err(ParamError(format!("unknown field `{extra}`")));
        }
        let after_seq = match params.get("after_seq") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_u64().ok_or_else(|| ParamError("after_seq must be a non-negative integer".into()))?),
        };
        let epoch = match params.get("epoch") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_str().ok_or_else(|| ParamError("epoch must be a string".into()))?.to_string()),
        };
        Ok(SubscribeParams { epoch, after_seq })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubscribeResult {
    pub topic: String,
    pub epoch: String,
    pub seq: u64,
    pub reset: bool,
}

/// What the app is asked to do after an adjustment.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Request {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hard_reload: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clear: Option<Clear>,
}

impl Request {
    pub fn is_empty(&self) -> bool {
        self.hard_reload != Some(true) && self.clear.is_none()
    }
}

fn base(seq: u64, kind: &str, epoch: &str, thread: &str, tab: &str, at_ms: u64) -> (u64, String, Map<String, Value>) {
    let mut data = Map::new();
    for (name, value) in [("thread", json!(thread)), ("tab", json!(tab)), ("at_ms", json!(at_ms)), ("epoch", json!(epoch)), ("revision", json!(seq))] {
        data.insert(name.into(), value);
    }
    (seq, kind.to_string(), data)
}

fn event((seq, kind, data): (u64, String, Map<String, Value>)) -> Event {
    Event { seq, thread: None, turn: None, kind, data }
}

/// `opened`, `navigated` and `resized` carry the tab as it is after the change; `request` is what an adjustment asked of the app.
pub fn session_event(seq: u64, kind: &str, epoch: &str, session: &SessionView, request: Option<&Request>) -> Event {
    let (seq, kind, mut data) = base(seq, kind, epoch, &session.thread, &session.tab, session.updated_ms);
    data.insert("session".into(), serde_json::to_value(session).unwrap_or(Value::Null));
    if let Some(request) = request.filter(|r| !r.is_empty()) {
        data.insert("request".into(), serde_json::to_value(request).unwrap_or(Value::Null));
    }
    event((seq, kind, data))
}

/// A navigation that failed, with what the app reported of it.
pub fn failed_event(seq: u64, epoch: &str, session: &SessionView, url: &str, title: &str, code: i32, description: &str) -> Event {
    let (seq, kind, mut data) = base(seq, kind::FAILED, epoch, &session.thread, &session.tab, session.updated_ms);
    for (name, value) in [("url", json!(url)), ("title", json!(title)), ("code", json!(code)), ("description", json!(description))] {
        data.insert(name.into(), value);
    }
    event((seq, kind, data))
}

pub fn closed_event(seq: u64, epoch: &str, thread: &str, tab: &str, at_ms: u64) -> Event {
    event(base(seq, kind::CLOSED, epoch, thread, tab, at_ms))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::{Nav, Viewport};

    #[test]
    fn given_subscribe_params_when_read_then_epoch_and_after_seq_are_optional_and_typed_and_nothing_else_is_accepted() {
        assert_eq!(SubscribeParams::parse(&json!({})).unwrap(), SubscribeParams { epoch: None, after_seq: None });
        assert_eq!(SubscribeParams::parse(&json!({"epoch": "e", "after_seq": 9})).unwrap(), SubscribeParams { epoch: Some("e".into()), after_seq: Some(9) });
        assert_eq!(SubscribeParams::parse(&json!({"after_seq": "9"})).unwrap_err().0, "after_seq must be a non-negative integer");
        assert_eq!(SubscribeParams::parse(&json!({"epoch": 1})).unwrap_err().0, "epoch must be a string");
        assert_eq!(SubscribeParams::parse(&json!({"scope": "x"})).unwrap_err().0, "unknown field `scope`");
    }

    #[test]
    fn given_the_kinds_of_event_when_built_then_each_names_the_tab_the_epoch_and_the_revision_it_is() {
        let view = SessionView { thread: "t".into(), tab: "tab-1".into(), nav: Nav::Idle {}, can_go_back: false, can_go_forward: false, viewport: Viewport::Fill {}, color_scheme: None, zoom: None, profile: None, reveal: None, updated_ms: 5 };
        let opened = session_event(2, kind::OPENED, "e1", &view, None).to_value();
        assert_eq!((opened["event"]["type"].clone(), opened["event"]["revision"].clone(), opened["event"]["epoch"].clone(), opened["event"]["session"]["tab"].clone()), (json!("preview.opened"), json!(2), json!("e1"), json!("tab-1")));
        assert!(opened["event"].get("request").is_none());
        let reload = session_event(3, kind::RESIZED, "e1", &view, Some(&Request { hard_reload: Some(true), clear: None })).to_value();
        assert_eq!(reload["event"]["request"], json!({"hard_reload": true}));
        assert_eq!(closed_event(4, "e1", "t", "tab-1", 7).to_value(), json!({"event": {"seq": 4, "type": "preview.closed", "thread": "t", "tab": "tab-1", "at_ms": 7, "epoch": "e1", "revision": 4}}));
        assert_eq!(failed_event(5, "e1", &view, "http://a.test/", "", -105, "name not resolved").to_value()["event"]["code"], json!(-105));
    }
}
