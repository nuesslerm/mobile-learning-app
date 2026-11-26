use super::service::{Refusal, Service};
use crate::domain::state::is_archived;
use crate::ports::{AgentError, Op};
use serde_json::{json, Value};
use t3_contracts::wire;

fn upstream(error: AgentError) -> Refusal {
    Refusal { status: 502, body: wire::host_error(&error.message()) }
}

impl Service {
    pub fn mobile_workspaces_http(&self, auth: Option<&str>, checkout: Option<&str>) -> Result<Value, Refusal> {
        self.read_gate("mobile.workspaces", auth)?;
        let catalog = self.agent().run(&Op::MobileWorktrees).map_err(upstream)?;
        let rows = catalog["checkouts"].as_array().ok_or_else(|| Refusal::bare(502))?;
        let Some(id) = checkout else {
            return Ok(json!({"host": {"name": self.0.env.label, "os": self.0.env.os}, "repo": catalog["repo"],
                "checkouts": rows.iter().map(|r| json!({"id": r["id"], "name": r["name"], "main": r["main"], "branch": r["branch"]})).collect::<Vec<_>>()}));
        };
        let row = rows.iter().find(|r| r["id"] == id).ok_or_else(|| Refusal::bare(404))?;
        let workspace = row["workspace"].as_str().ok_or_else(|| Refusal::bare(502))?;
        let listed = self.agent().run(&Op::ThreadList).map_err(upstream)?;
        let threads = listed["threads"].as_array().ok_or_else(|| Refusal::bare(502))?;
        let sessions = self.engine().read(|state| threads.iter().filter(|t| t["workspace"] == workspace).filter_map(|t| {
            let id = t["id"].as_str()?;
            let shell = state.threads.get(id)?;
            if is_archived(shell) || state.deleted.contains_key(id) ||
                shell.get("worktreePath").and_then(Value::as_str).is_some_and(|path| row["path"] != path) {
                return None;
            }
            Some(json!({"id": id, "title": shell.get("title")?}))
        }).collect::<Vec<_>>());
        let active = self.agent().run(&Op::StateGet { scope: "orca".into(), key: format!("tabs.active.{workspace}") }).map_err(upstream)?;
        let selected = sessions.iter().find(|t| t["id"] == active["value"]).map(|t| &t["id"]);
        Ok(json!({"active": selected, "sessions": sessions}))
    }
}
