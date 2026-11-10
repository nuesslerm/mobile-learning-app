//! The preview groups of the agent API (`docs/plans/rca-parity/plan.md`, section 2). `preview` is the workspace's dev server, run by the executor
//! that owns the workspace (`exec-preview`): each method forwards the params it lists to the action it names, so the service keeps no process and
//! no path. `sessions` is the tabs an app shows a preview in: the service keeps which tabs a thread has, their viewport, zoom, colour scheme and
//! what the app reported of their navigation, and the app draws the page itself (the service starts no browser). `watch` is the stream of that.
//! Neutral names: snake_case fields, times in milliseconds since the epoch.
use agent_api_proto::{text, GroupName, Method, ParamError};
use serde_json::{Map, Value};

pub mod sessions;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
pub mod watch;

pub const GROUP: GroupName = GroupName::new("preview");

pub const START: Method = Method::new(GROUP, "preview.start");
pub const STOP: Method = Method::new(GROUP, "preview.stop");
pub const SERVERS: Method = Method::new(GROUP, "preview.servers");

/// Every method of the group.
pub const METHODS: &[Method] = &[START, STOP, SERVERS];

/// What answers one method: the executor action, and the params forwarded to it (any other param is dropped).
#[derive(Debug, Clone, Copy)]
pub struct Spec {
    pub method: Method,
    pub action: &'static str,
    pub keys: &'static [&'static str],
}

pub const SPECS: [Spec; 3] = [
    Spec { method: START, action: exec_preview::PREVIEW_START, keys: &["command", "port", "cwd", "name", "env", "ready_timeout_ms"] },
    Spec { method: STOP, action: exec_preview::PREVIEW_STOP, keys: &["id"] },
    Spec { method: SERVERS, action: exec_preview::PREVIEW_LIST, keys: &[] },
];

pub fn spec_of(method: Method) -> Option<&'static Spec> {
    SPECS.iter().find(|s| s.method == method)
}

/// `workspace` of every method of the group.
pub fn workspace_param(params: &Value) -> Result<String, ParamError> {
    text(params, "workspace").map(str::to_string).ok_or_else(|| ParamError("workspace is required".into()))
}

/// The executor action's args for a request: the params `spec` forwards that the request has (a null is as good as absent).
pub fn executor_args(spec: &Spec, params: &Value) -> Value {
    let mut args = Map::new();
    for key in spec.keys {
        match params.get(key) {
            None | Some(Value::Null) => {}
            Some(v) => {
                args.insert((*key).to_string(), v.clone());
            }
        }
    }
    Value::Object(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn given_the_specs_when_compared_with_the_methods_then_each_method_has_one_in_order_and_forwards_exactly_its_actions_properties() {
        // Given the specs and the contract's methods
        assert_eq!(SPECS.iter().map(|s| s.method).collect::<Vec<_>>(), METHODS);
        for spec in &SPECS {
            // Then each names an action of the executor's contract, forwards only its properties and drops none of them
            let action = exec_preview::ACTIONS.iter().find(|a| a.name == spec.action).unwrap_or_else(|| panic!("{} is no action", spec.action));
            for key in spec.keys {
                assert!(action.properties.iter().any(|p| p.name == *key), "{} forwards {key}, which {} lacks", spec.method.name(), action.name);
            }
            for p in action.properties {
                assert!(spec.keys.contains(&p.name), "{} drops {} of {}", spec.method.name(), p.name, action.name);
            }
        }
    }

    #[test]
    fn given_params_with_extras_and_nulls_when_turned_into_args_then_only_the_set_forwarded_keys_remain() {
        let args = executor_args(spec_of(START).unwrap(), &json!({"workspace": "w", "command": "x", "port": 4000, "cwd": null, "worktree": "y", "other": 1}));
        assert_eq!(args, json!({"command": "x", "port": 4000}));
        assert_eq!(workspace_param(&json!({})).unwrap_err().0, "workspace is required");
    }
}
