//! `nativeChat.subscribe`: the transcript of a chat as the phone's rows, a snapshot of the newest window (or of what follows a row the phone
//! holds, when it asks to resume), then the rows each new rca event changed, with the chat's turn state. A chat is an rca thread; the rows are the
//! message items of its folded log, so a row is sent again whole while its text grows and the phone merges by id. The stream asks rca for the events
//! after the one it reached at most every `POLL`; an agent this host does not run gets the host's "Transcript unavailable" snapshot, and a chat rca
//! does not have yet gets silence until it does.
use crate::fold::{fold, Folded, Item};
use crate::journal::{events_after, view};
use crate::session::driver_of;
use orca_core::wire::{received, Failure};
use orca_core::{AgentApi, Ctx, Subscription};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(250);
const DEFAULT_WINDOW: usize = 40;
const MAX_WINDOW: usize = 2000;

pub(crate) struct NativeStream {
    id: String,
    agent: Arc<dyn AgentApi>,
    session: String,
    window: usize,
    resume: Option<(String, u64)>,
    events: Vec<Value>,
    /// The `seq` the phone has been sent up to; none until the chat exists.
    sent: Option<u64>,
    lifecycle: Option<Value>,
    opening: Option<Value>,
    asked: Option<Instant>,
}

fn row(i: &Item) -> Value {
    json!({"id": i.id, "role": i.body["role"], "blocks": i.body["blocks"], "timestamp": null, "source": "transcript", "transcriptOffset": i.sequence, "transcriptOrder": i.sequence})
}

fn rows(folded: &Folded) -> Vec<&Item> {
    folded.items.iter().filter(|i| i.body["kind"] == "message").collect()
}

/// The chat's turn as the phone's lifecycle: none until a turn exists; a failed turn reads as completed (the phone has no failed state).
fn lifecycle(folded: &Folded) -> Option<Value> {
    let turn = folded.items.iter().rev().find(|i| i.body["kind"] == "turn")?;
    let state = match turn.body["state"].as_str()? {
        "running" => "working",
        "interrupted" => "interrupted",
        _ => "completed",
    };
    Some(json!({"state": state, "turnId": turn.body["turnId"], "timestamp": null}))
}

fn first_message(params: &Value, key: &str, missing: &str) -> Result<String, Failure> {
    match params.get(key) {
        Some(Value::String(s)) if !s.is_empty() => Ok(s.clone()),
        _ => Err(Failure::invalid(missing.to_string())),
    }
}

/// `{agent, sessionId, limit?, subscriptionId?, resumeAfter?}`: the first issue of the pinned schema is the message; unknown keys are dropped.
fn read(params: &Value) -> Result<(String, String, usize, Option<String>, Option<(String, u64)>), Failure> {
    if !params.is_object() {
        return Err(Failure::invalid(format!("Invalid input: expected object, received {}", if params.is_null() { "undefined" } else { received(params) })));
    }
    let agent = first_message(params, "agent", "Missing agent")?;
    let session = first_message(params, "sessionId", "Missing session id")?;
    let window = match params.get("limit") {
        None | Some(Value::Null) => DEFAULT_WINDOW,
        Some(Value::Number(n)) => match n.as_u64().or_else(|| n.as_f64().filter(|f| f.fract() == 0.0 && *f >= 0.0).map(|f| f as u64)) {
            Some(0) => return Err(Failure::invalid("Too small: expected number to be >0")),
            Some(v) => (v as usize).min(MAX_WINDOW),
            None if n.as_f64().is_some_and(|f| f.fract() != 0.0) => return Err(Failure::invalid("Invalid input: expected int, received number")),
            None => return Err(Failure::invalid("Too small: expected number to be >0")),
        },
        Some(other) => return Err(Failure::invalid(format!("Invalid input: expected number, received {}", received(other)))),
    };
    let token = match params.get("subscriptionId") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.is_empty() => return Err(Failure::invalid("Too small: expected string to have >=1 characters")),
        Some(Value::String(s)) => Some(s.clone()),
        Some(other) => return Err(Failure::invalid(format!("Invalid input: expected string, received {}", received(other)))),
    };
    // a resume point needs a non-empty id, a number and a null (or numeric) timestamp; anything else is dropped, never an error
    let resume = params.get("resumeAfter").and_then(|r| {
        let (id, order, stamp) = (r.get("id")?.as_str().filter(|s| !s.is_empty())?, r.get("transcriptOrder")?.as_u64()?, r.get("timestamp")?);
        (stamp.is_null() || stamp.is_number()).then(|| (id.to_string(), order))
    });
    Ok((agent, session, window, token, resume))
}

fn unavailable() -> Value {
    json!({"type": "snapshot", "messages": [], "hasMore": false, "error": "Transcript unavailable"})
}

fn snapshot(folded: &Folded, window: usize, resume: &Option<(String, u64)>) -> Value {
    let all = rows(folded);
    let tail_start = all.len().saturating_sub(window);
    // a resume point is looked for in the newest window only; a hit pages from that row on
    let hit = resume.as_ref().and_then(|(id, order)| all[tail_start..].iter().position(|i| &i.id == id && i.sequence == *order)).map(|at| tail_start + at);
    let from = hit.unwrap_or(tail_start);
    let shown = &all[from..];
    let shown = &shown[shown.len().saturating_sub(window)..];
    let start = all.len() - shown.len();
    let mut frame = json!({"type": "snapshot", "messages": shown.iter().map(|i| row(i)).collect::<Vec<_>>(), "hasMore": start > 0, "beforeOffset": all.get(start).map_or(0, |i| i.sequence)});
    if hit.is_some() {
        frame["resumed"] = true.into();
    }
    if let Some(l) = lifecycle(folded) {
        frame["lifecycle"] = l;
    }
    frame
}

pub(crate) fn open(ctx: &mut Ctx, params: &Value) -> Result<Box<dyn Subscription>, Failure> {
    let (agent_name, session, window, token, resume) = read(params)?;
    let id = format!("nativeChat:{}:{}", ctx.connection_id, token.unwrap_or_else(|| format!("{agent_name}:{session}")));
    // one stream per cleanup token: a second open of it replaces the first
    ctx.subscriptions.cancel(&id);
    let agent = ctx.agent_handle.clone();
    let mut stream = NativeStream { id, agent, session: session.clone(), window, resume, events: Vec::new(), sent: None, lifecycle: None, opening: None, asked: None };
    if driver_of(&agent_name).is_none() || session.trim().is_empty() {
        stream.opening = Some(unavailable());
        stream.sent = Some(u64::MAX);
        return Ok(Box::new(stream));
    }
    stream.load()?;
    Ok(Box::new(stream))
}

/// The id a phone names to end its native-chat streams: all of a connection's, or one by its token.
pub(crate) fn prefix(connection: &str) -> String {
    format!("nativeChat:{connection}")
}

impl NativeStream {
    /// The snapshot, once rca has the chat; a chat it does not have yet is not an error, the stream waits for it.
    fn load(&mut self) -> Result<(), Failure> {
        match view(self.agent.as_ref(), &self.session) {
            Ok(_) => {}
            Err(f) if f.message == "agent_session_not_found" => return Ok(()),
            Err(f) => return Err(f),
        }
        let (events, _) = events_after(self.agent.as_ref(), &self.session, 0)?;
        let folded = fold(&events);
        self.opening = Some(snapshot(&folded, self.window, &self.resume));
        self.lifecycle = lifecycle(&folded);
        self.sent = Some(folded.last_seq);
        self.events = events;
        Ok(())
    }
}

impl Subscription for NativeStream {
    fn id(&self) -> &str {
        &self.id
    }

    fn poll(&mut self) -> Vec<Value> {
        if let Some(first) = self.opening.take() {
            self.asked = Some(Instant::now());
            return vec![first];
        }
        if self.sent == Some(u64::MAX) || self.asked.is_some_and(|t| t.elapsed() < POLL) {
            return Vec::new();
        }
        self.asked = Some(Instant::now());
        let Some(sent) = self.sent else {
            let _ = self.load();
            return self.opening.take().into_iter().collect();
        };
        let Ok((fresh, _)) = events_after(self.agent.as_ref(), &self.session, sent) else { return Vec::new() };
        if fresh.is_empty() {
            return Vec::new();
        }
        self.events.extend(fresh);
        let folded = fold(&self.events);
        let changed: Vec<Value> = rows(&folded).into_iter().filter(|i| i.revision > sent).map(row).collect();
        let now = lifecycle(&folded);
        let moved = now != self.lifecycle;
        self.sent = Some(folded.last_seq);
        self.lifecycle = now.clone();
        if changed.is_empty() && !moved {
            return Vec::new();
        }
        let mut frame = json!({"type": "appended", "messages": changed});
        if let Some(l) = now {
            frame["lifecycle"] = l;
        }
        vec![frame]
    }

    fn end(&mut self) -> Vec<Value> {
        vec![json!({"type": "end"})]
    }
}
