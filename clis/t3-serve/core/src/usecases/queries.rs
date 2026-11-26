//! The reads and stops of M4 that are not commands: the thread family, search, the older settled shelf, turn items, command output, the force
//! stop of a thread and of a workflow, and the two snapshots a phone fetches over HTTP. What the agent API knows (the family, the search, the
//! shelf) is asked of it; what t3 keeps as a cache of the thread's log (items, archived rows, snapshots) is read from its rows.
use super::history::EnvironmentRequest;
use super::service::{Refusal, Service};
use crate::domain::state::is_deleted;
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
/// 9999-12-31T23:59:59Z, the latest date the shelf can be asked below.
const LATEST_INSTANT_MS: u64 = 253_402_300_799_000;

pub(super) fn field<'a>(payload: &'a Value, key: &str) -> Result<&'a str, QueryFailure> {
    payload.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or_else(|| QueryFailure::Defect(format!("Missing key\n  at [\"{key}\"]")))
}

/// The most bytes of a workflow script `orchestration.getWorkflowScript` returns.
const WORKFLOW_SCRIPT_CAP: u64 = 256 * 1024;

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

    pub(super) fn thread_is_held(&self, thread: &str) -> bool {
        self.row(thread).is_some()
    }

    fn known_thread(&self, payload: &Value) -> Result<String, QueryFailure> {
        let thread = field(payload, "threadId")?;
        self.row(thread).map(|_| thread.to_string()).ok_or_else(|| QueryFailure::Error(wire::not_found_error("thread", thread)))
    }

    /// The thread of `payload` as the RPC's own error `tag` names its absence, with `extra`'s fields beside the cause.
    fn thread_or_missing(&self, payload: &Value, tag: &str, extra: Value) -> Result<String, QueryFailure> {
        let thread = field(payload, "threadId")?;
        self.row(thread).map(|_| thread.to_string()).ok_or_else(|| QueryFailure::Error(wire::thread_missing_error(tag, thread, extra)))
    }

    /// `orchestration.getTurnItem`: the row the thread's log made, or null once it is gone (as it is for a thread that never was).
    pub fn turn_item(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let (thread, item) = (field(payload, "threadId")?.to_string(), field(payload, "itemId")?);
        if self.row(&thread).is_none() {
            return Ok(json!({"item": null}));
        }
        self.sync_thread(&thread);
        Ok(json!({"item": self.engine().read(|state| state.entities.get(&thread).and_then(|e| e.turn_items.iter().find(|i| i["id"] == item).map(crate::domain::state::turn_item_wire)))}))
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

    /// The thread ids of `threadIds`, a decode defect for anything else.
    fn requested_ids(payload: &Value) -> Result<Vec<&str>, QueryFailure> {
        let ids = payload.get("threadIds").and_then(Value::as_array).ok_or_else(|| QueryFailure::Defect("Missing key\n  at [\"threadIds\"]".into()))?;
        ids.iter().map(|id| id.as_str().filter(|s| !s.is_empty()).ok_or_else(|| QueryFailure::Defect("Expected a non-empty string at [\"threadIds\"][*]".into()))).collect()
    }

    /// `orchestration.getThreadTitles`: the titles of the asked threads t3 holds, archived ones included; unknown ids are absent.
    pub fn thread_titles(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let titles: Map<String, Value> = Self::requested_ids(payload)?.into_iter().filter_map(|id| self.row(id).map(|row| (id.to_string(), row["title"].clone()))).collect();
        Ok(Value::Object(titles))
    }

    /// `orchestration.getThreadLinkMetadata`: title and project of each asked thread t3 holds.
    pub fn thread_link_metadata(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let meta: Map<String, Value> = Self::requested_ids(payload)?.into_iter().filter_map(|id| self.row(id).map(|row| (id.to_string(), json!({"title": row["title"], "projectId": row["projectId"]})))).collect();
        Ok(Value::Object(meta))
    }

    /// `orchestration.getNativeChildWork`: the work of an Antigravity subagent's children. rca runs no provider-native subagents, so a thread t3
    /// holds has none (the pinned server answers the same for any thread that is not one), and an unknown thread is the query's declared error.
    pub fn native_child_work(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let thread = field(payload, "threadId")?;
        if self.row(thread).is_none() {
            return Err(QueryFailure::Error(wire::thread_missing_error("OrchestrationV2GetNativeChildWorkError", thread, json!({"threadId": thread}))));
        }
        Ok(json!({"children": []}))
    }

    /// `orchestration.subscribePromptSuggestion`: the predicted next prompt of a run, which rca does not predict, so the stream holds the one
    /// frame the pinned server sends when there is none (null), for a thread t3 holds or not.
    pub fn prompt_suggestion(&self, payload: &Value) -> Result<Vec<Value>, QueryFailure> {
        field(payload, "threadId")?;
        Ok(vec![Value::Null])
    }

    /// `orchestration.getOlderSettledThreads`: pages the agent API's settled shelf below the cursor until `limit` rows of the asked projects are
    /// found, counting across pages; with `limit` 0 only the per-project counts.
    pub fn older_settled(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let before = payload.get("before").filter(|b| b.is_object()).ok_or_else(|| QueryFailure::Defect("Missing key\n  at [\"before\"]".into()))?;
        let settled = before["settledAt"].as_str().ok_or_else(|| QueryFailure::Defect("Expected a string at [\"before\"][\"settledAt\"]".into()))?;
        // The pin compares the text: what is no date sorts after every date when it starts above the digits, and before them otherwise
        let at = parse_iso(settled).and_then(|ms| u64::try_from(ms).ok()).unwrap_or(if settled > "9" { LATEST_INSTANT_MS } else { 0 });
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
        let thread = self.thread_or_missing(payload, "OrchestrationV2ForceStopThreadError", json!({"threadId": field(payload, "threadId")?}))?;
        let turn = payload.get("runId").and_then(Value::as_str).map(|run| self.turn_of_started_run(run).unwrap_or_else(|| run.to_string()));
        let stopped = self.1.force_stop(&thread, turn).map_err(failed)?;
        Ok(json!({"outcome": stopped["outcome"], "sharedWithThreadIds": stopped["shared_with"]}))
    }

    /// `orchestration.stopWorkflow`: stops what the thread's running turn runs in the background.
    pub fn stop_workflow(&self, payload: &Value) -> Result<(), QueryFailure> {
        let (asked, workflow) = (field(payload, "threadId")?, field(payload, "subagentId")?);
        let thread = self.thread_or_missing(payload, "OrchestrationV2StopWorkflowError", json!({"threadId": asked, "subagentId": workflow, "reason": "unavailable"}))?;
        let turn = self.engine().active_run(&thread).map(|run| self.turn_of_started_run(&run).unwrap_or(run));
        let Some(turn) = turn else { return Err(QueryFailure::Error(json!({"_tag": "OrchestrationV2StopWorkflowError", "threadId": thread, "subagentId": workflow, "reason": "not-running", "message": "This workflow is no longer running."}))) };
        self.agent().run(&Op::WorkflowStop { thread, turn, workflow: Some(workflow.to_string()) }).map(drop).map_err(failed)
    }

    /// `orchestration.getWorkflowScript`: a script below the executor's `~/.claude/projects`, at most 256 KiB of it.
    pub fn workflow_script(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let (asked, path) = (field(payload, "threadId")?, field(payload, "scriptPath")?);
        let refused = |reason: &str, message: &str| QueryFailure::Error(json!({"_tag": "OrchestrationGetWorkflowScriptError", "cause": {"message": message, "name": "OrchestrationWorkflowFileError"}, "reason": reason, "scriptPath": path}));
        if !(path.starts_with('/') && path.ends_with(".js")) {
            self.log("workflow.script.refused", &[("thread", asked.to_string()), ("reason", "invalid-path".to_string())]);
            return Err(refused("invalid-path", "Workflow files must be absolute paths with the expected extension."));
        }
        self.known_thread(payload)?;
        match self.agent().run(&Op::WorkflowScript { path: path.to_string(), length: WORKFLOW_SCRIPT_CAP }) {
            Ok(read) => {
                use base64::Engine;
                let bytes = base64::engine::general_purpose::STANDARD.decode(read["data"].as_str().unwrap_or_default()).unwrap_or_default();
                Ok(json!({"scriptPath": path, "contents": String::from_utf8_lossy(&bytes), "truncated": read["eof"] != json!(true)}))
            }
            Err(e) => {
                let (reason, message) = match &e {
                    AgentError::Refused { code, message } if code == "NOT_FOUND" && message.contains("root is unavailable") => ("root-unavailable", "Workflow file root unavailable."),
                    AgentError::Refused { code, .. } if code == "NOT_FOUND" => ("not-found", "Workflow file not found."),
                    AgentError::Refused { code, .. } if code == "PATH_OUTSIDE_ROOT" => ("outside-root", "Path is outside the workflow files root."),
                    AgentError::Refused { code, .. } if code == "NOT_A_FILE" => ("not-regular-file", "Workflow file is not a regular file."),
                    _ => ("read-failed", "Workflow file read failed."),
                };
                self.log("workflow.script.refused", &[("thread", asked.to_string()), ("reason", reason.to_string())]);
                Err(refused(reason, message))
            }
        }
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
        let thread = field(payload, "threadId")?;
        self.log("secret.answer.unknown", &[("thread", thread.to_string())]);
        if self.row(thread).is_none() {
            return Err(QueryFailure::Error(wire::thread_missing_error("SecretRequestError", thread, json!({"reason": "load_failed"}))));
        }
        Err(QueryFailure::Error(json!({"_tag": "SecretRequestError", "reason": "not_found", "message": "This secret request no longer exists."})))
    }

    /// `orchestration.subscribeCommandOutput`: the frames of a command's output: the whole text as one replace, running only while the command is.
    pub fn command_output(&self, payload: &Value) -> Result<Vec<Value>, QueryFailure> {
        let (thread, item) = (field(payload, "threadId")?.to_string(), field(payload, "itemId")?);
        let no_command = || QueryFailure::Error(json!({"_tag": "OrchestrationV2CommandOutputError", "threadId": thread, "message": "No such command."}));
        if self.row(&thread).is_none() {
            return Err(no_command());
        }
        self.sync_thread(&thread);
        let found = self.engine().read(|state| state.entities.get(&thread).and_then(|e| e.turn_items.iter().find(|i| i["id"] == item && i["type"] == "command_execution").cloned()));
        let Some(found) = found else { return Err(no_command()) };
        let running = found["status"] == "running";
        Ok(vec![json!({"kind": "replace", "text": found["output"].as_str().unwrap_or_default(), "truncated": false, "running": running})])
    }

    /// `GET /api/orchestration/shell`: the shell snapshot as the stream's first frame holds it.
    pub fn shell_snapshot_http(&self, request: &EnvironmentRequest) -> Result<Value, Refusal> {
        self.environment_gate("shellSnapshot", request)?;
        Ok(self.engine().read(|state| state.shell_snapshot()["snapshot"].clone()))
    }

    /// `GET /api/projects`: the live projects, as `projects.mutate` answers a row, behind the bearer and `orchestration:read`.
    pub fn projects_snapshot_http(&self, authorization: Option<&str>) -> Result<Value, Refusal> {
        self.read_gate("projectsSnapshot", authorization)?;
        let projects = self.engine().read(|state| state.projects.iter().filter(|(_, p)| !is_deleted(p)).filter_map(|(id, _)| state.project_row(id)).collect::<Vec<_>>());
        Ok(json!({"projects": projects, "updatedAt": iso(self.0.clock.now_ms())}))
    }

    /// `GET /api/orchestration/threads/:threadId/bounded`: the thread's projection whole, which is no bigger than a window of it here.
    pub fn thread_bounded_snapshot(&self, request: &EnvironmentRequest, thread: &str, include_layout: bool) -> Result<Value, Refusal> {
        self.environment_gate("threadBoundedSnapshot", request)?;
        self.sync_thread(thread);
        self.engine().read(|state| {
            let projection = state.thread_projection(thread)?;
            let mut result = json!({"snapshotSequence": state.last_sequence, "projection": projection, "hasMoreHistory": false});
            if include_layout {
                if let Some(root) = projection["thread"]["projectId"].as_str().and_then(|id| state.projects.get(id)).and_then(|row| row["workspaceRoot"].as_str()) {
                    result["rcaProjectRoot"] = json!(root);
                }
                result["rcaToolTextOffsets"] = state.entities.get(thread).map_or_else(|| json!({}), |e| json!(e.tool_text_offsets));
            }
            Some(result)
        }).ok_or_else(|| self.history_refusal("threadBoundedSnapshot", crate::domain::history::HistoryError::ThreadNotFound))
    }
}
