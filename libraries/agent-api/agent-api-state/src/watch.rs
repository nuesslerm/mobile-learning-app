//! The `state.watch` sub-group: `state.subscribe` follows the values of one scope. The stream is numbered from 1 by a count of its own (one per
//! scope); a caller resumes by the last `seq` it saw, and one older than the tail the service keeps (or ahead of the stream, as after a restart)
//! is answered with the scope's values as they are and `reset` true.
use agent_api_proto::{text, Event, GroupName, Method, ParamError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub const GROUP: GroupName = GroupName::new("state.watch");

pub const SUBSCRIBE: Method = Method::new(GROUP, "state.subscribe");

/// Every method of the group.
pub const METHODS: &[Method] = &[SUBSCRIBE];

/// The `type` of the event that carries one value written to the scope.
pub const CHANGED: &str = "state.changed";

/// The params of `state.subscribe`: the scope to follow and the last `seq` the caller saw.
#[derive(Debug, Clone, PartialEq)]
pub struct SubscribeParams {
    pub scope: String,
    pub after_seq: Option<u64>,
}

impl SubscribeParams {
    pub fn parse(params: &Value) -> Result<SubscribeParams, ParamError> {
        let scope = text(params, "scope").ok_or_else(|| ParamError("scope is required".into()))?.to_string();
        let after_seq = match params.get("after_seq") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_u64().ok_or_else(|| ParamError("after_seq must be a non-negative integer".into()))?),
        };
        Ok(SubscribeParams { scope, after_seq })
    }
}

/// The answer to `state.subscribe`: the scope's values now and the `seq` they stand at; the events that follow are numbered after it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubscribeResult {
    pub scope: String,
    pub values: Map<String, Value>,
    pub seq: u64,
    /// True when `after_seq` was older than the events kept or ahead of the stream: `values` replaces what the caller held.
    pub reset: bool,
}

/// One value written to a scope, numbered `seq` in the scope's own count.
pub fn changed_event(seq: u64, scope: &str, key: &str, value: &Value) -> Event {
    let mut data = Map::new();
    for (name, v) in [("scope", json!(scope)), ("key", json!(key)), ("value", value.clone())] {
        data.insert(name.into(), v);
    }
    Event { seq, thread: None, turn: None, kind: CHANGED.to_string(), data }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_subscribe_params_when_read_then_the_scope_is_required_and_after_seq_is_optional_but_numeric() {
        // Given params with no scope, a bad after_seq, a null after_seq and a good one
        // Then each is read or refused with what is wrong
        assert_eq!(SubscribeParams::parse(&json!({})).unwrap_err().0, "scope is required");
        assert_eq!(SubscribeParams::parse(&json!({"scope": "a", "after_seq": "3"})).unwrap_err().0, "after_seq must be a non-negative integer");
        assert_eq!(SubscribeParams::parse(&json!({"scope": "a", "after_seq": null})).unwrap().after_seq, None);
        assert_eq!(SubscribeParams::parse(&json!({"scope": "a", "after_seq": 4})).unwrap(), SubscribeParams { scope: "a".into(), after_seq: Some(4) });
    }

    #[test]
    fn given_a_write_when_its_event_is_built_then_the_frame_carries_seq_type_scope_key_and_value() {
        // Given a value written to a scope
        let event = changed_event(7, "phone-a", "theme", &json!({"dark": true}));
        // Then the event frame names the stream position, the kind and the written value
        assert_eq!(event.to_value(), json!({"event": {"seq": 7, "type": "state.changed", "scope": "phone-a", "key": "theme", "value": {"dark": true}}}));
        assert_eq!(METHODS, [SUBSCRIBE]);
    }
}
