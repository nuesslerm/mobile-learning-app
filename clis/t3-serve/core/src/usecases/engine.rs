//! The command engine: decide, append, project and publish under one lock, so the sequence, the read model and every
//! subscriber's view agree. Commands are idempotent by `commandId`; a stream starts from a snapshot or, inside the gap, a replay.
use crate::domain::auth::uuid_v4;
use crate::domain::command::{Command, Family};
use crate::domain::decider::{decide, decide_adopt, decide_run_step, Ctx, Effect, Rejection, RunStep};
use crate::domain::state::{event_json, State};
use crate::ports::{Clock, EventDraft, EventStore, Ids, StoreError, StoredEvent};
use serde_json::{json, Value};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

/// A client further behind than this many events gets a fresh snapshot instead of a replay (`ws.ts:583,1172`).
pub const REPLAY_GAP: u64 = 1000;

pub struct Published {
    pub event: StoredEvent,
    pub shell: Vec<Value>,
}

/// Why a command did not run: a payload the decoder refuses (a defect on the wire) or a refusal the command's own error names.
#[derive(Debug, PartialEq)]
pub enum Failure {
    Defect(String),
    Refused { command_id: String, command_type: String, message: String },
    ProjectNotEmpty { command_id: String, project_id: String },
}

pub struct Dispatched {
    pub sequence: u64,
    /// The project row a `projects.mutate` answers with.
    pub row: Option<Value>,
    pub effect: Option<Effect>,
    /// The command id was accepted before: this is its receipt, and nothing was written.
    pub replayed: bool,
}

struct Inner {
    state: State,
    subscribers: Vec<Sender<Arc<Published>>>,
}

pub struct Engine {
    store: Arc<dyn EventStore>,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn Ids>,
    inner: Mutex<Inner>,
}

fn uuid(ids: &dyn Ids) -> String {
    uuid_v4(&ids.random_bytes(16))
}

impl Engine {
    pub fn open(store: Arc<dyn EventStore>, clock: Arc<dyn Clock>, ids: Arc<dyn Ids>) -> Result<Engine, StoreError> {
        let mut state = State::default();
        for event in store.read_after(0)? {
            state.apply(&event);
        }
        Ok(Engine { store, clock, ids, inner: Mutex::new(Inner { state, subscribers: Vec::new() }) })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn register(&self, tx: Sender<Arc<Published>>) {
        self.lock().subscribers.push(tx);
    }

    fn event_id(&self, draft: &EventDraft, command_id: &str) -> String {
        match (&draft.thread_id, &draft.project_id) {
            (Some(t), _) => format!("event:thread:{t}:command:{command_id}:{}", uuid(self.ids.as_ref())),
            (None, Some(p)) => format!("event:project:{p}:command:{command_id}:{}", uuid(self.ids.as_ref())),
            _ => format!("event:command:{command_id}:{}", uuid(self.ids.as_ref())),
        }
    }

    fn commit(&self, inner: &mut Inner, mut drafts: Vec<EventDraft>, command_id: &str, receipt: Option<(&str, &str)>) -> Result<Vec<StoredEvent>, StoreError> {
        for d in &mut drafts {
            d.event_id = self.event_id(d, command_id);
        }
        let stored = self.store.append(drafts, receipt)?;
        for event in stored.iter() {
            let applied = inner.state.apply(event);
            let published = Arc::new(Published { event: event.clone(), shell: inner.state.shell_frames(event, &applied) });
            inner.subscribers.retain(|tx| tx.send(published.clone()).is_ok());
        }
        Ok(stored)
    }

    pub fn dispatch(&self, family: Family, raw: &Value) -> Result<Dispatched, Failure> {
        let cmd = Command::parse(raw, family).map_err(Failure::Defect)?;
        let mut inner = self.lock();
        let refuse = |m: String| Failure::Refused { command_id: cmd.command_id.clone(), command_type: cmd.kind.clone(), message: m };
        let project_row = |inner: &Inner| (family == Family::Project).then(|| cmd.str("projectId").and_then(|id| inner.state.project_row(id))).flatten();
        if let Some(receipt) = self.store.receipt(&cmd.command_id).map_err(|e| refuse(e.0))? {
            return Ok(Dispatched { sequence: receipt.sequence, row: project_row(&inner), effect: None, replayed: true });
        }
        let new_id = || uuid(self.ids.as_ref());
        let decision = decide(&inner.state, family, &cmd, &Ctx { now: self.clock.now_ms(), new_id: &new_id }).map_err(|r| match r {
            Rejection::Refused(m) => refuse(m),
            Rejection::ProjectNotEmpty(project_id) => Failure::ProjectNotEmpty { command_id: cmd.command_id.clone(), project_id },
        })?;
        let stored = self.commit(&mut inner, decision.drafts, &cmd.command_id, Some((&cmd.command_id, &cmd.kind))).map_err(|e| refuse(e.0))?;
        let sequence = stored.last().map_or(inner.state.last_sequence, |e| e.sequence);
        Ok(Dispatched { sequence, row: project_row(&inner), effect: decision.effect, replayed: false })
    }

    /// Moves a thread into a project (the folder it works in became one), once per `command_id`.
    pub fn adopt_project(&self, thread_id: &str, project_id: &str, command_id: &str) -> Result<(), String> {
        let mut inner = self.lock();
        if self.store.receipt(command_id).map_err(|e| e.0)?.is_some() {
            return Ok(());
        }
        let new_id = || uuid(self.ids.as_ref());
        let decision = decide_adopt(&inner.state, thread_id, project_id, &Ctx { now: self.clock.now_ms(), new_id: &new_id })?;
        self.commit(&mut inner, decision.drafts, command_id, Some((command_id, "thread.project.adopt-folder"))).map(|_| ()).map_err(|e| e.0)
    }

    /// Whether a command with this id was accepted before.
    pub fn accepted(&self, command_id: &str) -> Result<bool, StoreError> {
        self.store.receipt(command_id).map(|r| r.is_some())
    }

    /// A read of the read model as it is now, under the engine's lock.
    pub fn read<R>(&self, f: impl FnOnce(&State) -> R) -> R {
        f(&self.lock().state)
    }

    /// What a running turn reports, recorded as events of its run. A turn that already ended records nothing.
    pub fn record(&self, thread_id: &str, run_id: &str, step: &RunStep) -> Result<(), StoreError> {
        self.record_as(thread_id, run_id, step, &format!("run-{run_id}"), false)
    }

    /// The same for the agent event `seq` of the thread. Its receipt is written with its rows, and an event that has one is skipped, so a replay
    /// of the thread's log writes nothing twice.
    pub fn record_agent(&self, thread_id: &str, run_id: &str, seq: u64, step: &RunStep) -> Result<(), StoreError> {
        self.record_as(thread_id, run_id, step, &format!("agent:{thread_id}:{seq}"), true)
    }

    fn record_as(&self, thread_id: &str, run_id: &str, step: &RunStep, command_id: &str, receipt: bool) -> Result<(), StoreError> {
        let mut inner = self.lock();
        if receipt && self.store.receipt(command_id)?.is_some() {
            return Ok(());
        }
        let new_id = || uuid(self.ids.as_ref());
        let drafts = decide_run_step(&inner.state, thread_id, run_id, step, &Ctx { now: self.clock.now_ms(), new_id: &new_id });
        if drafts.is_empty() {
            return Ok(());
        }
        self.commit(&mut inner, drafts, command_id, receipt.then_some((command_id, "agent.event"))).map(|_| ())
    }

    pub fn thread_exists(&self, thread_id: &str) -> bool {
        self.lock().state.threads.contains_key(thread_id)
    }

    /// The run of the thread that is running.
    pub fn active_run(&self, thread_id: &str) -> Option<String> {
        let inner = self.lock();
        inner.state.entities.get(thread_id)?.runs.iter().rev().find(|r| r["status"] == "running").and_then(|r| r["id"].as_str().map(str::to_string))
    }

    /// Every thread with a run that is running, and that run.
    pub fn running(&self) -> Vec<(String, String)> {
        let inner = self.lock();
        let running = |(thread, e): (&String, &crate::domain::state::Entities)| e.runs.iter().rev().find(|r| r["status"] == "running").and_then(|r| r["id"].as_str().map(|run| (thread.clone(), run.to_string())));
        inner.state.entities.iter().filter_map(running).collect()
    }

    fn replayable(after: Option<u64>, last: u64) -> Option<u64> {
        after.filter(|a| *a <= last && last - *a <= REPLAY_GAP)
    }

    /// The frames that bring a shell subscriber up to date, and the sequence they end at.
    pub fn shell_start(&self, after: Option<u64>) -> Result<(Vec<Value>, u64), String> {
        let inner = self.lock();
        let last = inner.state.last_sequence;
        let Some(after) = Self::replayable(after, last) else {
            return Ok((vec![inner.state.shell_snapshot()], last));
        };
        let mut scratch = State::default();
        let mut frames = Vec::new();
        for event in self.store.read_after(0).map_err(|e| e.0)? {
            let applied = scratch.apply(&event);
            if event.sequence > after {
                frames.extend(scratch.shell_frames(&event, &applied));
            }
        }
        Ok((frames, last))
    }

    pub fn thread_start(&self, thread_id: &str, after: Option<u64>) -> Result<(Vec<Value>, u64), String> {
        let inner = self.lock();
        let last = inner.state.last_sequence;
        let Some(projection) = inner.state.thread_projection(thread_id) else {
            return Err(format!("thread {thread_id} was not found"));
        };
        let Some(after) = Self::replayable(after, last) else {
            return Ok((vec![json!({"kind": "snapshot", "snapshotSequence": last, "projection": projection})], last));
        };
        let events = self.store.read_after(after).map_err(|e| e.0)?;
        let frames = events.iter().filter(|e| e.thread_id.as_deref() == Some(thread_id)).map(thread_event_frame).collect();
        Ok((frames, last))
    }

    pub fn thread_projection(&self, thread_id: &str) -> Option<Value> {
        self.lock().state.thread_projection(thread_id)
    }

    /// The directory a project's files live under, for the file RPCs.
    pub fn project_root(&self, project_id: &str) -> Option<String> {
        self.lock().state.projects.get(project_id).and_then(|p| p["workspaceRoot"].as_str().map(str::to_string))
    }
}

pub fn thread_event_frame(e: &StoredEvent) -> Value {
    json!({"kind": "event", "sequence": e.sequence, "event": event_json(e)})
}

pub type Subscription = Receiver<Arc<Published>>;
