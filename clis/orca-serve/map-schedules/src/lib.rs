//! Orca's automation rows over rca's `schedules` group: `automation.list`, `show`, `create`, `update`, `delete` and `runNow`. An automation is an rca schedule
//! whose `meta` holds an `orca` object (what the phone sent that rca has no field for: the project and workspace it named, its recurrence text, timezone and
//! start), so a schedule another client made is not listed. rca runs a prompt in an existing workspace at a time of day on some weekdays at a fixed UTC offset;
//! what it cannot run (a worktree per run, a precheck command, an hourly or minute recurrence, a timezone that needs a database, a reused session, a task source,
//! a grace for missed runs) is refused by name and never saved as something else. `automation.runs` stays pending: rca keeps a schedule's last run, no history.
use agent_api_proto::Method as AgentMethod;
use agent_api_schedules::{DELETE, LIST, RUN_NOW, UPSERT};
use orca_core::wire::{Failure, METHOD_NOT_FOUND, RUNTIME_ERROR};
use orca_core::{AgentError, Area, Ctx, Kind, Method};
use serde_json::{json, Value};

mod automation;
mod recurrence;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

/// The agent methods this crate sends.
pub const USES: &[AgentMethod] = &[LIST, UPSERT, DELETE, RUN_NOW];

const METHODS: &[Method] = &[
    Method { name: "automation.list", kind: Kind::Unary },
    Method { name: "automation.show", kind: Kind::Unary },
    Method { name: "automation.create", kind: Kind::Unary },
    Method { name: "automation.update", kind: Kind::Unary },
    Method { name: "automation.delete", kind: Kind::Unary },
    Method { name: "automation.runNow", kind: Kind::Unary },
];

pub struct SchedulesArea;

impl Area for SchedulesArea {
    fn name(&self) -> &'static str {
        "schedules"
    }

    fn methods(&self) -> &'static [Method] {
        METHODS
    }

    fn call(&self, ctx: &mut Ctx, method: &str, params: &Value) -> Result<Value, Failure> {
        let agent = ctx.agent;
        match method {
            "automation.list" => automation::list(agent, params),
            "automation.show" => automation::show(agent, params),
            "automation.create" => automation::create(agent, params),
            "automation.update" => automation::update(agent, params),
            "automation.delete" => automation::delete(agent, params),
            "automation.runNow" => automation::run_now(agent, params),
            other => Err(Failure::new(METHOD_NOT_FOUND, format!("Unknown method: {other}"))),
        }
    }
}

pub(crate) fn refusal(error: AgentError) -> Failure {
    match error {
        AgentError::Refused { code, message } if code == "INVALID_PARAMS" => Failure::invalid(message),
        AgentError::Refused { message, .. } => Failure::new(RUNTIME_ERROR, message),
        AgentError::Failed(_) => Failure::new("runtime_unavailable", "runtime_unavailable"),
    }
}

pub(crate) fn ask(agent: &dyn orca_core::AgentApi, method: AgentMethod, params: Value) -> Result<Value, Failure> {
    agent.request(method.name(), params).map_err(refusal)
}

pub(crate) fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty())
}

pub(crate) fn not_found() -> Failure {
    Failure::new(RUNTIME_ERROR, "Automation not found.")
}

pub(crate) fn ok_delete(id: &str) -> Value {
    json!({"removed": true, "id": id})
}
