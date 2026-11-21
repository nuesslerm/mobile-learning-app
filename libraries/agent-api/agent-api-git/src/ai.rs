//! The `git.ai` sub-group: `git.generateCommitMessage` reads the change of a workspace's repository from its executor (`git_diff`) and has a model
//! write the message, through the service's `ModelApi` port. It commits nothing: the caller sends the message to `git.commit`.
//! `git.generatePullRequestFields` reads what the current branch changed since it left `base` (`git_compare`, then `git_diff` from the merge base)
//! and has the model write the title, body and draft flag of a pull request. It opens nothing: the caller sends the fields to `pr.create`.
use agent_api_proto::{GroupName, Method, ParamError};
use serde_json::{json, Value};

pub const GROUP: GroupName = GroupName::new("git.ai");

pub const GENERATE_COMMIT_MESSAGE: Method = Method::new(GROUP, "git.generateCommitMessage");

pub const GENERATE_PULL_REQUEST_FIELDS: Method = Method::new(GROUP, "git.generatePullRequestFields");

/// Every method of the group.
pub const METHODS: &[Method] = &[GENERATE_COMMIT_MESSAGE, GENERATE_PULL_REQUEST_FIELDS];

/// The codes of this group's refusals.
pub mod code {
    /// No model is configured, or the one that is could not answer.
    pub const MODEL_API_UNAVAILABLE: &str = "MODEL_API_UNAVAILABLE";
    /// There is no change to describe.
    pub const NOTHING_TO_COMMIT: &str = "NOTHING_TO_COMMIT";
    /// The branch holds no commit that `base` lacks.
    pub const NO_BRANCH_CHANGES: &str = "NO_BRANCH_CHANGES";
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

/// The params of `git.generatePullRequestFields`: the workspace, the branch the pull request goes to, and what the form holds now (the model keeps
/// what it already says where it is sound).
#[derive(Debug, Clone, PartialEq)]
pub struct PullRequestParams {
    pub workspace: String,
    pub base: String,
    pub title: String,
    pub body: String,
    pub draft: bool,
}

/// The longest base, title and body the request takes: what goes to the model is bounded before it is read.
const BASE_CHARS: usize = 255;
const TITLE_CHARS: usize = 1_000;
const BODY_CHARS: usize = 20_000;

impl PullRequestParams {
    pub fn parse(params: &Value) -> Result<PullRequestParams, ParamError> {
        let workspace = super::workspace_param(params)?;
        let base = params.get("base").and_then(Value::as_str).filter(|b| !b.is_empty()).ok_or_else(|| ParamError("base is required".into()))?;
        if base.chars().count() > BASE_CHARS || base.starts_with('-') || !base.chars().all(|c| c.is_ascii_alphanumeric() || "._/~^-".contains(c)) {
            return Err(ParamError("base must be a branch or revision name".into()));
        }
        let text = |key: &str, max: usize| match params.get(key) {
            None | Some(Value::Null) => Ok(String::new()),
            Some(Value::String(s)) if s.chars().count() <= max => Ok(s.clone()),
            Some(Value::String(_)) => Err(ParamError(format!("{key} is longer than {max} characters"))),
            Some(_) => Err(ParamError(format!("{key} must be a string"))),
        };
        let draft = match params.get("draft") {
            None | Some(Value::Null) => false,
            Some(v) => v.as_bool().ok_or_else(|| ParamError("draft must be a boolean".into()))?,
        };
        Ok(PullRequestParams { workspace, base: base.to_string(), title: text("title", TITLE_CHARS)?, body: text("body", BODY_CHARS)?, draft })
    }
}

/// The result: the fields of the pull request and the model that wrote them. `base` is the one asked for.
pub fn pull_request_result(base: &str, title: &str, body: &str, draft: bool, model: &str) -> Value {
    json!({ "base": base, "title": title, "body": body, "draft": draft, "model": model })
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
        assert_eq!(METHODS, [GENERATE_COMMIT_MESSAGE, GENERATE_PULL_REQUEST_FIELDS]);
    }

    #[test]
    fn given_pull_request_params_when_read_then_a_base_that_could_be_an_option_or_a_pathspec_is_refused_and_the_form_defaults_to_empty() {
        // Given a bare request and the same with the form filled
        let bare = PullRequestParams::parse(&json!({"workspace": "w1", "base": "main"})).unwrap();
        let full = PullRequestParams::parse(&json!({"workspace": "w1", "base": "origin/release-1.2", "title": "t", "body": "b", "draft": true})).unwrap();
        // Then the form is empty and not a draft by default, and the asked values stand
        assert_eq!(bare, PullRequestParams { workspace: "w1".into(), base: "main".into(), title: String::new(), body: String::new(), draft: false });
        assert_eq!((full.base.as_str(), full.title.as_str(), full.body.as_str(), full.draft), ("origin/release-1.2", "t", "b", true));
        // And a missing base, one that starts with a dash or holds a space or a colon, and a wrong kind are each refused by name
        for (params, says) in [
            (json!({"workspace": "w1"}), "base is required"),
            (json!({"workspace": "w1", "base": ""}), "base is required"),
            (json!({"workspace": "w1", "base": "--output=x"}), "base must be a branch or revision name"),
            (json!({"workspace": "w1", "base": "a b"}), "base must be a branch or revision name"),
            (json!({"workspace": "w1", "base": "a:b"}), "base must be a branch or revision name"),
            (json!({"workspace": "w1", "base": "main", "title": 3}), "title must be a string"),
            (json!({"workspace": "w1", "base": "main", "draft": "no"}), "draft must be a boolean"),
            (json!({"workspace": "w1", "base": "main", "body": "x".repeat(20_001)}), "body is longer than 20000 characters"),
            (json!({"base": "main"}), "workspace is required"),
        ] {
            assert_eq!(PullRequestParams::parse(&params).unwrap_err().0, says, "{params}");
        }
        assert_eq!(pull_request_result("main", "t", "b", false, "m"), json!({"base": "main", "title": "t", "body": "b", "draft": false, "model": "m"}));
    }
}
