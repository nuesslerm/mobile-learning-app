//! A double of rca's `schedules` group: schedules in memory, saved and run as the service does. A save is read by the contract's own parser, so a request
//! without what the contract requires is refused as the service refuses it, never read as an empty value.
use agent_api_schedules::{RunNowResult, RunSource, RunState, RunStatus, RunView, Schedule, TaskView, UpsertParams};
use orca_core::testing::ScriptedAgent;
use orca_core::AgentError;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

/// The double's clock: every save and run happens at this instant.
pub const NOW_MS: u64 = 1_790_942_400_000;

#[derive(Default)]
struct State {
    tasks: Vec<TaskView>,
    runs: u64,
    run_error: Option<String>,
}

#[derive(Clone, Default)]
pub struct Schedules(Arc<Mutex<State>>);

fn refused(code: &str, message: impl Into<String>) -> AgentError {
    AgentError::Refused { code: code.into(), message: message.into() }
}

fn due(schedule: &Schedule) -> Option<u64> {
    match schedule {
        Schedule::Interval { every_ms } => Some(NOW_MS + every_ms),
        Schedule::FixedTime { .. } => Some(NOW_MS + 3_600_000),
        Schedule::Webhook {} => None,
    }
}

impl Schedules {
    /// A schedule another client made (no `orca` meta), as the T3 mapper keeps its own.
    pub fn foreign(&self, id: &str, title: &str) -> &Self {
        self.0.lock().unwrap().tasks.push(view(id, title, true, Schedule::Interval { every_ms: 3_600_000 }, "w", "codex", Some(json!({"t3": {"project": "p"}}))));
        self
    }

    /// Every schedule as the group lists it now.
    pub fn tasks(&self) -> Vec<TaskView> {
        self.0.lock().unwrap().tasks.clone()
    }

    /// Makes the next `runNow` end as a failed run with `error`.
    pub fn next_run_fails(&self, error: &str) {
        self.0.lock().unwrap().run_error = Some(error.into());
    }

    /// Makes `agent` answer the methods of `USES` from these schedules.
    pub fn install(&self, agent: &ScriptedAgent) {
        let me = self.clone();
        agent.reply("schedule.list", move |_| Ok(json!({"tasks": me.tasks()})));
        let me = self.clone();
        agent.reply("schedule.upsert", move |p| me.upsert(p));
        let me = self.clone();
        agent.reply("schedule.delete", move |p| me.delete(p));
        let me = self.clone();
        agent.reply("schedule.runNow", move |p| me.run_now(p));
    }

    fn upsert(&self, p: &Value) -> Result<Value, AgentError> {
        let params = UpsertParams::parse(p).map_err(|e| refused("INVALID_PARAMS", e.0))?;
        let mut s = self.0.lock().unwrap();
        let id = match &params.id {
            Some(id) => id.clone(),
            None => format!("sched-{}", s.tasks.len() + 1),
        };
        let at = s.tasks.iter().position(|t| t.id == id);
        if at.is_none() && params.require_existing == Some(true) {
            return Err(refused("SCHEDULE_NOT_FOUND", format!("no schedule has the id {id}")));
        }
        let mut task = view(&id, &params.title, params.enabled, params.schedule, &params.workspace, &params.driver, params.meta);
        task.prompt = params.prompt;
        if let Some(i) = at {
            task.created_at_ms = s.tasks[i].created_at_ms;
            s.tasks[i] = task.clone();
        } else {
            s.tasks.push(task.clone());
        }
        Ok(json!({"task": task}))
    }

    fn delete(&self, p: &Value) -> Result<Value, AgentError> {
        let id = p["id"].as_str().ok_or_else(|| refused("INVALID_PARAMS", "id is required"))?;
        let mut s = self.0.lock().unwrap();
        let before = s.tasks.len();
        s.tasks.retain(|t| t.id != id);
        if s.tasks.len() == before {
            return Err(refused("SCHEDULE_NOT_FOUND", format!("no schedule has the id {id}")));
        }
        Ok(json!({"id": id}))
    }

    fn run_now(&self, p: &Value) -> Result<Value, AgentError> {
        let id = p["id"].as_str().ok_or_else(|| refused("INVALID_PARAMS", "id is required"))?;
        let mut s = self.0.lock().unwrap();
        s.runs += 1;
        let (n, error) = (s.runs, s.run_error.take());
        let task = s.tasks.iter_mut().find(|t| t.id == id).ok_or_else(|| refused("SCHEDULE_NOT_FOUND", format!("no schedule has the id {id}")))?;
        let state = if error.is_some() { RunState::Failed } else { RunState::Started };
        task.last_run_ms = Some(NOW_MS);
        task.last_run_status = if error.is_some() { RunStatus::Failed } else { RunStatus::Running };
        task.last_run_error = error.clone();
        task.run_count += 1;
        let run = RunView { run: format!("run-{n}"), schedule: id.into(), source: RunSource::Manual, at_ms: NOW_MS, state, thread: error.is_none().then(|| format!("thread-{n}")), turn: None, error };
        Ok(serde_json::to_value(RunNowResult { task: task.clone(), run }).unwrap())
    }
}

fn view(id: &str, title: &str, enabled: bool, schedule: Schedule, workspace: &str, driver: &str, meta: Option<Value>) -> TaskView {
    TaskView {
        id: id.into(),
        title: title.into(),
        prompt: "p".into(),
        enabled,
        next_run_ms: enabled.then(|| due(&schedule)).flatten(),
        schedule,
        workspace: workspace.into(),
        driver: driver.into(),
        model: None,
        thread: None,
        runtime_mode: None,
        interaction_mode: None,
        created_at_ms: NOW_MS,
        updated_at_ms: NOW_MS,
        last_run_ms: None,
        last_run_status: RunStatus::Never,
        last_run_error: None,
        run_count: 0,
        webhook: None,
        meta,
    }
}
