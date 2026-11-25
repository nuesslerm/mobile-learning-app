//! The `agent.queue` group: prompts that wait for a turn to end, and `turn.steer`. The engine decides every change of the queue; this group asks and starts what is due.
//! A queued prompt starts by itself when a turn ends, a steer ends the CLI and runs another in the same turn, and a forced stop holds the queue until `queue.resume`.
use crate::port::Command;
use crate::refusal::{respond, to_value, Answer, Refusal};
use crate::runner::Runner;
use agent_api_agent::queue::{self as api, AddParams, AddResult, DeleteParams, DeleteResult, EditHoldAction, EditHoldParams, EditHoldResult, EditHoldStatus, EditParams, EditResult, ListParams, ListResult, MoveParams, MoveResult, QueueItem, ResumeParams, ResumeResult, SteerParams, SteerResult};
use agent_api_proto::{GroupName, Response};
use async_trait::async_trait;
use rca_agent_core::{Call, Group, Served};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const SERVED: [Served; 8] = [
    Served::service(api::QUEUE_LIST, &[]),
    Served::service(api::QUEUE_ADD, &[]),
    Served::service(api::QUEUE_EDIT, &[]),
    Served::service(api::QUEUE_EDIT_HOLD, &[]),
    Served::service(api::QUEUE_MOVE, &[]),
    Served::service(api::QUEUE_DELETE, &[]),
    Served::cli(api::QUEUE_RESUME, &[]),
    Served::cli(api::TURN_STEER, &[]),
];

const EDIT_HOLD_MS: u64 = 30_000;

struct EditHold {
    owner: String,
    expires: Instant,
}

#[derive(Default)]
pub(crate) struct EditHolds(HashMap<String, HashMap<String, EditHold>>);

impl EditHolds {
    pub(crate) fn prune(&mut self) {
        let now = Instant::now();
        self.0.retain(|_, holds| {
            holds.retain(|_, hold| hold.expires > now);
            !holds.is_empty()
        });
    }

    pub(crate) fn holds(&self, thread: &str, item: &str) -> bool {
        self.0.get(thread).is_some_and(|holds| holds.contains_key(item))
    }

    fn release(&mut self, p: &EditHoldParams) -> bool {
        let Some(holds) = self.0.get_mut(&p.thread) else { return false };
        if holds.get(&p.item).is_none_or(|hold| hold.owner != p.edit) {
            return false;
        }
        holds.remove(&p.item);
        true
    }

    fn hold(&mut self, p: &EditHoldParams, prompt: &str) -> EditHoldStatus {
        let current = self.0.get(&p.thread).and_then(|holds| holds.get(&p.item));
        let owns = current.is_some_and(|hold| hold.owner == p.edit);
        if p.action == EditHoldAction::Renew && !owns {
            return EditHoldStatus::Expired;
        }
        if p.action == EditHoldAction::Acquire && (p.expected_prompt.as_deref() != Some(prompt) || (current.is_some() && !owns)) {
            return EditHoldStatus::Changed;
        }
        self.0.entry(p.thread.clone()).or_default().insert(p.item.clone(), EditHold {
            owner: p.edit.clone(),
            expires: Instant::now() + Duration::from_millis(EDIT_HOLD_MS),
        });
        EditHoldStatus::Held
    }
}

pub struct AgentQueueGroup {
    runner: Arc<Runner>,
}

impl AgentQueueGroup {
    pub fn new(runner: Arc<Runner>) -> AgentQueueGroup {
        AgentQueueGroup { runner }
    }

    /// Whether the queue is held, and its items in the order they start.
    fn queue(&self, thread: &str) -> Result<(bool, Vec<QueueItem>), Refusal> {
        self.runner.engine().queue(thread)?.ok_or_else(|| Refusal::not_found(thread))
    }

    fn list(&self, params: &Value) -> Answer {
        let p = ListParams::parse(params)?;
        let edits = self.runner.edit_holds();
        let (held, items) = self.queue(&p.thread)?;
        let editing = items.iter().filter(|item| edits.holds(&p.thread, &item.item)).map(|item| item.item.clone()).collect();
        to_value(&ListResult { thread: p.thread, held, items, editing })
    }

    fn add(&self, params: &Value) -> Answer {
        let p = AddParams::parse(params)?;
        if p.attachments.as_ref().is_some_and(|ids| !ids.is_empty()) {
            return Err(Refusal::invalid("attachments are placed by turn.start only: send this message as a turn, or without them"));
        }
        let events = self.runner.decide(&p.thread, Command::QueueAdd(p.clone()))?;
        let item = events.first().and_then(|e| e.data.get("item")).and_then(|v| v.get("item")).and_then(Value::as_str).map(str::to_string).or(p.item.clone());
        let (_, items) = self.queue(&p.thread)?;
        let at = item.as_deref().and_then(|id| items.iter().position(|q| q.item == id));
        let queued = at.and_then(|at| items.get(at)).cloned().ok_or_else(|| Refusal::new(agent_api_agent::code::THREADS_UNAVAILABLE, "the queued prompt is not in the queue"))?;
        to_value(&AddResult { seq: self.runner.seq_of(&p.thread, &events)?, thread: p.thread, item: queued, position: at.map_or(1, |at| at as u32 + 1) })
    }

    fn edit(&self, params: &Value) -> Answer {
        let p = EditParams::parse(params)?;
        let _edits = self.runner.edit_holds();
        let events = self.runner.decide(&p.thread, Command::QueueEdit { item: p.item.clone(), prompt: p.prompt })?;
        to_value(&EditResult { seq: self.runner.seq_of(&p.thread, &events)?, thread: p.thread, item: p.item })
    }

    fn edit_hold(&self, params: &Value) -> Answer {
        let p = EditHoldParams::parse(params)?;
        let mut edits = self.runner.edit_holds();
        if p.action == EditHoldAction::Release {
            let released = edits.release(&p);
            drop(edits);
            if released {
                self.runner.start_next_later(&p.thread);
            }
            return to_value(&EditHoldResult { status: EditHoldStatus::Released, prompt: None, lease_duration_ms: None });
        }
        let (_, items) = self.queue(&p.thread)?;
        let Some(item) = items.into_iter().find(|item| item.item == p.item) else {
            return to_value(&EditHoldResult { status: EditHoldStatus::Gone, prompt: None, lease_duration_ms: None });
        };
        let status = edits.hold(&p, &item.prompt);
        let held = status == EditHoldStatus::Held;
        drop(edits);
        if held {
            self.runner.wake_queue_after(&p.thread, Duration::from_millis(EDIT_HOLD_MS));
        }
        to_value(&EditHoldResult { status, prompt: held.then_some(item.prompt), lease_duration_ms: held.then_some(EDIT_HOLD_MS) })
    }

    fn move_item(&self, params: &Value) -> Answer {
        let p = MoveParams::parse(params)?;
        let events = self.runner.decide(&p.thread, Command::QueueMove { item: p.item, before: p.before })?;
        let (_, items) = self.queue(&p.thread)?;
        to_value(&MoveResult { seq: self.runner.seq_of(&p.thread, &events)?, thread: p.thread, order: items.into_iter().map(|q| q.item).collect() })
    }

    fn delete(&self, params: &Value) -> Answer {
        let p = DeleteParams::parse(params)?;
        let events = self.runner.decide(&p.thread, Command::QueueDelete { item: p.item.clone() })?;
        to_value(&DeleteResult { seq: self.runner.seq_of(&p.thread, &events)?, thread: p.thread, item: p.item })
    }

    /// Releases the hold, and starts the first prompt (the one named, which the engine moved to the front) when no turn runs.
    async fn resume(&self, params: &Value) -> Answer {
        let p = ResumeParams::parse(params)?;
        let events = {
            let edits = self.runner.edit_holds();
            if p.item.as_deref().is_some_and(|item| edits.holds(&p.thread, item)) {
                return Err(Refusal::new(api::code::ITEM_EDITING, "This queued message is being edited."));
            }
            self.runner.decide(&p.thread, Command::QueueResume { item: p.item })?
        };
        let seq = self.runner.seq_of(&p.thread, &events)?;
        let started = self.runner.start_next(&p.thread).await;
        to_value(&ResumeResult { thread: p.thread, held: false, started, seq })
    }

    async fn steer(&self, params: &Value) -> Answer {
        let p = SteerParams::parse(params)?;
        let (turn, events) = self.runner.steer(&p.thread, p.turn, p.prompt, p.item).await?;
        to_value(&SteerResult { seq: self.runner.seq_of(&p.thread, &events)?, thread: p.thread, turn })
    }
}

#[async_trait]
impl Group for AgentQueueGroup {
    fn name(&self) -> GroupName {
        api::GROUP
    }

    fn methods(&self) -> &[Served] {
        &SERVED
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let Call { id, method, params, .. } = call;
        let answer = match method.name() {
            "queue.list" => self.list(params),
            "queue.add" => self.add(params),
            "queue.edit" => self.edit(params),
            "queue.editHold" => self.edit_hold(params),
            "queue.move" => self.move_item(params),
            "queue.delete" => self.delete(params),
            "queue.resume" => self.resume(params).await,
            "turn.steer" => self.steer(params).await,
            other => unreachable!("{other} is no method of agent.queue (SERVED)"),
        };
        respond(id, answer)
    }
}
