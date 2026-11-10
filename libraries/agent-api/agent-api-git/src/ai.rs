//! The `git.ai` sub-group: `git.generateCommitMessage` reads the change of a workspace's repository from its executor (`git_diff`) and has a model
//! write the message, through the service's `ModelApi` port. It commits nothing: the caller sends the message to `git.commit`.
use agent_api_proto::{GroupName, Method, ParamError};
use serde_json::{json, Value};

pub const GROUP: GroupName = GroupName::new("git.ai");

pub const GENERATE_COMMIT_MESSAGE: Method = Method::new(GROUP, "git.generateCommitMessage");

/// Every method of the group.
pub const METHODS: &[Method] = &[GENERATE_COMMIT_MESSAGE];

/// The codes of this group's refusals.
pub mod code {
    /// No model is configured, or the one that is could not answer.
    pub const MODEL_API_UNAVAILABLE: &str = "MODEL_API_UNAVAILABLE";
    /// There is no change to describe.
    pub const NOTHING_TO_COMMIT: &str = "NOTHING_TO_COMMIT";
}

/// The params of `git.generateCommitMessage`: the workspace, whether to describe the staged change (the default) or the whole working tree, and
/// the style of the message.
#[derive(Debug, Clone, PartialEq)]
pub struct GenerateParams {
    pub workspace: String,
    pub staged: bool,
    pub style: Style,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Conventional,
    Plain,
}

impl GenerateParams {
    pub fn parse(params: &Value) -> Result<GenerateParams, ParamError> {
        let workspace = super::workspace_param(params)?;
        let staged = match params.get("staged") {
            None | Some(Value::Null) => true,
            Some(v) => v.as_bool().ok_or_else(|| ParamError("staged must be a boolean".into()))?,
        };
        let style = match params.get("style") {
            None | Some(Value::Null) => Style::Conventional,
            Some(v) => match v.as_str() {
                Some("conventional") => Style::Conventional,
                Some("plain") => Style::Plain,
                _ => return Err(ParamError("style must be conventional or plain".into())),
            },
        };
        Ok(GenerateParams { workspace, staged, style })
    }
}

/// The result: the message and the model that wrote it.
pub fn generate_result(message: &str, model: &str) -> Value {
    json!({ "message": message, "model": model })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_generate_params_when_read_then_staged_and_conventional_are_the_defaults_and_bad_values_are_refused() {
        // Given a bare request, one that asks for the working tree in plain style, and bad values
        let bare = GenerateParams::parse(&json!({"workspace": "w1"})).unwrap();
        let tree = GenerateParams::parse(&json!({"workspace": "w1", "staged": false, "style": "plain"})).unwrap();
        // Then the defaults are the staged change in conventional style, and the asked values stand
        assert_eq!(bare, GenerateParams { workspace: "w1".into(), staged: true, style: Style::Conventional });
        assert_eq!((tree.staged, tree.style), (false, Style::Plain));
        assert_eq!(GenerateParams::parse(&json!({})).unwrap_err().0, "workspace is required");
        assert_eq!(GenerateParams::parse(&json!({"workspace": "w1", "staged": "yes"})).unwrap_err().0, "staged must be a boolean");
        assert_eq!(GenerateParams::parse(&json!({"workspace": "w1", "style": "haiku"})).unwrap_err().0, "style must be conventional or plain");
        assert_eq!(GenerateParams::parse(&json!({"workspace": "w1", "style": 3})).unwrap_err().0, "style must be conventional or plain");
        assert_eq!(generate_result("fix: x", "m"), json!({"message": "fix: x", "model": "m"}));
        assert_eq!(METHODS, [GENERATE_COMMIT_MESSAGE]);
    }
}
