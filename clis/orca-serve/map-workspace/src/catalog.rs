//! `folderWorkspace.list` and `mobileRelay.projectSections`: what the phone's project sidebar reads. The folder workspaces are a `state` key the phone's
//! own settings wrote; the section catalog is a desktop window's and is null here.
use crate::repos::array;
use orca_core::wire::Failure;
use orca_core::AgentApi;
use serde_json::{json, Value};

const FOLDER_WORKSPACES_KEY: &str = "orca.folder-workspaces";

/// The folder workspaces the phone's settings stored: a list of the host's own `FolderWorkspace` rows, empty when none was.
pub(crate) fn folder_workspaces(agent: &dyn AgentApi) -> Result<Value, Failure> {
    Ok(json!({"folderWorkspaces": array(agent, FOLDER_WORKSPACES_KEY)?}))
}

/// The project catalog is the window a desktop publishes for its sidebar; a host with no desktop window answers null as the pin does, and the phone groups the repos itself.
pub(crate) fn project_sections() -> Value {
    json!({"catalog": null})
}
