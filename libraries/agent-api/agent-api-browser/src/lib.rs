//! The browser group of the agent API: each method forwards the params it lists to the executor action it names (`exec-browser`), so the service
//! keeps no browser and no page. A page belongs to a `key`: the caller's own `key`, else its `thread`, else its `workspace`, so two threads of one
//! workspace do not share a page unless they say so. The group answers `BROWSER_DISABLED` for every method unless the service was started with the
//! browser on (`RCA_BROWSER`), because Chromium needs memory the VM does not yet have to spare (`docs/gaps/vm-memory-saturation.md`).
//! Neutral names: snake_case fields.
use agent_api_proto::{text, GroupName, Method, ParamError};
use serde_json::{Map, Value};

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub const GROUP: GroupName = GroupName::new("browser");

pub const OPEN: Method = Method::new(GROUP, "browser.open");
pub const GOTO: Method = Method::new(GROUP, "browser.goto");
pub const HISTORY: Method = Method::new(GROUP, "browser.history");
pub const EVAL: Method = Method::new(GROUP, "browser.eval");
pub const SCREENSHOT: Method = Method::new(GROUP, "browser.screenshot");
pub const FRAME: Method = Method::new(GROUP, "browser.frame");
pub const INPUT: Method = Method::new(GROUP, "browser.input");
pub const CLOSE: Method = Method::new(GROUP, "browser.close");
pub const PAGES: Method = Method::new(GROUP, "browser.pages");

/// Every method of the group.
pub const METHODS: &[Method] = &[OPEN, GOTO, HISTORY, EVAL, SCREENSHOT, FRAME, INPUT, CLOSE, PAGES];

pub mod code {
    /// The service was started without the browser.
    pub const BROWSER_DISABLED: &str = "BROWSER_DISABLED";

    pub const ALL: [&str; 1] = [BROWSER_DISABLED];
}

pub mod message {
    pub const DISABLED: &str = "the browser is off on this service (set RCA_BROWSER=1 once the host has memory for Chromium)";
}

/// What answers one method: the executor action, and the params forwarded to it (any other param is dropped).
#[derive(Debug, Clone, Copy)]
pub struct Spec {
    pub method: Method,
    pub action: &'static str,
    pub keys: &'static [&'static str],
}

pub const SPECS: [Spec; 9] = [
    Spec { method: OPEN, action: exec_browser::BROWSER_OPEN, keys: &["key", "url"] },
    Spec { method: GOTO, action: exec_browser::BROWSER_GOTO, keys: &["key", "url"] },
    Spec { method: HISTORY, action: exec_browser::BROWSER_HISTORY, keys: &["key", "direction"] },
    Spec { method: EVAL, action: exec_browser::BROWSER_EVAL, keys: &["key", "expression"] },
    Spec { method: SCREENSHOT, action: exec_browser::BROWSER_SCREENSHOT, keys: &["key"] },
    Spec { method: FRAME, action: exec_browser::BROWSER_FRAME, keys: &["key", "width", "height", "quality", "mobile"] },
    Spec { method: INPUT, action: exec_browser::BROWSER_INPUT, keys: &["key", "kind", "x", "y", "button", "modifiers", "dx", "dy", "text"] },
    Spec { method: CLOSE, action: exec_browser::BROWSER_CLOSE, keys: &["key"] },
    Spec { method: PAGES, action: exec_browser::BROWSER_LIST, keys: &[] },
];

pub fn spec_of(method: Method) -> Option<&'static Spec> {
    SPECS.iter().find(|s| s.method == method)
}

/// `workspace` of every method of the group.
pub fn workspace_param(params: &Value) -> Result<String, ParamError> {
    text(params, "workspace").map(str::to_string).ok_or_else(|| ParamError("workspace is required".into()))
}

/// The page's key: `key`, else `thread`, else the workspace.
pub fn key_of(params: &Value, workspace: &str) -> String {
    text(params, "key").or_else(|| text(params, "thread")).unwrap_or(workspace).to_string()
}

/// The executor action's args for a request: the params `spec` forwards that the request has (a null is as good as absent), with the page's key.
pub fn executor_args(spec: &Spec, params: &Value, workspace: &str) -> Value {
    let mut args = Map::new();
    for key in spec.keys {
        match params.get(key) {
            None | Some(Value::Null) => {}
            Some(v) => {
                args.insert((*key).to_string(), v.clone());
            }
        }
    }
    if spec.keys.contains(&"key") {
        args.insert("key".to_string(), Value::String(key_of(params, workspace)));
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
            let action = exec_browser::ACTIONS.iter().find(|a| a.name == spec.action).unwrap_or_else(|| panic!("{} is no action", spec.action));
            for key in spec.keys {
                assert!(action.properties.iter().any(|p| p.name == *key), "{} forwards {key}, which {} lacks", spec.method.name(), action.name);
            }
            for p in action.properties {
                assert!(spec.keys.contains(&p.name), "{} drops {} of {}", spec.method.name(), p.name, action.name);
            }
        }
    }

    #[test]
    fn given_params_with_extras_and_nulls_when_turned_into_args_then_only_the_set_forwarded_keys_remain_and_the_key_falls_back_to_the_thread_then_the_workspace() {
        let spec = spec_of(GOTO).unwrap();
        assert_eq!(executor_args(spec, &json!({"workspace": "w", "url": "https://example.com", "x": 1, "thread": null}), "w"), json!({"url": "https://example.com", "key": "w"}));
        assert_eq!(executor_args(spec, &json!({"workspace": "w", "thread": "t1", "url": "u"}), "w"), json!({"url": "u", "key": "t1"}));
        assert_eq!(executor_args(spec, &json!({"workspace": "w", "thread": "t1", "key": "k", "url": "u"}), "w"), json!({"url": "u", "key": "k"}));
        assert_eq!(executor_args(spec_of(PAGES).unwrap(), &json!({"workspace": "w", "key": "k"}), "w"), json!({}));
        assert_eq!(workspace_param(&json!({})).unwrap_err().0, "workspace is required");
    }
}
