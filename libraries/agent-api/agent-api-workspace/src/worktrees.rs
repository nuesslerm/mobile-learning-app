//! The `workspace.worktrees` group: a worktree is a workspace of its own, a checkout of the repository of another workspace under the
//! executor's worktree base. `worktree.create` adds one (and may start its setup commands), `worktree.setup.subscribe` follows the setup's
//! progress and `worktree.setup.cancel` stops it. `scratch.ensure`, `repo.refs` and `repo.sparse.*` of the owner set are not served yet.
use crate::{workspace_param, RemoveResult};
use agent_api_proto::{text, Event, GroupName, Method, ParamError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub const GROUP: GroupName = GroupName::new("workspace.worktrees");

pub const WORKTREE_CREATE: Method = Method::new(GROUP, "worktree.create");
pub const WORKTREE_REMOVE: Method = Method::new(GROUP, "worktree.remove");
pub const WORKTREE_LIST: Method = Method::new(GROUP, "worktree.list");
pub const SETUP_SUBSCRIBE: Method = Method::new(GROUP, "worktree.setup.subscribe");
pub const SETUP_CANCEL: Method = Method::new(GROUP, "worktree.setup.cancel");

pub const METHODS: &[Method] = &[WORKTREE_CREATE, WORKTREE_REMOVE, WORKTREE_LIST, SETUP_SUBSCRIBE, SETUP_CANCEL];

/// The type of every event of a setup stream is the executor's kind (`setup.started`, `step.output`, ...); the stream's events carry the
/// worktree's workspace name as `workspace` and a `seq` counting from 1 per worktree.
pub const EVENT_WORKSPACE: &str = "workspace";

/// The params of `worktree.create`: a worktree named `worktree` (its workspace name and its directory under the base) of the repository
/// `workspace` belongs to, on `branch` (created from `base` when given), whose `setup` commands run in order once it exists.
#[derive(Debug, Clone, PartialEq)]
pub struct CreateParams {
    pub workspace: String,
    pub worktree: String,
    pub branch: String,
    pub base: Option<String>,
    pub setup: Vec<String>,
}

impl CreateParams {
    pub fn parse(params: &Value) -> Result<CreateParams, ParamError> {
        let required = |key: &str| text(params, key).map(str::to_string).ok_or_else(|| ParamError(format!("{key} is required")));
        let (workspace, worktree, branch) = (required("workspace")?, required("worktree")?, required("branch")?);
        let base = match params.get("base") {
            None | Some(Value::Null) => None,
            Some(Value::String(base)) if !base.is_empty() => Some(base.clone()),
            Some(_) => return Err(ParamError("base must be a non-empty string".into())),
        };
        let setup = match params.get("setup") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(commands)) => commands.iter().map(|c| c.as_str().filter(|c| !c.trim().is_empty()).map(str::to_string)).collect::<Option<Vec<_>>>().ok_or_else(|| ParamError("setup must be an array of non-empty strings".into()))?,
            Some(_) => return Err(ParamError("setup must be an array of non-empty strings".into())),
        };
        Ok(CreateParams { workspace, worktree, branch, base, setup })
    }
}

/// The result of `worktree.create`: the new workspace, the repository workspace it came from, the executor that owns both, and where the
/// executor put it. `setup` is true when the setup commands were started: follow them with `worktree.setup.subscribe`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateResult {
    pub workspace: String,
    pub repo: String,
    pub executor: String,
    pub path: String,
    pub branch: String,
    pub head: String,
    pub setup: bool,
}

/// The params of `worktree.remove`: the worktree's workspace, and whether to remove it with uncommitted changes.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoveParams {
    pub workspace: String,
    pub force: bool,
}

impl RemoveParams {
    pub fn parse(params: &Value) -> Result<RemoveParams, ParamError> {
        Ok(RemoveParams { workspace: workspace_param(params)?, force: params.get("force").and_then(Value::as_bool).unwrap_or(false) })
    }
}

pub type RemovedResult = RemoveResult;

/// The params of `worktree.setup.subscribe`: replay the events after `after_seq`, then follow.
#[derive(Debug, Clone, PartialEq)]
pub struct SubscribeParams {
    pub workspace: String,
    pub after_seq: u64,
}

impl SubscribeParams {
    pub fn parse(params: &Value) -> Result<SubscribeParams, ParamError> {
        let after_seq = match params.get("after_seq") {
            None | Some(Value::Null) => 0,
            Some(v) => v.as_u64().ok_or_else(|| ParamError("after_seq must be a non-negative integer".into()))?,
        };
        Ok(SubscribeParams { workspace: workspace_param(params)?, after_seq })
    }
}

/// The result of `worktree.setup.subscribe`: where the replay ends and whether the setup was still running when it was asked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubscribeResult {
    pub workspace: String,
    pub after_seq: u64,
    pub last_seq: u64,
    pub running: bool,
}

/// The result of `worktree.setup.cancel`: false when the setup had finished already.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CancelResult {
    pub workspace: String,
    pub cancelled: bool,
}

/// The event of a setup stream for the executor's `event` (a subscribe frame's body) numbered `seq`: its `kind` is the type, the rest is data.
pub fn setup_event(workspace: &str, seq: u64, executor_event: &Value) -> Event {
    let mut data: Map<String, Value> = executor_event.as_object().cloned().unwrap_or_default();
    let kind = data.remove("kind").and_then(|k| k.as_str().map(str::to_string)).unwrap_or_else(|| "setup.event".into());
    data.insert(EVENT_WORKSPACE.into(), json!(workspace));
    Event { seq, thread: None, turn: None, kind, data }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_the_params_of_worktree_create_when_read_then_name_and_branch_are_required_and_setup_is_a_list_of_commands() {
        // Given a minimal request, a full one, and requests that miss a key or have the wrong kind
        let min = CreateParams::parse(&json!({"workspace": "w", "worktree": "t", "branch": "b"})).unwrap();
        let full = CreateParams::parse(&json!({"workspace": "w", "worktree": "t", "branch": "b", "base": "main", "setup": ["pnpm i", "pnpm build"]})).unwrap();
        // Then the minimal one has no base and no setup, and the full one keeps both in order
        assert_eq!((min.base, min.setup), (None, vec![]));
        assert_eq!((full.base.as_deref(), full.setup), (Some("main"), vec!["pnpm i".to_string(), "pnpm build".to_string()]));
        // And each missing key is named, and a base or setup of the wrong kind is refused
        for key in ["workspace", "worktree", "branch"] {
            let mut params = json!({"workspace": "w", "worktree": "t", "branch": "b"});
            params.as_object_mut().unwrap().remove(key);
            assert_eq!(CreateParams::parse(&params).unwrap_err().0, format!("{key} is required"));
        }
        let with = |extra: Value| {
            let mut params = json!({"workspace": "w", "worktree": "t", "branch": "b"});
            params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            CreateParams::parse(&params).unwrap_err().0
        };
        assert_eq!(with(json!({"base": 5})), "base must be a non-empty string");
        assert_eq!(with(json!({"setup": "ls"})), "setup must be an array of non-empty strings");
        assert_eq!(with(json!({"setup": ["ls", " "]})), "setup must be an array of non-empty strings");
    }

    #[test]
    fn given_the_executors_setup_event_when_mapped_then_its_kind_is_the_type_and_the_workspace_and_numbering_are_ours() {
        // Given a step output event of the executor
        let event = setup_event("wt", 3, &json!({"kind": "step.output", "step": 1, "stream": "stdout", "data": "hi\n"}));
        // When it is encoded
        // Then it is a stream event of no thread, whose type is the kind and whose data carries the workspace beside the executor's fields
        assert_eq!(event.to_value(), json!({"event": {"seq": 3, "type": "step.output", "step": 1, "stream": "stdout", "data": "hi\n", "workspace": "wt"}}));
        assert_eq!(event.topic(), None);
        // And an event with no kind still maps, and the subscribe params default to the start
        assert_eq!(setup_event("wt", 1, &json!({})).kind, "setup.event");
        assert_eq!(SubscribeParams::parse(&json!({"workspace": "wt"})).unwrap().after_seq, 0);
        assert_eq!(SubscribeParams::parse(&json!({"workspace": "wt", "after_seq": -1})).unwrap_err().0, "after_seq must be a non-negative integer");
    }

    #[test]
    fn given_the_params_of_worktree_remove_when_read_then_force_is_false_unless_true() {
        // Given a request with force true, with force of the wrong kind and with none
        // Then only a boolean true forces
        assert!(RemoveParams::parse(&json!({"workspace": "t", "force": true})).unwrap().force);
        assert!(!RemoveParams::parse(&json!({"workspace": "t", "force": "yes"})).unwrap().force);
        assert!(!RemoveParams::parse(&json!({"workspace": "t"})).unwrap().force);
        assert_eq!(RemoveParams::parse(&json!({})).unwrap_err().0, "workspace is required");
    }
}
