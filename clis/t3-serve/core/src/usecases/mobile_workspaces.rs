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

fn is_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn public_row(row: &Value) -> Value {
    json!({"id": row["id"], "name": row["name"], "branch": row["branch"], "main": row["main"],
        "automaticName": row["automaticName"], "pinned": row["pinned"] == true, "unread": row["unread"] == true})
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CheckoutEdit {
    checkout: String,
    display_name: Option<String>,
    is_pinned: Option<bool>,
    is_unread: Option<bool>,
}

impl Service {
    pub fn checkout_changes_http(&self, auth: Option<&str>, checkout: Option<&str>) -> Result<Value, Refusal> {
        self.read_gate("mobile.checkoutChanges", auth)?;
        let checkout = checkout.filter(|id| !id.is_empty()).ok_or_else(|| Refusal::bare(422))?;
        self.agent()
            .run(&Op::CheckoutChanges { checkout: checkout.into() })
            .map_err(|error| match &error {
                AgentError::Refused { code, .. } if code == "CHECKOUT_NOT_FOUND" => Refusal::bare(404),
                _ => upstream(error),
            })
    }

    pub fn checkout_file_http(
        &self,
        auth: Option<&str>,
        checkout: Option<&str>,
        head: Option<&str>,
        base: Option<&str>,
        path: Option<&str>,
        old_path: Option<&str>,
    ) -> Result<Value, Refusal> {
        self.read_gate("mobile.checkoutFile", auth)?;
        let valid_path = |path: &str| !path.contains('\0') && path.split('/').all(|part| !matches!(part, "" | "." | ".."));
        let checkout = checkout.filter(|id| !id.is_empty()).ok_or_else(|| Refusal::bare(422))?;
        let head = head.filter(|id| is_oid(id)).ok_or_else(|| Refusal::bare(422))?;
        let path = path.filter(|path| valid_path(path)).ok_or_else(|| Refusal::bare(422))?;
        if base.is_some_and(|id| !is_oid(id)) || old_path.is_some_and(|path| !valid_path(path) || base.is_none()) {
            return Err(Refusal::bare(422));
        }
        self.agent()
            .run(&Op::CheckoutFile {
                checkout: checkout.into(),
                head: head.to_ascii_lowercase(),
                base: base.map(str::to_ascii_lowercase),
                path: path.into(),
                old_path: old_path.map(str::to_owned),
            })
            .map_err(|error| match &error {
                AgentError::Refused { code, .. } if code == "CHECKOUT_NOT_FOUND" || code == "FILE_NOT_FOUND" => Refusal::bare(404),
                _ => upstream(error),
            })
    }

    pub fn checkout_commit_http(&self, auth: Option<&str>, checkout: Option<&str>, sha: Option<&str>) -> Result<Value, Refusal> {
        self.read_gate("mobile.checkoutCommit", auth)?;
        let checkout = checkout.filter(|id| !id.is_empty()).ok_or_else(|| Refusal::bare(422))?;
        let sha = sha.filter(|sha| is_oid(sha)).ok_or_else(|| Refusal::bare(422))?;
        self.agent()
            .run(&Op::CheckoutCommit {
                checkout: checkout.into(),
                sha: sha.to_ascii_lowercase(),
            })
            .map_err(|error| match &error {
                AgentError::Refused { code, .. } if code == "CHECKOUT_NOT_FOUND" || code == "COMMIT_NOT_FOUND" => Refusal::bare(404),
                _ => upstream(error),
            })
    }

    pub fn checkout_history_http(
        &self,
        auth: Option<&str>,
        checkout: Option<&str>,
        head: Option<&str>,
        base: Option<&str>,
        offset: Option<&str>,
    ) -> Result<Value, Refusal> {
        self.read_gate("mobile.checkoutHistory", auth)?;
        let checkout = checkout.filter(|id| !id.is_empty()).ok_or_else(|| Refusal::bare(422))?;
        let offset = offset.unwrap_or("0").parse::<u32>().map_err(|_| Refusal::bare(422))?;
        if (offset > 0 && head.is_none()) || head.is_some_and(|s| !is_oid(s)) || base.is_some_and(|s| !is_oid(s)) {
            return Err(Refusal::bare(422));
        }
        self.agent()
            .run(&Op::CheckoutHistory {
                checkout: checkout.into(),
                expected_head: head.map(str::to_ascii_lowercase),
                base: base.map(str::to_ascii_lowercase),
                offset,
            })
            .map_err(|error| match &error {
                AgentError::Refused { code, .. } if code == "CHECKOUT_NOT_FOUND" => Refusal::bare(404),
                AgentError::Refused { code, .. } if code == "CHECKOUT_HEAD_CHANGED" => Refusal::bare(409),
                _ => upstream(error),
            })
    }

    pub fn edit_mobile_workspace(&self, auth: Option<&str>, body: Value) -> Result<Value, Refusal> {
        self.scope_gate("mobile.workspaces.edit", auth, "orchestration:operate")?;
        let edit: CheckoutEdit = serde_json::from_value(body).map_err(|_| Refusal::bare(422))?;
        if edit.display_name.is_none() && edit.is_pinned.is_none() && edit.is_unread.is_none() {
            return Err(Refusal::bare(422));
        }
        let _guard = self
            .held()
            .workspace_edits
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let catalog = self.agent().run(&Op::MobileWorktrees).map_err(upstream)?;
        let rows = catalog["checkouts"]
            .as_array()
            .ok_or_else(|| Refusal::bare(502))?;
        let row = rows
            .iter()
            .find(|r| r["id"] == edit.checkout)
            .ok_or_else(|| Refusal::bare(404))?;
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
        let pending = row["unreadPanes"].as_array().map(Vec::as_slice).unwrap_or_default();
        if let Some(unread) = edit.is_unread {
            meta.insert("isUnread".into(), json!(unread));
        }
        let mut result = public_row(row);
        result["unread"] = json!(meta.get("isUnread") == Some(&Value::Bool(true)) || (edit.is_unread != Some(false) && !pending.is_empty()));
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
        if edit.is_unread == Some(false) && !pending.is_empty() {
            let panes = pending.iter().map(|k| k.as_str().map(str::to_owned).ok_or_else(|| Refusal::bare(502))).collect::<Result<Vec<_>, _>>()?;
            self.agent().run(&Op::ReadPanes { panes }).map_err(upstream)?;
        }
        Ok(result)
    }

    pub fn mobile_home_http(&self, auth: Option<&str>) -> Result<Value, Refusal> {
        self.read_gate("mobile.home", auth)?;
        let catalog = self.agent().run(&Op::WorktreeList { root: String::new() }).map_err(upstream)?;
        let worktrees = catalog["worktrees"].as_array().ok_or_else(|| Refusal::bare(502))?;
        Ok(json!({
            "host": { "name": self.0.env.label, "os": self.0.env.os },
            "worktreeCount": worktrees.len()
        }))
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
