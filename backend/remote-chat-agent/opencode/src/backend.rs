//! The opencode `Backend` (`data/opencodeGateway.ts` and `data/adapters/gatewayProviderAdapter.ts`): sessions, prompts, aborts and
//! answers over opencode's HTTP API, and its event stream re-shaped the way the gateway re-shapes it (permission requests, model and
//! usage lifecycle, plan snapshots, a turn completion before every idle).

use rca_service_core::application::ports::Gateway;
use rca_service_core::application::settings::Settings;
use remote_chat_agent_core::application::ports::{Backend, EventStream, SendTurn};
use remote_chat_agent_core::domain::agent::AgentEvent;
use rca_service_core::application::opencode_client::Client;
use async_trait::async_trait;
use futures_util::StreamExt;
use rca_service_core::domain::util::encode_uri_component;
use rca_service_core::domain::models;
use rca_service_core::application::log;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

pub type DirFor = Box<dyn Fn(&str) -> String + Send + Sync>;
type PlanModes = Arc<Mutex<HashMap<String, bool>>>;

pub struct OpencodeBackend {
    client: Client,
    dir_for: DirFor,
    sessions: Mutex<Sessions>,
    plan_modes: PlanModes,
    default_model: Option<String>,
}

#[derive(Default)]
struct Sessions {
    by_thread: HashMap<String, String>,
}

fn permission_rules(runtime_mode: &str) -> Value {
    if runtime_mode == "full-access" {
        return json!([{ "permission": "*", "pattern": "*", "action": "allow" }]);
    }
    let rule = |permission: &str, action: &str| json!({ "permission": permission, "pattern": "*", "action": action });
    json!([
        rule("*", "ask"),
        rule("bash", "ask"),
        rule("edit", "ask"),
        rule("webfetch", "ask"),
        rule("websearch", "ask"),
        rule("codesearch", "ask"),
        rule("external_directory", "ask"),
        rule("doom_loop", "ask"),
        rule("question", "allow"),
    ])
}

fn permission_kind(permission: &str) -> &'static str {
    match permission {
        "bash" | "command" => "command",
        "edit" | "write" | "patch" => "file-change",
        "read" => "file-read",
        _ => "unknown",
    }
}

fn reply_for(decision: &str) -> &'static str {
    match decision {
        "accept" => "once",
        "acceptForSession" => "always",
        _ => "reject",
    }
}

fn js_text(value: &Value) -> String {
    rca_service_core::domain::util::js_string(value)
}

impl OpencodeBackend {
    pub fn new(client: Client, dir_for: DirFor, settings: &Settings) -> Self {
        Self { client, dir_for, default_model: settings.opencode_default_model.clone(), sessions: Mutex::new(Sessions::default()), plan_modes: Arc::new(Mutex::new(HashMap::new())) }
    }

    fn query(&self, thread_id: &str) -> String {
        format!("?directory={}", encode_uri_component(&(self.dir_for)(thread_id)))
    }

    async fn post(&self, path: &str, body: Option<&Value>) -> Result<(), String> {
        let reply = self.client.request("POST", path, body).await.map_err(|e| e.to_string())?;
        if reply.ok() {
            Ok(())
        } else {
            Err(format!("opencode {path} responded with {}", reply.status))
        }
    }

    fn model_field(&self, model: &remote_chat_agent_core::domain::types::ModelSelection) -> Option<Value> {
        if let (Some(provider), Some(id)) = (model.provider_id.as_deref().filter(|p| !p.is_empty()), model.model_id.as_deref().filter(|m| !m.is_empty())) {
            return Some(json!({ "providerID": provider, "modelID": id }));
        }
        models::resolve_compatible(&model.cli, None, self.default_model.as_deref()).map(|(provider, id)| json!({ "providerID": provider, "modelID": id }))
    }
}

#[async_trait]
impl Gateway for OpencodeBackend {
    /// The question the user answered, to opencode by request id and directory (the route has no thread to take them from).
    async fn question_reply(&self, request_id: &str, directory: &str, answers: &Value) -> Result<(), String> {
        self.post(&format!("/question/{}/reply?directory={}", encode_uri_component(request_id), encode_uri_component(directory)), Some(&json!({ "answers": answers }))).await
    }

    async fn session_abort(&self, session_id: &str, directory: &str) -> Result<(), String> {
        self.post(&format!("/session/{}/abort?directory={}", encode_uri_component(session_id), encode_uri_component(directory)), None).await
    }

    /// A prompt that is queued behind the running turn (`deferred`) or injected into it (`immediate`).
    async fn session_prompt(&self, session_id: &str, directory: &str, text: &str, delivery: &str) -> Result<(), String> {
        self.post(
            &format!("/api/session/{}/prompt?directory={}", encode_uri_component(session_id), encode_uri_component(directory)),
            Some(&json!({ "prompt": { "text": text }, "delivery": delivery })),
        )
        .await
    }
}

#[async_trait]
impl Backend for OpencodeBackend {
    fn provider(&self) -> &str {
        "opencode"
    }

    async fn start_session(&self, thread_id: &str, runtime_mode: &str) -> Result<String, String> {
        let path = format!("/session{}", self.query(thread_id));
        let created = self.client.fetch_json("POST", &path, Some(&json!({ "permission": permission_rules(runtime_mode) }))).await;
        let id = created.ok().and_then(|v| v.get("id").and_then(Value::as_str).filter(|id| !id.is_empty()).map(str::to_string));
        let Some(id) = id else { return Err(format!("gateway.session.create returned no session id for thread {thread_id}")) };
        self.sessions.lock().unwrap_or_else(|e| e.into_inner()).by_thread.insert(thread_id.to_string(), id.clone());
        Ok(id)
    }

    async fn send_turn(&self, input: &SendTurn<'_>) -> Result<(), String> {
        let Some(session) = self.session_for(input.thread_id) else {
            return Err(format!("gatewayProviderAdapter.sendTurn: no session for thread {}; call startSession first", input.thread_id));
        };
        let session_path = encode_uri_component(&session);
        // The permission ruleset follows the thread's runtime mode on every turn; a refusal leaves the one the session was created with.
        if let Err(e) = self.client.request("PATCH", &format!("/session/{session_path}"), Some(&json!({ "permission": permission_rules(input.runtime_mode) }))).await {
            log::warn!("opencode.session_update_failed", err = e.to_string());
        }
        let plan = input.interaction_mode == "plan";
        self.plan_modes.lock().unwrap_or_else(|e| e.into_inner()).insert(session.clone(), plan);
        let mut body = Map::new();
        let mut parts = vec![json!({ "type": "text", "text": input.text })];
        parts.extend(input.files.iter().cloned());
        body.insert("parts".into(), Value::Array(parts));
        if let Some(model) = self.model_field(input.model) {
            body.insert("model".into(), model);
        }
        if plan {
            body.insert("agent".into(), json!("plan"));
        }
        self.post(&format!("/session/{session_path}/prompt_async{}", self.query(input.thread_id)), Some(&Value::Object(body))).await
    }

    async fn interrupt_turn(&self, thread_id: &str) -> Result<(), String> {
        match self.session_for(thread_id) {
            Some(session) => self.session_abort(&session, &(self.dir_for)(thread_id)).await,
            None => Ok(()),
        }
    }

    async fn respond_to_request(&self, thread_id: &str, request_id: &str, decision: &str) -> Result<(), String> {
        self.post(&format!("/permission/{}/reply{}", encode_uri_component(request_id), self.query(thread_id)), Some(&json!({ "reply": reply_for(decision) }))).await
    }

    async fn respond_to_user_input(&self, thread_id: &str, request_id: &str, answers: &Map<String, Value>) -> Result<(), String> {
        let matrix: Vec<Value> = answers
            .values()
            .map(|v| match v {
                Value::Array(items) => Value::Array(items.iter().map(|i| json!(js_text(i))).collect()),
                other => json!([js_text(other)]),
            })
            .collect();
        self.question_reply(request_id, &(self.dir_for)(thread_id), &Value::Array(matrix)).await
    }

    async fn stop_session(&self, thread_id: &str) -> Result<(), String> {
        let session = self.sessions.lock().unwrap_or_else(|e| e.into_inner()).by_thread.remove(thread_id);
        let Some(session) = session else { return Ok(()) };
        // Forgetting the session must always work, even when opencode refuses the abort.
        if let Err(e) = self.session_abort(&session, &(self.dir_for)(thread_id)).await {
            log::warn!("adapter.stopSession_abort_failed", threadId = thread_id, err = e);
        }
        Ok(())
    }

    fn session_for(&self, thread_id: &str) -> Option<String> {
        self.sessions.lock().unwrap_or_else(|e| e.into_inner()).by_thread.get(thread_id).cloned()
    }

    fn adopt_session(&self, thread_id: &str, session_id: &str) {
        self.sessions.lock().unwrap_or_else(|e| e.into_inner()).by_thread.entry(thread_id.to_string()).or_insert_with(|| session_id.to_string());
    }

    async fn open_events(&self, directory: &str) -> Result<EventStream, String> {
        let stream = self.client.open_stream(&format!("/event?directory={}", encode_uri_component(directory))).await?;
        let mut mapper = EventMapper::new(Arc::clone(&self.plan_modes));
        Ok(stream
            .flat_map(move |data| {
                let events: Vec<Result<AgentEvent, String>> = match data {
                    Ok(data) => serde_json::from_str::<Value>(&data).map(|raw| mapper.map(&raw)).unwrap_or_default().into_iter().map(Ok).collect(),
                    Err(e) => vec![Err(e)],
                };
                futures_util::stream::iter(events)
            })
            .boxed())
    }
}

/// opencode's raw events into the gateway's: what `mapSdkEventStream` does.
pub struct EventMapper {
    plan_modes: PlanModes,
    model_used: HashSet<String>,
    usage_emitted: HashSet<String>,
    failed_sessions: HashSet<String>,
    assistant_text: HashMap<String, String>,
}

impl EventMapper {
    pub fn new(plan_modes: PlanModes) -> Self {
        Self { plan_modes, model_used: HashSet::new(), usage_emitted: HashSet::new(), failed_sessions: HashSet::new(), assistant_text: HashMap::new() }
    }

    pub fn map(&mut self, raw: &Value) -> Vec<AgentEvent> {
        let kind = raw.get("type").and_then(Value::as_str).unwrap_or_default();
        let props = raw.get("properties").cloned().unwrap_or_else(|| json!({}));
        let text = |name: &str| props.get(name).and_then(Value::as_str);
        let passthrough = AgentEvent { kind: kind.to_string(), props: props.clone() };
        let mut out = Vec::new();
        match kind {
            "permission.asked" => {
                let permission = text("permission").unwrap_or_default();
                let patterns: Vec<&str> = props.get("patterns").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
                let mut p = Map::new();
                p.insert("id".into(), json!(text("id").unwrap_or_default()));
                p.insert("sessionID".into(), json!(text("sessionID").unwrap_or_default()));
                p.insert("kind".into(), json!(permission_kind(permission)));
                p.insert("title".into(), json!(if permission.is_empty() { "unknown" } else { permission }));
                if !patterns.is_empty() {
                    p.insert("detail".into(), json!(patterns.join(", ")));
                }
                if let Some(metadata) = props.get("metadata") {
                    p.insert("args".into(), metadata.clone());
                }
                out.push(AgentEvent::new("permission.asked", Value::Object(p)));
            }
            "permission.replied" => {
                out.push(AgentEvent::new("permission.replied", json!({ "id": text("requestID").unwrap_or_default(), "sessionID": text("sessionID").unwrap_or_default(), "reply": text("reply").unwrap_or_default() })));
            }
            "session.error" => {
                if let Some(session) = text("sessionID") {
                    self.failed_sessions.insert(session.to_string());
                }
                out.push(passthrough);
            }
            "session.idle" => {
                let session = text("sessionID").unwrap_or_default().to_string();
                if self.plan_modes.lock().unwrap_or_else(|e| e.into_inner()).get(&session) == Some(&true) {
                    out.push(AgentEvent::new("session.plan.proposed", json!({ "sessionID": session, "planMarkdown": self.assistant_text.remove(&session).unwrap_or_default(), "done": true })));
                }
                let failed = self.failed_sessions.remove(&session);
                out.push(AgentEvent::new("session.turn.completed", json!({ "sessionID": session, "state": if failed { "failed" } else { "completed" } })));
                out.push(passthrough);
            }
            "message.updated" => {
                let info = props.get("info").cloned().unwrap_or_else(|| json!({}));
                let session = info.get("sessionID").and_then(Value::as_str).unwrap_or_default();
                let message = info.get("id").and_then(Value::as_str).unwrap_or_default();
                if info.get("role").and_then(Value::as_str) == Some("assistant") && !session.is_empty() && !message.is_empty() {
                    let key = format!("{session}:{message}");
                    if let (Some(model), Some(provider)) = (info.get("modelID").and_then(Value::as_str), info.get("providerID").and_then(Value::as_str)) {
                        if self.model_used.insert(key.clone()) {
                            out.push(AgentEvent::new("session.model.used", json!({ "sessionID": session, "providerID": provider, "modelID": model })));
                        }
                    }
                    let tokens = info.get("tokens");
                    let number = |v: Option<&Value>| v.and_then(Value::as_f64);
                    if let (Some(input), Some(output)) = (number(tokens.and_then(|t| t.get("input"))), number(tokens.and_then(|t| t.get("output")))) {
                        if self.usage_emitted.insert(key) {
                            let cache = tokens.and_then(|t| t.get("cache"));
                            let mut usage = Map::new();
                            usage.insert("sessionID".into(), json!(session));
                            usage.insert("input".into(), json!(input as u64));
                            usage.insert("output".into(), json!(output as u64));
                            if let (Some(read), Some(write)) = (number(cache.and_then(|c| c.get("read"))), number(cache.and_then(|c| c.get("write")))) {
                                usage.insert("cached".into(), json!((read + write) as u64));
                            }
                            out.push(AgentEvent::new("session.usage.updated", Value::Object(usage)));
                        }
                    }
                }
                out.push(passthrough);
            }
            "message.part.updated" => {
                let part = props.get("part").cloned().unwrap_or_else(|| json!({}));
                let session = part.get("sessionID").and_then(Value::as_str).unwrap_or_default().to_string();
                match part.get("type").and_then(Value::as_str) {
                    Some("todo") => {
                        let plan: Vec<Value> = part.get("todos").and_then(Value::as_array).map(|todos| {
                            todos.iter().map(|t| json!({ "step": t.get("content").and_then(Value::as_str).unwrap_or_default(), "status": t.get("status").and_then(Value::as_str).unwrap_or_default() })).collect()
                        }).unwrap_or_default();
                        out.push(AgentEvent::new("session.plan.updated", json!({ "sessionID": session, "plan": plan })));
                        out.push(passthrough);
                    }
                    Some("tool") => {
                        let mut tool = part.clone();
                        let call = part.get("callID").and_then(Value::as_str).unwrap_or_default();
                        tool["callID"] = json!(call);
                        out.push(AgentEvent::new("message.part.updated", json!({ "sessionID": session, "part": tool })));
                    }
                    Some("text") => {
                        if let (Some(text), false) = (part.get("text").and_then(Value::as_str), session.is_empty()) {
                            self.assistant_text.insert(session, text.to_string());
                        }
                        out.push(passthrough);
                    }
                    _ => out.push(passthrough),
                }
            }
            _ => out.push(passthrough),
        }
        out
    }
}
