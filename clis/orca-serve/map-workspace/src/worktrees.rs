//! The worktree rows: `worktree.list/show/create/rm/set/listRetiredNames/prefetchCreateBase`. A worktree is a checkout of a repo (`<repo>::<path>`
//! as its id): rca's `worktree.list` names the checkouts git knows, `worktree.create` and `worktree.remove` act on the ones rca manages, and
//! what Orca keeps of one (name, comment, pin) is a `state` value of the phone's own.
use crate::agent::ask;
use crate::repos::{array, object, put, repo_of, repo_targets};
use crate::target::{id_of, targets, text, Target};
use agent_api_git::FETCH;
use agent_api_workspace::worktrees::{WORKTREE_CREATE, WORKTREE_LIST, WORKTREE_REMOVE};
use orca_core::wire::{Failure, RUNTIME_ERROR};
use orca_core::AgentApi;
use serde_json::{json, Map, Value};

const NOT_MANAGED: &str = "Worktree is not managed by this host";

struct Checkout {
    path: String,
    head: String,
    branch: String,
    is_main: bool,
}

fn meta_key(id: &str) -> String {
    format!("orca.worktree.{id}")
}

fn retired_key(repo: &str) -> String {
    format!("orca.retired-names.{repo}")
}

fn checkouts(agent: &dyn AgentApi, repo: &Target) -> Result<Vec<Checkout>, Failure> {
    let listed = ask(agent, WORKTREE_LIST.name(), json!({"workspace": repo.workspace}))?;
    Ok(listed["worktrees"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|r| {
                    Some(Checkout { path: r["path"].as_str()?.to_string(), head: r["head"].as_str().unwrap_or("").to_string(), branch: r["branch"].as_str().unwrap_or("").to_string(), is_main: r["is_main"] == true })
                })
                .collect()
        })
        .unwrap_or_default())
}

fn basename(path: &str) -> &str {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or(path)
}

fn record(agent: &dyn AgentApi, repo: &Target, c: &Checkout) -> Result<Value, Failure> {
    let id = id_of(&repo.workspace, &c.path);
    let full = if c.branch.is_empty() { String::new() } else { format!("refs/heads/{}", c.branch) };
    let mut row = Map::new();
    let name = if c.branch.is_empty() { basename(&c.path) } else { &c.branch };
    for (k, v) in [
        ("displayName", json!(name)),
        ("comment", json!("")),
        ("linkedIssue", Value::Null),
        ("linkedPR", Value::Null),
        ("linkedLinearIssue", Value::Null),
        ("isArchived", json!(false)),
        ("isUnread", json!(false)),
        ("isPinned", json!(false)),
        ("sortOrder", json!(0)),
        ("lastActivityAt", json!(0)),
    ] {
        row.insert(k.into(), v);
    }
    row.extend(object(agent, &meta_key(&id))?);
    row.insert("id".into(), id.into());
    row.insert("repoId".into(), repo.workspace.clone().into());
    row.insert("path".into(), c.path.clone().into());
    row.insert("head".into(), c.head.clone().into());
    row.insert("branch".into(), full.clone().into());
    row.insert("isMainWorktree".into(), c.is_main.into());
    row.insert("parentWorktreeId".into(), Value::Null);
    row.insert("childWorktreeIds".into(), json!([]));
    row.insert("lineage".into(), Value::Null);
    row.insert("git".into(), json!({"path": c.path, "head": c.head, "branch": full, "isBare": false, "isMainWorktree": c.is_main}));
    Ok(Value::Object(row))
}

/// `id:<repo>::<path>`, `path:<path>`, a bare id or path, or a branch name, as the host's worktree selector reads them.
fn find(agent: &dyn AgentApi, selector: &str) -> Result<(Target, Checkout), Failure> {
    let selector = selector.trim();
    let want = selector.strip_prefix("id:").or_else(|| selector.strip_prefix("path:")).unwrap_or(selector);
    for repo in repo_targets(agent)?.into_iter().filter(|t| t.kind == "root") {
        let found = checkouts(agent, &repo)?.into_iter().find(|c| id_of(&repo.workspace, &c.path) == want || c.path == want || (!c.branch.is_empty() && c.branch == want));
        if let Some(c) = found {
            return Ok((repo, c));
        }
    }
    Err(Failure::new("selector_not_found", "selector_not_found"))
}

fn selector(params: &Value) -> Result<&str, Failure> {
    text(params, "worktree").filter(|s| !s.is_empty()).ok_or_else(|| Failure::invalid("Missing worktree selector"))
}

pub(crate) fn list(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let repos = match text(params, "repo").filter(|s| !s.is_empty()) {
        Some(_) => vec![repo_of(agent, params)?],
        None => repo_targets(agent)?.into_iter().filter(|t| t.kind == "root").collect(),
    };
    let mut rows = Vec::new();
    for repo in &repos {
        for c in checkouts(agent, repo)? {
            rows.push(record(agent, repo, &c)?);
        }
    }
    let total = rows.len();
    let limit = params.get("limit").and_then(Value::as_u64).map_or(total, |l| l as usize);
    rows.truncate(limit);
    Ok(json!({"truncated": rows.len() < total, "totalCount": total, "worktrees": rows}))
}

pub(crate) fn show(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let (repo, c) = find(agent, selector(params)?)?;
    Ok(json!({"worktree": record(agent, &repo, &c)?}))
}

/// A name git and the filesystem both take: letters, digits, `.`, `_` and `-`, anything else a dash.
fn safe_name(name: &str) -> String {
    let mapped: String = name.trim().chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '-' }).collect();
    mapped.trim_matches(['-', '.']).to_string()
}

pub(crate) fn create(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let repo = repo_of(agent, params)?;
    let name = match text(params, "name").map(safe_name).filter(|n| !n.is_empty()) {
        Some(n) => n,
        None => format!("wt-{}", checkouts(agent, &repo)?.len() + 1),
    };
    let mut args = json!({"workspace": repo.workspace, "worktree": name, "branch": name});
    if let Some(base) = text(params, "baseBranch").filter(|b| !b.is_empty()) {
        args["base"] = base.into();
    }
    let made = ask(agent, WORKTREE_CREATE.name(), args)?;
    let c = Checkout { path: made["path"].as_str().unwrap_or_default().to_string(), head: made["head"].as_str().unwrap_or_default().to_string(), branch: made["branch"].as_str().unwrap_or(&name).to_string(), is_main: false };
    if let Some(display) = text(params, "displayName").filter(|d| !d.is_empty()) {
        put(agent, &meta_key(&id_of(&repo.workspace, &c.path)), json!({"displayName": display}))?;
    }
    let mut reply = json!({"worktree": record(agent, &repo, &c)?, "lineage": null, "warnings": []});
    if let Some(error) = made["setup_error"].as_str() {
        reply["warning"] = error.into();
    }
    Ok(reply)
}

pub(crate) fn remove(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let (repo, c) = find(agent, selector(params)?)?;
    if c.is_main {
        return Err(Failure::new(RUNTIME_ERROR, "Cannot remove the main worktree"));
    }
    let managed = targets(agent)?.into_iter().find(|t| t.kind == "worktree" && t.root_path == c.path).ok_or_else(|| Failure::new(RUNTIME_ERROR, NOT_MANAGED))?;
    ask(agent, WORKTREE_REMOVE.name(), json!({"workspace": managed.workspace, "force": params["force"] == true}))?;
    let mut names = array(agent, &retired_key(&repo.workspace))?;
    let retired = Value::String(basename(&c.path).to_string());
    if !names.contains(&retired) {
        names.push(retired);
    }
    put(agent, &retired_key(&repo.workspace), Value::Array(names))?;
    Ok(json!({"removed": true}))
}

pub(crate) fn set(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let (repo, c) = find(agent, selector(params)?)?;
    let key = meta_key(&id_of(&repo.workspace, &c.path));
    let mut meta = object(agent, &key)?;
    for (k, v) in params.as_object().into_iter().flatten().filter(|(k, _)| k.as_str() != "worktree") {
        meta.insert(k.clone(), v.clone());
    }
    put(agent, &key, Value::Object(meta))?;
    Ok(json!({"worktree": record(agent, &repo, &c)?}))
}

pub(crate) fn retired_names(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let repo = repo_of(agent, params)?;
    let names = array(agent, &retired_key(&repo.workspace))?;
    Ok(json!({"retiredNamesByRepo": {repo.workspace.clone(): names}, "retiredNameTiersByRepo": {repo.workspace: 0}}))
}

/// A best-effort fetch of the remote the new worktree starts from: a repo with no remote has nothing to fetch, which is not a failure.
pub(crate) fn prefetch_create_base(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let repo = repo_of(agent, params)?;
    match ask(agent, FETCH.name(), json!({"workspace": repo.workspace})) {
        Ok(_) => Ok(Value::Null),
        Err(f) if f.code == orca_core::wire::INVALID_ARGUMENT => Ok(Value::Null),
        Err(f) => Err(f),
    }
}
