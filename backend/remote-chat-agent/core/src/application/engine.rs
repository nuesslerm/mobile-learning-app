//! The command engine (`orchestration/engine.ts`): one lock around decide, append, project and notify so concurrent dispatches
//! never mint the same sequence number, lazy hydration of a thread from its log, thread and shell subscriptions that replay
//! before they go live, and the restart sweep that interrupts turns a crash left running.

use super::store::Store;
use crate::domain::decider::{decide, materialize_internal, Context, Decision};
use crate::domain::lifecycle::Lifecycle;
use crate::domain::projector::{project, NeutralThread, ReadModel};
use crate::domain::queries::{self, Cursor, SearchHit, Shelf, ShelfPage};
use crate::domain::types::*;
use crate::domain::ids::{Clock, IdSource};
use serde_json::json;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver as Receiver, UnboundedSender as Sender};

type Hook = Arc<dyn Fn(&[Event]) + Send + Sync>;

struct State {
    model: ReadModel,
    summaries: Vec<Summary>,
    thread_listeners: std::collections::HashMap<String, Vec<Sender<ThreadItem>>>,
    shell_listeners: Vec<Sender<ShellItem>>,
}

/// A turn the engine found running at start and interrupted, with the agent session it ran in.
pub struct StaleTurn {
    pub thread_id: String,
    pub provider_session_id: Option<String>,
}

pub struct Engine {
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdSource>,
    store: Box<dyn Store>,
    default_cwd: String,
    state: Mutex<State>,
    on_events: Mutex<Option<Hook>>,
}

/// The kind of the event that holds one neutral event of the agent API.
pub const NEUTRAL_EVENT: &str = crate::domain::decider::NEUTRAL_KIND;

/// A thread of the agent API: its summary, what it was created with and what its lifecycle keeps (settle, lineage, queue, a merge that waits).
#[derive(Debug, Clone)]
pub struct NeutralView {
    pub thread: Summary,
    pub neutral: NeutralThread,
    pub lifecycle: Lifecycle,
}

fn neutral_view(st: &State, thread_id: &str) -> Option<NeutralView> {
    let neutral = st.model.neutral.get(thread_id)?;
    st.model.threads.get(thread_id).map(|thread| NeutralView { thread: thread.summary(), neutral: neutral.clone(), lifecycle: thread.lifecycle.clone() })
}

fn set_summary(summaries: &mut Vec<Summary>, summary: Summary) {
    match summaries.iter_mut().find(|s| s.id == summary.id) {
        Some(existing) => *existing = summary,
        None => summaries.push(summary),
    }
}

impl Engine {
    pub fn new(store: Box<dyn Store>, default_cwd: String, clock: Arc<dyn Clock>, ids: Arc<dyn IdSource>) -> Self {
        Self {
            store,
            default_cwd,
            state: Mutex::new(State { model: ReadModel::empty(&clock.now_iso()), summaries: Vec::new(), thread_listeners: Default::default(), shell_listeners: Vec::new() }),
            on_events: Mutex::new(None),
            clock,
            ids,
        }
    }

    /// The clock and the id source this engine mints with, for the translators and reactors that stamp the same way.
    pub fn clock(&self) -> Arc<dyn Clock> {
        Arc::clone(&self.clock)
    }

    pub fn ids(&self) -> Arc<dyn IdSource> {
        Arc::clone(&self.ids)
    }

    /// A fresh UUID from the engine's id source.
    pub fn new_id(&self) -> String {
        self.ids.uuid()
    }

    /// `n` random bytes as hex from the engine's id source.
    pub fn random_hex(&self, n: usize) -> String {
        self.ids.hex(n)
    }

    /// The side channel every committed batch of events goes to (the reactor); it must not block.
    pub fn set_on_events(&self, hook: Hook) {
        *self.on_events.lock().unwrap_or_else(|e| e.into_inner()) = Some(hook);
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Loads a thread the process has not seen yet from its snapshot and log.
    fn hydrate(&self, st: &mut State, thread_id: &str) -> Result<(), String> {
        if st.model.threads.contains_key(thread_id) {
            return Ok(());
        }
        let snapshot = self.store.read_snapshot(thread_id)?;
        let events = self.store.read_events(thread_id, None)?;
        if let Some(thread) = snapshot {
            st.model.threads.insert(thread_id.to_string(), thread);
        }
        let base = st.model.snapshot_sequence;
        for event in &events {
            project(&mut st.model, event);
            st.model.snapshot_sequence = base.max(event.sequence);
        }
        if let Some(thread) = st.model.threads.get(thread_id) {
            let summary = thread.summary();
            set_summary(&mut st.summaries, summary);
        }
        Ok(())
    }

    fn commit(&self, st: &mut State, command_id: &str, thread_id: &str, events: Vec<Event>) -> Result<Receipt, String> {
        if !events.is_empty() {
            self.store.append(thread_id, &events)?;
        }
        for event in &events {
            project(&mut st.model, event);
        }
        if let Some(thread) = st.model.threads.get(thread_id) {
            let summary = thread.summary();
            set_summary(&mut st.summaries, summary);
        }
        for event in &events {
            if let Some(listeners) = st.thread_listeners.get_mut(thread_id) {
                listeners.retain(|tx| tx.send(ThreadItem::Event(event.clone())).is_ok());
            }
            st.shell_listeners.retain(|tx| tx.send(ShellItem::Event(event.clone())).is_ok());
        }
        if st.thread_listeners.get(thread_id).is_some_and(Vec::is_empty) {
            st.thread_listeners.remove(thread_id);
        }
        let hook = self.on_events.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(hook) = hook {
            hook(&events);
        }
        Ok(Receipt { command_id: command_id.to_string(), accepted: true, events, error: None })
    }

    /// Decides the command against the thread's state and commits what it decides under the one lock, so no two commands mint the same number. A
    /// fork or a merge back also needs the thread it names: it is loaded from its log first, and a fork is given that log to copy.
    pub fn dispatch(&self, command: Command, actor: &str) -> Result<Receipt, String> {
        let mut st = self.lock();
        self.hydrate(&mut st, &command.thread_id)?;
        let other = match &command.body {
            Body::Fork { source, .. } | Body::MergeBack { source, .. } => Some(source.clone()),
            _ => None,
        };
        let mut source_events = Vec::new();
        if let Some(source) = &other {
            self.hydrate(&mut st, source)?;
            if matches!(command.body, Body::Fork { .. }) {
                source_events = self.store.read_events(source, None)?;
            }
        }
        let (now, id) = (|| self.clock.now_iso(), || self.ids.uuid());
        let ctx = Context { now: &now, new_id: &id, default_cwd: &self.default_cwd, actor, now_ms: self.clock.now_ms(), source_events: &source_events };
        if command.is_internal() {
            let events = materialize_internal(&st.model, &command, &ctx);
            return self.commit(&mut st, &command.command_id, &command.thread_id, events);
        }
        match decide(&st.model, &command, &ctx) {
            Decision::Rejected { code, message } => Ok(Receipt { command_id: command.command_id, accepted: false, events: Vec::new(), error: Some((code, message)) }),
            Decision::Accepted(events) => self.commit(&mut st, &command.command_id, &command.thread_id, events),
        }
    }

    pub fn with_thread<R>(&self, thread_id: &str, f: impl FnOnce(Option<&Thread>) -> R) -> Result<R, String> {
        let mut st = self.lock();
        self.hydrate(&mut st, thread_id)?;
        Ok(f(st.model.threads.get(thread_id)))
    }

    pub fn get_thread(&self, thread_id: &str) -> Result<Option<Thread>, String> {
        self.with_thread(thread_id, |t| t.cloned())
    }

    pub fn list_threads(&self) -> Vec<Summary> {
        self.lock().summaries.clone()
    }

    /// A thread of the agent API as the engine holds it: its summary and what the agent API keeps beside it. None for an id that was not created
    /// by a `NeutralCreate` (a thread of the thread view is not one).
    pub fn neutral_thread(&self, thread_id: &str) -> Result<Option<NeutralView>, String> {
        let mut st = self.lock();
        self.hydrate(&mut st, thread_id)?;
        Ok(neutral_view(&st, thread_id))
    }

    /// Every thread of the agent API that is not deleted, the one with the newest activity first (the later created first when two have the same
    /// time). An archived thread is listed: the lifecycle says it is archived.
    pub fn neutral_threads(&self) -> Result<Vec<NeutralView>, String> {
        let mut st = self.lock();
        self.hydrate_all(&mut st)?;
        let ids: Vec<String> = st.summaries.iter().map(|s| s.id.clone()).collect();
        let mut views: Vec<NeutralView> = ids.iter().filter_map(|id| neutral_view(&st, id)).filter(|v| v.thread.deleted_at.is_none()).collect();
        views.reverse();
        views.sort_by(|a, b| b.thread.updated_at.cmp(&a.thread.updated_at));
        Ok(views)
    }

    fn hydrate_all(&self, st: &mut State) -> Result<(), String> {
        let ids: Vec<String> = st.summaries.iter().map(|s| s.id.clone()).collect();
        for id in &ids {
            self.hydrate(st, id)?;
        }
        Ok(())
    }

    /// The threads whose title or a message holds every word of `query` (`domain::queries::search`), at most `limit`.
    pub fn search_threads(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, String> {
        let mut st = self.lock();
        self.hydrate_all(&mut st)?;
        Ok(queries::search(&st.model, query, limit))
    }

    /// The root of the thread's family and the live threads under it, the oldest first; no root and no threads for a thread nobody created.
    pub fn thread_family(&self, thread_id: &str) -> Result<(Option<String>, Vec<NeutralView>), String> {
        let mut st = self.lock();
        self.hydrate_all(&mut st)?;
        self.hydrate(&mut st, thread_id)?;
        let (root, members) = queries::family(&st.model, thread_id);
        Ok((root, members.iter().filter_map(|id| neutral_view(&st, id)).collect()))
    }

    /// A page of the archived or the settled threads (`domain::queries::shelf`) with the views of the threads on it, in the page's order.
    pub fn thread_shelf(&self, which: Shelf, before: Option<&Cursor>, limit: usize) -> Result<(ShelfPage, Vec<NeutralView>), String> {
        let mut st = self.lock();
        self.hydrate_all(&mut st)?;
        let page = queries::shelf(&st.model, which, before, limit);
        let views = page.rows.iter().filter_map(|row| neutral_view(&st, &row.thread)).collect();
        Ok((page, views))
    }

    /// The neutral events of the thread after `after`, oldest first, and the sequence of its newest; None for a thread that is not the agent API's.
    pub fn neutral_events(&self, thread_id: &str, after: i64) -> Result<Option<(Vec<Event>, i64)>, String> {
        let mut st = self.lock();
        self.hydrate(&mut st, thread_id)?;
        let Some(neutral) = st.model.neutral.get(thread_id) else { return Ok(None) };
        let events = self.store.read_events(thread_id, Some(after))?.into_iter().filter(|e| e.kind == NEUTRAL_EVENT).collect();
        Ok(Some((events, neutral.last_seq)))
    }

    pub fn get_turn_diff(&self, thread_id: &str, turn_id: &str) -> Result<Option<String>, String> {
        let mut st = self.lock();
        self.hydrate(&mut st, thread_id)?;
        Ok(st.model.turn_diffs.get(turn_id).cloned())
    }

    /// The thread's working directory, for the adapters; None when the engine has never seen the thread.
    pub fn cwd_of(&self, thread_id: &str) -> Option<String> {
        let mut st = self.lock();
        self.hydrate(&mut st, thread_id).ok()?;
        st.model.threads.get(thread_id).map(|t| t.cwd.clone())
    }

    /// The channel gets the snapshot (or the events after `after`), then `Synchronized`, then every later event. Dropping the
    /// receiver unsubscribes.
    pub fn subscribe_thread(&self, thread_id: &str, after: Option<i64>) -> Result<Receiver<ThreadItem>, String> {
        let (tx, rx) = unbounded_channel();
        self.subscribe_thread_with(thread_id, after, tx)?;
        Ok(rx)
    }

    /// `subscribe_thread` into a channel the caller made, so it can also put an item on it (to wake whoever reads it).
    pub fn subscribe_thread_with(&self, thread_id: &str, after: Option<i64>, tx: Sender<ThreadItem>) -> Result<(), String> {
        let mut st = self.lock();
        match after {
            None => {
                self.hydrate(&mut st, thread_id)?;
                if let Some(thread) = st.model.threads.get(thread_id) {
                    let _ = tx.send(ThreadItem::Snapshot(Box::new(thread.clone())));
                }
            }
            Some(after) => {
                for event in self.store.read_events(thread_id, Some(after))? {
                    let _ = tx.send(ThreadItem::Event(event));
                }
            }
        }
        let _ = tx.send(ThreadItem::Synchronized);
        st.thread_listeners.entry(thread_id.to_string()).or_default().push(tx);
        Ok(())
    }

    pub fn subscribe_shell_with(&self, tx: Sender<ShellItem>) {
        let mut st = self.lock();
        let _ = tx.send(ShellItem::Snapshot(st.summaries.clone()));
        let _ = tx.send(ShellItem::Synchronized);
        st.shell_listeners.push(tx);
    }

    fn mark_interrupted(&self, thread_id: &str) -> Result<Option<StaleTurn>, String> {
        let mut st = self.lock();
        self.hydrate(&mut st, thread_id)?;
        let Some(thread) = st.model.threads.get(thread_id) else { return Ok(None) };
        let Some(turn) = thread.latest_turn.as_ref().filter(|t| t.state == "running") else { return Ok(None) };
        let stale = StaleTurn { thread_id: thread_id.to_string(), provider_session_id: thread.session.as_ref().and_then(|s| s.provider_session_id.clone()) };
        let turn_id = turn.turn_id.clone();
        let session = Session {
            cli: thread.session.as_ref().map_or_else(|| "opencode".to_string(), |s| s.cli.clone()),
            provider_session_id: thread.session.as_ref().and_then(|s| s.provider_session_id.clone()),
            status: "interrupted".into(),
            active_turn_id: Some(turn_id.clone()),
            last_error: None,
        };
        let (now, id) = (|| self.clock.now_iso(), || self.ids.uuid());
        let ctx = Context { now: &now, new_id: &id, default_cwd: &self.default_cwd, actor: "system", now_ms: self.clock.now_ms(), source_events: &[] };
        let set = Command { command_id: format!("system:restart:{}", self.ids.uuid()), thread_id: thread_id.to_string(), body: Body::SessionSet { session } };
        let events = materialize_internal(&st.model, &set, &ctx);
        self.commit(&mut st, &set.command_id, thread_id, events)?;
        let activity = Activity {
            id: self.ids.uuid(),
            tone: "info".into(),
            kind: "turn.interrupted-on-restart".into(),
            summary: "Turn interrupted on restart".into(),
            payload: json!({ "turnId": turn_id, "reason": "engine restarted" }),
            turn_id: Some(turn_id),
            sequence: None,
            created_at: self.clock.now_iso(),
        };
        let append = Command { command_id: format!("system:restart-activity:{}", self.ids.uuid()), thread_id: thread_id.to_string(), body: Body::ActivityAppend { activity } };
        let events = materialize_internal(&st.model, &append, &ctx);
        self.commit(&mut st, &append.command_id, thread_id, events)?;
        Ok(Some(stale))
    }

    /// Lists the threads on disk (the index first, then every log the index does not know, which is every one in practice: the
    /// TypeScript engine never wrote the index), and interrupts the ones a crash left with a running turn.
    /// Returns the turns it interrupted, so the caller can tell the agent that still runs them.
    pub fn start(&self) -> Result<Vec<StaleTurn>, String> {
        let mut known = self.store.list_summaries()?;
        for id in self.store.list_thread_ids()? {
            if known.iter().any(|s| s.id == id) {
                continue;
            }
            let mut scratch = ReadModel::empty(&self.clock.now_iso());
            let base = scratch.snapshot_sequence;
            let events = match self.store.read_events(&id, None) {
                Ok(events) => events,
                Err(e) => {
                    tracing::warn!(thread_id = %id, error = %e, "engine.start.thread_skipped");
                    continue;
                }
            };
            for event in events {
                project(&mut scratch, &event);
                scratch.snapshot_sequence = base.max(event.sequence);
            }
            if let Some(thread) = scratch.threads.get(&id) {
                known.push(thread.summary());
            }
        }
        {
            let mut st = self.lock();
            for summary in &known {
                set_summary(&mut st.summaries, summary.clone());
            }
        }
        let mut stale = Vec::new();
        for summary in &known {
            if summary.latest_turn.as_ref().is_some_and(|t| t.state == "running") {
                match self.mark_interrupted(&summary.id) {
                    Ok(turn) => stale.extend(turn),
                    Err(e) => tracing::warn!(thread_id = %summary.id, error = %e, "engine.start.thread_skipped"),
                }
            }
        }
        Ok(stale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::store::MemoryStore;
    use crate::testing::{CountingIds, FixedClock};
    use serde_json::{json, Value};

    fn engine_over(store: Arc<MemoryStore>) -> Engine {
        Engine::new(Box::new(store), "/work".into(), Arc::new(FixedClock(1_700_000_000_000)), Arc::new(CountingIds::default()))
    }

    fn create(engine: &Engine, id: &str, workspace: Option<&str>) {
        let body = Body::NeutralCreate { title: format!("title of {id}"), workspace: workspace.map(String::from), driver: Some("claudeAgent".into()), model: None };
        let receipt = engine.dispatch(Command { command_id: format!("c-{id}"), thread_id: id.into(), body }, "agent").unwrap();
        assert!(receipt.accepted, "{:?}", receipt.error);
    }

    fn record(engine: &Engine, id: &str, turn: Option<&str>, event: Value) -> i64 {
        let body = Body::NeutralEvent { turn_id: turn.map(String::from), event };
        engine.dispatch(Command { command_id: "e".into(), thread_id: id.into(), body }, "agent").unwrap().events[0].sequence
    }

    #[test]
    fn given_threads_that_interleave_their_events_when_read_then_each_sees_only_its_own_in_the_one_sequence_with_the_gaps_the_other_leaves() {
        // Given two threads of the agent API and a thread of the thread view, whose events are recorded alternately
        let engine = engine_over(Arc::default());
        create(&engine, "a", Some("w1"));
        create(&engine, "b", None);
        let view = Body::Create { title: "view".into(), cwd: None, model_selection: ModelSelection { cli: "opencode".into(), provider_id: None, model_id: None }, runtime_mode: "full-access".into(), interaction_mode: "default".into(), branch: None, worktree_path: None };
        engine.dispatch(Command { command_id: "v".into(), thread_id: "v".into(), body: view }, "client").unwrap();
        let seqs = [
            record(&engine, "a", Some("u1"), json!({"type": "turn.started"})),
            record(&engine, "b", Some("u2"), json!({"type": "turn.started"})),
            record(&engine, "a", Some("u1"), json!({"type": "message.delta", "role": "assistant", "text": "x"})),
        ];
        // When each thread's neutral events are read from the start and after its first
        let (a_all, a_last) = engine.neutral_events("a", 0).unwrap().unwrap();
        let (a_after, _) = engine.neutral_events("a", seqs[0]).unwrap().unwrap();
        let (b_all, b_last) = engine.neutral_events("b", 0).unwrap().unwrap();
        // Then a has its two events with the sequence numbers they were given, b its one, and the newest of each is its own
        assert_eq!(a_all.iter().map(|e| e.sequence).collect::<Vec<_>>(), [seqs[0], seqs[2]]);
        assert_eq!((a_last, b_all.iter().map(|e| e.sequence).collect::<Vec<_>>(), b_last), (seqs[2], vec![seqs[1]], seqs[1]));
        assert_eq!(a_after.iter().map(|e| e.sequence).collect::<Vec<_>>(), [seqs[2]]);
        assert!(seqs[2] - seqs[0] > 1, "b's event sits between a's, so a's sequence has a gap: {seqs:?}");
        // And the thread of the thread view is no thread of the agent API, and a thread nobody created is none either
        assert!(engine.neutral_events("v", 0).unwrap().is_none() && engine.neutral_thread("v").unwrap().is_none() && engine.neutral_thread("nope").unwrap().is_none());
        assert_eq!(engine.neutral_threads().unwrap().iter().map(|t| t.thread.id.as_str()).collect::<Vec<_>>(), ["b", "a"]);
    }

    #[test]
    fn given_turns_that_end_for_each_reason_when_the_thread_is_read_then_it_runs_until_the_turn_finishes_and_a_late_finish_settles_nothing() {
        let engine = engine_over(Arc::default());
        create(&engine, "t", Some("w1"));
        let state = |engine: &Engine| engine.neutral_thread("t").unwrap().unwrap().thread.latest_turn.map(|turn| (turn.turn_id, turn.state));
        assert_eq!(state(&engine), None);
        for (reason, settled) in [("completed", "completed"), ("interrupted", "interrupted"), ("failed", "error")] {
            // Given a turn that started
            let turn = format!("turn-{reason}");
            record(&engine, "t", Some(&turn), json!({"type": "turn.started"}));
            // Then the thread runs it
            assert_eq!(state(&engine), Some((turn.clone(), "running".into())), "{reason}");
            // When it finishes for the reason, Then the thread has settled it that way
            record(&engine, "t", Some(&turn), json!({"type": "turn.finished", "reason": reason}));
            assert_eq!(state(&engine), Some((turn.clone(), settled.into())), "{reason}");
        }
        // And a finish that names a turn other than the running one leaves the running one running
        record(&engine, "t", Some("current"), json!({"type": "turn.started"}));
        record(&engine, "t", Some("old"), json!({"type": "turn.finished", "reason": "completed"}));
        assert_eq!(state(&engine), Some(("current".into(), "running".into())));
    }

    #[test]
    fn given_a_log_on_a_store_when_another_engine_opens_it_then_the_threads_defaults_turn_and_newest_sequence_are_the_same() {
        // Given an engine whose threads ran, over a store that outlives it
        let store: Arc<MemoryStore> = Arc::default();
        let first = engine_over(store.clone());
        create(&first, "t", Some("w1"));
        record(&first, "t", Some("u1"), json!({"type": "turn.started"}));
        let newest = record(&first, "t", Some("u1"), json!({"type": "message.delta", "role": "assistant", "text": "hi"}));
        drop(first);
        // When an engine opens the same log and starts (as the service does after a restart)
        let second = engine_over(store);
        let stale = second.start().unwrap();
        // Then the thread has the defaults it was created with, the sequence of its newest event, and the turn the crash left running was interrupted
        let view = second.neutral_thread("t").unwrap().unwrap();
        assert_eq!((view.neutral.workspace.as_deref(), view.neutral.driver.as_deref(), view.neutral.last_seq), (Some("w1"), Some("claudeAgent"), newest));
        assert_eq!(stale.iter().map(|s| s.thread_id.as_str()).collect::<Vec<_>>(), ["t"]);
        assert_eq!(view.thread.latest_turn.map(|t| t.state), Some("interrupted".to_string()));
        // And the next event of the thread continues the sequence
        assert!(record(&second, "t", None, json!({"type": "usage", "input_tokens": 1, "output_tokens": 1})) > newest);
    }

    #[test]
    fn given_a_thread_when_it_is_created_again_then_the_engine_refuses_it() {
        let engine = engine_over(Arc::default());
        create(&engine, "t", None);
        let again = Body::NeutralCreate { title: "other".into(), workspace: None, driver: None, model: None };
        let receipt = engine.dispatch(Command { command_id: "c2".into(), thread_id: "t".into(), body: again }, "agent").unwrap();
        assert_eq!((receipt.accepted, receipt.error.map(|e| e.0)), (false, Some("thread_exists".to_string())));
    }

    fn ask(engine: &Engine, thread: &str, body: Body) -> Receipt {
        engine.dispatch(Command { command_id: "c".into(), thread_id: thread.into(), body }, "agent").unwrap()
    }

    fn turn(engine: &Engine, thread: &str, id: &str, prompt: &str, reply: &str, end: Option<&str>) {
        record(engine, thread, Some(id), json!({"type": "turn.started"}));
        record(engine, thread, Some(id), json!({"type": "message.delta", "role": "user", "text": prompt}));
        record(engine, thread, Some(id), json!({"type": "message.delta", "role": "assistant", "text": reply}));
        if let Some(reason) = end {
            record(engine, thread, Some(id), json!({"type": "turn.finished", "reason": reason}));
        }
    }

    fn kinds(engine: &Engine, thread: &str) -> Vec<String> {
        engine.neutral_events(thread, 0).unwrap().unwrap().0.iter().map(|e| e.payload["event"]["type"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn given_a_source_thread_on_a_store_when_a_fresh_engine_forks_it_then_it_loads_the_source_from_its_log_and_the_fork_survives_a_restart_with_its_turns_and_lineage() {
        // Given a source with a completed turn and a running one, written by an engine that is gone
        let store: Arc<MemoryStore> = Arc::default();
        let before = engine_over(store.clone());
        create(&before, "src", Some("w1"));
        turn(&before, "src", "u1", "one", "first", Some("completed"));
        turn(&before, "src", "u2", "two", "partial", None);
        let source_newest = before.neutral_thread("src").unwrap().unwrap().neutral.last_seq;
        drop(before);
        // When an engine that has not seen the source forks it (naming the running turn first), and an engine over the same log reads the fork
        let engine = engine_over(store.clone());
        let running = ask(&engine, "fork", Body::Fork { source: "src".into(), at_turn: Some("u2".into()), title: None });
        let forked = ask(&engine, "fork", Body::Fork { source: "src".into(), at_turn: None, title: None });
        let missing = ask(&engine, "other", Body::Fork { source: "nobody".into(), at_turn: None, title: None });
        let reopened = engine_over(store);
        reopened.start().unwrap();
        // Then the running turn is refused and so is a source nobody has, and the fork was decided and kept
        assert_eq!((running.accepted, running.error.map(|e| e.0)), (false, Some("FORK_SOURCE_RUNNING".to_string())));
        assert_eq!((missing.accepted, missing.error.map(|e| e.0)), (false, Some("THREAD_NOT_FOUND".to_string())));
        assert!(forked.accepted, "{:?}", forked.error);
        assert_eq!(forked.events[0].kind, "thread.created");
        // And its events are the marker and the finished turn's four, numbered after everything the source had, and the source's own are untouched
        assert_eq!(kinds(&engine, "fork"), ["thread.forked", "turn.started", "message.delta", "message.delta", "turn.finished"]);
        let fork_seqs: Vec<i64> = engine.neutral_events("fork", 0).unwrap().unwrap().0.iter().map(|e| e.sequence).collect();
        assert!(fork_seqs[0] > source_newest && fork_seqs.windows(2).all(|w| w[0] < w[1]), "{fork_seqs:?} after {source_newest}");
        assert_eq!(kinds(&engine, "src").len(), 7);
        // And the reopened engine has the fork's lineage, title and turn from its log, and the source is not changed by being forked
        let view = reopened.neutral_thread("fork").unwrap().unwrap();
        assert_eq!(view.thread.title, "title of src fork");
        assert_eq!(view.lifecycle.lineage.as_ref().map(|l| (l.parent.as_deref(), l.root.as_str())), Some((Some("src"), "src")));
        assert_eq!(view.neutral.turns.iter().map(|t| (t.id.as_str(), t.state.as_str(), t.prompt.as_str())).collect::<Vec<_>>(), [("u1", "completed", "one")]);
        assert_eq!(view.neutral.inherited, ["u1"]);
    }

    #[test]
    fn given_queue_and_settle_commands_when_dispatched_then_a_refusal_is_a_receipt_with_the_contracts_code_and_what_was_decided_is_in_the_thread_s_state_after_a_restart() {
        // Given a thread, a prompt added with no item id (the engine names it), and one added with a name
        let store: Arc<MemoryStore> = Arc::default();
        let engine = engine_over(store.clone());
        create(&engine, "t", Some("w1"));
        let add = |item: Option<&str>, prompt: &str| Body::QueueAdd { item: item.map(String::from), prompt: prompt.into(), attachments: vec![], workspace: None, driver: Some("claudeAgent".into()), model: None, runtime_mode: None, interaction_mode: None };
        let named = ask(&engine, "t", add(None, "first"));
        ask(&engine, "t", add(Some("q2"), "second"));
        // When it is snoozed until the instant of the clock, then until a moment after, and a queued prompt is started while the queue is not first in line
        let now = 1_700_000_000_000u64;
        let past = ask(&engine, "t", Body::SettleSet { to: crate::domain::lifecycle::SettleTo::Snoozed, until_ms: Some(now), at_ms: None });
        let queued = ask(&engine, "t", Body::SettleSet { to: crate::domain::lifecycle::SettleTo::Snoozed, until_ms: Some(now + 1), at_ms: None });
        let out_of_turn = ask(&engine, "t", Body::QueueStart { item: "q2".into() });
        // Then the engine named the item, the snoozes were refused (the first as not in the future, the second as queued), and so was the start
        assert!(named.accepted);
        let item = named.events[0].payload["event"]["item"]["item"].as_str().unwrap().to_string();
        assert!(!item.is_empty() && item != "q2", "{item}");
        assert_eq!(past.error.map(|e| e.0), Some("SNOOZE_NOT_FUTURE".to_string()));
        assert_eq!(queued.error.map(|e| e.0), Some("SNOOZE_QUEUED".to_string()));
        assert_eq!((out_of_turn.accepted, out_of_turn.error.map(|e| e.0)), (false, Some("ITEM_NOT_QUEUED".to_string())));
        // And the queue is what was decided, in a fresh engine over the same log too, with the time the add was made
        let reopened = engine_over(store);
        for engine in [&engine, &reopened] {
            let view = engine.neutral_thread("t").unwrap().unwrap();
            assert_eq!(view.lifecycle.queue.iter().map(|q| q.item.as_str()).collect::<Vec<_>>(), [item.as_str(), "q2"]);
            assert_eq!(view.lifecycle.queue[0].created_at_ms, now);
        }
    }

    #[test]
    fn given_threads_of_several_kinds_when_searched_listed_and_shelved_through_a_fresh_engine_then_it_hydrates_them_all_and_deleted_ones_leave_the_list_and_archived_ones_the_search() {
        // Given a log of threads: one titled for the search, one archived, one deleted and one settled, written by an engine that is gone
        let store: Arc<MemoryStore> = Arc::default();
        let before = engine_over(store.clone());
        for id in ["login", "archived", "deleted", "settled"] {
            create(&before, id, Some("w1"));
        }
        for id in ["login", "archived", "deleted"] {
            ask(&before, id, Body::MetaUpdate { title: Some(format!("{id} fix")), model_selection: None, cwd: None, branch: None, worktree_path: None });
        }
        ask(&before, "archived", Body::LifecycleArchive { archive: true });
        ask(&before, "deleted", Body::LifecycleDelete { only_if_unused: false });
        ask(&before, "settled", Body::SettleSet { to: crate::domain::lifecycle::SettleTo::Settled, until_ms: None, at_ms: None });
        drop(before);
        // When a fresh engine starts over the log and is searched, listed and asked for the shelves
        let engine = engine_over(store);
        engine.start().unwrap();
        let hits = engine.search_threads("fix", 10).unwrap();
        let listed: Vec<String> = engine.neutral_threads().unwrap().iter().map(|v| v.thread.id.clone()).collect();
        let (archived, archived_views) = engine.thread_shelf(Shelf::Archived, None, 10).unwrap();
        let (settled, _) = engine.thread_shelf(Shelf::Settled, None, 10).unwrap();
        let (root, family) = engine.thread_family("login").unwrap();
        // Then the search finds the live titled thread only, the list has everything but the deleted one, and the shelves hold their own
        assert_eq!(hits.iter().map(|h| h.thread.as_str()).collect::<Vec<_>>(), ["login"]);
        let mut listed_sorted = listed;
        listed_sorted.sort();
        assert_eq!(listed_sorted, ["archived", "login", "settled"]);
        assert_eq!((archived.total, archived_views[0].thread.id.as_str()), (1, "archived"));
        assert_eq!(settled.rows.iter().map(|r| r.thread.as_str()).collect::<Vec<_>>(), ["settled"]);
        assert_eq!((root.as_deref(), family.iter().map(|v| v.thread.id.as_str()).collect::<Vec<_>>()), (Some("login"), vec!["login"]));
    }
}
