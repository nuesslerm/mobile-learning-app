//! The `git` group of the agent API: the repository of a workspace, read and changed by the executor that owns the workspace (`exec-git`'s
//! repository actions). Every method takes `workspace` and forwards the params it lists to the action it names, so the service holds no
//! repository and no path; results and refusals are the executor's own. `git.subscribe` streams the status: the answer says where the numbering
//! of events stands, and each `git.status` event carries the whole status (a caller that lost the stream subscribes again and is sent the
//! current status first).
use agent_api_proto::{text, Event, GroupName, Method, ParamError};
use serde_json::{json, Map, Value};

pub mod ai;
#[cfg(feature = "testing")]
pub mod testing;

pub const GROUP: GroupName = GroupName::new("git");

pub const STATUS: Method = Method::new(GROUP, "git.status");
pub const REFS: Method = Method::new(GROUP, "git.refs");
pub const HISTORY: Method = Method::new(GROUP, "git.history");
pub const DIFF: Method = Method::new(GROUP, "git.diff");
pub const COMPARE: Method = Method::new(GROUP, "git.compare");
pub const STAGE: Method = Method::new(GROUP, "git.stage");
pub const UNSTAGE: Method = Method::new(GROUP, "git.unstage");
pub const DISCARD: Method = Method::new(GROUP, "git.discard");
pub const COMMIT: Method = Method::new(GROUP, "git.commit");
pub const CHECKOUT: Method = Method::new(GROUP, "git.checkout");
pub const CREATE_REF: Method = Method::new(GROUP, "git.createRef");
pub const FETCH: Method = Method::new(GROUP, "git.fetch");
pub const PULL: Method = Method::new(GROUP, "git.pull");
pub const PUSH: Method = Method::new(GROUP, "git.push");
pub const FAST_FORWARD: Method = Method::new(GROUP, "git.fastForward");
pub const UPSTREAM_STATUS: Method = Method::new(GROUP, "git.upstreamStatus");
pub const MERGE: Method = Method::new(GROUP, "git.merge");
pub const REBASE: Method = Method::new(GROUP, "git.rebase");
pub const CONFLICT_RESOLVE: Method = Method::new(GROUP, "git.conflictResolve");
pub const BLOB: Method = Method::new(GROUP, "git.blob");
/// A repository made in a directory below the root, optionally with first files committed (the one request that is not about the workspace's own repository).
pub const INIT: Method = Method::new(GROUP, "git.init");
pub const SUBSCRIBE: Method = Method::new(GROUP, "git.subscribe");

pub const METHODS: &[Method] = &[STATUS, REFS, HISTORY, DIFF, COMPARE, STAGE, UNSTAGE, DISCARD, COMMIT, CHECKOUT, CREATE_REF, FETCH, PULL, PUSH, FAST_FORWARD, UPSTREAM_STATUS, MERGE, REBASE, CONFLICT_RESOLVE, BLOB, INIT, SUBSCRIBE];

/// The kind of the event of `git.subscribe`.
pub mod kind {
    pub const STATUS: &str = "git.status";
}

/// What answers one request method: the executor action, and the params forwarded to it (any other param is dropped).
#[derive(Debug, Clone, Copy)]
pub struct Spec {
    pub method: Method,
    pub action: &'static str,
    pub keys: &'static [&'static str],
}

const fn spec(method: Method, action: &'static str, keys: &'static [&'static str]) -> Spec {
    Spec { method, action, keys }
}

/// Every method but `git.subscribe`, in the contract's order.
pub const SPECS: [Spec; 21] = [
    spec(STATUS, exec_git::GIT_STATUS, &[]),
    spec(REFS, exec_git::GIT_REFS, &["kind", "query", "limit"]),
    spec(HISTORY, exec_git::GIT_LOG, &["rev", "path", "limit", "skip"]),
    spec(DIFF, exec_git::GIT_DIFF, &["path", "base", "head", "staged"]),
    spec(COMPARE, exec_git::GIT_COMPARE, &["base", "head", "limit"]),
    spec(STAGE, exec_git::GIT_STAGE, &["paths"]),
    spec(UNSTAGE, exec_git::GIT_UNSTAGE, &["paths"]),
    spec(DISCARD, exec_git::GIT_DISCARD, &["paths", "scope", "untracked", "source"]),
    spec(COMMIT, exec_git::GIT_COMMIT, &["message", "amend", "all", "allow_empty"]),
    spec(CHECKOUT, exec_git::GIT_CHECKOUT, &["ref", "detach", "track", "force"]),
    spec(CREATE_REF, exec_git::GIT_CREATE_REF, &["name", "kind", "from", "checkout", "message"]),
    spec(FETCH, exec_git::GIT_FETCH, &["remote", "prune", "refspecs"]),
    spec(PULL, exec_git::GIT_PULL, &["remote", "branch", "strategy"]),
    spec(PUSH, exec_git::GIT_PUSH, &["remote", "branch", "set_upstream", "force_with_lease"]),
    spec(FAST_FORWARD, exec_git::GIT_FAST_FORWARD, &["branch", "to"]),
    spec(UPSTREAM_STATUS, exec_git::GIT_UPSTREAM, &[]),
    spec(MERGE, exec_git::GIT_MERGE, &["op", "ref", "strategy", "message"]),
    spec(REBASE, exec_git::GIT_REBASE, &["op", "ref"]),
    spec(CONFLICT_RESOLVE, exec_git::GIT_CONFLICT_RESOLVE, &["path", "resolution", "content"]),
    spec(BLOB, exec_git::GIT_BLOB, &["rev", "path", "max_bytes"]),
    spec(INIT, exec_git::GIT_INIT, &["path", "initial_branch", "files", "message"]),
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

#[derive(Debug, Clone, PartialEq)]
pub struct SubscribeParams {
    pub workspace: String,
    /// The number the caller's last event had: this stream numbers its events from there.
    pub after_seq: u64,
    /// How often the executor looks at the repository (its own default and limits when absent).
    pub interval_ms: Option<u64>,
}

impl SubscribeParams {
    pub fn parse(params: &Value) -> Result<SubscribeParams, ParamError> {
        let uint = |key: &str| match params.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v.as_u64().map(Some).ok_or_else(|| ParamError(format!("{key} must be a non-negative integer"))),
        };
        Ok(SubscribeParams { workspace: workspace_param(params)?, after_seq: uint("after_seq")?.unwrap_or(0), interval_ms: uint("interval_ms")? })
    }
}

/// An event of the status stream: the executor's `git.status` event body, numbered `seq`.
pub fn status_event(seq: u64, workspace: &str, status: &Value) -> Event {
    let mut data = Map::new();
    data.insert("workspace".into(), json!(workspace));
    data.insert("status".into(), status.clone());
    Event { seq, thread: None, turn: None, kind: kind::STATUS.to_string(), data }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_the_specs_when_compared_with_the_methods_then_each_method_but_subscribe_has_one_in_order_and_the_actions_are_the_repository_ones() {
        // Given the specs and the contract's methods
        let methods: Vec<Method> = SPECS.iter().map(|s| s.method).collect();
        // Then they are the methods without the subscription, in order, and each names a distinct action of the executor's contract
        assert_eq!(methods, METHODS[..METHODS.len() - 1]);
        let mut actions: Vec<&str> = SPECS.iter().map(|s| s.action).collect();
        actions.sort_unstable();
        actions.dedup();
        assert_eq!(actions.len(), SPECS.len());
        for spec in &SPECS {
            let action = exec_git::ACTIONS.iter().find(|a| a.name == spec.action).unwrap_or_else(|| panic!("{} is no action", spec.action));
            // And every key it forwards is a property of the action, and every property it has but `worktree` is forwarded
            for key in spec.keys {
                assert!(action.properties.iter().any(|p| p.name == *key), "{} forwards {key}, which {} lacks", spec.method.name(), action.name);
            }
            for p in action.properties.iter().filter(|p| p.name != "worktree") {
                assert!(spec.keys.contains(&p.name), "{} drops {} of {}", spec.method.name(), p.name, action.name);
            }
        }
    }

    #[test]
    fn given_params_with_extras_and_nulls_when_turned_into_args_then_only_the_forwarded_keys_that_are_set_remain_and_a_worktree_is_never_forwarded() {
        let args = executor_args(spec_of(COMMIT).unwrap(), &json!({"workspace": "w", "message": "m", "amend": null, "all": true, "worktree": "x", "other": 1}));
        assert_eq!(args, json!({"message": "m", "all": true}));
    }

    #[test]
    fn given_subscribe_params_when_read_then_the_workspace_is_required_and_numbers_are_checked() {
        assert_eq!(SubscribeParams::parse(&json!({"workspace": "w"})).unwrap(), SubscribeParams { workspace: "w".into(), after_seq: 0, interval_ms: None });
        assert_eq!(SubscribeParams::parse(&json!({"workspace": "w", "after_seq": 4, "interval_ms": 200})).unwrap().after_seq, 4);
        assert_eq!(SubscribeParams::parse(&json!({})).unwrap_err().0, "workspace is required");
        assert_eq!(SubscribeParams::parse(&json!({"workspace": "w", "after_seq": -1})).unwrap_err().0, "after_seq must be a non-negative integer");
        let event = status_event(3, "w", &json!({"branch": "main"}));
        assert_eq!((event.seq, event.kind.as_str(), event.data["status"]["branch"].clone()), (3, "git.status", json!("main")));
    }
}
