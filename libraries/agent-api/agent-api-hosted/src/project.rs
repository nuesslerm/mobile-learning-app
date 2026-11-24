//! `hosted.project`: GitHub Projects and the writes a project view makes by repository slug, each asked of an executor whose `gh` is signed in
//! (no `workspace` needed: a request with none goes to such an executor, one that holds the `project` scope first). Projects are GitHub's: `provider`
//! defaults to `github` and no other is served. Each method is one executor action.
use super::{account_spec, repo_spec, Spec, GROUP_PROJECT};
use agent_api_proto::Method;

pub const LIST: Method = Method::new(GROUP_PROJECT, "project.list");
pub const RESOLVE: Method = Method::new(GROUP_PROJECT, "project.resolve");
pub const VIEWS: Method = Method::new(GROUP_PROJECT, "project.views");
pub const SET_FIELD: Method = Method::new(GROUP_PROJECT, "project.setField");
pub const CLEAR_FIELD: Method = Method::new(GROUP_PROJECT, "project.clearField");
pub const COMMENT_ADD: Method = Method::new(GROUP_PROJECT, "project.commentAdd");
pub const COMMENT_EDIT: Method = Method::new(GROUP_PROJECT, "project.commentEdit");
pub const COMMENT_DELETE: Method = Method::new(GROUP_PROJECT, "project.commentDelete");
pub const ISSUE_TYPES: Method = Method::new(GROUP_PROJECT, "project.issueTypes");
pub const SET_ISSUE_TYPE: Method = Method::new(GROUP_PROJECT, "project.setIssueType");
pub const UPDATE_ISSUE: Method = Method::new(GROUP_PROJECT, "project.updateIssue");

pub const METHODS: &[Method] = &[LIST, RESOLVE, VIEWS, SET_FIELD, CLEAR_FIELD, COMMENT_ADD, COMMENT_EDIT, COMMENT_DELETE, ISSUE_TYPES, SET_ISSUE_TYPE, UPDATE_ISSUE];

/// The sign-in scope that reads and writes GitHub Projects.
pub const PROJECT_SCOPE: &str = "project";

pub const SPECS: &[Spec] = &[
    account_spec(LIST, &[exec_hosted::PROJECT_LIST], Some("github"), Some(PROJECT_SCOPE)),
    account_spec(RESOLVE, &[exec_hosted::PROJECT_RESOLVE], Some("github"), Some(PROJECT_SCOPE)),
    account_spec(VIEWS, &[exec_hosted::PROJECT_VIEWS], Some("github"), Some(PROJECT_SCOPE)),
    account_spec(SET_FIELD, &[exec_hosted::PROJECT_FIELD_SET], Some("github"), Some(PROJECT_SCOPE)),
    account_spec(CLEAR_FIELD, &[exec_hosted::PROJECT_FIELD_CLEAR], Some("github"), Some(PROJECT_SCOPE)),
    repo_spec(COMMENT_ADD, &[exec_hosted::COMMENT_ADD], Some("github")),
    repo_spec(COMMENT_EDIT, &[exec_hosted::COMMENT_EDIT], Some("github")),
    repo_spec(COMMENT_DELETE, &[exec_hosted::COMMENT_DELETE], Some("github")),
    repo_spec(ISSUE_TYPES, &[exec_hosted::REPO_ISSUE_TYPES], Some("github")),
    repo_spec(SET_ISSUE_TYPE, &[exec_hosted::ISSUE_TYPE_SET], Some("github")),
    repo_spec(UPDATE_ISSUE, &[exec_hosted::ISSUE_UPDATE], Some("github")),
];
