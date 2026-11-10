//! The automation rows: the phone's record from an rca schedule and its kept `orca` meta, and each write as one `schedule.upsert`.
use crate::recurrence::{next_after, schedule_of};
use crate::{ask, not_found, ok_delete, text};
use agent_api_schedules::{ListResult, MutationResult, RunNowResult, RunSource, RunState, RunStatus, TaskView, DELETE, LIST, RUN_NOW, UPSERT};
use orca_core::wire::{Failure, RUNTIME_ERROR};
use orca_core::AgentApi;
use orca_map_agent::{agent_of, driver_of};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::time::{SystemTime, UNIX_EPOCH};

/// The key of the schedule's `meta` the automation rows own.
const NS: &str = "orca";

/// What the phone sent that rca has no field for, kept as it came so the record reads back as it was written.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Kept {
    project_id: String,
    workspace_id: String,
    rrule: String,
    timezone: String,
    dtstart: serde_json::Number,
    setup_decision: Option<String>,
    creation_key: Option<String>,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn bad_answer(what: &str) -> Failure {
    Failure::new(RUNTIME_ERROR, format!("rca's {what}"))
}

fn tasks(agent: &dyn AgentApi) -> Result<Vec<(TaskView, Kept)>, Failure> {
    let listed: ListResult = serde_json::from_value(ask(agent, LIST, json!({}))?).map_err(|e| bad_answer(&format!("schedule.list answered no list of schedules: {e}")))?;
    listed.tasks.into_iter().filter_map(|t| kept_of(t).transpose()).collect()
}

/// The schedule's kept data, none for one that is not an automation.
fn kept_of(task: TaskView) -> Result<Option<(TaskView, Kept)>, Failure> {
    let Some(own) = task.meta.as_ref().and_then(|m| m.get(NS)) else { return Ok(None) };
    let kept = serde_json::from_value(own.clone()).map_err(|e| bad_answer(&format!("schedule {} holds an automation record that is unreadable: {e}", task.id)))?;
    Ok(Some((task, kept)))
}

fn find(agent: &dyn AgentApi, id: &str) -> Result<(TaskView, Kept), Failure> {
    tasks(agent)?.into_iter().find(|(t, _)| t.id == id).ok_or_else(not_found)
}

fn run_status(status: RunStatus) -> Value {
    match status {
        RunStatus::Never => Value::Null,
        RunStatus::Running => json!("dispatched"),
        RunStatus::Succeeded => json!("completed"),
        RunStatus::Failed => json!("dispatch_failed"),
    }
}

fn next_run(task: &TaskView) -> Result<u64, Failure> {
    match (task.enabled, task.next_run_ms) {
        (true, Some(at)) => Ok(at),
        (true, None) => Err(bad_answer(&format!("schedule {} is enabled and names no next run", task.id))),
        (false, _) => next_after(&task.schedule, now_ms()),
    }
}

/// The phone's record of an automation.
fn record(task: &TaskView, kept: &Kept) -> Result<Value, Failure> {
    let agent_id = agent_of(&task.driver).ok_or_else(|| bad_answer(&format!("schedule {} runs the driver {}, which no Orca agent is", task.id, task.driver)))?;
    let mut automation = json!({
        "id": task.id, "name": task.title, "prompt": task.prompt, "precheck": null, "agentId": agent_id, "projectId": kept.project_id,
        "executionTargetType": "local", "executionTargetId": "local", "schedulerOwner": "local_host_service", "workspaceMode": "existing", "workspaceId": kept.workspace_id,
        "baseBranch": null, "reuseSession": false, "timezone": kept.timezone, "rrule": kept.rrule, "dtstart": kept.dtstart, "enabled": task.enabled, "nextRunAt": next_run(task)?,
        "missedRunPolicy": "run_once_within_grace", "missedRunGraceMinutes": 0, "createdAt": task.created_at_ms, "updatedAt": task.updated_at_ms,
    });
    if let Some(decision) = &kept.setup_decision {
        automation["setupDecision"] = json!(decision);
    }
    if let Some(at) = task.last_run_ms {
        automation["lastRunAt"] = json!(at);
    }
    Ok(automation)
}

pub fn list(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    match params.pointer("/selector/kind").and_then(Value::as_str) {
        None | Some("self") => {}
        Some("orphan") => return Ok(json!({"automations": [], "items": []})),
        Some(other) => return Err(Failure::invalid(format!("The selector {other} is not offered: this host has no ssh targets"))),
    }
    let (mut automations, mut items) = (Vec::new(), Vec::new());
    for (task, kept) in tasks(agent)? {
        automations.push(record(&task, &kept)?);
        items.push(json!({"automationId": task.id, "selector": {"kind": "self"}, "usageSummary": {"lastRunStatus": run_status(task.last_run_status), "lastRunAt": task.last_run_ms}}));
    }
    Ok(json!({"automations": automations, "items": items}))
}

fn id_of(params: &Value) -> Result<&str, Failure> {
    text(params, "id").ok_or_else(|| Failure::invalid("Missing automation id"))
}

/// Only this host's own automations exist: the phone's owner fence names another host or none.
fn own_host_only(params: &Value) -> Result<(), Failure> {
    match params.pointer("/expectedOwner/selector/kind").and_then(Value::as_str) {
        None | Some("self") => Ok(()),
        Some(other) => Err(Failure::invalid(format!("The owner {other} is not offered: this host has no ssh targets"))),
    }
}

pub fn show(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    own_host_only(params)?;
    let (task, kept) = find(agent, id_of(params)?)?;
    Ok(json!({"automation": record(&task, &kept)?}))
}

pub fn delete(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    own_host_only(params)?;
    let (task, _) = find(agent, id_of(params)?)?;
    ask(agent, DELETE, json!({"id": task.id}))?;
    Ok(ok_delete(&task.id))
}

fn unset(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => true,
        Some(Value::String(s)) => s.trim().is_empty(),
        _ => false,
    }
}

/// A field the phone may leave out and rca cannot honour: refused when it is set, so no setting is saved and then ignored.
fn refuse_unsupported(fields: &Value) -> Result<(), Failure> {
    let refused = [
        ("precheck", "a precheck command"),
        ("extraAgentArgs", "extra agent arguments"),
        ("launchSetupPatch", "a launch setup"),
        ("runContext", "a project host setup"),
        ("sourceContext", "a task source"),
        ("reuseSession", "a reused session"),
        ("missedRunGraceMinutes", "a grace for missed runs"),
        ("baseBranch", "a base branch"),
    ];
    for (key, what) in refused {
        if !unset(fields.get(key)) {
            return Err(Failure::invalid(format!("{what} is not offered: an rca schedule runs its prompt in an existing workspace, with none of it")));
        }
    }
    if let Some(mode) = fields.get("workspaceMode").and_then(Value::as_str).filter(|m| *m != "existing") {
        return Err(Failure::invalid(format!("The workspace mode {mode} is not offered: an rca schedule runs in an existing workspace, never in a worktree it makes for each run")));
    }
    if let Some(kind) = fields.pointer("/destination/selector/kind").and_then(Value::as_str).filter(|k| *k != "self") {
        return Err(Failure::invalid(format!("The destination {kind} is not offered: this host has no ssh targets")));
    }
    Ok(())
}

fn driver(agent_id: &str) -> Result<&'static str, Failure> {
    driver_of(agent_id).ok_or_else(|| Failure::invalid("Unknown provider"))
}

/// The workspace rca runs in and the ids the phone reads back, from the project and workspace selectors the phone sent.
fn place(agent: &dyn AgentApi, fields: &Value) -> Result<(String, String, String), Failure> {
    let (repo, workspace) = (text(fields, "repo"), text(fields, "workspace"));
    let chosen = workspace.or(repo).ok_or_else(|| Failure::invalid("Automation requires --repo or --workspace."))?;
    let target = orca_map_workspace::resolve(agent, chosen)?;
    Ok((target.workspace.clone(), repo.map_or(target.workspace, str::to_string), workspace.map_or(target.id, str::to_string)))
}

fn upsert(agent: &dyn AgentApi, id: Option<&str>, task: Value, kept: &Kept) -> Result<TaskView, Failure> {
    let mut params = task;
    params["meta"] = Value::Object(Map::from_iter([(NS.to_string(), json!(kept))]));
    if let Some(id) = id {
        params["id"] = json!(id);
        params["require_existing"] = json!(true);
    }
    let saved: MutationResult = serde_json::from_value(ask(agent, UPSERT, params)?).map_err(|e| bad_answer(&format!("schedule.upsert answered no schedule: {e}")))?;
    Ok(saved.task)
}

fn required<'a>(fields: &'a Value, key: &str, what: &str) -> Result<&'a str, Failure> {
    text(fields, key).ok_or_else(|| Failure::invalid(format!("Missing {what}")))
}

pub fn create(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    refuse_unsupported(params)?;
    let creation_key = text(params, "creationKey").map(str::to_string);
    if let Some(key) = &creation_key {
        if let Some((task, kept)) = tasks(agent)?.into_iter().find(|(_, k)| k.creation_key.as_ref() == Some(key)) {
            return Ok(json!({"automation": record(&task, &kept)?}));
        }
    }
    let (name, prompt) = (required(params, "name", "automation name")?, required(params, "prompt", "automation prompt")?);
    let (agent_id, rrule, timezone) = (required(params, "agentId", "provider")?, required(params, "rrule", "trigger")?, required(params, "timezone", "timezone")?);
    let dtstart = params.get("dtstart").and_then(Value::as_number).ok_or_else(|| Failure::invalid("Missing trigger start time"))?;
    let schedule = schedule_of(rrule, timezone)?;
    let (workspace, project_id, workspace_id) = place(agent, params)?;
    let kept = Kept { project_id, workspace_id, rrule: rrule.into(), timezone: timezone.into(), dtstart: dtstart.clone(), setup_decision: text(params, "setupDecision").map(str::to_string), creation_key };
    let task = json!({"title": name, "prompt": prompt, "enabled": params.get("enabled").and_then(Value::as_bool).unwrap_or(true), "schedule": schedule, "workspace": workspace, "driver": driver(agent_id)?});
    let saved = upsert(agent, None, task, &kept)?;
    Ok(json!({"automation": record(&saved, &kept)?}))
}

pub fn update(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    own_host_only(params)?;
    let updates = params.get("updates").filter(|u| u.is_object()).ok_or_else(|| Failure::invalid("Missing updates"))?;
    refuse_unsupported(updates)?;
    refuse_unsupported(&json!({"destination": params.get("destination")}))?;
    if params.get("expectedWorkspaceMode").is_some() {
        return Err(Failure::invalid("The workspace mode is not new_per_run: an rca schedule runs in an existing workspace"));
    }
    let (task, mut kept) = find(agent, id_of(params)?)?;
    let pick = |key: &str| text(updates, key);
    let (rrule, timezone) = (pick("rrule").map_or(kept.rrule.clone(), str::to_string), pick("timezone").map_or(kept.timezone.clone(), str::to_string));
    let schedule = if pick("rrule").is_some() || pick("timezone").is_some() { schedule_of(&rrule, &timezone)? } else { task.schedule.clone() };
    let (workspace, project_id, workspace_id) = if pick("repo").is_some() || pick("workspace").is_some() { place(agent, updates)? } else { (task.workspace.clone(), kept.project_id.clone(), kept.workspace_id.clone()) };
    kept = Kept {
        project_id,
        workspace_id,
        rrule,
        timezone,
        dtstart: updates.get("dtstart").and_then(Value::as_number).cloned().unwrap_or(kept.dtstart),
        setup_decision: pick("setupDecision").map(str::to_string).or(kept.setup_decision),
        creation_key: kept.creation_key,
    };
    let mut next = Map::new();
    next.insert("title".into(), json!(pick("name").unwrap_or(&task.title)));
    next.insert("prompt".into(), json!(pick("prompt").unwrap_or(&task.prompt)));
    next.insert("enabled".into(), json!(updates.get("enabled").and_then(Value::as_bool).unwrap_or(task.enabled)));
    next.insert("schedule".into(), json!(schedule));
    next.insert("workspace".into(), json!(workspace));
    next.insert("driver".into(), json!(pick("agentId").map(driver).transpose()?.unwrap_or(task.driver.as_str())));
    let saved = upsert(agent, Some(&task.id), Value::Object(next), &kept)?;
    Ok(json!({"automation": record(&saved, &kept)?}))
}

fn run_of(task: &TaskView, kept: &Kept, done: &RunNowResult) -> Result<Value, Failure> {
    let run = &done.run;
    let status = match run.state {
        RunState::Started => "dispatched",
        RunState::Succeeded => "completed",
        RunState::Failed => "dispatch_failed",
        RunState::Skipped => "skipped_unavailable",
    };
    let began = matches!(run.state, RunState::Started | RunState::Succeeded);
    if run.state == RunState::Failed && run.error.is_none() {
        return Err(bad_answer("schedule.runNow answered a failed run with no error"));
    }
    Ok(json!({
        "id": run.run, "automationId": task.id, "title": task.title, "scheduledFor": run.at_ms, "status": status,
        "trigger": if run.source == RunSource::Manual { "manual" } else { "scheduled" }, "workspaceId": kept.workspace_id, "workspaceDisplayName": null,
        "sessionKind": "chat", "chatSessionId": run.thread, "terminalSessionId": null, "terminalPaneKey": null, "terminalPtyId": null,
        "outputSnapshot": null, "precheckResult": null, "usage": null, "error": run.error,
        "startedAt": began.then_some(run.at_ms), "dispatchedAt": began.then_some(run.at_ms), "createdAt": run.at_ms,
    }))
}

pub fn run_now(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    own_host_only(params)?;
    let (task, kept) = find(agent, id_of(params)?)?;
    let done: RunNowResult = serde_json::from_value(ask(agent, RUN_NOW, json!({"id": task.id}))?).map_err(|e| bad_answer(&format!("schedule.runNow answered no run: {e}")))?;
    Ok(json!({"run": run_of(&task, &kept, &done)?}))
}
