//! One agent turn run to its end and read as text (`lib/streamChat.ts`): create or reuse a session, subscribe to the directory's events
//! before prompting, send the prompt with its system text, tool switches and agent, then read the stream until the session goes idle
//! after having worked. What the voice planner, the Tamil check and the image description are built on.

use remote_chat_agent_core::domain::accumulator::Accumulator;
use super::opencode_client::Client;
use remote_chat_agent_core::application::ports::DataStream;
use crate::domain::util::encode_uri_component;
use super::log;
use futures_util::StreamExt;
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// The first request to a directory boots its opencode instance, which installs the dependencies of its plugins and tools first: as long
/// as `opencode run` waits for the same.
const BOOT_TIMEOUT: Duration = Duration::from_secs(180);

const PROBE_QUERY: &str = "__remote_chat_agent_probe__";

#[derive(Default)]
pub struct Turn {
    pub session_id: Option<String>,
    pub cwd: String,
    pub model: Option<(String, String)>,
    pub system: Option<String>,
    pub tools: Option<Map<String, Value>>,
    pub agent: Option<String>,
    pub file_part: Option<Value>,
    /// More file parts, as `opencode run -f` attaches them.
    pub file_parts: Vec<Value>,
    /// A custom command of the project (`.opencode/command/<name>.md`) to run with the message as its arguments, as `opencode run --command`.
    pub command: Option<String>,
    /// What `opencode run` does with a permission it is asked for: approve it (`--auto`) or reject it.
    pub approve_asks: bool,
    /// The model variant (provider-specific reasoning effort: `high`, `max`, `minimal`).
    pub variant: Option<String>,
    /// The title of a session this turn creates.
    pub title: Option<String>,
    /// The permission rules of a session this turn creates (`[{ permission, pattern, action }]`).
    pub permission: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Usage {
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub reasoning: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
}

/// One tool part as opencode reported it: a snapshot of a call, not the call.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub call_id: Option<String>,
    pub name: String,
    /// `running`, `done` or `failed`.
    pub state: &'static str,
    pub args: Option<Map<String, Value>>,
}

/// What a turn has said and done so far: kept when the turn fails, so the part that happened is not lost.
#[derive(Debug, Default)]
pub struct Captured {
    pub text: String,
    pub tools: Vec<ToolCall>,
    /// The turn as `opencode run --format json` prints it: one `{ type, timestamp, sessionID, part }` object per step start, finished
    /// text, finished tool call and step finish, in the order they happened.
    pub events: Vec<Value>,
}

#[derive(Debug)]
pub struct Reply {
    pub text: String,
    pub session_id: String,
    pub usage: Option<Usage>,
}

#[derive(Debug)]
pub enum Failure {
    /// The agent backend cannot resolve the directory on its own filesystem.
    DirectoryNotVisible { cwd: String, detail: Option<String> },
    SessionCreate,
    /// opencode reported a session error (context overflow, an abort, a refused request).
    Session { message: String },
    Stall { session_id: String, events: usize, last: String, seen_busy: bool, timeout_ms: u128 },
    Empty { session_id: String, cwd: String, events: usize },
    /// The caller's own time budget ran out first.
    Budget { session_id: String },
    /// Anything else: the network, a refused prompt.
    Other(String),
}

impl Failure {
    pub fn message(&self) -> String {
        match self {
            Self::DirectoryNotVisible { cwd, detail } => format!(
                "the agent backend cannot see the working directory {cwd} on its own filesystem{} — it runs out-of-process, so the directory must be mounted at the SAME path there (see remote-chat-agent's docker-compose.prod.yml)",
                detail.as_ref().map(|d| format!(" ({d})")).unwrap_or_default()
            ),
            Self::SessionCreate => "opencode session.create returned no session ID".into(),
            Self::Session { message } => format!("opencode session error: {message}"),
            Self::Stall { session_id, events, last, seen_busy, timeout_ms } => {
                format!("opencode stream stalled: no events for {timeout_ms}ms (session={session_id}, eventsSeen={events}, last={last}, seenBusy={seen_busy})")
            }
            Self::Empty { session_id, cwd, events } => format!("opencode finished the turn without emitting any content (session={session_id}, cwd={cwd}, eventsSeen={events})"),
            Self::Budget { .. } => "the turn ran out of its time budget".into(),
            Self::Other(message) => message.clone(),
        }
    }
}

/// One line of `opencode run --format json`: the part as opencode reported it, under the kind `run` prints it as.
fn run_event(timestamp: u64, kind: &str, session_id: &str, part: &Value) -> Value {
    json!({ "type": kind, "timestamp": timestamp, "sessionID": session_id, "part": part })
}

/// The silence after which a stream is given up on: `OPENCODE_RESPONSE_TIMEOUT_MS`, a minute by default.
fn response_timeout(client: &Client) -> Duration {
    Duration::from_millis(client.env().settings().opencode_response_timeout_ms)
}

fn diag(event: &str, meta: &[(&str, Value)]) {
    log::info(&format!("opencode.{event}"), meta);
}

async fn probe(client: &Client, cwd: &str) -> Result<(), Failure> {
    let path = format!("/find/file?directory={}&query={PROBE_QUERY}", encode_uri_component(cwd));
    let reply = client.request_with("GET", &path, None, BOOT_TIMEOUT).await.map_err(|e| Failure::Other(format!("fetch failed: {e}")))?;
    if reply.ok() {
        return Ok(());
    }
    let detail = if reply.status == 401 || reply.status == 403 {
        format!(
            "opencode rejected the credential ({}) — this is an AUTH failure, not a missing mount. Check OPENCODE_SERVER_PASSWORD matches between the agent and the opencode service in docker-compose.prod.yml.",
            reply.status
        )
    } else {
        format!("opencode /find/file responded {}", reply.status)
    };
    diag("cwd.not.visible", &[("cwd", json!(cwd)), ("detail", json!(detail))]);
    Err(Failure::DirectoryNotVisible { cwd: cwd.into(), detail: Some(detail) })
}

fn usage_of(tokens: &Value) -> Usage {
    let number = |v: Option<&Value>| v.and_then(Value::as_f64);
    Usage {
        input: number(tokens.get("input")),
        output: number(tokens.get("output")),
        reasoning: number(tokens.get("reasoning")),
        cache_read: number(tokens.get("cache").and_then(|c| c.get("read"))),
        cache_write: number(tokens.get("cache").and_then(|c| c.get("write"))),
    }
}

/// The next event that parses as JSON, waiting no longer than the silence limit or what is left of the caller's budget.
async fn next_event(stream: &mut DataStream, idle: Duration, deadline: Option<Instant>) -> Result<Option<Value>, ReadError> {
    loop {
        let wait = match deadline {
            Some(d) => {
                let left = d.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(ReadError::Budget);
                }
                left.min(idle)
            }
            None => idle,
        };
        match tokio::time::timeout(wait, stream.next()).await {
            Err(_) => return Err(if deadline.is_some_and(|d| Instant::now() >= d) { ReadError::Budget } else { ReadError::Idle }),
            Ok(None) => return Ok(None),
            Ok(Some(Err(e))) => return Err(ReadError::Io(e)),
            Ok(Some(Ok(data))) => {
                if let Ok(value) = serde_json::from_str::<Value>(&data) {
                    return Ok(Some(value));
                }
            }
        }
    }
}

enum ReadError {
    Idle,
    Budget,
    Io(String),
}

/// Runs one turn. `deadline` is the caller's own wall-clock budget on top of the silence limit.
pub async fn run(client: &Client, message: &str, turn: &Turn, deadline: Option<Instant>) -> Result<Reply, Failure> {
    run_capturing(client, message, turn, deadline, &mut Captured::default()).await
}

/// `run`, keeping the text and the tool calls in `captured` even when the turn then fails.
pub async fn run_capturing(client: &Client, message: &str, turn: &Turn, deadline: Option<Instant>, captured: &mut Captured) -> Result<Reply, Failure> {
    let cwd = &turn.cwd;
    let started = Instant::now();
    probe(client, cwd).await?;

    let session_id = match &turn.session_id {
        Some(id) => {
            diag("session.reused", &[("sessionId", json!(id)), ("cwd", json!(cwd))]);
            id.clone()
        }
        None => {
            let path = format!("/session?directory={}", encode_uri_component(cwd));
            let mut create = Map::new();
            if let Some(title) = turn.title.as_ref().filter(|t| !t.is_empty()) {
                create.insert("title".into(), json!(title));
            }
            if let Some(permission) = &turn.permission {
                create.insert("permission".into(), permission.clone());
            }
            let created = client.fetch_json("POST", &path, Some(&Value::Object(create))).await.map_err(|e| Failure::Other(e.to_string()))?;
            let id = created.get("id").and_then(Value::as_str).filter(|i| !i.is_empty()).ok_or(Failure::SessionCreate)?.to_string();
            diag("session.created", &[("sessionId", json!(id)), ("cwd", json!(cwd))]);
            id
        }
    };

    // Subscribe before the prompt so nothing it triggers is missed.
    let mut stream = client.open_stream(&format!("/event?directory={}", encode_uri_component(cwd))).await.map_err(|e| Failure::Other(format!("fetch failed: {e}")))?;
    diag("stream.subscribed", &[("sessionId", json!(session_id)), ("cwd", json!(cwd))]);

    // As `opencode run`: an agent nobody defined is a warning and the default agent answers (the server would refuse the prompt).
    let agent = match turn.agent.as_ref().filter(|a| !a.is_empty()) {
        Some(name) => {
            let known = client.fetch_json("GET", &format!("/agent?directory={}", encode_uri_component(cwd)), None).await.ok().and_then(|list| list.as_array().map(|agents| agents.iter().filter_map(|a| a.get("name").and_then(Value::as_str)).any(|known| known == name)));
            if known == Some(false) {
                diag("agent.unknown", &[("sessionId", json!(session_id)), ("agent", json!(name))]);
                None
            } else {
                Some(name.clone())
            }
        }
        None => None,
    };
    let mut parts = vec![json!({ "type": "text", "text": message })];
    if let Some(file) = &turn.file_part {
        parts.push(file.clone());
    }
    parts.extend(turn.file_parts.iter().cloned());
    let mut body = Map::new();
    body.insert("parts".into(), Value::Array(parts));
    if let Some((provider, model)) = &turn.model {
        body.insert("model".into(), json!({ "providerID": provider, "modelID": model }));
    }
    if let Some(system) = turn.system.as_ref().filter(|s| !s.is_empty()) {
        body.insert("system".into(), json!(system));
    }
    if let Some(tools) = &turn.tools {
        body.insert("tools".into(), Value::Object(tools.clone()));
    }
    if let Some(agent) = &agent {
        body.insert("agent".into(), json!(agent));
    }
    if let Some(variant) = turn.variant.as_ref().filter(|v| !v.is_empty()) {
        body.insert("variant".into(), json!(variant));
    }
    if let Some(name) = turn.command.as_ref().filter(|c| !c.is_empty()) {
        // The command endpoint answers when the command is done; the events of the stream are what this turn reads, so the call
        // is left running on its own, with the time a long turn may take.
        let mut command = Map::new();
        command.insert("command".into(), json!(name));
        command.insert("arguments".into(), json!(message.trim()));
        if let Some((provider, model)) = &turn.model {
            command.insert("model".into(), json!(format!("{provider}/{model}")));
        }
        if let Some(agent) = &agent {
            command.insert("agent".into(), json!(agent));
        }
        if let Some(variant) = turn.variant.as_ref().filter(|v| !v.is_empty()) {
            command.insert("variant".into(), json!(variant));
        }
        let path = format!("/session/{}/command?directory={}", encode_uri_component(&session_id), encode_uri_component(cwd));
        let (caller, body) = (client.clone(), Value::Object(command));
        tokio::spawn(async move {
            let _ = caller.request_with("POST", &path, Some(&body), Duration::from_secs(3600)).await;
        });
    } else {
        let prompt_path = format!("/session/{}/prompt_async?directory={}", encode_uri_component(&session_id), encode_uri_component(cwd));
        let prompted = client.request("POST", &prompt_path, Some(&Value::Object(body))).await.map_err(|e| Failure::Other(format!("fetch failed: {e}")))?;
        if !prompted.ok() {
            return Err(Failure::Other(format!("opencode {prompt_path} responded with {}", prompted.status)));
        }
    }
    diag("prompt.fired", &[("sessionId", json!(session_id)), ("cwd", json!(cwd)), ("promptChars", json!(message.chars().count()))]);

    // Known user messages are rejected and unknown ones taken as possibly the assistant's: on a reused session the assistant's
    // own `message.updated` can arrive after its text parts.
    let mut assistant_ids: HashSet<String> = HashSet::new();
    let mut user_ids: HashSet<String> = HashSet::new();
    let mut accumulator = Accumulator::default();
    let mut part_kinds: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    let mut anonymous = 0;
    let mut emitted: HashSet<String> = HashSet::new();
    let mut usage: Option<Usage> = None;
    let mut other_content = false;
    let (mut seen_busy, mut events, mut last) = (false, 0usize, "(none)".to_string());
    let idle = response_timeout(client);

    loop {
        let event = match next_event(&mut stream, idle, deadline).await {
            Ok(Some(event)) => event,
            Ok(None) => break,
            Err(ReadError::Idle) => {
                diag("stream.stalled", &[("sessionId", json!(session_id)), ("cwd", json!(cwd)), ("eventCount", json!(events)), ("lastEventType", json!(last)), ("seenBusy", json!(seen_busy))]);
                return Err(Failure::Stall { session_id, events, last, seen_busy, timeout_ms: idle.as_millis() });
            }
            Err(ReadError::Budget) => return Err(Failure::Budget { session_id }),
            Err(ReadError::Io(e)) => return Err(Failure::Other(e)),
        };
        events += 1;
        let kind = event.get("type").and_then(Value::as_str).unwrap_or_default().to_string();
        last = kind.clone();
        let props = event.get("properties").cloned().unwrap_or_else(|| json!({}));
        if kind == "permission.asked" {
            // Nobody is there to answer: `opencode run` approves with --auto and rejects without it, for the session and its subagents.
            if let Some(id) = props.get("id").and_then(Value::as_str) {
                let path = format!("/permission/{}/reply?directory={}", encode_uri_component(id), encode_uri_component(cwd));
                let body = json!({ "reply": if turn.approve_asks { "once" } else { "reject" } });
                let caller = client.clone();
                tokio::spawn(async move {
                    let _ = caller.request("POST", &path, Some(&body)).await;
                });
                diag("permission.answered", &[("sessionId", json!(session_id)), ("approved", json!(turn.approve_asks)), ("permission", props.get("permission").cloned().unwrap_or(Value::Null))]);
            }
            continue;
        }
        if let Some(theirs) = props.get("sessionID").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            if theirs != session_id {
                continue;
            }
        }
        if kind == "session.error" {
            let error = props.get("error");
            let message = error.and_then(|e| e.get("data")).and_then(|d| d.get("message")).and_then(Value::as_str).or_else(|| error.and_then(|e| e.get("name")).and_then(Value::as_str)).unwrap_or("session.error").to_string();
            diag("stream.session.error", &[("sessionId", json!(session_id)), ("cwd", json!(cwd)), ("eventCount", json!(events))]);
            return Err(Failure::Session { message });
        }
        if kind == "session.idle" {
            if seen_busy {
                diag("stream.idle.terminal", &[("sessionId", json!(session_id)), ("cwd", json!(cwd)), ("ms", json!(started.elapsed().as_millis() as u64)), ("eventCount", json!(events))]);
                break;
            }
            continue;
        }
        seen_busy = true;

        match kind.as_str() {
            "message.updated" => {
                let info = props.get("info");
                let id = info.and_then(|i| i.get("id")).and_then(Value::as_str).filter(|i| !i.is_empty());
                match (info.and_then(|i| i.get("role")).and_then(Value::as_str), id) {
                    (Some("assistant"), Some(id)) => {
                        assistant_ids.insert(id.to_string());
                    }
                    (Some("user"), Some(id)) => {
                        user_ids.insert(id.to_string());
                    }
                    _ => {}
                }
                if info.and_then(|i| i.get("role")).and_then(Value::as_str) == Some("assistant") {
                    if let Some(tokens) = info.and_then(|i| i.get("tokens")).filter(|t| t.is_object()) {
                        // The last snapshot carries the final counts.
                        usage = Some(usage_of(tokens));
                    }
                }
            }
            "message.part.delta" => {
                let part_id = props.get("partID").and_then(Value::as_str).filter(|p| !p.is_empty());
                let delta = props.get("delta").and_then(Value::as_str).filter(|d| !d.is_empty());
                let (Some(part_id), Some(delta)) = (part_id, delta) else { continue };
                let Some(is_text) = part_kinds.get(part_id).copied() else { continue };
                if props.get("field").and_then(Value::as_str) != Some("text") {
                    continue;
                }
                if let Some(chunk) = accumulator.delta(part_id, delta) {
                    if is_text {
                        captured.text.push_str(&chunk);
                    }
                }
            }
            "message.part.updated" => {
                let Some(part) = props.get("part").filter(|p| p.is_object()) else { continue };
                let message_id = part.get("messageID").and_then(Value::as_str).filter(|m| !m.is_empty());
                if message_id.is_some_and(|m| user_ids.contains(m)) {
                    continue;
                }
                if message_id.is_some_and(|m| !assistant_ids.is_empty() && !assistant_ids.contains(m)) {
                    continue;
                }
                match part.get("type").and_then(Value::as_str) {
                    Some(t @ ("text" | "reasoning")) => {
                        let is_text = t == "text";
                        let part_id = part.get("id").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| {
                            anonymous += 1;
                            format!("anonymous:{anonymous}")
                        });
                        part_kinds.insert(part_id.clone(), is_text);
                        if let Some(chunk) = accumulator.snapshot(&part_id, part.get("text").and_then(Value::as_str).unwrap_or_default()) {
                            if is_text {
                                captured.text.push_str(&chunk);
                            }
                        }
                        if is_text && part.get("time").and_then(|t| t.get("end")).is_some() && emitted.insert(part_id.clone()) {
                            captured.events.push(run_event(client.now_ms(), "text", &session_id, part));
                        }
                    }
                    Some(t @ ("step-start" | "step-finish")) => {
                        let id = part.get("id").and_then(Value::as_str).unwrap_or_default();
                        if emitted.insert(format!("{t}:{id}")) {
                            captured.events.push(run_event(client.now_ms(), &t.replace('-', "_"), &session_id, part));
                        }
                    }
                    Some("tool") => {
                        let state = part.get("state");
                        let status = state.and_then(|s| s.get("status")).and_then(Value::as_str).unwrap_or("running");
                        if matches!(status, "completed" | "error") {
                            let key = part.get("callID").and_then(Value::as_str).or_else(|| part.get("id").and_then(Value::as_str)).unwrap_or_default();
                            if emitted.insert(format!("tool:{key}")) {
                                captured.events.push(run_event(client.now_ms(), "tool_use", &session_id, part));
                            }
                        }
                        if status != "pending" {
                            other_content = true;
                            let name = part.get("toolName").filter(|n| !n.is_null()).or_else(|| part.get("tool")).and_then(Value::as_str).unwrap_or("tool");
                            captured.tools.push(ToolCall {
                                call_id: part.get("callID").and_then(Value::as_str).filter(|c| !c.is_empty()).map(str::to_string),
                                name: name.to_string(),
                                state: match status {
                                    "completed" => "done",
                                    "failed" => "failed",
                                    _ => "running",
                                },
                                args: state.and_then(|s| s.get("input")).and_then(Value::as_object).cloned(),
                            });
                        }
                    }
                    _ => {}
                }
            }
            "question.asked" | "session.next.tool.called" | "session.next.tool.success" | "session.next.tool.failed" => other_content = true,
            _ => {}
        }
    }
    drop(stream);

    // A turn that reached its idle having said nothing is a failure, not an empty answer: opencode logs some prompt-level faults
    // to its own file and never publishes them as a session error.
    if accumulator.emitted_chars() == 0 && !other_content {
        diag("stream.empty.response", &[("sessionId", json!(session_id)), ("cwd", json!(cwd)), ("eventCount", json!(events))]);
        return Err(Failure::Empty { session_id, cwd: cwd.clone(), events });
    }
    Ok(Reply { text: captured.text.clone(), session_id, usage })
}

/// Stops the session's turn (a budget ran out on it); the answer is not waited for.
pub async fn abort(client: &Client, session_id: &str, directory: &str) {
    let path = format!("/session/{}/abort?directory={}", encode_uri_component(session_id), encode_uri_component(directory));
    let _ = client.request("POST", &path, None).await;
}
