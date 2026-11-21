//! `hosted.issue`: a repository's issues, read by the executor with the CLI it already holds a login for. `issue.*` defaults to GitHub; a request
//! names `provider: "gitlab"` for GitLab's issues. Each method is one executor action.
use super::{account_spec, repo_spec, Spec, GROUP_ISSUE};
use agent_api_proto::Method;

pub const LIST: Method = Method::new(GROUP_ISSUE, "issue.list");
pub const VIEW: Method = Method::new(GROUP_ISSUE, "issue.view");

pub const SEARCH: Method = Method::new(GROUP_ISSUE, "issue.search");
pub const COUNT: Method = Method::new(GROUP_ISSUE, "issue.count");
pub const TODOS: Method = Method::new(GROUP_ISSUE, "issue.todos");
pub const COMMENTS: Method = Method::new(GROUP_ISSUE, "issue.comments");
pub const COMMENT: Method = Method::new(GROUP_ISSUE, "issue.comment");
pub const EDIT: Method = Method::new(GROUP_ISSUE, "issue.edit");
pub const SET_STATE: Method = Method::new(GROUP_ISSUE, "issue.setState");
pub const CREATE: Method = Method::new(GROUP_ISSUE, "issue.create");

pub const METHODS: &[Method] = &[LIST, VIEW, SEARCH, COUNT, TODOS, COMMENTS, COMMENT, EDIT, SET_STATE, CREATE];

pub const SPECS: &[Spec] = &[
    repo_spec(LIST, &[exec_hosted::ISSUE_LIST], Some("github")),
    repo_spec(VIEW, &[exec_hosted::ISSUE_VIEW], Some("github")),
    repo_spec(SEARCH, &[exec_hosted::ISSUE_SEARCH], Some("github")),
    repo_spec(COUNT, &[exec_hosted::ISSUE_COUNT], Some("github")),
    account_spec(TODOS, &[exec_hosted::ISSUE_TODOS], Some("github"), None),
    repo_spec(COMMENTS, &[exec_hosted::ISSUE_COMMENTS], Some("github")),
    repo_spec(COMMENT, &[exec_hosted::ISSUE_COMMENT], Some("github")),
    repo_spec(EDIT, &[exec_hosted::ISSUE_EDIT], Some("github")),
    repo_spec(SET_STATE, &[exec_hosted::ISSUE_STATE], Some("github")),
    repo_spec(CREATE, &[exec_hosted::ISSUE_CREATE], Some("github")),
];
