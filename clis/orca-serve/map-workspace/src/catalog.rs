//! `folderWorkspace.list` and `mobileRelay.projectSections`: what the phone's project sidebar lists. A project is an rca workspace that is a root
//! or a directory (a worktree is a workspace of its project); what Orca keeps about it beyond that is a `state` key the phone's own settings wrote.
use crate::repos::{array, repo_row, repo_targets};
use orca_core::wire::Failure;
use orca_core::AgentApi;
use serde_json::{json, Value};

const FOLDER_WORKSPACES_KEY: &str = "orca.folder-workspaces";

/// The folder workspaces the phone's settings stored: a list of the host's own `FolderWorkspace` rows, empty when none was.
pub(crate) fn folder_workspaces(agent: &dyn AgentApi) -> Result<Value, Failure> {
    Ok(json!({"folderWorkspaces": array(agent, FOLDER_WORKSPACES_KEY)?}))
}

/// The project catalog: every root or directory workspace as a repo and as a project, named as its `orca.repo.<workspace>` key says.
pub(crate) fn project_sections(agent: &dyn AgentApi) -> Result<Value, Failure> {
    let (mut repos, mut projects) = (Vec::new(), Vec::new());
    for t in repo_targets(agent)? {
        let row = repo_row(agent, &t)?;
        projects.push(json!({"id": t.workspace, "displayName": row["displayName"]}));
        repos.push(row);
    }
    Ok(json!({"catalog": {"repos": repos, "projects": projects, "setups": [], "projectOrderBy": "manual", "projectGroups": []}}))
}
