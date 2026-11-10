//! `hosted.clone`: a clone of a hosted repository into a directory of the workspace. `clone.start` makes the executor's `hosted_repo_clone` (an
//! action that stays open until the CLI ends) and answers at once with the clone's id; `clone.status` and `clone.subscribe` follow it and
//! `clone.cancel` cancels the executor's request. The service answers the last three itself. States: `running`, `done` (with `path`),
//! `failed` and `cancelled` (with `message`).
use super::{spec, workspace_param, Spec, GROUP_CLONE};
use agent_api_proto::{text, Method, ParamError};
use serde_json::Value;

pub const START: Method = Method::new(GROUP_CLONE, "clone.start");
pub const STATUS: Method = Method::new(GROUP_CLONE, "clone.status");
pub const CANCEL: Method = Method::new(GROUP_CLONE, "clone.cancel");
pub const SUBSCRIBE: Method = Method::new(GROUP_CLONE, "clone.subscribe");

pub const METHODS: &[Method] = &[START, STATUS, CANCEL, SUBSCRIBE];
pub const SPECS: &[Spec] = &[spec(START, &[exec_hosted::REPO_CLONE], None), spec(STATUS, &[], None), spec(CANCEL, &[], None), spec(SUBSCRIBE, &[], None)];

/// The params of `clone.status` and `clone.cancel`: the workspace and the clone's id.
#[derive(Debug, Clone, PartialEq)]
pub struct CloneRef {
    pub workspace: String,
    pub clone: String,
}

impl CloneRef {
    pub fn parse(params: &Value) -> Result<CloneRef, ParamError> {
        let clone = text(params, "clone").ok_or_else(|| ParamError("clone is required".into()))?;
        Ok(CloneRef { workspace: workspace_param(params)?, clone: clone.to_string() })
    }
}

/// The params of `clone.subscribe`: the workspace and the last `seq` the peer has seen.
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn given_clone_params_when_read_then_workspace_and_clone_are_both_required() {
        assert_eq!(CloneRef::parse(&json!({"workspace": "w", "clone": "c1"})).unwrap(), CloneRef { workspace: "w".into(), clone: "c1".into() });
        assert_eq!(CloneRef::parse(&json!({"workspace": "w"})).unwrap_err().0, "clone is required");
        assert_eq!(CloneRef::parse(&json!({"clone": "c"})).unwrap_err().0, "workspace is required");
    }
}
