//! `agentSession.subscribeStatus`: every chat's status on one stream, so a phone's session list reads turn state without replaying transcripts. The
//! first frames are the naming snapshot and the sessions snapshot; after them a `status` frame carries each session whose summary changed. rca has no
//! stream of all threads, so the stream lists them at most every `POLL` and reads the log only of the ones that moved. The pinned host admits only a
//! client that declares `agent-session.structured.v1`; a phone that does not is refused as the recorded host refuses it.
use crate::fold::{fold, Folded};
use crate::journal::events_after;
use crate::session::agent_of;
use agent_api_agent::THREAD_LIST;
use orca_core::wire::{Failure, RUNTIME_ERROR};
use orca_core::{AgentApi, Ctx, Subscription};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(250);
pub(crate) const STRUCTURED: &str = "agent-session.structured.v1";
pub(crate) const STATUS_FEED: &str = "agent-session.status-feed.v1";

pub(crate) struct StatusStream {
    id: String,
    agent: Arc<dyn AgentApi>,
    opened: bool,
    /// The summary last sent per session, and the `last_seq` and status of the thread it was built from.
    sent: BTreeMap<String, (u64, String, Value)>,
    asked: Option<Instant>,
}

pub(crate) fn open(ctx: &mut Ctx) -> Result<Box<dyn Subscription>, Failure> {
    if !ctx.client_capabilities.iter().any(|c| c == STRUCTURED) {
        let mut refusal = Failure::new(RUNTIME_ERROR, "structured_agent_session_unsupported");
        refusal.data = Some(json!({"refusal": {"code": "structured_agent_session_unsupported", "details": {"reason": "clientCapabilityMissing"}}}));
        return Err(refusal);
    }
    let id = format!("agentSession.status:{}:{}", ctx.connection_id, ctx.request_id);
    Ok(Box::new(StatusStream { id, agent: ctx.agent_handle.clone(), opened: false, sent: BTreeMap::new(), asked: None }))
}

/// Milliseconds since the epoch of an RFC 3339 time (`2026-11-04T10:00:00Z`, with optional fraction), 0 when it does not parse.
fn epoch_ms(stamp: &str) -> u64 {
    let n = |r: std::ops::Range<usize>| stamp.get(r).and_then(|s| s.parse::<i64>().ok());
    let (Some(y), Some(mo), Some(d), Some(h), Some(mi), Some(s)) = (n(0..4), n(5..7), n(8..10), n(11..13), n(14..16), n(17..19)) else { return 0 };
    let (y, m) = if mo <= 2 { (y - 1, mo + 9) } else { (y, mo - 3) };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let millis = stamp.get(19..).and_then(|r| r.strip_prefix('.')).map(|f| f.chars().take_while(char::is_ascii_digit).take(3).collect::<String>()).filter(|f| !f.is_empty()).map_or(0, |f| format!("{f:0<3}").parse::<i64>().unwrap_or(0));
    u64::try_from((days * 86_400 + h * 3600 + mi * 60 + s) * 1000 + millis).unwrap_or(0)
}

fn text_of(item: &crate::fold::Item) -> Option<String> {
    item.body["blocks"].as_array()?.iter().filter_map(|b| b["text"].as_str()).next().map(str::to_string)
}

/// One chat as the phone's status row, or none for a thread this host does not run (no workspace, or a driver that is no Orca agent).
fn summary(view: &Value, folded: Option<&Folded>) -> Option<Value> {
    let (id, workspace, driver) = (view["id"].as_str()?, view["workspace"].as_str()?, view["driver"].as_str()?);
    let mut row = Map::new();
    row.insert("sessionId".into(), id.into());
    row.insert("workspaceId".into(), workspace.into());
    row.insert("agent".into(), agent_of(driver)?.into());
    if let Some(title) = view["title"].as_str().filter(|t| !t.is_empty()) {
        row.insert("conversationName".into(), title.into());
    }
    if let Some(model) = view["model"].as_str() {
        row.insert("model".into(), model.into());
    }
    row.insert("updatedAt".into(), epoch_ms(view["updated_at"].as_str().unwrap_or("")).into());
    let messages: Vec<&crate::fold::Item> = folded.map(|f| f.items.iter().filter(|i| i.body["kind"] == "message").collect()).unwrap_or_default();
    let running = view["status"] == "running";
    let attention = folded.is_some_and(|f| !f.pending_prompt_ids().is_empty());
    let status = if messages.is_empty() && !running { Value::Null } else if attention { "attention".into() } else if running { "working".into() } else { "idle".into() };
    if let Some(prompt) = messages.iter().rev().find(|m| m.body["role"] == "user").and_then(|m| text_of(m)) {
        row.insert("latestPrompt".into(), prompt.into());
    }
    if let Some(answer) = messages.iter().rev().find(|m| m.body["role"] == "assistant").and_then(|m| text_of(m)) {
        row.insert("lastAssistantMessage".into(), answer.into());
    }
    if status == "idle" {
        if let Some(outcome) = folded.and_then(|f| f.items.iter().rev().find(|i| i.body["kind"] == "turn")).and_then(|t| t.body["state"].as_str()) {
            row.insert("turnOutcome".into(), outcome.into());
        }
    }
    row.insert("status".into(), status);
    Some(Value::Object(row))
}

impl StatusStream {
    fn threads(&self) -> Option<Vec<Value>> {
        let listed = self.agent.request(THREAD_LIST.name(), json!({})).ok()?;
        listed["threads"].as_array().cloned()
    }

    fn build(&self, view: &Value) -> Option<Value> {
        let id = view["id"].as_str()?;
        let folded = events_after(self.agent.as_ref(), id, 0).ok().map(|(events, _)| fold(&events));
        summary(view, folded.as_ref())
    }

    fn remember(&mut self, view: &Value, summary: &Value) {
        self.sent.insert(summary["sessionId"].as_str().unwrap_or_default().to_string(), (view["last_seq"].as_u64().unwrap_or(0), view["status"].as_str().unwrap_or("").to_string(), summary.clone()));
    }
}

impl Subscription for StatusStream {
    fn id(&self) -> &str {
        &self.id
    }

    fn poll(&mut self) -> Vec<Value> {
        if self.asked.is_some_and(|t| t.elapsed() < POLL) {
            return Vec::new();
        }
        self.asked = Some(Instant::now());
        let Some(threads) = self.threads() else { return Vec::new() };
        if !self.opened {
            self.opened = true;
            let rows: Vec<(Value, Value)> = threads.iter().filter_map(|v| self.build(v).map(|s| (v.clone(), s))).collect();
            let names: Vec<Value> = rows.iter().map(|(_, s)| json!({"sessionId": s["sessionId"], "conversationName": s.get("conversationName").cloned().unwrap_or(Value::Null)})).collect();
            let mut naming = json!({"type": "snapshot", "sessions": [], "namingActiveSessionIds": []});
            if !names.is_empty() {
                naming["conversationNames"] = names.into();
            }
            let sessions: Vec<Value> = rows.iter().map(|(_, s)| s.clone()).collect();
            for (view, s) in &rows {
                self.remember(view, s);
            }
            return vec![naming, json!({"type": "snapshot", "sessions": sessions})];
        }
        let mut frames = Vec::new();
        for view in &threads {
            let Some(id) = view["id"].as_str() else { continue };
            let moved = self.sent.get(id).is_none_or(|(seq, status, _)| *seq != view["last_seq"].as_u64().unwrap_or(0) || status != view["status"].as_str().unwrap_or(""));
            if !moved {
                continue;
            }
            let Some(now) = self.build(view) else { continue };
            let same = self.sent.get(id).is_some_and(|(_, _, before)| *before == now);
            self.remember(view, &now);
            if !same {
                frames.push(json!({"type": "status", "session": now}));
            }
        }
        frames
    }

    fn end(&mut self) -> Vec<Value> {
        vec![json!({"type": "end"})]
    }
}
