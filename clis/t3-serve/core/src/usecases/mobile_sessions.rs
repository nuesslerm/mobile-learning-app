//! Saved Orca worktree tabs use RCA thread ids. T3 adopts their projections only when selected.
use super::history::EnvironmentRequest;
use super::queries::{field, QueryFailure};
use super::service::{Refusal, Service};
use crate::domain::state::is_archived;
use crate::ports::{AgentError, Op};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use t3_contracts::wire;

struct Tab {
    id: String,
    title: String,
    agent: Option<String>,
    selection: Value,
    needs_projection: bool,
}

struct Context {
    workspace: Option<String>,
    tabs: Vec<Tab>,
}

fn upstream(error: AgentError) -> QueryFailure {
    QueryFailure::Error(wire::host_error(&error.message()))
}

impl Service {
    fn session_tabs(
        &self,
        active: &Map<String, Value>,
        thread: &str,
        saved_tabs: bool,
    ) -> Result<Context, QueryFailure> {
        let listed = self.agent().run(&Op::ThreadList).map_err(upstream)?;
        let rows = listed["threads"]
            .as_array()
            .ok_or_else(|| QueryFailure::Error(wire::host_error("Missing RCA thread list.")))?;
        let workspace = rows
            .iter()
            .find(|r| r["id"] == thread)
            .and_then(|r| r["workspace"].as_str())
            .filter(|name| !name.is_empty());
        let by_id: HashMap<_, _> = rows
            .iter()
            .filter(|r| r["id"] == thread || workspace.is_some_and(|w| r["workspace"] == w))
            .filter_map(|r| Some((r["id"].as_str()?, r)))
            .collect();
        let stored = match workspace.filter(|_| saved_tabs) {
            Some(name) => self
                .agent()
                .run(&Op::StateGet {
                    scope: "orca".into(),
                    key: format!("tabs.extra.{name}"),
                })
                .map_err(upstream)?,
            None => Value::Null,
        };
        let saved = stored["value"].as_array().filter(|tabs| {
            tabs.iter()
                .any(|tab| tab["kind"] == "agent-session" && tab["sessionId"] == thread)
        });
        let tabs = self.engine().read(|state| {
            let tab = |id: &str, native: Option<&Value>| {
                let row = by_id.get(id)?;
                let shell = state.threads.get(id);
                if state.deleted.contains_key(id)
                    || shell.is_some_and(|s| {
                        s.get("projectId") != active.get("projectId") || is_archived(s)
                    })
                {
                    return None;
                }
                if shell.is_none() && native.is_none() {
                    return None;
                }
                let title = native
                    .and_then(|t| t["title"].as_str())
                    .filter(|t| !t.is_empty())
                    .or_else(|| shell.and_then(|s| s.get("title")?.as_str()))
                    .or_else(|| row["title"].as_str())?;
                Some(Tab {
                    id: id.into(),
                    title: title.into(),
                    needs_projection: shell.is_none(),
                    agent: native
                        .and_then(|t| t["agent"].as_str())
                        .or_else(|| row["driver"].as_str())
                        .map(str::to_owned),
                    selection: json!({"instanceId": row["driver"], "model": row["model"]}),
                })
            };
            match saved {
                Some(tabs) => tabs
                    .iter()
                    .filter(|t| t["kind"] == "agent-session")
                    .filter_map(|t| tab(t["sessionId"].as_str()?, Some(t)))
                    .collect(),
                None => rows
                    .iter()
                    .filter_map(|r| tab(r["id"].as_str()?, None))
                    .collect(),
            }
        });
        let checkout = active
            .get("worktreePath")
            .and_then(Value::as_str)
            .and_then(|path| {
                path.trim_end_matches(['/', '\\'])
                    .rsplit(['/', '\\'])
                    .next()
            })
            .filter(|s| !s.is_empty());
        Ok(Context {
            workspace: checkout.or(workspace).map(str::to_owned),
            tabs,
        })
    }

    pub fn session_context_http(
        &self,
        request: &EnvironmentRequest,
        thread: &str,
        saved_tabs: bool,
    ) -> Result<Value, Refusal> {
        const ROUTE: &str = "mobileSessionContext";
        self.environment_gate(ROUTE, request)?;
        let active = self
            .engine()
            .read(|s| s.threads.get(thread).cloned())
            .ok_or_else(|| {
                self.history_refusal(ROUTE, crate::domain::history::HistoryError::ThreadNotFound)
            })?;
        let context = self.session_tabs(&active, thread, saved_tabs).map_err(|e| match e {
            QueryFailure::Error(body) => Refusal { status: 502, body },
            QueryFailure::Defect(_) => Refusal::bare(502),
        })?;
        let sessions: Vec<_> = context
            .tabs
            .into_iter()
            .map(|tab| {
                let mut item = json!({"id": tab.id, "title": tab.title});
                if let Some(agent) = tab.agent {
                    item["agent"] = json!(agent);
                }
                if tab.needs_projection {
                    item["needsProjection"] = json!(true);
                }
                item
            })
            .collect();
        Ok(json!({"workspace":context.workspace, "sessions":sessions}))
    }

    pub fn open_saved_session(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let (thread, target) = (field(payload, "threadId")?, field(payload, "tabId")?);
        let unavailable = || QueryFailure::Error(wire::not_found_error("tab", target));
        let active = self
            .engine()
            .read(|s| s.threads.get(thread).cloned())
            .ok_or_else(unavailable)?;
        let selected = self
            .session_tabs(&active, thread, true)?
            .tabs
            .into_iter()
            .find(|tab| tab.id == target)
            .ok_or_else(unavailable)?;
        self.engine()
            .open_mobile_tab(thread, target, &selected.title, selected.selection)
            .map_err(|m| QueryFailure::Error(wire::host_error(&m)))?;
        Ok(json!({"threadId":target}))
    }
}
