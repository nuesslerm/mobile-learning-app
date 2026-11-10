//! The workspace a phone's `worktree` selector names. An Orca repo or worktree is an rca workspace (`repo.list`); its id is
//! `<workspace>::<path>`, the host's `repoId::worktreePath` with the workspace name as repo id, the path being where rca says the workspace lies
//! (empty for an executor's own root, whose absolute path the service does not know).
use crate::agent::ask;
use agent_api_workspace::REPO_LIST;
use orca_core::wire::Failure;
use orca_core::AgentApi;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub workspace: String,
    pub id: String,
    pub root_path: String,
    pub kind: String,
}

pub(crate) fn id_of(workspace: &str, path: &str) -> String {
    format!("{workspace}::{path}")
}

/// Every workspace the service knows, as the targets a selector can name.
pub fn targets(agent: &dyn AgentApi) -> Result<Vec<Target>, Failure> {
    let listed = ask(agent, REPO_LIST.name(), json!({}))?;
    Ok(listed["repos"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|r| {
                    let workspace = r["workspace"].as_str()?;
                    let path = r["path"].as_str().unwrap_or("");
                    Some(Target { workspace: workspace.to_string(), id: id_of(workspace, path), root_path: path.to_string(), kind: r["kind"].as_str().unwrap_or("root").to_string() })
                })
                .collect()
        })
        .unwrap_or_default())
}

/// `id:<workspace>::<path>`, `id:<workspace>`, `path:<path>` or a bare workspace name or path, as the host's selector resolution reads them.
pub fn resolve(agent: &dyn AgentApi, selector: &str) -> Result<Target, Failure> {
    let all = targets(agent)?;
    let selector = selector.trim();
    let found = if let Some(path) = selector.strip_prefix("path:") {
        all.iter().find(|t| !t.root_path.is_empty() && t.root_path == path)
    } else {
        let id = selector.strip_prefix("id:").unwrap_or(selector);
        let (name, below) = id.split_once("::").unwrap_or((id, ""));
        // A worktree's id names its repo, and the path says which checkout of it: that path is the worktree's own workspace.
        let by_path = |p: &str| all.iter().find(|t| !p.is_empty() && t.root_path == p);
        all.iter().find(|t| t.id == id).or_else(|| by_path(below)).or_else(|| all.iter().find(|t| t.workspace == name)).or_else(|| by_path(id))
    };
    found.cloned().ok_or_else(|| Failure::new("selector_not_found", "selector_not_found"))
}

/// The string param `key`, or the host's wording for a missing one.
pub(crate) fn text<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    params.get(key).and_then(Value::as_str)
}

pub(crate) fn selector(params: &Value) -> Result<&str, Failure> {
    text(params, "worktree").filter(|s| !s.is_empty()).ok_or_else(|| Failure::invalid("Missing worktree selector"))
}
