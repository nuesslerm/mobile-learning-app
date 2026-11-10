//! The `state` group of the agent API: small per-device values a mapper keeps for its app (quick commands, UI state, settings), by scope
//! and key. `state.watch` is a separate sub-group (`docs/plans/rca-parity/plan.md`, section 2).
use agent_api_proto::{text, GroupName, Method, ParamError};
use serde_json::{json, Value};

#[cfg(feature = "testing")]
pub mod testing;
pub mod watch;

pub const GROUP: GroupName = GroupName::new("state");

pub const GET: Method = Method::new(GROUP, "state.get");
pub const SET: Method = Method::new(GROUP, "state.set");

/// Every method of the group.
pub const METHODS: &[Method] = &[GET, SET];

/// The code of a `state.set` the store could not keep; the message is the store's reason.
pub const STATE_UNAVAILABLE: &str = "STATE_UNAVAILABLE";

/// Where a value lives: the device's scope and the key in it.
#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub scope: String,
    pub key: String,
}

impl Key {
    pub fn parse(params: &Value) -> Result<Key, ParamError> {
        match (text(params, "scope"), text(params, "key")) {
            (Some(scope), Some(key)) => Ok(Key { scope: scope.to_string(), key: key.to_string() }),
            _ => Err(ParamError("scope and key are required".into())),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SetArgs {
    pub key: Key,
    pub value: Value,
}

impl SetArgs {
    pub fn parse(params: &Value) -> Result<SetArgs, ParamError> {
        let key = Key::parse(params)?;
        let value = params.get("value").cloned().ok_or_else(|| ParamError("value is required".into()))?;
        Ok(SetArgs { key, value })
    }
}

/// The result of `state.get`: the value, or null when none is kept.
pub fn get_result(value: Option<Value>) -> Value {
    json!({ "value": value.unwrap_or(Value::Null) })
}

/// The result of a `state.set` that was kept.
pub fn set_result() -> Value {
    json!({})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_params_missing_a_part_when_read_then_each_is_refused_with_what_is_required() {
        // Given params with no key, with an empty scope, and a set with no value
        // Then the scope and key are required (a null value is still a value)
        assert_eq!(Key::parse(&json!({"scope": "a"})).unwrap_err().0, "scope and key are required");
        assert_eq!(Key::parse(&json!({"scope": "", "key": "k"})).unwrap_err().0, "scope and key are required");
        assert_eq!(SetArgs::parse(&json!({"scope": "a", "key": "k"})).unwrap_err().0, "value is required");
        assert_eq!(SetArgs::parse(&json!({"scope": "a", "key": "k", "value": null})).unwrap().value, Value::Null);
    }

    #[test]
    fn given_a_stored_value_and_none_when_the_get_result_is_built_then_none_is_null() {
        // Given a value and an absent one
        // Then the result carries the value, or null
        assert_eq!(get_result(Some(json!(["ls"]))), json!({"value": ["ls"]}));
        assert_eq!(get_result(None), json!({"value": null}));
        assert_eq!(set_result(), json!({}));
        assert_eq!(METHODS, [GET, SET]);
    }
}
