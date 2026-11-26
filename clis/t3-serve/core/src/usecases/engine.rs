//! The command engine: decide, append, project and publish under one lock, so the sequence, the read model and every
//! subscriber's view agree. Commands are idempotent by `commandId`; a stream starts from a snapshot or, inside the gap, a replay.
use crate::domain::auth::uuid_v4;
use crate::domain::command::{Command, Family};
use crate::domain::decider::{decide, decide_adopt, decide_run_step, observe_run, queue_hold_events, Ctx, Effect, Rejection, RunStep};
use crate::domain::state::{event_json, State};
use crate::ports::{Clock, EventDraft, EventStore, Ids, StoreError, StoredEvent};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, VecDeque};
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
    ThreadMissing { command_id: String, command_type: String, thread_id: String },
    PreviouslyRejected { command_id: String, command_type: String, reason: String },
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
    shell_replay: VecDeque<(u64, Vec<Value>)>,
    /// Command ids refused for a missing thread, with the first refusal's message; the pin refuses a repeat with it (a restart forgets them).
    rejected: HashMap<String, String>,
}

/// Bound on remembered refusals, so a client cannot grow them without end; past it the oldest knowledge is dropped whole.
const MAX_REJECTED: usize = 10_000;

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
        let events = store.read_after(0)?;
        let replay_after = events.last().map_or(0, |event| event.sequence.saturating_sub(REPLAY_GAP));
        let mut shell_replay = VecDeque::new();
        for event in events {
            let applied = state.apply(&event);
            if event.sequence > replay_after {
                let frames = state.shell_frames(&event, &applied);
                if !frames.is_empty() {
                    shell_replay.push_back((event.sequence, frames));
                }
            }
        }
        Ok(Engine { store, clock, ids, inner: Mutex::new(Inner { state, subscribers: Vec::new(), shell_replay, rejected: HashMap::new() }) })
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
            if !published.shell.is_empty() {
                inner.shell_replay.push_back((event.sequence, published.shell.clone()));
            }
            let cutoff = event.sequence.saturating_sub(REPLAY_GAP);
            while inner.shell_replay.front().is_some_and(|(seq, _)| *seq <= cutoff) {
                inner.shell_replay.pop_front();
            }
            inner.subscribers.retain(|tx| tx.send(published.clone()).is_ok());
        }
        Ok(stored)
    }

    pub fn dispatch(&self, family: Family, raw: &Value) -> Result<Dispatched, Failure> {
        let cmd = Command::parse(raw, family).map_err(Failure::Defect)?;
        self.run(&mut self.lock(), family, cmd)
    }

    /// A command the server issues itself (`SERVER_ONLY`), which no client can dispatch.
    pub fn dispatch_server(&self, body: Map<String, Value>) -> Result<Dispatched, Failure> {
        let cmd = Command::server(body).map_err(Failure::Defect)?;
        self.run(&mut self.lock(), Family::Thread, cmd)
    }

    fn run(&self, inner: &mut Inner, family: Family, cmd: Command) -> Result<Dispatched, Failure> {
        let refuse = |m: String| Failure::Refused { command_id: cmd.command_id.clone(), command_type: cmd.kind.clone(), message: m };
        let project_row = |inner: &Inner| (family == Family::Project).then(|| cmd.str("projectId").and_then(|id| inner.state.project_row(id))).flatten();
        if let Some(receipt) = self.store.receipt(&cmd.command_id).map_err(|e| refuse(e.0))? {
            return Ok(Dispatched { sequence: receipt.sequence, row: project_row(inner), effect: None, replayed: true });
        }
        if let Some(reason) = inner.rejected.get(&cmd.command_id).filter(|_| family == Family::Thread) {
            return Err(Failure::PreviouslyRejected { command_id: cmd.command_id.clone(), command_type: cmd.kind.clone(), reason: reason.clone() });
        }
        let new_id = || uuid(self.ids.as_ref());
        let decided = decide(&inner.state, family, &cmd, &Ctx { now: self.clock.now_ms(), new_id: &new_id });
        let decision = decided.map_err(|r| match r {
            Rejection::Refused(m) => refuse(m),
            Rejection::ProjectNotEmpty(project_id) => Failure::ProjectNotEmpty { command_id: cmd.command_id.clone(), project_id },
            Rejection::ThreadMissing(thread_id) => {
                if inner.rejected.len() >= MAX_REJECTED {
                    inner.rejected.clear();
                }
                inner.rejected.insert(cmd.command_id.clone(), t3_contracts::wire::projection_load_failed(&thread_id));
                Failure::ThreadMissing { command_id: cmd.command_id.clone(), command_type: cmd.kind.clone(), thread_id }
            }
        })?;
        let stored = self.commit(inner, decision.drafts, &cmd.command_id, Some((&cmd.command_id, &cmd.kind))).map_err(|e| refuse(e.0))?;
        let sequence = stored.last().map_or(inner.state.last_sequence, |e| e.sequence);
        Ok(Dispatched { sequence, row: project_row(inner), effect: decision.effect, replayed: false })
    }

    /// Projects a pre-existing RCA chat once, without creating a second agent thread or reopening a removed T3 row.
    pub(super) fn open_mobile_tab(&self, source: &str, target: &str, title: &str, selection: Value) -> Result<(), String> {
        let mut inner = self.lock();
        let current = inner.state.threads.get(source).filter(|t| !crate::domain::state::is_archived(t)).ok_or("The source chat is no longer open.")?;
        let project = current.get("projectId").and_then(Value::as_str).ok_or("Missing source project.")?;
        if let Some(known) = inner.state.threads.get(target) {
            return (known.get("projectId") == current.get("projectId") && !crate::domain::state::is_archived(known))
                .then_some(()).ok_or_else(|| "This tab is unavailable in the current project.".into());
        }
        if inner.state.deleted.contains_key(target) { return Err("This tab was deleted.".into()); }
        let body = json!({"type":"thread.create", "commandId":uuid(self.ids.as_ref()), "threadId":target, "projectId":project,
            "title":title, "modelSelection":selection, "createdBy":"user", "creationSource":"mobile",
            "runtimeMode":current.get("runtimeMode"), "interactionMode":current.get("interactionMode"),
            "branch":current.get("branch"), "worktreePath":current.get("worktreePath")});
        let cmd = Command::parse(&body, Family::Thread)?;
        self.run(&mut inner, Family::Thread, cmd).map(|_| ()).map_err(super::projects::failure_text)
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

    pub fn observe_agent_run(&self, thread: &str, run: &str, text: &str, attachments: &[Value]) -> Result<bool, StoreError> {
        let mut inner = self.lock();
        let new_id = || uuid(self.ids.as_ref());
        let drafts = observe_run(&inner.state, thread, run, text, attachments, &Ctx { now: self.clock.now_ms(), new_id: &new_id });
        if drafts.is_empty() {
            return Ok(false);
        }
        self.commit(&mut inner, drafts, run, None)?;
        Ok(true)
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

    pub fn record_queue_hold(&self, thread: &str, held: bool) -> Result<(), StoreError> {
        let mut inner = self.lock();
        let drafts = queue_hold_events(&inner.state, thread, held, self.clock.now_ms());
        if drafts.is_empty() {
            return Ok(());
        }
        self.commit(&mut inner, drafts, &format!("queue-hold-{thread}"), None).map(|_| ())
    }

    /// A queued run the agent API did not accept ends cancelled, with its message taken back.
    pub fn cancel_queued(&self, thread_id: &str, run_id: &str) -> Result<(), StoreError> {
        let mut inner = self.lock();
        let Some(run) = inner.state.entities.get(thread_id).and_then(|e| e.runs.iter().find(|r| r["id"] == run_id && r["status"] == "queued")).cloned() else { return Ok(()) };
        let drafts = crate::domain::decider::cancelled(&run, self.clock.now_ms());
        self.commit(&mut inner, drafts, &format!("queue-failed-{run_id}"), None).map(|_| ())
    }

    pub fn thread_exists(&self, thread_id: &str) -> bool {
        self.lock().state.threads.contains_key(thread_id)
    }

    /// The run of the thread that is running.
    pub fn active_run(&self, thread_id: &str) -> Option<String> {
        let inner = self.lock();
        inner.state.entities.get(thread_id)?.runs.iter().rev().find(|r| r["status"] == "running").and_then(|r| r["id"].as_str().map(str::to_string))
    }

    /// The run that starts next from the thread's queue.
    pub fn next_queued(&self, thread_id: &str) -> Option<String> {
        let inner = self.lock();
        crate::domain::decider::queued_runs(&inner.state, thread_id).first().and_then(|r| r["id"].as_str().map(str::to_string))
    }

    /// Whether the thread's rows hold a run of this id.
    pub fn knows_run(&self, thread_id: &str, run_id: &str) -> bool {
        self.lock().state.entities.get(thread_id).is_some_and(|e| e.runs.iter().any(|r| r["id"] == run_id))
    }

    /// Threads with active or queued work whose updates may have arrived while T3 was down.
    pub fn recoverable_threads(&self) -> Vec<String> {
        let inner = self.lock();
        inner.state.entities.iter().filter(|(thread, entities)| {
            inner.state.threads.contains_key(*thread) && entities.runs.iter().any(|r| matches!(r["status"].as_str(), Some("running" | "queued")))
        }).map(|(thread, _)| thread.clone()).collect()
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
        let frames = inner.shell_replay.iter().filter(|(seq, _)| *seq > after).flat_map(|(_, frames)| frames.iter().cloned()).collect();
        Ok((frames, last))
    }

    pub fn thread_start(&self, thread_id: &str, after: Option<u64>) -> Result<(Vec<Value>, u64), String> {
        let inner = self.lock();
        let last = inner.state.last_sequence;
        // A subscriber that has the log up to a sequence asks for what follows, whether or not it holds the thread: only a snapshot needs the projection
        let Some(after) = Self::replayable(after, last) else {
            let Some(projection) = inner.state.thread_projection(thread_id) else {
                return Err(format!("thread {thread_id} was not found"));
            };
            return Ok((vec![json!({"kind": "snapshot", "snapshotSequence": last, "projection": projection})], last));
        };
        let events = self.store.read_after(after).map_err(|e| e.0)?;
        let frames = events.iter().filter(|e| e.thread_id.as_deref() == Some(thread_id)).map(thread_event_frame).collect();
        Ok((frames, last))
    }

    pub fn thread_projection(&self, thread_id: &str) -> Option<Value> {
        self.lock().state.thread_projection_read(thread_id)
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
