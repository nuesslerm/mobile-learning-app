use super::service::{Refusal, Service};
use crate::domain::state::is_archived;
use crate::ports::{AgentError, Op};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use t3_contracts::wire;

fn upstream(error: AgentError) -> Refusal {
    Refusal {
        status: 502,
        body: wire::host_error(&error.message()),
    }
}

fn public_row(row: &Value) -> Value {
    json!({"id": row["id"], "name": row["name"], "branch": row["branch"], "main": row["main"],
        "automaticName": row["automaticName"], "pinned": row["pinned"] == true})
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CheckoutEdit {
    checkout: String,
    display_name: Option<String>,
    is_pinned: Option<bool>,
}

impl Service {
    pub fn edit_mobile_workspace(&self, auth: Option<&str>, body: Value) -> Result<Value, Refusal> {
        self.scope_gate("mobile.workspaces.edit", auth, "orchestration:operate")?;
        let edit: CheckoutEdit = serde_json::from_value(body).map_err(|_| Refusal::bare(422))?;
        if edit.display_name.is_none() && edit.is_pinned.is_none() {
            return Err(Refusal::bare(422));
        }
        let catalog = self.agent().run(&Op::MobileWorktrees).map_err(upstream)?;
        let rows = catalog["checkouts"]
            .as_array()
            .ok_or_else(|| Refusal::bare(502))?;
        let row = rows
            .iter()
            .find(|r| r["id"] == edit.checkout)
            .ok_or_else(|| Refusal::bare(404))?;
        let _guard = self
            .held()
            .workspace_edits
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let key = format!("orca.worktree.{}", edit.checkout);
        let stored = self
            .agent()
            .run(&Op::StateGet {
                scope: "orca".into(),
                key: key.clone(),
            })
            .map_err(upstream)?;
        let mut meta = match &stored["value"] {
            Value::Null => Map::new(),
            Value::Object(map) => map.clone(),
            _ => return Err(Refusal::bare(502)),
        };
        if let Some(name) = edit.display_name {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                meta.remove("displayName");
            } else {
                meta.insert("displayName".into(), json!(trimmed));
            }
        }
        if let Some(pinned) = edit.is_pinned {
            meta.insert("isPinned".into(), json!(pinned));
        }
        let mut result = public_row(row);
        result["name"] = meta
            .get("displayName")
            .filter(|v| v.as_str().is_some_and(|s| !s.is_empty()))
            .unwrap_or(&row["automaticName"])
            .clone();
        result["pinned"] = json!(meta
            .get("isPinned")
            .and_then(Value::as_bool)
            .unwrap_or(false));
        self.agent()
            .run(&Op::StateSet {
                scope: "orca".into(),
                key,
                value: Value::Object(meta),
            })
            .map_err(upstream)?;
        Ok(result)
    }

    pub fn mobile_workspaces_http(
        &self,
        auth: Option<&str>,
        checkout: Option<&str>,
    ) -> Result<Value, Refusal> {
        self.read_gate("mobile.workspaces", auth)?;
        let catalog = self.agent().run(&Op::MobileWorktrees).map_err(upstream)?;
        let rows = catalog["checkouts"]
            .as_array()
            .ok_or_else(|| Refusal::bare(502))?;
        let Some(id) = checkout else {
            return Ok(
                json!({"host": {"name": self.0.env.label, "os": self.0.env.os}, "repo": catalog["repo"],
                "checkouts": rows.iter().map(public_row).collect::<Vec<_>>()}),
            );
        };
        let row = rows
            .iter()
            .find(|r| r["id"] == id)
            .ok_or_else(|| Refusal::bare(404))?;
        let workspace = row["workspace"]
            .as_str()
            .ok_or_else(|| Refusal::bare(502))?;
        let listed = self.agent().run(&Op::ThreadList).map_err(upstream)?;
        let threads = listed["threads"]
            .as_array()
            .ok_or_else(|| Refusal::bare(502))?;
        let sessions = self.engine().read(|state| {
            threads
                .iter()
                .filter(|t| t["workspace"] == workspace)
                .filter_map(|t| {
                    let id = t["id"].as_str()?;
                    let shell = state.threads.get(id)?;
                    if is_archived(shell)
                        || state.deleted.contains_key(id)
                        || shell
                            .get("worktreePath")
                            .and_then(Value::as_str)
                            .is_some_and(|path| row["path"] != path)
                    {
                        return None;
                    }
                    Some(json!({"id": id, "title": shell.get("title")?}))
                })
                .collect::<Vec<_>>()
        });
        let active = self
            .agent()
            .run(&Op::StateGet {
                scope: "orca".into(),
                key: format!("tabs.active.{workspace}"),
            })
            .map_err(upstream)?;
        let selected = sessions
            .iter()
            .find(|t| t["id"] == active["value"])
            .map(|t| &t["id"]);
        Ok(json!({"active": selected, "sessions": sessions}))
    }
}
