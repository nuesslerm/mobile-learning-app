//! `agentSession.subscribe`: the snapshot (or, for a phone that holds a cursor, what it missed, or a reset when its cursor is no cursor of this
//! log), then a batch of the items each new rca event changed. The stream asks rca for the events after the one it reached, at most every
//! `POLL` interval; a failed ask is tried again on the next one.
use crate::fold::fold;
use crate::journal::{self, cursor, current_work, events_after, page, EPOCH, FENCE};
use crate::session::now_ms;
use orca_core::wire::Failure;
use orca_core::{AgentApi, Ctx, Subscription};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(250);

pub(crate) struct SessionStream {
    id: String,
    agent: Arc<dyn AgentApi>,
    session: String,
    events: Vec<Value>,
    /// The `seq` the phone has been sent up to.
    sent: u64,
    opening: Option<Value>,
    asked: Option<Instant>,
}

/// The stream id the phone names to end it: `agentSession:<connection>:<session>`, and the request id after it.
pub(crate) fn base(connection: &str, session: &str) -> String {
    format!("agentSession:{connection}:{session}")
}

fn batch(session: &str, folded: &crate::fold::Folded, after: u64) -> Value {
    let items: Vec<Value> = folded.items.iter().filter(|i| i.revision > after).map(|i| i.render()).collect();
    let mut frame = json!({"type": "batch", "sessionId": session, "batch": {"cursor": cursor(folded.last_seq), "items": items, "removedItemIds": [], "submissions": []}, "fence": FENCE, "hostNow": now_ms()});
    if let (Some(f), Some(w)) = (frame.as_object_mut(), current_work(folded).as_object()) {
        f.extend(w.clone());
    }
    frame
}

pub(crate) fn open(ctx: &mut Ctx, params: &Value) -> Result<Box<dyn Subscription>, Failure> {
    let session = crate::session::text(params, "sessionId").ok_or_else(|| Failure::invalid("Invalid session id"))?.to_string();
    let agent = ctx.agent_handle.clone();
    crate::journal::view(agent.as_ref(), &session)?;
    let (events, _) = events_after(agent.as_ref(), &session, 0)?;
    let folded = fold(&events);
    let from = params.get("cursor").filter(|c| c.is_object());
    let mut opening = match from {
        None => json!({"type": "snapshot", "sessionId": session, "page": page(&session, &folded, "tail", None, journal::DEFAULT_LIMIT), "fence": FENCE, "hostNow": now_ms()}),
        Some(c) => {
            let at = c["sequence"].as_u64().unwrap_or(0);
            let reset = if c["epoch"] != EPOCH { Some("epoch_changed") } else if at > folded.last_seq { Some("cursor_ahead") } else { None };
            match reset {
                Some(reason) => json!({"type": "reset", "sessionId": session, "reset": reason, "page": page(&session, &folded, "tail", None, journal::DEFAULT_LIMIT), "fence": FENCE}),
                None => batch(&session, &folded, at),
            }
        }
    };
    let _ = crate::queued::publish(agent.as_ref(), &session, &mut opening);
    let id = format!("{}:{}", base(ctx.connection_id, &session), ctx.request_id);
    Ok(Box::new(SessionStream { id, agent, session, sent: folded.last_seq, events, opening: Some(opening), asked: None }))
}

impl Subscription for SessionStream {
    fn id(&self) -> &str {
        &self.id
    }

    fn poll(&mut self) -> Vec<Value> {
        if let Some(first) = self.opening.take() {
            self.asked = Some(Instant::now());
            return vec![first];
        }
        if self.asked.is_some_and(|t| t.elapsed() < POLL) {
            return Vec::new();
        }
        self.asked = Some(Instant::now());
        let Ok((fresh, _)) = events_after(self.agent.as_ref(), &self.session, self.sent) else { return Vec::new() };
        if fresh.is_empty() {
            return Vec::new();
        }
        self.events.extend(fresh);
        let folded = fold(&self.events);
        let mut frame = batch(&self.session, &folded, self.sent);
        let _ = crate::queued::publish(self.agent.as_ref(), &self.session, &mut frame);
        self.sent = folded.last_seq;
        vec![frame]
    }

    fn end(&mut self) -> Vec<Value> {
        vec![json!({"type": "end"})]
    }
}
