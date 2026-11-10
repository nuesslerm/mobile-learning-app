//! The hosted source control of the agent API: pull and merge requests, runs, clones and the provider sign-in, done by the executor that owns a
//! workspace with the credentials its machine already holds (`exec-hosted`). The service holds no token. Three sub-groups:
//!
//! - `hosted.pr`: `pr.*` and the same methods as `mr.*` (the neutral spelling for GitLab's merge requests; they differ only in the provider a
//!   request that names none gets: `github` for `pr.*`, `gitlab` for `mr.*`), `pr.runs` and `pr.run`, and the repository's label and reviewer candidates.
//! - `hosted.clone`: `clone.start` clones into a directory of the workspace and stays observable (`clone.status`, `clone.subscribe`) until it
//!   ends or `clone.cancel` stops it; retrying is starting again.
//! - `hosted.auth`: `sourceControl.*`: what the machine is signed in to, repository lookup and publish, and the CLI's web sign-in.
//!
//! Every method takes `workspace` and forwards the params the executor action it names lists (snake_case, times in milliseconds); results and
//! refusals are the executor's own, so a refusal's code is the executor's and its message says which of `NOT_INSTALLED` and `NOT_SIGNED_IN`
//! applies (`code_of`).
use agent_api_proto::{text, Event, GroupName, Method, ParamError};
use serde_json::{json, Map, Value};

#[cfg(feature = "testing")]
pub mod testing;

pub mod auth;
pub mod clone;
pub mod pr;

pub const GROUP_PR: GroupName = GroupName::new("hosted.pr");
pub const GROUP_CLONE: GroupName = GroupName::new("hosted.clone");
pub const GROUP_AUTH: GroupName = GroupName::new("hosted.auth");

/// The codes of this family's refusals that are the service's own.
pub mod code {
    /// The executor's machine has no `gh` or `glab`.
    pub const CLI_NOT_INSTALLED: &str = "CLI_NOT_INSTALLED";
    /// The CLI is installed but not signed in to the provider's host.
    pub const PROVIDER_NOT_SIGNED_IN: &str = "PROVIDER_NOT_SIGNED_IN";
    /// No clone has this id.
    pub const CLONE_NOT_FOUND: &str = "CLONE_NOT_FOUND";
}

/// What answers one request method: the executor actions it makes in order (none when the service answers on its own), and the provider a
/// request without one gets.
#[derive(Debug, Clone, Copy)]
pub struct Spec {
    pub method: Method,
    pub actions: &'static [&'static str],
    pub default_provider: Option<&'static str>,
}

pub const fn spec(method: Method, actions: &'static [&'static str], default_provider: Option<&'static str>) -> Spec {
    Spec { method, actions, default_provider }
}

/// `workspace` of every method of the family.
pub fn workspace_param(params: &Value) -> Result<String, ParamError> {
    text(params, "workspace").map(str::to_string).ok_or_else(|| ParamError("workspace is required".into()))
}

/// The names of the properties of an executor action (they are the params a method forwards to it).
pub fn properties_of(action: &str) -> Vec<&'static str> {
    exec_hosted::ACTIONS.iter().find(|a| a.name == action).map_or_else(Vec::new, |a| a.properties.iter().map(|p| p.name).collect())
}

/// The executor action's args for a request: the params the action lists that the request has (a null is as good as absent), and the
/// provider the method defaults to when the request names none.
pub fn executor_args(spec: &Spec, action: &str, params: &Value) -> Value {
    let mut args = Map::new();
    for name in properties_of(action) {
        match params.get(name) {
            None | Some(Value::Null) => {}
            Some(v) => {
                args.insert(name.to_string(), v.clone());
            }
        }
    }
    if let (false, Some(default)) = (args.contains_key("provider"), spec.default_provider) {
        args.insert("provider".into(), json!(default));
    }
    Value::Object(args)
}

/// The code a caller can act on in an executor's refusal: the CLI is missing, or it is not signed in; none for any other refusal.
pub fn code_of(message: &str) -> Option<&'static str> {
    if message.contains(exec_hosted::NOT_INSTALLED) {
        Some(code::CLI_NOT_INSTALLED)
    } else if message.starts_with(exec_hosted::NOT_SIGNED_IN) {
        Some(code::PROVIDER_NOT_SIGNED_IN)
    } else {
        None
    }
}

/// An event of a clone's stream.
pub mod kind {
    pub const CLONE_STATE: &str = "clone.state";
}

pub fn clone_event(seq: u64, workspace: &str, clone: &str, state: &str, repo: &str, detail: Value) -> Event {
    let mut data = Map::new();
    data.insert("workspace".into(), json!(workspace));
    data.insert("clone".into(), json!(clone));
    data.insert("state".into(), json!(state));
    data.insert("repo".into(), json!(repo));
    data.insert("detail".into(), detail);
    Event { seq, thread: None, turn: None, kind: kind::CLONE_STATE.to_string(), data }
}

/// Every method of the three sub-groups.
pub fn all_methods() -> Vec<Method> {
    pr::METHODS.iter().chain(clone::METHODS).chain(auth::METHODS).copied().collect()
}

/// Every spec of the family.
pub fn all_specs() -> Vec<Spec> {
    pr::SPECS.iter().chain(clone::SPECS).chain(auth::SPECS).copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_the_specs_when_compared_with_the_methods_and_the_executor_contract_then_every_method_has_one_and_every_action_is_a_hosted_one() {
        let specs = all_specs();
        // Then the specs are the methods in order, no method is named twice, and every action a spec names is an action of the executor contract
        assert_eq!(specs.iter().map(|s| s.method).collect::<Vec<_>>(), all_methods());
        let mut names: Vec<&str> = all_methods().iter().map(|m| m.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), all_methods().len());
        for s in &specs {
            for action in s.actions {
                assert!(exec_hosted::ACTIONS.iter().any(|a| a.name == *action), "{} names {action}, which is no action", s.method.name());
            }
        }
        // And every action of the executor contract is reached by some method
        for action in exec_hosted::ACTIONS {
            assert!(specs.iter().any(|s| s.actions.contains(&action.name)), "{} is reached by no method", action.name);
        }
    }

    #[test]
    fn given_the_two_spellings_when_compared_then_each_pr_method_has_an_mr_twin_for_the_same_action_with_the_other_default_provider() {
        for (pr, mr) in pr::SPECS[..pr::SPECS.len() / 2].iter().zip(&pr::SPECS[pr::SPECS.len() / 2..]) {
            assert_eq!(pr.method.name().replacen("pr.", "mr.", 1), mr.method.name());
            assert_eq!(pr.actions, mr.actions, "{}", pr.method.name());
            assert_eq!((pr.default_provider, mr.default_provider), (Some("github"), Some("gitlab")));
        }
    }

    #[test]
    fn given_params_with_extras_and_nulls_when_turned_into_args_then_only_the_actions_properties_that_are_set_remain_and_the_default_provider_fills_in() {
        let list = pr::SPECS.iter().find(|s| s.method == pr::LIST).unwrap();
        let mr_list = pr::SPECS.iter().find(|s| s.method == pr::MR_LIST).unwrap();
        let params = json!({"workspace": "w", "repo": "octo/hello", "state": null, "limit": 5, "token": "x", "argv": ["--web"]});
        assert_eq!(executor_args(list, exec_hosted::PR_LIST, &params), json!({"repo": "octo/hello", "limit": 5, "provider": "github"}));
        assert_eq!(executor_args(mr_list, exec_hosted::PR_LIST, &params)["provider"], "gitlab");
        assert_eq!(executor_args(list, exec_hosted::PR_LIST, &json!({"provider": "gitlab"}))["provider"], "gitlab");
    }

    #[test]
    fn given_the_executors_refusal_texts_when_classified_then_a_missing_cli_and_a_missing_sign_in_have_their_codes() {
        assert_eq!(code_of("gh is not installed on the executor machine (not found on PATH)"), Some(code::CLI_NOT_INSTALLED));
        assert_eq!(code_of("not signed in to github.com: gh says run gh auth login"), Some(code::PROVIDER_NOT_SIGNED_IN));
        assert_eq!(code_of("gh exited 1: boom"), None);
    }

    #[test]
    fn given_a_clone_event_when_built_then_it_carries_its_state_and_the_workspace() {
        let e = clone_event(2, "w", "c1", "done", "octo/hello", json!({"path": "clones/hello"}));
        assert_eq!((e.seq, e.kind.as_str(), e.data["state"].clone(), e.data["detail"]["path"].clone()), (2, "clone.state", json!("done"), json!("clones/hello")));
    }
}

#[cfg(test)]
mod fixture_tests {
    use super::*;

    #[test]
    fn given_the_recorded_exchanges_when_read_then_each_names_a_method_of_the_contract_and_the_actions_it_makes_are_the_ones_its_spec_lists() {
        let specs = all_specs();
        let fixtures = agent_api_proto::testing::load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"));
        // Then every method but the clone stream and start has a recorded exchange
        for s in specs.iter().filter(|s| ![clone::START, clone::SUBSCRIBE].contains(&s.method)) {
            assert!(fixtures.iter().any(|f| f.method == s.method.name()), "{} has no fixture", s.method.name());
        }
        for fixture in &fixtures {
            let spec = specs.iter().find(|s| s.method.name() == fixture.method).unwrap_or_else(|| panic!("{} is no method", fixture.method));
            for exchange in &fixture.exchanges {
                assert_eq!(exchange.request["method"], fixture.method);
                // And the actions it makes are a prefix of the spec's
                assert!(spec.actions.starts_with(&exchange.actions.iter().map(String::as_str).collect::<Vec<_>>()), "{}: {:?}", fixture.method, exchange.actions);
            }
        }
    }
}
