//! `hosted.issue`: a repository's issues, read by the executor with the CLI it already holds a login for. `issue.*` defaults to GitHub; a request
//! names `provider: "gitlab"` for GitLab's issues. Each method is one executor action.
use super::{spec, Spec, GROUP_ISSUE};
use agent_api_proto::Method;

pub const LIST: Method = Method::new(GROUP_ISSUE, "issue.list");
pub const VIEW: Method = Method::new(GROUP_ISSUE, "issue.view");

pub const COMMENTS: Method = Method::new(GROUP_ISSUE, "issue.comments");
pub const COMMENT: Method = Method::new(GROUP_ISSUE, "issue.comment");
pub const EDIT: Method = Method::new(GROUP_ISSUE, "issue.edit");
pub const SET_STATE: Method = Method::new(GROUP_ISSUE, "issue.setState");
pub const CREATE: Method = Method::new(GROUP_ISSUE, "issue.create");

pub const METHODS: &[Method] = &[LIST, VIEW, COMMENTS, COMMENT, EDIT, SET_STATE, CREATE];

pub const SPECS: &[Spec] = &[
    spec(LIST, &[exec_hosted::ISSUE_LIST], Some("github")),
    spec(VIEW, &[exec_hosted::ISSUE_VIEW], Some("github")),
    spec(COMMENTS, &[exec_hosted::ISSUE_COMMENTS], Some("github")),
    spec(COMMENT, &[exec_hosted::ISSUE_COMMENT], Some("github")),
    spec(EDIT, &[exec_hosted::ISSUE_EDIT], Some("github")),
    spec(SET_STATE, &[exec_hosted::ISSUE_STATE], Some("github")),
    spec(CREATE, &[exec_hosted::ISSUE_CREATE], Some("github")),
];
