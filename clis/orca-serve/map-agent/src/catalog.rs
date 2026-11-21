//! The rows over `agent.catalog`: what a chat can be run with (`agentSession.options/modelCatalog/commands/createSupport/setOption`), which agent
//! CLIs the executor has (`preflight.detectAgents/refreshAgents/detectRemoteAgents`) and its skills (`skills.discover`). The service asks the
//! EXECUTOR for the CLIs and the skills, so what the phone sees is the machine that runs the agent.
use crate::agent::{ask, refusal, refused_code, state_set};
use crate::journal::view;
use crate::ops::{lookup, refused, Envelope};
use crate::session::{accepted, agent_of, agent_session_id, driver_of, finish, now_ms, text, unsupported};
use agent_api_catalog::{code as catalog_code, AGENTS_DETECT, COMMANDS_LIST, DRIVERS, MODELS_LIST};
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
    agent_session_id(params)
}

/// `agentSession.options`: the models of the session's driver and the model its next turn runs on.
/// `agentSession.agents`: the agents this host runs and what a session of each can do, before any session exists. Nothing here is an installed-CLI
/// check (the pinned host's list is static too). A session here compacts, rewinds to a user message and takes images, holds a follow-up for a running turn until it ends, and has no goal,
/// context meter or fork; approvals are answered by this host.
pub(crate) fn agents(params: &Value) -> Result<Value, Failure> {
    if let Some(key) = params.as_object().and_then(|o| o.keys().next()) {
        return Err(Failure::invalid(format!("Unrecognized key: \"{key}\"")));
    }
    let capabilities = json!({"rewind": true, "compact": true, "threadGoal": false, "contextUsage": false, "imagePrompts": true, "steering": "queue", "approvalEnforcement": "orca"});
    let served: Vec<&str> = drivers_served().into_iter().filter_map(agent_of).collect();
    // the built-ins in the pinned host's order, then any other
    let agents: Vec<Value> = ["claude", "codex", "opencode"].into_iter().filter(|a| served.contains(a)).map(|a| json!({"agent": a, "capabilities": capabilities})).collect();
    Ok(json!({"agents": agents}))
}

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
    Ok(json!({"models": model_options(&catalog), "current": current, "acceptsImages": true, "rewind": {"supported": true}, "conversationCommands": ["clear", "compact"], "compactInstructions": true}))
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
    let id = text(params, "agent").ok_or_else(|| Failure::invalid("Invalid agent"))?;
    if params.get("sessionId").is_some() {
        agent_session_id(params)?;
    }
    let driver = driver_of(id).ok_or_else(unsupported)?;
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

/// `preflight.check`: git, gh and glab as the executor finds them, and gh's and glab's sign-in at the workspace host (only for one that is
/// installed, as the host asks). Bitbucket, Azure DevOps and Gitea are the host's answer for a machine where none is configured: rca keeps no
/// credentials for them. `force` asks the executor again, past the service's kept answer.
pub(crate) fn preflight_check(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    if !params.is_null() && !params.is_object() {
        return Err(Failure::invalid(format!("Invalid input: expected object, received {}", orca_core::wire::received(params))));
    }
    let force = match params.get("force") {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(other) => return Err(Failure::invalid(format!("Invalid input: expected boolean, received {}", orca_core::wire::received(other)))),
    };
    let found = detect(agent, None, force).map_err(refusal)?;
    let installed = |program: &str| found["tools"].as_array().into_iter().flatten().any(|t| t["program"] == program && t["installed"] == true && t.get("error").is_none_or(Value::is_null));
    let (git, gh, glab) = (installed("git"), installed("gh"), installed("glab"));
    let workspace = if gh || glab {
        let all = orca_map_workspace::targets(agent)?;
        let root = all.iter().find(|t| t.kind == "root").or(all.first()).ok_or_else(|| Failure::new(RUNTIME_ERROR, "no executor is attached"))?;
        Some(root.workspace.clone())
    } else {
        None
    };
    // the host counts any failure of `auth status` as signed out
    let signed_in = |there: bool, provider: &str| -> Result<bool, Failure> {
        let Some(workspace) = workspace.as_deref().filter(|_| there) else { return Ok(false) };
        match agent.request(orca_map_hosted::AUTH_DISCOVER.name(), json!({"workspace": workspace, "provider": provider})) {
            Ok(v) => Ok(v["providers"][0]["signed_in"] == true),
            Err(AgentError::Refused { .. }) => Ok(false),
            Err(failed) => Err(refusal(failed)),
        }
    };
    let (gh_auth, glab_auth) = (signed_in(gh, "github")?, signed_in(glab, "gitlab")?);
    let none = json!({"configured": false, "authenticated": false, "account": null, "baseUrl": null, "tokenConfigured": false});
    Ok(json!({
        "git": {"installed": git},
        "gh": {"installed": gh, "authenticated": gh_auth},
        "glab": {"installed": glab, "authenticated": glab_auth},
        "bitbucket": {"configured": false, "authenticated": false, "account": null},
        "azureDevOps": none,
        "gitea": none,
    }))
}

/// The optional string keys of a detection call and the host id, which only this machine (`local`) answers to.
fn detection_target(params: &Value, keys: &[&str]) -> Result<(), Failure> {
    for key in keys {
        orca_core::wire::string_field(params, key)?;
    }
    if params.get("userInitiated").is_some_and(|v| !v.is_boolean()) {
        return Err(Failure::invalid(format!("Invalid input: expected boolean, received {}", orca_core::wire::received(&params["userInitiated"]))));
    }
    if orca_core::wire::string_field(params, "worktreeHostId")?.is_some_and(|h| h != "local") {
        return Err(Failure::invalid("Invalid host id"));
    }
    Ok(())
}

fn no_project(params: &Value) -> Result<(), Failure> {
    if params.get("project").is_some_and(|p| !p.is_null()) {
        return Err(Failure::invalid("Detection for a project is not offered by this host"));
    }
    Ok(())
}

/// `preflight.detectAgents`: the Orca ids of the agents whose CLI the executor has.
pub(crate) fn detect_agents(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    detection_target(params, &["repoId", "worktreeId", "worktreeHostId"])?;
    no_project(params)?;
    Ok(json!(installed(&detect(agent, None, false).map_err(refusal)?)))
}

/// `preflight.refreshAgents`: the same list asked of the executor again, answered with the path facts the reference reports (its shell was hydrated).
pub(crate) fn refresh_agents(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    detection_target(params, &["repoId", "worktreeId", "worktreeHostId"])?;
    let found = detect(agent, None, true).map_err(refusal)?;
    Ok(json!({"agents": installed(&found), "addedPathSegments": [], "shellHydrationOk": true, "pathSource": "shell_hydrate", "pathFailureReason": "none"}))
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

/// Every driver of the contract has an Orca agent id here: a flow test holds this to the catalog's `DRIVERS`.
pub fn drivers_served() -> Vec<&'static str> {
    DRIVERS.iter().copied().filter(|d| agent_of(d).is_some()).collect()
}
