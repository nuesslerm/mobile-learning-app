//! The use cases of the schedules groups and the numbered `schedules` stream. Every change to the schedules and every run is an event on the
//! stream, numbered by a count of its own that starts from the service's base, so a `seq` of an earlier run is older than any tail and gets a
//! reset; the last `keep` events are kept, so a caller that lost the stream resumes by `seq`.
use crate::domain::{self, Request};
use crate::ports::{Clock, Ids, Refusal, RunRequest, ScheduleStore, Started, Stored, TurnTrigger};
use agent_api_proto::Event;
use agent_api_schedules::watch::{self, SubscribeResult};
use agent_api_schedules::{code, message, MutationResult, RunNowResult, RunSource, RunState, RunStatus, RunView, Schedule, TaskView, UpsertParams, WebhookView};
use rca_agent_core::token_digest;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::watch as signal;

/// Events kept.
pub const KEEP: usize = 256;

/// What a use case refuses with: a code of the group, or the agent group's own.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub code: String,
    pub message: String,
}

impl Failure {
    fn not_found(id: &str) -> Failure {
        Failure { code: code::SCHEDULE_NOT_FOUND.into(), message: message::not_found(id) }
    }

    fn unavailable(why: String) -> Failure {
        Failure { code: code::SCHEDULES_UNAVAILABLE.into(), message: why }
    }
}

impl From<Refusal> for Failure {
    fn from(r: Refusal) -> Failure {
        Failure { code: r.code, message: r.message }
    }
}

type Pending = Box<dyn FnOnce(u64) -> Event + Send>;

struct Log {
    seq: u64,
    events: VecDeque<Event>,
    changed: signal::Sender<u64>,
    keep: usize,
}

impl Log {
    fn oldest(&self) -> u64 {
        self.events.front().map_or(self.seq + 1, |e| e.seq)
    }

    fn push(&mut self, make: Pending) {
        self.seq += 1;
        self.events.push_back(make(self.seq));
        if self.events.len() > self.keep {
            self.events.pop_front();
        }
        self.changed.send_replace(self.seq);
    }
}

/// Events after a `seq`, or that they are no longer all kept.
pub struct Since {
    pub events: Vec<Event>,
    pub lost: bool,
}

/// A subscription as opened: what to answer, where the stream continues from, and the wake-ups of the stream.
pub struct Opened {
    pub result: SubscribeResult,
    pub from: u64,
    pub changes: signal::Receiver<u64>,
}

/// What a request to a webhook came to.
#[derive(Debug, Clone, PartialEq)]
pub enum WebhookOutcome {
    /// The token was right: the run that began.
    Accepted(RunView),
    /// The token is not the schedule's, or the schedule is no webhook, or none has the id: the caller is told no more than that.
    Rejected,
    Disabled,
    /// The token was right and the turn could not begin.
    Failed(Refusal),
}

pub use domain::Request as WebhookRequest;

pub struct Schedules {
    store: Arc<dyn ScheduleStore>,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn Ids>,
    trigger: Arc<dyn TurnTrigger>,
    log: Mutex<Log>,
}

impl Schedules {
    /// `base` is the `seq` the stream counts on from: above any an earlier run of the service gave out.
    pub fn new(store: Arc<dyn ScheduleStore>, clock: Arc<dyn Clock>, ids: Arc<dyn Ids>, trigger: Arc<dyn TurnTrigger>, base: u64) -> Arc<Schedules> {
        Schedules::keeping(store, clock, ids, trigger, base, KEEP)
    }

    pub fn keeping(store: Arc<dyn ScheduleStore>, clock: Arc<dyn Clock>, ids: Arc<dyn Ids>, trigger: Arc<dyn TurnTrigger>, base: u64, keep: usize) -> Arc<Schedules> {
        let log = Log { seq: base, events: VecDeque::new(), changed: signal::channel(base).0, keep: keep.max(1) };
        Arc::new(Schedules { store, clock, ids, trigger, log: Mutex::new(log) })
    }

    fn log(&self) -> MutexGuard<'_, Log> {
        self.log.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Reads, changes and writes the schedules under the one lock, and numbers the events `change` asks for only when the write succeeded.
    fn edit<R>(&self, change: impl FnOnce(&mut Vec<Stored>, &mut Vec<Pending>) -> Result<R, Failure>) -> Result<R, Failure> {
        let mut log = self.log();
        let mut all = self.store.load().map_err(Failure::unavailable)?;
        let mut pending = Vec::new();
        let result = change(&mut all, &mut pending)?;
        self.store.save(&all).map_err(Failure::unavailable)?;
        for make in pending {
            log.push(make);
        }
        Ok(result)
    }

    fn announce(&self, make: Pending) {
        self.log().push(make);
    }

    fn stored(&self, id: &str) -> Result<Stored, Failure> {
        self.store.load().map_err(Failure::unavailable)?.into_iter().find(|s| s.task.id == id).ok_or_else(|| Failure::not_found(id))
    }

    /// A schedule a crash left `running` ended with the service: it is failed, so the clock can start it again. How many were.
    pub fn recover(&self) -> Result<usize, Failure> {
        self.edit(|all, _| {
            let stale: Vec<&mut Stored> = all.iter_mut().filter(|s| s.task.last_run_status == RunStatus::Running).collect();
            let count = stale.len();
            for s in stale {
                s.task.last_run_status = RunStatus::Failed;
                s.task.last_run_error = Some("the service restarted while the turn ran".into());
            }
            Ok(count)
        })
    }

    pub fn list(&self) -> Result<Vec<TaskView>, Failure> {
        Ok(self.store.load().map_err(Failure::unavailable)?.into_iter().map(|s| s.task).collect())
    }

    pub fn upsert(&self, p: UpsertParams) -> Result<MutationResult, Failure> {
        let now = self.clock.now_ms();
        self.edit(|all, pending| {
            let at = p.id.as_deref().and_then(|id| all.iter().position(|s| s.task.id == id));
            if at.is_none() && p.require_existing == Some(true) {
                return Err(Failure::not_found(p.id.as_deref().unwrap_or_default()));
            }
            let mut stored = match at {
                Some(at) => all[at].clone(),
                None => {
                    let id = p.id.clone().unwrap_or_else(|| self.ids.id("sch"));
                    Stored { task: blank(&id, now, &p), token_digest: None }
                }
            };
            let task = &mut stored.task;
            (task.title, task.prompt, task.enabled, task.schedule) = (p.title.clone(), p.prompt.clone(), p.enabled, p.schedule.clone());
            (task.workspace, task.driver, task.model, task.thread) = (p.workspace.clone(), p.driver.clone(), p.model.clone(), p.thread.clone());
            (task.runtime_mode, task.interaction_mode, task.updated_at_ms) = (p.runtime_mode.clone(), p.interaction_mode.clone(), now);
            task.meta = p.meta.clone();
            task.next_run_ms = if task.enabled { domain::next_run(&task.schedule, now) } else { None };
            let mut minted = None;
            if !task.schedule.is_webhook() {
                stored.token_digest = None;
            } else if stored.token_digest.is_none() {
                let token = self.ids.token();
                stored.token_digest = Some(token_digest(&token));
                minted = Some(token);
            }
            stored.task.webhook = webhook_view(&stored);
            let view = stored.task.clone();
            match at {
                Some(at) => all[at] = stored,
                None => all.push(stored),
            }
            let (id, shown) = (view.id.clone(), view.clone());
            pending.push(Box::new(move |seq| watch::changed_event(seq, &id, watch::change::SAVED, Some(&shown))));
            Ok(MutationResult { task: view, webhook_token: minted })
        })
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<MutationResult, Failure> {
        let now = self.clock.now_ms();
        self.edit(|all, pending| {
            let stored = all.iter_mut().find(|s| s.task.id == id).ok_or_else(|| Failure::not_found(id))?;
            if stored.task.enabled != enabled {
                stored.task.enabled = enabled;
                stored.task.updated_at_ms = now;
                stored.task.next_run_ms = if enabled { domain::next_run(&stored.task.schedule, now) } else { None };
                let (shown, what) = (stored.task.clone(), if enabled { watch::change::ENABLED } else { watch::change::DISABLED });
                let id = id.to_string();
                pending.push(Box::new(move |seq| watch::changed_event(seq, &id, what, Some(&shown))));
            }
            Ok(MutationResult { task: stored.task.clone(), webhook_token: None })
        })
    }

    pub fn delete(&self, id: &str) -> Result<(), Failure> {
        self.edit(|all, pending| {
            let at = all.iter().position(|s| s.task.id == id).ok_or_else(|| Failure::not_found(id))?;
            all.remove(at);
            let id = id.to_string();
            pending.push(Box::new(move |seq| watch::changed_event(seq, &id, watch::change::DELETED, None)));
            Ok(())
        })
    }

    pub fn rotate_webhook_token(&self, id: &str) -> Result<MutationResult, Failure> {
        let now = self.clock.now_ms();
        self.edit(|all, pending| {
            let stored = all.iter_mut().find(|s| s.task.id == id).ok_or_else(|| Failure::not_found(id))?;
            if !stored.task.schedule.is_webhook() {
                return Err(Failure { code: code::NOT_A_WEBHOOK.into(), message: message::not_a_webhook(id) });
            }
            let token = self.ids.token();
            stored.token_digest = Some(token_digest(&token));
            stored.task.updated_at_ms = now;
            stored.task.webhook = webhook_view(stored);
            let (shown, id) = (stored.task.clone(), id.to_string());
            pending.push(Box::new(move |seq| watch::changed_event(seq, &id, watch::change::TOKEN_ROTATED, Some(&shown))));
            Ok(MutationResult { task: stored.task.clone(), webhook_token: Some(token) })
        })
    }

    /// Starts the schedule's turn now, whether it is enabled or not.
    pub async fn run_now(self: &Arc<Self>, id: &str) -> Result<RunNowResult, Failure> {
        let stored = self.stored(id)?;
        let run = self.fire(&stored.task, RunSource::Manual, None).await?;
        Ok(RunNowResult { task: self.stored(id).map_or(stored.task, |s| s.task), run })
    }

    /// Starts every schedule that is due by the clock: how many began. A due schedule is moved on before its turn starts, so a slow start is not
    /// started twice, and one whose previous turn still runs is skipped (and recorded as skipped).
    pub async fn tick(self: &Arc<Self>) -> usize {
        let now = self.clock.now_ms();
        let due = self
            .edit(|all, _| {
                let mut due = Vec::new();
                for s in all.iter_mut().filter(|s| s.task.enabled && s.task.next_run_ms.is_some_and(|at| at <= now)) {
                    s.task.next_run_ms = domain::next_run(&s.task.schedule, now);
                    due.push(s.task.clone());
                }
                Ok(due)
            })
            .unwrap_or_default();
        let mut began = 0;
        for task in due {
            if self.fire(&task, RunSource::Timer, None).await.is_ok_and(|run| run.state == RunState::Started) {
                began += 1;
            }
        }
        began
    }

    /// A request to the webhook of schedule `id` with `token`. The caller learns only whether it was let in; every request is an event.
    pub async fn webhook(self: &Arc<Self>, id: &str, token: &str, request: &Request) -> WebhookOutcome {
        let now = self.clock.now_ms();
        let presented = token_digest(token);
        let found = self.stored(id).ok().filter(|s| s.task.schedule.is_webhook());
        let kept = found.as_ref().and_then(|s| s.token_digest.clone()).unwrap_or_else(|| "0".repeat(presented.len()));
        let known = domain::digests_equal(&presented, &kept);
        let announce = |what: &'static str| {
            let id = id.to_string();
            self.announce(Box::new(move |seq| watch::webhook_event(seq, &id, what, now)));
        };
        let Some(stored) = found.filter(|_| known) else {
            announce(watch::outcome::REJECTED);
            return WebhookOutcome::Rejected;
        };
        if !stored.task.enabled {
            announce(watch::outcome::DISABLED);
            return WebhookOutcome::Disabled;
        }
        announce(watch::outcome::ACCEPTED);
        match self.fire(&stored.task, RunSource::Webhook, Some(domain::render(&stored.task.prompt, request))).await {
            Ok(run) => WebhookOutcome::Accepted(run),
            Err(failure) => WebhookOutcome::Failed(Refusal { code: failure.code, message: failure.message }),
        }
    }

    /// One run: the turn is started through the agent group, and the run recorded as it began or failed to; a turn that began is waited for, in the
    /// background, to record how it ended. A refusal to start is the error, and is a recorded run too.
    async fn fire(self: &Arc<Self>, task: &TaskView, source: RunSource, prompt: Option<String>) -> Result<RunView, Failure> {
        let (run_id, now) = (self.ids.id("run"), self.clock.now_ms());
        let mut run = RunView { run: run_id.clone(), schedule: task.id.clone(), source, at_ms: now, state: RunState::Started, thread: None, turn: None, error: None };
        if source == RunSource::Timer && task.last_run_status == RunStatus::Running {
            run.state = RunState::Skipped;
            run.error = Some("the previous turn of this schedule still runs".into());
            self.record(&run, None)?;
            return Ok(run);
        }
        let request = RunRequest { schedule: task.id.clone(), run: run_id, title: task.title.clone(), prompt: prompt.unwrap_or_else(|| task.prompt.clone()), workspace: task.workspace.clone(), driver: task.driver.clone(), model: task.model.clone(), thread: task.thread.clone(), runtime_mode: task.runtime_mode.clone(), interaction_mode: task.interaction_mode.clone() };
        match self.trigger.start(&request).await {
            Ok(started) => {
                (run.thread, run.turn) = (Some(started.thread.clone()), Some(started.turn.clone()));
                self.record(&run, Some(RunStatus::Running))?;
                let (this, spawned) = (self.clone(), run.clone());
                tokio::spawn(async move { this.settle(spawned, started).await });
                Ok(run)
            }
            Err(refusal) => {
                run.state = RunState::Failed;
                run.error = Some(refusal.message.clone());
                self.record(&run, Some(RunStatus::Failed))?;
                Err(refusal.into())
            }
        }
    }

    async fn settle(self: Arc<Self>, mut run: RunView, started: Started) {
        let end = self.trigger.finished(&started).await;
        (run.at_ms, run.state, run.error) = (self.clock.now_ms(), if end.is_ok() { RunState::Succeeded } else { RunState::Failed }, end.err());
        let status = if run.state == RunState::Succeeded { RunStatus::Succeeded } else { RunStatus::Failed };
        // The schedule may be gone by now: its run is still announced
        let _ = self.record_if_running(&run, status);
    }

    fn record_if_running(&self, run: &RunView, status: RunStatus) -> Result<(), Failure> {
        self.edit(|all, pending| {
            if let Some(s) = all.iter_mut().find(|s| s.task.id == run.schedule && s.task.last_run_status == RunStatus::Running) {
                s.task.last_run_status = status;
                s.task.last_run_error = run.error.clone();
            }
            let run = run.clone();
            pending.push(Box::new(move |seq| watch::run_event(seq, &run)));
            Ok(())
        })
    }

    /// Notes the run on its schedule (`status` none leaves the status alone) and announces it.
    fn record(&self, run: &RunView, status: Option<RunStatus>) -> Result<(), Failure> {
        self.edit(|all, pending| {
            if let (Some(s), Some(status)) = (all.iter_mut().find(|s| s.task.id == run.schedule), status) {
                s.task.last_run_ms = Some(run.at_ms);
                s.task.last_run_status = status;
                s.task.last_run_error = run.error.clone();
                s.task.run_count += 1;
            }
            let run = run.clone();
            pending.push(Box::new(move |seq| watch::run_event(seq, &run)));
            Ok(())
        })
    }

    /// A caller with no `after_seq`, one older than the tail and one ahead of the stream or from an earlier run each get `reset` true for the first,
    /// the last two; a caller within the tail has its missed events follow. `tasks` is always the schedules as they are.
    pub fn open(&self, after_seq: Option<u64>) -> Result<Opened, Failure> {
        let log = self.log();
        let tasks = self.list()?;
        let reset = after_seq.is_some_and(|after| after > log.seq || after + 1 < log.oldest());
        let resumed = after_seq.filter(|_| !reset);
        let result = SubscribeResult { topic: watch::TOPIC.into(), tasks, seq: log.seq, reset };
        Ok(Opened { result, from: resumed.unwrap_or(log.seq), changes: log.changed.subscribe() })
    }

    pub fn since(&self, after: u64) -> Since {
        let log = self.log();
        Since { events: log.events.iter().filter(|e| e.seq > after).cloned().collect(), lost: after + 1 < log.oldest() }
    }
}

fn webhook_view(stored: &Stored) -> Option<WebhookView> {
    stored.task.schedule.is_webhook().then(|| WebhookView { path: agent_api_schedules::webhook_path(&stored.task.id), has_token: stored.token_digest.is_some() })
}

fn blank(id: &str, now: u64, p: &UpsertParams) -> TaskView {
    TaskView {
        id: id.to_string(),
        title: String::new(),
        prompt: String::new(),
        enabled: p.enabled,
        schedule: Schedule::Webhook {},
        workspace: String::new(),
        driver: String::new(),
        model: None,
        thread: None,
        runtime_mode: None,
        interaction_mode: None,
        created_at_ms: now,
        updated_at_ms: now,
        next_run_ms: None,
        last_run_ms: None,
        last_run_status: RunStatus::Never,
        last_run_error: None,
        run_count: 0,
        webhook: None,
        meta: None,
    }
}
