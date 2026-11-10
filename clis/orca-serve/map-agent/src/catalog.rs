//! The rows over `agent.catalog`: what a chat can be run with (`agentSession.options/modelCatalog/commands/createSupport/setOption`), which agent
//! CLIs the executor has (`preflight.detectAgents/refreshAgents/detectRemoteAgents`) and its skills (`skills.discover`). The service asks the
//! EXECUTOR for the CLIs and the skills, so what the phone sees is the machine that runs the agent.
use crate::agent::{ask, refusal, refused_code, state_set};
use crate::journal::view;
use crate::ops::{lookup, refused, Envelope};
use crate::session::{accepted, agent_of, driver_of, finish, now_ms, text, unsupported};
use agent_api_catalog::{code as catalog_code, AGENTS_DETECT, COMMANDS_LIST, DRIVERS, MODELS_LIST, SKILLS_LIST};
use orca_core::wire::{Failure, RUNTIME_ERROR};
use orca_core::{AgentApi, AgentError};
use serde_json::{json, Map, Value};

/// The host's own wording for a remote machine it cannot reach.
pub(crate) const REMOTE_NOT_SERVED: &str = "Remote connection dropped. Click Reconnect on the SSH target before retrying.";

fn options_key(session: &str) -> String {
    format!("session-options.{session}")
}

fn stored(agent: &dyn AgentApi, session: &str) -> Result<Map<String, Value>, Failure> {
    Ok(match crate::agent::state_get(agent, &options_key(session))? {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    })
}

/// The model a person picked for the session, if any: a turn runs on it unless the send names another.
pub(crate) fn stored_model(agent: &dyn AgentApi, session: &str) -> Result<Option<String>, Failure> {
    Ok(stored(agent, session)?.get("model").and_then(Value::as_str).map(str::to_string))
}

fn models_of(agent: &dyn AgentApi, driver: &str) -> Result<Value, Failure> {
    ask(agent, MODELS_LIST.name(), json!({"driver": driver}))
}

/// A model as the phone lists it: the neutral snake_case names of the catalog in the host's camelCase.
fn model_option(m: &Value) -> Value {
    let mut o = json!({"id": m["id"], "label": m["label"], "isDefault": m["is_default"], "efforts": m["efforts"]});
    for (from, to) in [("description", "description"), ("default_effort", "defaultEffort")] {
        if !m[from].is_null() {
            o[to] = m[from].clone();
        }
    }
    o
}

fn model_options(catalog: &Value) -> Vec<Value> {
    catalog["models"].as_array().into_iter().flatten().map(model_option).collect()
}

fn default_model(catalog: &Value) -> Option<String> {
    catalog["models"].as_array()?.iter().find(|m| m["is_default"] == true).and_then(|m| m["id"].as_str()).map(str::to_string)
}

fn session_driver(thread: &Value) -> Result<&str, Failure> {
    thread["driver"].as_str().ok_or_else(|| Failure::new(RUNTIME_ERROR, "structured_agent_session_unsupported"))
}

fn session_id(params: &Value) -> Result<&str, Failure> {
    text(params, "sessionId").ok_or_else(|| Failure::invalid("Invalid session id"))
}

/// `agentSession.options`: the models of the session's driver and the model its next turn runs on.
pub(crate) fn options(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let session = session_id(params)?;
    let thread = view(agent, session)?;
    let catalog = models_of(agent, session_driver(&thread)?)?;
    let picked = stored_model(agent, session)?;
    let named = picked.or_else(|| thread["model"].as_str().map(str::to_string));
    let model = named.clone().or_else(|| default_model(&catalog));
    let mut current = json!({});
    if let Some(m) = model {
        current["model"] = m.into();
    }
    if named.is_none() {
        current["defaulted"] = json!(["model"]);
    }
    Ok(json!({"models": model_options(&catalog), "current": current, "acceptsImages": true, "conversationCommands": ["clear", "compact"], "compactInstructions": true}))
}

/// `agentSession.setOption`: the model of the session's next turns. The turn takes no effort and no permission mode of the phone's, so those keys are refused.
pub(crate) fn set_option(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let env = Envelope::read(params)?;
    if let Some(done) = lookup(agent, &env)? {
        return Ok(done);
    }
    let key = text(params, "key").ok_or_else(|| Failure::invalid("Invalid option key"))?;
    let value = params["value"].as_str().ok_or_else(|| Failure::invalid("Invalid option value"))?;
    if key != "model" {
        return Ok(refused("agent_session_option_unsupported", &format!("the option {key} is not offered by this host")));
    }
    let thread = view(agent, &env.session)?;
    let catalog = models_of(agent, session_driver(&thread)?)?;
    if !model_options(&catalog).iter().any(|m| m["id"] == value) {
        return Ok(refused("agent_session_option_unsupported", &format!("{value} is not a model of this agent")));
    }
    let mut options = stored(agent, &env.session)?;
    if default_model(&catalog).as_deref() == Some(value) {
        options.remove("model");
    } else {
        options.insert("model".into(), value.into());
    }
    state_set(agent, &options_key(&env.session), Value::Object(options.clone()))?;
    let mut shown = options;
    if shown.is_empty() {
        shown.insert("model".into(), value.into());
    }
    let answer = accepted(agent, &env.session, json!({"key": key, "value": value, "options": shown}))?;
    finish(agent, &env, answer)
}

/// `agentSession.modelCatalog`: the models of an agent, from the service's table (a listing the driver does not make by itself).
pub(crate) fn model_catalog(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let driver = driver_of(text(params, "agent").ok_or_else(|| Failure::invalid("Invalid agent"))?).ok_or_else(unsupported)?;
    let catalog = models_of(agent, driver)?;
    Ok(json!({"origin": "probe", "models": model_options(&catalog), "fetchedAt": now_ms(), "defaultHoldsInEveryWorkspace": true}))
}

/// `agentSession.commands`: the commands of the session's driver and the skills it reads in the session's workspace.
pub(crate) fn commands(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let thread = view(agent, session_id(params)?)?;
    let mut ask_for = json!({"driver": session_driver(&thread)?});
    if let Some(workspace) = thread["workspace"].as_str() {
        ask_for["workspace"] = workspace.into();
    }
    let listed = ask(agent, COMMANDS_LIST.name(), ask_for)?;
    let commands: Vec<Value> = listed["commands"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            let mut o = json!({"name": c["name"], "kind": c["kind"]});
            for (from, to) in [("description", "description"), ("argument_hint", "argumentHint"), ("path", "path")] {
                if !c[from].is_null() {
                    o[to] = c[from].clone();
                }
            }
            if c["builtin"] == true {
                o["builtin"] = true.into();
            }
            o
        })
        .collect();
    Ok(json!({"commands": commands}))
}

/// The drivers whose CLI the executor has, as Orca agent ids, in the catalog's order.
fn installed(detected: &Value) -> Vec<&'static str> {
    detected["agents"].as_array().into_iter().flatten().filter(|a| a["installed"] == true).filter_map(|a| a["driver"].as_str()).filter_map(agent_of).collect()
}

fn detect(agent: &dyn AgentApi, executor: Option<&str>, refresh: bool) -> Result<Value, AgentError> {
    let mut p = json!({});
    if let Some(e) = executor {
        p["executor"] = e.into();
    }
    if refresh {
        p["refresh"] = true.into();
    }
    agent.request(AGENTS_DETECT.name(), p)
}

fn no_project(params: &Value) -> Result<(), Failure> {
    if params.get("project").is_some_and(|p| !p.is_null()) {
        return Err(Failure::invalid("Detection for a project is not offered by this host"));
    }
    Ok(())
}

/// `preflight.detectAgents`: the Orca ids of the agents whose CLI the executor has.
pub(crate) fn detect_agents(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    no_project(params)?;
    Ok(json!(installed(&detect(agent, None, false).map_err(refusal)?)))
}

/// `preflight.refreshAgents`: the same list asked of the executor again; the executor's own PATH is what it searches, so no shell is hydrated.
pub(crate) fn refresh_agents(agent: &dyn AgentApi, _params: &Value) -> Result<Value, Failure> {
    let found = detect(agent, None, true).map_err(refusal)?;
    Ok(json!({"agents": installed(&found), "addedPathSegments": [], "shellHydrationOk": false, "pathSource": "sync_seed_only", "pathFailureReason": null}))
}

/// `preflight.detectRemoteAgents`: the executor named by the connection id, which an unknown one is a machine the host cannot reach.
pub(crate) fn detect_remote_agents(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let connection = text(params, "connectionId").ok_or_else(|| Failure::invalid("Invalid connection id"))?;
    match detect(agent, Some(connection), false) {
        Ok(found) => Ok(json!(installed(&found))),
        Err(e) if refused_code(&e) == Some(catalog_code::UNKNOWN_EXECUTOR) => Err(Failure::new(RUNTIME_ERROR, REMOTE_NOT_SERVED)),
        Err(e) => Err(refusal(e)),
    }
}

/// `agentSession.createSupport`: a chat can be created when the agent has a driver and the executor has its CLI.
pub(crate) fn create_support(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    if params.get("resumeFrom").is_some_and(|r| !r.is_null()) {
        return Err(unsupported());
    }
    orca_map_workspace::resolve(agent, text(params, "worktree").ok_or_else(|| Failure::invalid("Invalid worktree selector"))?)?;
    let id = text(params, "agent").ok_or_else(|| Failure::invalid("Invalid agent"))?;
    if driver_of(id).is_none() {
        return Ok(json!({"supported": false, "reason": "agent"}));
    }
    let found = detect(agent, None, false).map_err(refusal)?;
    if !installed(&found).contains(&id) {
        return Ok(json!({"supported": false, "reason": "agent"}));
    }
    Ok(json!({"supported": true}))
}

/// The skills directories of the executor and the agent each belongs to (`None` is the shared one): what the phone reads a skill's providers by.
const DIRS: [(&str, &str, Option<&str>); 3] = [(".claude/skills", "claude", Some("claude")), (".agents/skills", "agent-skills", None), (".codex/skills", "codex", Some("codex"))];

fn dir_row(dir: &str) -> Option<(&'static str, Option<&'static str>)> {
    DIRS.iter().find(|(d, ..)| *d == dir).map(|(_, provider, owner)| (*provider, *owner))
}

/// `skills.discover`: the skills of one workspace and of the executor's home, as `skill_scan` found them.
pub(crate) fn skills_discover(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let selector = match (text(params, "worktreeId"), text(params, "cwd")) {
        (Some(id), _) => format!("id:{id}"),
        (None, Some(cwd)) => format!("path:{cwd}"),
        (None, None) => return Err(Failure::invalid("Skill discovery needs a workspace")),
    };
    let target = orca_map_workspace::resolve(agent, &selector)?;
    let listed = ask(agent, SKILLS_LIST.name(), json!({"workspace": target.workspace}))?;
    let kinds: Option<Vec<&str>> = params["sourceKinds"].as_array().map(|k| k.iter().filter_map(Value::as_str).collect());
    let names: Option<Vec<&str>> = params["names"].as_array().map(|k| k.iter().filter_map(Value::as_str).collect());
    let (mut skills, mut found) = (Vec::new(), Vec::new());
    for s in listed["skills"].as_array().into_iter().flatten() {
        let (Some(dir), Some(name), Some(path)) = (s["dir"].as_str(), s["name"].as_str(), s["path"].as_str()) else { continue };
        let Some((provider, _)) = dir_row(dir) else { continue };
        let home = s["scope"] == "home";
        let kind = if home { "home" } else { "repo" };
        let root = if home { format!("~/{dir}") } else { format!("{}/{dir}", target.root_path.trim_end_matches('/')) };
        let file = if home { path.to_string() } else { format!("{}/{path}", target.root_path.trim_end_matches('/')) };
        found.push((kind, dir.to_string(), root.clone()));
        if kinds.as_ref().is_some_and(|k| !k.contains(&kind)) || names.as_ref().is_some_and(|n| !n.contains(&name)) {
            continue;
        }
        let directory = file.rsplit_once('/').map_or(file.clone(), |(d, _)| d.to_string());
        skills.push(json!({"id": format!("{kind}:{file}"), "name": name, "description": s["description"], "providers": [provider], "sourceKind": kind, "sourceLabel": format!("{} {dir}", if home { "Home" } else { "Project" }), "rootPath": root, "directoryPath": directory, "skillFilePath": file, "installed": true, "updatedAt": null}));
    }
    let mut sources = Vec::new();
    for (kind, home) in [("repo", false), ("home", true)] {
        for (dir, provider, owner) in DIRS {
            let path = if home { format!("~/{dir}") } else { format!("{}/{dir}", target.root_path.trim_end_matches('/')) };
            if kinds.as_ref().is_some_and(|k| !k.contains(&kind)) {
                continue;
            }
            let exists = found.iter().any(|(k, _, p)| *k == kind && *p == path);
            sources.push(json!({"id": format!("{kind}:{path}"), "label": format!("{} {dir}", if home { "Home" } else { "Project" }), "path": path, "sourceKind": kind, "providers": [provider], "owner": owner, "exists": exists}));
        }
    }
    Ok(json!({"skills": skills, "sources": sources, "scannedAt": now_ms()}))
}

/// Every driver of the contract has an Orca agent id here: a flow test holds this to the catalog's `DRIVERS`.
pub fn drivers_served() -> Vec<&'static str> {
    DRIVERS.iter().copied().filter(|d| agent_of(d).is_some()).collect()
}
