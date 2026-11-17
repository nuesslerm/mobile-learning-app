//! Runs a thread's turns on the agent API and keeps the thread's rows in step with the thread's log there. A message starts a turn and the turn's
//! events are folded into its run as they arrive; a thread read after a restart is caught up from `thread.history`; a run a restart left
//! running is followed to its end. An agent event is folded once: its receipt in the event store says so, so a replay writes nothing twice.
use super::engine::Engine;
use crate::domain::decider::{Launch, Outcome, RunStep};
use crate::ports::{AgentApi, AgentError, AgentEvent, AgentEventKind, AgentThread, AgentTurn, Follow, Log, MetaStore, Op};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Events a catch-up asks for at a time.
const PAGE: u32 = 200;

pub struct Runner {
    agent: Arc<dyn AgentApi>,
    engine: Arc<Engine>,
    meta: Arc<dyn MetaStore>,
    log: Arc<dyn Log>,
    stops: Mutex<HashMap<String, Arc<AtomicBool>>>,
    /// Threads whose queue a forced stop holds: nothing queued is followed until the phone starts one or resumes the queue.
    held: Mutex<HashSet<String>>,
    /// Threads whose queued turns one task is following, so two never follow the same one.
    draining: Mutex<HashSet<String>>,
}

fn turn_key(thread: &str, turn: &str) -> String {
    format!("agent-turn:{thread}:{turn}")
}

fn run_key(run: &str) -> String {
    format!("agent-run:{run}")
}

fn cursor_key(thread: &str) -> String {
    format!("agent-seq:{thread}")
}

impl Runner {
    pub fn new(agent: Arc<dyn AgentApi>, engine: Arc<Engine>, meta: Arc<dyn MetaStore>, log: Arc<dyn Log>) -> Runner {
        Runner { agent, engine, meta, log, stops: Mutex::new(HashMap::new()), held: Mutex::new(HashSet::new()), draining: Mutex::new(HashSet::new()) }
    }

    pub fn agent(&self) -> &dyn AgentApi {
        self.agent.as_ref()
    }

    fn stops(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<AtomicBool>>> {
        self.stops.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn remember(&self, key: &str, value: &str) {
        if let Err(e) = self.meta.set(key, value) {
            self.log.event("run.link.failed", &[("key", key.to_string()), ("error", e.0)]);
        }
    }

    fn recall(&self, key: &str) -> Option<String> {
        self.meta.get(key).ok().flatten()
    }

    /// The agent turn a run of t3 is, once the run's turn was started.
    pub fn turn_of_run(&self, run: &str) -> Option<String> {
        self.recall(&run_key(run))
    }

    /// The turn of a run the phone names after it started, which can be a moment before the link is written: waits for the link, and gives
    /// up after half a second, when the name is not a run of ours at all.
    pub fn turn_of_started_run(&self, run: &str) -> Option<String> {
        for _ in 0..50 {
            if let Some(turn) = self.turn_of_run(run) {
                return Some(turn);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        None
    }

    fn set<'a>(&self, which: &'a Mutex<HashSet<String>>) -> std::sync::MutexGuard<'a, HashSet<String>> {
        which.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn launch(self: &Arc<Self>, launch: Launch) {
        let (me, thread) = (self.clone(), launch.thread_id.clone());
        let key = thread.clone();
        self.spawn(&key, move |stop| {
            me.run(launch, stop);
            me.drain(&thread);
        });
    }

    /// Puts a message behind the running turn in the agent API's queue; the agent API starts it by itself when the turns before it end.
    pub fn queue(self: &Arc<Self>, launch: Launch) {
        let me = self.clone();
        std::thread::spawn(move || {
            let thread = AgentThread { thread_id: launch.thread_id.clone(), instance_id: launch.instance_id.clone(), model: launch.model.clone() };
            let added = me.agent.ensure_thread(&thread).and_then(|_| me.agent.run(&Op::QueueAdd { thread: launch.thread_id.clone(), item: launch.run_id.clone(), prompt: launch.prompt.clone() }));
            if let Err(e) = added {
                me.log.event("run.queue.failed", &[("run", launch.run_id.clone()), ("error", e.message())]);
                if let Err(e) = me.engine.cancel_queued(&launch.thread_id, &launch.run_id) {
                    me.log.event("run.record.failed", &[("run", launch.run_id.clone()), ("error", e.0)]);
                }
                return;
            }
            me.remember(&turn_key(&launch.thread_id, &launch.run_id), &launch.run_id);
            me.remember(&run_key(&launch.run_id), &launch.run_id);
            if me.engine.active_run(&launch.thread_id).is_none() {
                me.drain(&launch.thread_id);
            }
        });
    }

    /// The phone started a queued run, or resumed the queue: the hold is over and the turns the agent API starts are followed.
    pub fn resume_queue(self: &Arc<Self>, thread: &str, started: Option<String>) {
        self.set(&self.held).remove(thread);
        let (me, key) = (self.clone(), thread.to_string());
        let name = key.clone();
        self.spawn(&name, move |stop| {
            if let Some(run) = started {
                if me.claim(&key) {
                    me.remember(&turn_key(&key, &run), &run);
                    me.remember(&run_key(&run), &run);
                    me.follow_to_end(&key, &run, &stop);
                    me.set(&me.draining).remove(&key);
                }
            }
            me.drain(&key);
        });
    }

    fn claim(&self, thread: &str) -> bool {
        self.set(&self.draining).insert(thread.to_string())
    }

    /// Follows the turn of a queued run from the log's cursor to its end and records how it ended.
    fn follow_to_end(&self, thread: &str, run: &str, stop: &AtomicBool) {
        let result = self.follow(thread, run, run, self.cursor(thread));
        let outcome = match result {
            _ if stop.load(Ordering::SeqCst) => Outcome::Interrupted,
            Ok(()) => Outcome::Completed,
            Err(e) => Outcome::Failed(e.message()),
        };
        self.finish(thread, run, outcome.clone());
        self.log.event("run.ended", &[("run", run.to_string()), ("outcome", format!("{outcome:?}"))]);
    }

    /// Follows the queued runs one after the other, as the agent API starts them, until the queue is empty or a forced stop holds it.
    pub fn drain(&self, thread: &str) {
        if !self.claim(thread) {
            return;
        }
        let stop = self.stops().get(thread).cloned().unwrap_or_default();
        while !self.set(&self.held).contains(thread) {
            let Some(run) = self.engine.next_queued(thread) else { break };
            self.remember(&turn_key(thread, &run), &run);
            self.remember(&run_key(&run), &run);
            self.follow_to_end(thread, &run, &stop);
        }
        self.set(&self.draining).remove(thread);
    }

    fn spawn(&self, thread: &str, body: impl FnOnce(Arc<AtomicBool>) + Send + 'static) {
        let stop = Arc::new(AtomicBool::new(false));
        self.stops().insert(thread.to_string(), stop.clone());
        std::thread::spawn(move || body(stop));
    }

    /// Stops the thread's turn; with `hold` through the agent API's forced stop, which also keeps its queue from starting.
    pub fn interrupt(&self, thread_id: &str, hold: bool) {
        if let Some(stop) = self.stops().get(thread_id) {
            stop.store(true, Ordering::SeqCst);
        }
        let stopped = if hold {
            self.set(&self.held).insert(thread_id.to_string());
            self.agent.run(&Op::ThreadForceStop { thread: thread_id.to_string(), turn: None }).map(drop)
        } else {
            self.agent.interrupt(thread_id)
        };
        if let Err(e) = stopped {
            self.log.event("run.interrupt.failed", &[("thread", thread_id.to_string()), ("error", e.message())]);
        }
    }

    fn fold(&self, thread: &str, run: &str, event: &AgentEvent) {
        let Some(step) = RunStep::of(&event.kind) else { return };
        if let Err(e) = self.engine.record_agent(thread, run, event.seq, &step) {
            self.log.event("run.record.failed", &[("run", run.to_string()), ("seq", event.seq.to_string()), ("error", e.0)]);
        }
    }

    fn cursor(&self, thread: &str) -> u64 {
        self.recall(&cursor_key(thread)).and_then(|v| v.parse().ok()).unwrap_or(0)
    }

    fn advance(&self, thread: &str, seq: u64) {
        if seq > self.cursor(thread) {
            self.remember(&cursor_key(thread), &seq.to_string());
        }
    }

    fn finish(&self, thread: &str, run: &str, outcome: Outcome) {
        if let Err(e) = self.engine.record(thread, run, &RunStep::Finished(outcome)) {
            self.log.event("run.record.failed", &[("run", run.to_string()), ("error", e.0)]);
        }
    }

    /// Follows `turn` from `after` into the run until the thread says it finished.
    fn follow(&self, thread: &str, run: &str, turn: &str, after: u64) -> Result<(), AgentError> {
        let mut seen = after;
        self.agent.follow(thread, after, &mut |event| {
            if event.turn.as_deref() != Some(turn) {
                return Follow::Continue;
            }
            seen = event.seq;
            self.fold(thread, run, &event);
            if matches!(event.kind, AgentEventKind::TurnFinished(_)) {
                Follow::Stop
            } else {
                Follow::Continue
            }
        })?;
        self.advance(thread, seen);
        Ok(())
    }

    fn run(&self, launch: Launch, stop: Arc<AtomicBool>) {
        let (thread, run) = (launch.thread_id.as_str(), launch.run_id.as_str());
        self.log.event("run.started", &[("thread", thread.to_string()), ("run", run.to_string()), ("instance", launch.instance_id.clone())]);
        let result = self.start(&launch, &stop).and_then(|(turn, after)| self.follow(thread, run, &turn, after));
        let outcome = match result {
            _ if stop.load(Ordering::SeqCst) => Outcome::Interrupted,
            Ok(()) => Outcome::Completed,
            Err(e) => Outcome::Failed(e.message()),
        };
        // A turn that finished has ended its run already, so this records nothing for it
        self.finish(thread, run, outcome.clone());
        self.log.event("run.ended", &[("run", run.to_string()), ("outcome", format!("{outcome:?}"))]);
        self.stops().remove(thread);
    }

    /// Makes the thread, starts the turn and remembers which run it is: the turn and the number to follow it from.
    fn start(&self, launch: &Launch, stop: &AtomicBool) -> Result<(String, u64), AgentError> {
        let thread = AgentThread { thread_id: launch.thread_id.clone(), instance_id: launch.instance_id.clone(), model: launch.model.clone() };
        let after = self.agent.ensure_thread(&thread)?;
        let turn = self.agent.start_turn(&AgentTurn {
            thread_id: launch.thread_id.clone(),
            instance_id: launch.instance_id.clone(),
            model: launch.model.clone(),
            prompt: launch.prompt.clone(),
            runtime_mode: launch.runtime_mode.clone(),
            interaction_mode: launch.interaction_mode.clone(),
            attachments: launch.attachments.clone(),
        })?;
        self.remember(&turn_key(&launch.thread_id, &turn), &launch.run_id);
        self.remember(&run_key(&launch.run_id), &turn);
        // A stop asked before the turn existed found nothing to stop
        if stop.load(Ordering::SeqCst) {
            let _ = self.agent.interrupt(&launch.thread_id);
        }
        Ok((turn, after))
    }

    /// The run an agent turn is. A turn t3 never recorded is the thread's running run when that has no turn yet: the service stopped between
    /// starting the turn and writing it down.
    fn run_of(&self, thread: &str, turn: &str) -> Option<String> {
        if let Some(run) = self.recall(&turn_key(thread, turn)) {
            return Some(run);
        }
        // A queued run's turn has its id
        if self.engine.knows_run(thread, turn) {
            return Some(turn.to_string());
        }
        let run = self.engine.active_run(thread).filter(|run| self.recall(&run_key(run)).is_none())?;
        self.remember(&turn_key(thread, turn), &run);
        self.remember(&run_key(&run), turn);
        Some(run)
    }

    /// Catches the thread's rows up with its log in the agent API. Whatever it cannot reach leaves the rows as they are.
    pub fn sync_thread(&self, thread: &str) {
        if !self.engine.thread_exists(thread) {
            return;
        }
        let mut after = self.cursor(thread);
        loop {
            let page = match self.agent.history(thread, after, PAGE) {
                Ok(page) => page,
                Err(AgentError::NoThread) => return,
                Err(e) => return self.log.event("run.sync.failed", &[("thread", thread.to_string()), ("error", e.message())]),
            };
            for event in &page.events {
                if let Some(run) = event.turn.as_deref().and_then(|turn| self.run_of(thread, turn)) {
                    self.fold(thread, &run, event);
                }
            }
            let Some(last) = page.events.last() else { return };
            after = last.seq;
            self.advance(thread, after);
            if !page.more {
                return;
            }
        }
    }

    /// Follows every run a restart left running: caught up from the log, then to its end, or failed when its turn was never started.
    pub fn resume(self: &Arc<Self>) {
        for (thread, run) in self.engine.running() {
            let me = self.clone();
            let key = thread.clone();
            self.spawn(&key, move |_| {
                me.sync_thread(&thread);
                if me.engine.active_run(&thread).as_deref() != Some(run.as_str()) {
                    return;
                }
                let result = match me.recall(&run_key(&run)) {
                    Some(turn) => me.follow(&thread, &run, &turn, me.cursor(&thread)).map_err(|e| e.message()),
                    None => Err("t3 serve stopped before the turn was started".to_string()),
                };
                match result {
                    Ok(()) => me.finish(&thread, &run, Outcome::Completed),
                    Err(message) => me.finish(&thread, &run, Outcome::Failed(message)),
                }
                me.log.event("run.resumed", &[("thread", thread.clone()), ("run", run.clone())]);
                me.stops().remove(&thread);
            });
        }
    }
}
