//! The reads and stops of M4 that are not commands: the thread family, search, the older settled shelf, turn items, command output, the force
//! stop of a thread and of a workflow, and the two snapshots a phone fetches over HTTP. What the agent API knows (the family, the search, the
//! shelf) is asked of it; what t3 keeps as a cache of the thread's log (items, archived rows, snapshots) is read from its rows.
use super::history::EnvironmentRequest;
use super::service::{Refusal, Service};
use crate::ports::{AgentError, Op};
use serde_json::{json, Map, Value};
use t3_contracts::time::{iso, parse_iso};
use t3_contracts::wire;

/// Why a query did not answer: a payload the decoder refuses (a defect on the wire) or the error the RPC declares.
#[derive(Debug, PartialEq)]
pub enum QueryFailure {
    Defect(String),
    Error(Value),
}

/// The most rows of the shelf one request of the agent API asks for.
const SHELF_PAGE: u32 = 50;
const SEARCH_DEFAULT: u32 = 20;

pub(super) fn field<'a>(payload: &'a Value, key: &str) -> Result<&'a str, QueryFailure> {
    payload.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or_else(|| QueryFailure::Defect(format!("Missing key\n  at [\"{key}\"]")))
}

fn failed(e: AgentError) -> QueryFailure {
    QueryFailure::Error(wire::host_error(&e.message()))
}

/// The error an RPC declares as `tag`, with the agent API's refusal as its message and `extra`'s fields beside it.
pub(super) fn failed_with(tag: &str, e: &AgentError, extra: Value) -> QueryFailure {
    let mut error = json!({"_tag": tag, "message": e.message()});
    if let (Some(into), Some(from)) = (error.as_object_mut(), extra.as_object()) {
        into.extend(from.clone());
    }
    QueryFailure::Error(error)
}

impl Service {
    fn row(&self, thread: &str) -> Option<Map<String, Value>> {
        self.engine().read(|state| state.threads.get(thread).cloned())
    }

    fn known_thread(&self, payload: &Value) -> Result<String, QueryFailure> {
        let thread = field(payload, "threadId")?;
        self.row(thread).map(|_| thread.to_string()).ok_or_else(|| QueryFailure::Error(wire::not_found_error("thread", thread)))
    }

    /// `orchestration.getTurnItem`: the row the thread's log made, or null once it is gone.
    pub fn turn_item(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let (thread, item) = (self.known_thread(payload)?, field(payload, "itemId")?);
        self.sync_thread(&thread);
        Ok(json!({"item": self.engine().read(|state| state.entities.get(&thread).and_then(|e| e.turn_items.iter().find(|i| i["id"] == item).cloned()))}))
    }

    /// `orchestration.searchThreads`: the agent API's matches, with the project of each thread. A title match reads as a user match on the title.
    pub fn search_threads(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let query = field(payload, "query")?.trim();
        if query.chars().count() < 2 {
            return Err(QueryFailure::Defect("Expected a string at least 2 characters long".into()));
        }
        let limit = payload.get("limit").and_then(Value::as_u64).map_or(SEARCH_DEFAULT, |l| l.clamp(1, 50) as u32);
        let found = self.agent().run(&Op::ThreadSearch { query: query.to_string(), limit }).map_err(failed)?;
        let matches: Vec<Value> = found["matches"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|hit| {
                let thread = hit["thread"].as_str()?;
                let row = self.row(thread)?;
                let source = if hit["source"] == "assistant" { "assistant" } else { "user" };
                let snippet = hit["snippet"].as_str().or_else(|| hit["title"].as_str()).unwrap_or_default();
                let at = hit["at_ms"].as_i64().map_or(Value::Null, |ms| json!(iso(ms)));
                Some(json!({"threadId": thread, "projectId": row["projectId"], "source": source, "snippet": snippet.chars().take(240).collect::<String>(), "messageCreatedAt": at}))
            })
            .collect();
        Ok(json!({"matches": matches, "snapshotSequence": self.engine().read(|s| s.last_sequence)}))
    }

    /// `orchestration.getThreadFamily`: the live threads of the thread's family, from the agent API's lineage, as t3's shell rows.
    pub fn thread_family(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let thread = field(payload, "threadId")?;
        let sequence = self.engine().read(|s| s.last_sequence);
        if self.row(thread).is_none() {
            return Ok(json!({"snapshotSequence": sequence, "threads": []}));
        }
        if let Some(agent_thread) = self.thread_for_agent(thread) {
            self.agent().ensure_thread(&agent_thread).map_err(failed)?;
        }
        let family = self.agent().run(&Op::ThreadFamily { thread: thread.to_string() }).map_err(failed)?;
        let rows: Vec<Value> = family["threads"].as_array().into_iter().flatten().filter_map(|t| t["thread"]["id"].as_str().and_then(|id| self.row(id))).map(Value::Object).collect();
        Ok(json!({"snapshotSequence": sequence, "threads": rows}))
    }

    /// `orchestration.getOlderSettledThreads`: pages the agent API's settled shelf below the cursor until `limit` rows of the asked projects are
    /// found, counting across pages; with `limit` 0 only the per-project counts.
    pub fn older_settled(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let before = payload.get("before").filter(|b| b.is_object()).ok_or_else(|| QueryFailure::Defect("Missing key\n  at [\"before\"]".into()))?;
        let at = before["settledAt"].as_str().and_then(parse_iso).and_then(|ms| u64::try_from(ms).ok()).ok_or_else(|| QueryFailure::Defect("Expected an ISO date at [\"before\"][\"settledAt\"]".into()))?;
        let limit = payload.get("limit").and_then(Value::as_u64).filter(|l| *l <= 50).ok_or_else(|| QueryFailure::Defect("Expected an integer between 0 and 50 at [\"limit\"]".into()))? as usize;
        let projects: Option<Vec<&str>> = payload.get("projectIds").and_then(Value::as_array).map(|p| p.iter().filter_map(Value::as_str).collect());
        let wanted = |row: &Map<String, Value>| projects.as_ref().is_none_or(|p| row["projectId"].as_str().is_some_and(|id| p.contains(&id)));
        let mut cursor = Some((at, before["threadId"].as_str().map(str::to_string)));
        let (mut rows, mut counts): (Vec<Value>, Vec<(String, u64)>) = (vec![], vec![]);
        while let Some(page_from) = cursor.take() {
            let page = self.agent().run(&Op::ThreadShelf { shelf: "settled", before: Some(page_from), limit: SHELF_PAGE }).map_err(failed)?;
            for entry in page["threads"].as_array().into_iter().flatten() {
                let Some(row) = entry["thread"]["id"].as_str().and_then(|id| self.row(id)).filter(&wanted) else { continue };
                if limit == 0 {
                    let project = row["projectId"].as_str().unwrap_or_default().to_string();
                    match counts.iter_mut().find(|(p, _)| *p == project) {
                        Some((_, n)) => *n += 1,
                        None => counts.push((project, 1)),
                    }
                } else if rows.len() < limit {
                    rows.push(Value::Object(row));
                }
            }
            let next = page["next"]["at_ms"].as_u64().map(|ms| (ms, page["next"]["thread"].as_str().map(str::to_string)));
            if limit == 0 || rows.len() < limit {
                cursor = next;
            }
        }
        let counts: Vec<Value> = counts.into_iter().map(|(project, count)| json!({"projectId": project, "count": count})).collect();
        Ok(json!({"snapshotSequence": self.engine().read(|s| s.last_sequence), "threads": rows, "counts": counts}))
    }

    /// `orchestration.getArchivedShellSnapshot`: the archived threads and the projects they are in.
    pub fn archived_shell(&self) -> Value {
        self.engine().read(|state| {
            let snapshot = &state.shell_snapshot()["snapshot"];
            json!({"schemaVersion": snapshot["schemaVersion"], "snapshotSequence": snapshot["snapshotSequence"], "projects": snapshot["projects"], "threads": snapshot["archivedThreads"]})
        })
    }

    /// `orchestration.forceStopThread`: the agent API ends the CLI process now and holds the thread's queue.
    pub fn force_stop(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let thread = self.known_thread(payload)?;
        let turn = payload.get("runId").and_then(Value::as_str).map(|run| self.turn_of_run(run).unwrap_or_else(|| run.to_string()));
        let stopped = self.agent().run(&Op::ThreadForceStop { thread, turn }).map_err(failed)?;
        Ok(json!({"outcome": stopped["outcome"], "sharedWithThreadIds": stopped["shared_with"]}))
    }

    /// `orchestration.stopWorkflow`: stops what the thread's running turn runs in the background.
    pub fn stop_workflow(&self, payload: &Value) -> Result<(), QueryFailure> {
        let (thread, workflow) = (self.known_thread(payload)?, field(payload, "subagentId")?);
        let turn = self.engine().active_run(&thread).map(|run| self.turn_of_run(&run).unwrap_or(run));
        let Some(turn) = turn else { return Err(QueryFailure::Error(json!({"_tag": "OrchestrationV2StopWorkflowError", "threadId": thread, "subagentId": workflow, "reason": "not-running", "message": "This workflow is no longer running."}))) };
        self.agent().run(&Op::WorkflowStop { thread, turn, workflow: Some(workflow.to_string()) }).map(drop).map_err(failed)
    }

    /// `orchestration.getWorkflowScript`: scripts live in the CLI's own projects directory, which no agent API method reaches.
    pub fn workflow_script(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let (thread, path) = (self.known_thread(payload)?, field(payload, "scriptPath")?);
        let (reason, message) = if path.starts_with('/') && path.ends_with(".js") { ("root-unavailable", "Script root unavailable.") } else { ("invalid-path", "Workflow scripts must be absolute .js paths.") };
        self.log("workflow.script.refused", &[("thread", thread), ("reason", reason.to_string())]);
        Err(QueryFailure::Error(json!({"_tag": "OrchestrationWorkflowFileError", "reason": reason, "path": path, "message": message})))
    }

    /// `secrets.answerRequest`: the agent API never asks for a secret, so no request exists to answer, and what was sent goes nowhere.
    pub fn answer_secret(&self, payload: &Value) -> Result<(), QueryFailure> {
        field(payload, "threadId")?;
        field(payload, "turnItemId")?;
        let answer = &payload["answer"];
        let valid = match answer["type"].as_str() {
            Some("save") => answer["secret"].as_str().is_some_and(|s| !s.trim().is_empty()),
            Some("decline") => true,
            _ => false,
        };
        if !valid {
            return Err(QueryFailure::Defect("Expected the answer to be save with a secret or decline at [\"answer\"]".into()));
        }
        self.log("secret.answer.unknown", &[("thread", payload["threadId"].as_str().unwrap_or_default().to_string())]);
        Err(QueryFailure::Error(json!({"_tag": "SecretRequestError", "reason": "not_found", "message": "This secret request no longer exists."})))
    }

    /// `orchestration.subscribeCommandOutput`: the frames of a command's output: the whole text as one replace, running only while the command is.
    pub fn command_output(&self, payload: &Value) -> Result<Vec<Value>, QueryFailure> {
        let (thread, item) = (self.known_thread(payload)?, field(payload, "itemId")?);
        self.sync_thread(&thread);
        let found = self.engine().read(|state| state.entities.get(&thread).and_then(|e| e.turn_items.iter().find(|i| i["id"] == item && i["type"] == "command_execution").cloned()));
        let Some(found) = found else { return Err(QueryFailure::Error(json!({"_tag": "OrchestrationV2CommandOutputError", "threadId": thread, "message": format!("{item} is not a command of this thread.")}))) };
        let running = found["status"] == "running";
        Ok(vec![json!({"kind": "replace", "text": found["output"].as_str().unwrap_or_default(), "truncated": false, "running": running})])
    }

    /// `GET /api/orchestration/shell`: the shell snapshot as the stream's first frame holds it.
    pub fn shell_snapshot_http(&self, request: &EnvironmentRequest) -> Result<Value, Refusal> {
        self.environment_gate("shellSnapshot", request)?;
        Ok(self.engine().read(|state| state.shell_snapshot()["snapshot"].clone()))
    }

    /// `GET /api/orchestration/threads/:threadId/bounded`: the thread's projection whole, which is no bigger than a window of it here.
    pub fn thread_bounded_snapshot(&self, request: &EnvironmentRequest, thread: &str) -> Result<Value, Refusal> {
        self.environment_gate("threadBoundedSnapshot", request)?;
        self.sync_thread(thread);
        let (projection, sequence) = self.engine().read(|state| (state.thread_projection(thread), state.last_sequence));
        match projection {
            Some(projection) => Ok(json!({"snapshotSequence": sequence, "projection": projection, "hasMoreHistory": false})),
            None => Err(self.history_refusal("threadBoundedSnapshot", crate::domain::history::HistoryError::ThreadNotFound)),
        }
    }
}
