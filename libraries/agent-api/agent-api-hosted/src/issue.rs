//! `hosted.issue`: a repository's issues, read by the executor with the CLI it already holds a login for. `issue.*` defaults to GitHub; a request
//! names `provider: "gitlab"` for GitLab's issues. Each method is one executor action.
use super::{spec, Spec, GROUP_ISSUE};
use agent_api_proto::Method;

pub const LIST: Method = Method::new(GROUP_ISSUE, "issue.list");
pub const VIEW: Method = Method::new(GROUP_ISSUE, "issue.view");

pub const METHODS: &[Method] = &[LIST, VIEW];

pub const SPECS: &[Spec] = &[spec(LIST, &[exec_hosted::ISSUE_LIST], Some("github")), spec(VIEW, &[exec_hosted::ISSUE_VIEW], Some("github"))];
