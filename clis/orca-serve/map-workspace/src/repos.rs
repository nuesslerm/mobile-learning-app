//! The repo rows: `repo.list/update/hooks/sparsePresets/saveSparsePreset/baseRefDefault/searchRefs` and the project views of the same list
//! (`project.list`, `projectHostSetup.list`, `projectGroup.list`). An Orca repo is an rca workspace that is a root or a directory; what Orca
//! keeps beyond that (name, colour, presets, groups) is a `state` value the phone's own calls wrote, never rca data.
use crate::agent::{ask, refusal};
use crate::files::read_prefix;
use crate::target::{resolve, targets, text, Target};
use agent_api_git::REFS;
use agent_api_state::SET;
use orca_core::wire::Failure;
use orca_core::{AgentApi, AgentError};
use serde_json::{json, Map, Value};

pub(crate) const STATE_SCOPE: &str = "orca";
const GROUPS_KEY: &str = "orca.project-groups";
const DEFAULT_COLOR: &str = "#737373";
const HOOKS_FILE: &str = "orca.yaml";

pub(crate) fn repo_key(workspace: &str) -> String {
    format!("orca.repo.{workspace}")
}

pub(crate) fn object(agent: &dyn AgentApi, key: &str) -> Result<Map<String, Value>, Failure> {
    match agent.state_get(STATE_SCOPE, key).map_err(refusal)? {
        Some(Value::Object(o)) => Ok(o),
        _ => Ok(Map::new()),
    }
}

pub(crate) fn array(agent: &dyn AgentApi, key: &str) -> Result<Vec<Value>, Failure> {
    match agent.state_get(STATE_SCOPE, key).map_err(refusal)? {
        Some(Value::Array(a)) => Ok(a),
        _ => Ok(Vec::new()),
    }
}

pub(crate) fn put(agent: &dyn AgentApi, key: &str, value: Value) -> Result<(), Failure> {
    ask(agent, SET.name(), json!({"scope": STATE_SCOPE, "key": key, "value": value})).map(|_| ())
}

/// The `repo` param resolved to a root or directory workspace; a worktree is not a repo.
pub(crate) fn repo_of(agent: &dyn AgentApi, params: &Value) -> Result<Target, Failure> {
    let selector = text(params, "repo").filter(|s| !s.is_empty()).ok_or_else(|| Failure::invalid("Missing repo selector"))?;
    let t = resolve(agent, selector)?;
    if t.kind == "worktree" {
        return Err(Failure::new("selector_not_found", "selector_not_found"));
    }
    Ok(t)
}

pub(crate) fn repo_targets(agent: &dyn AgentApi) -> Result<Vec<Target>, Failure> {
    Ok(targets(agent)?.into_iter().filter(|t| t.kind != "worktree").collect())
}

/// What the phone reads of a repo: the defaults rca implies, then what the phone itself stored; the id and the path stay rca's.
pub(crate) fn repo_row(agent: &dyn AgentApi, t: &Target) -> Result<Value, Failure> {
    let mut row = Map::new();
    row.insert("displayName".into(), t.workspace.clone().into());
    row.insert("badgeColor".into(), DEFAULT_COLOR.into());
    row.insert("addedAt".into(), 0.into());
    row.insert("kind".into(), "git".into());
    row.extend(object(agent, &repo_key(&t.workspace))?);
    row.insert("id".into(), t.workspace.clone().into());
    row.insert("path".into(), t.root_path.clone().into());
    Ok(Value::Object(row))
}

fn display_name(row: &Value) -> &str {
    row["displayName"].as_str().unwrap_or_default()
}

pub(crate) fn list(agent: &dyn AgentApi) -> Result<Value, Failure> {
    let rows = repo_targets(agent)?.iter().map(|t| repo_row(agent, t)).collect::<Result<Vec<_>, _>>()?;
    Ok(json!({"repos": rows}))
}

pub(crate) fn projects(agent: &dyn AgentApi) -> Result<Value, Failure> {
    let mut out = Vec::new();
    for t in repo_targets(agent)? {
        let row = repo_row(agent, &t)?;
        out.push(json!({
            "id": t.workspace, "displayName": display_name(&row), "badgeColor": row["badgeColor"], "kind": row["kind"],
            "sourceRepoIds": [t.workspace], "createdAt": row["addedAt"], "updatedAt": row["addedAt"],
        }));
    }
    Ok(json!({"projects": out}))
}

/// Every repo is one project set up on this host, as the host reports a repo it registered before projects existed (`legacy-repo`).
pub(crate) fn host_setups(agent: &dyn AgentApi) -> Result<Value, Failure> {
    let mut out = Vec::new();
    for t in repo_targets(agent)? {
        let row = repo_row(agent, &t)?;
        out.push(json!({
            "id": format!("setup:{}", t.workspace), "projectId": t.workspace, "hostId": "local", "repoId": t.workspace, "path": t.root_path,
            "displayName": display_name(&row), "kind": row["kind"], "setupState": "ready", "setupMethod": "legacy-repo",
            "createdAt": row["addedAt"], "updatedAt": row["addedAt"],
        }));
    }
    Ok(json!({"setups": out}))
}

pub(crate) fn groups(agent: &dyn AgentApi) -> Result<Value, Failure> {
    Ok(json!({"groups": array(agent, GROUPS_KEY)?}))
}

pub(crate) fn update(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let t = repo_of(agent, params)?;
    let updates = params.get("updates").and_then(Value::as_object).ok_or_else(|| Failure::invalid("Missing repo updates"))?;
    let mut meta = object(agent, &repo_key(&t.workspace))?;
    for (k, v) in updates.iter().filter(|(k, _)| !matches!(k.as_str(), "id" | "path")) {
        if v.is_null() {
            meta.remove(k);
        } else {
            meta.insert(k.clone(), v.clone());
        }
    }
    put(agent, &repo_key(&t.workspace), Value::Object(meta))?;
    Ok(json!({"repo": repo_row(agent, &t)?}))
}

fn presets_key(workspace: &str) -> String {
    format!("orca.sparse-presets.{workspace}")
}

pub(crate) fn sparse_presets(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let t = repo_of(agent, params)?;
    Ok(json!({"presets": array(agent, &presets_key(&t.workspace))?}))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

pub(crate) fn save_sparse_preset(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let t = repo_of(agent, params)?;
    let name = text(params, "name").filter(|s| !s.is_empty()).ok_or_else(|| Failure::invalid("Missing preset name"))?;
    let dirs: Vec<&str> = params["directories"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).ok_or_else(|| Failure::invalid("Missing preset directories"))?;
    let key = presets_key(&t.workspace);
    let mut all = array(agent, &key)?;
    let now = now_ms();
    let id = text(params, "id").filter(|s| !s.is_empty()).map(str::to_string).unwrap_or_else(|| format!("preset-{}", all.len() + 1));
    let previous = all.iter().position(|p| p["id"] == id);
    let created = previous.map_or(now, |i| all[i]["createdAt"].as_u64().unwrap_or(now));
    let preset = json!({"id": id, "repoId": t.workspace, "name": name, "directories": dirs, "createdAt": created, "updatedAt": now});
    match previous {
        Some(i) => all[i] = preset.clone(),
        None => all.push(preset.clone()),
    }
    put(agent, &key, Value::Array(all))?;
    Ok(json!({"preset": preset}))
}

fn refs(agent: &dyn AgentApi, t: &Target, extra: Value) -> Result<Value, Failure> {
    let mut args = json!({"workspace": t.workspace});
    args.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap_or_default());
    ask(agent, REFS.name(), args)
}

fn names(result: &Value, kind: &str) -> Vec<String> {
    result["refs"].as_array().map(|a| a.iter().filter(|r| r["kind"] == kind).filter_map(|r| r["name"].as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// The branch new worktrees start from: the remote's `main` or `master` when there is a remote, else a local one, else the checked-out branch.
pub(crate) fn base_ref_default(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let t = repo_of(agent, params)?;
    let all = refs(agent, &t, json!({}))?;
    let (remote, local) = (names(&all, "remote"), names(&all, "branch"));
    let remotes: std::collections::BTreeSet<&str> = remote.iter().filter_map(|r| r.split('/').next()).collect();
    let pick = |from: &[String], prefix: &str| ["main", "master"].iter().map(|b| format!("{prefix}{b}")).find(|c| from.contains(c));
    let chosen = pick(&remote, "origin/").or_else(|| pick(&local, "")).or_else(|| all["current"].as_str().filter(|c| !c.is_empty()).map(str::to_string));
    Ok(json!({"defaultBaseRef": chosen, "remoteCount": remotes.len()}))
}

pub(crate) fn search_refs(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let t = repo_of(agent, params)?;
    let query = text(params, "query").ok_or_else(|| Failure::invalid("Missing query"))?;
    let mut extra = json!({"query": query});
    if let Some(limit) = params.get("limit").and_then(Value::as_u64) {
        extra["limit"] = limit.into();
    }
    let all = refs(agent, &t, extra)?;
    let found: Vec<String> = all["refs"].as_array().map(|a| a.iter().filter(|r| r["kind"] != "tag").filter_map(|r| r["name"].as_str().map(str::to_string)).collect()).unwrap_or_default();
    Ok(json!({"refs": found, "truncated": all["truncated"] == true}))
}

/// `scripts:` of the repo's `orca.yaml`: each `name:` under it is one script, inline or a `|` block. Everything else in the file is not read.
fn scripts(yaml: &str) -> Map<String, Value> {
    let mut out = Map::new();
    let mut in_scripts = false;
    let mut current: Option<(String, Vec<String>)> = None;
    let flush = |current: &mut Option<(String, Vec<String>)>, out: &mut Map<String, Value>| {
        if let Some((name, lines)) = current.take() {
            out.insert(name, Value::String(lines.join("\n").trim_end().to_string()));
        }
    };
    for line in yaml.lines() {
        let indent = line.len() - line.trim_start().len();
        if indent == 0 && !line.trim().is_empty() {
            flush(&mut current, &mut out);
            in_scripts = line.trim_end() == "scripts:";
            continue;
        }
        if !in_scripts {
            continue;
        }
        if indent == 2 {
            flush(&mut current, &mut out);
            if let Some((name, rest)) = line.trim().split_once(':') {
                let rest = rest.trim();
                current = Some((name.to_string(), if rest == "|" || rest.is_empty() { Vec::new() } else { vec![rest.trim_matches(['"', '\'']).to_string()] }));
            }
        } else if let Some((_, lines)) = current.as_mut() {
            lines.push(line.trim_start().to_string());
        }
    }
    flush(&mut current, &mut out);
    out
}

pub(crate) fn hooks(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let t = repo_of(agent, params)?;
    let stat = agent.request(agent_api_workspace::edit::FILE_STAT.name(), json!({"workspace": t.workspace, "path": HOOKS_FILE}));
    let exists = match stat {
        Ok(_) => true,
        Err(AgentError::Refused { code, .. }) if code == "NOT_FOUND" => false,
        Err(other) => return Err(refusal(other)),
    };
    let hooks = if exists {
        let (bytes, _) = read_prefix(agent, &t, HOOKS_FILE, 512 * 1024)?;
        let scripts = scripts(&String::from_utf8_lossy(&bytes));
        (!scripts.is_empty()).then(|| json!({"scripts": scripts}))
    } else {
        None
    };
    Ok(json!({"hasHooksFile": exists, "hooks": hooks, "setupRunPolicy": "run-by-default", "source": exists.then_some("orca.yaml")}))
}

#[cfg(test)]
mod tests {
    use super::scripts;

    #[test]
    fn given_an_orca_yaml_with_inline_and_block_scripts_when_read_then_each_script_is_its_text() {
        // Given a file with an inline setup script, a block archive script and an unrelated key after them
        let yaml = "scripts:\n  setup: npm ci\n  archive: |\n    echo one\n    echo two\nissueCommand: x\n";
        // When the scripts are read
        let out = scripts(yaml);
        // Then each is its text and the other key is not a script
        assert_eq!(out["setup"], "npm ci");
        assert_eq!(out["archive"], "echo one\necho two");
        assert_eq!(out.len(), 2);
    }
}
