//! The `service.panes` group (`agent-api-service`): the agents of the service as a person's list shows them. Each thread is a pane; the service
//! learns what its turns do from the events the engine records (`Panes::hear`) and keeps, per thread, the state and when it began, when its finish
//! was last marked read, and when it was cleared. A `PaneStore` keeps the records across restarts. The thread's title, workspace and whether it
//! runs are the engine's, read when the list is.
use super::activity::ActivityClock;
use agent_api_agent::{FinishReason, Status, ThreadEvent};
use agent_api_proto::{GroupName, Response};
use agent_api_service::panes::{self as api, thread_of, ListParams, ListResult, Mark, MarkParams, MarkResult, PaneState, PaneView};
use async_trait::async_trait;
use rca_agent_core::{Call, Group, Served};
use rca_group_agent::ThreadEngine;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, MutexGuard};

const SERVED: [Served; 2] = [Served::service(api::LIST, &[]), Served::service(api::MARK, &[])];

/// What the service kept of one pane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneRecord {
    pub thread: String,
    pub state: PaneState,
    pub state_started_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleared_at_ms: Option<u64>,
}

/// Where the records are kept: the adapter keeps them across restarts.
pub trait PaneStore: Send + Sync {
    fn load(&self) -> Result<Vec<PaneRecord>, String>;
    /// Replaces what is kept with `all`, whole or not at all.
    fn save(&self, all: &[PaneRecord]) -> Result<(), String>;
}

/// The store a service has until it is given another: it keeps the records for the life of the process.
#[derive(Default)]
pub struct MemoryPanes(Mutex<Vec<PaneRecord>>);

impl PaneStore for MemoryPanes {
    fn load(&self) -> Result<Vec<PaneRecord>, String> {
        Ok(self.0.lock().unwrap_or_else(|p| p.into_inner()).clone())
    }

    fn save(&self, all: &[PaneRecord]) -> Result<(), String> {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = all.to_vec();
        Ok(())
    }
}

pub struct Panes {
    clock: Arc<dyn ActivityClock>,
    store: Arc<dyn PaneStore>,
    engine: Arc<dyn ThreadEngine>,
    records: Mutex<Vec<PaneRecord>>,
}

/// A refusal of a use case: the code and message of the answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub code: String,
    pub message: String,
}

impl Panes {
    /// The records come from `store`; one that cannot be read starts the service with none (and is logged), never refuses to start it.
    pub fn new(clock: Arc<dyn ActivityClock>, store: Arc<dyn PaneStore>, engine: Arc<dyn ThreadEngine>) -> Arc<Panes> {
        let records = store.load().unwrap_or_else(|why| {
            tracing::warn!(event = "panes.load_failed", reason = %why);
            Vec::new()
        });
        Arc::new(Panes { clock, store, engine, records: Mutex::new(records) })
    }

    fn records(&self) -> MutexGuard<'_, Vec<PaneRecord>> {
        self.records.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn save(&self, records: &[PaneRecord]) {
        if let Err(why) = self.store.save(records) {
            tracing::warn!(event = "panes.save_failed", reason = %why);
        }
    }

    /// What an event of a thread's turn says the pane is doing; none for an event that says nothing about it.
    fn state_after(event: &ThreadEvent) -> Option<PaneState> {
        match event {
            ThreadEvent::TurnStarted {} => Some(PaneState::Working),
            ThreadEvent::ApprovalRequested { .. } => Some(PaneState::Blocked),
            ThreadEvent::InputRequested { .. } => Some(PaneState::Waiting),
            ThreadEvent::TurnFinished { reason: FinishReason::Completed, .. } => Some(PaneState::Done),
            ThreadEvent::TurnFinished { reason: FinishReason::Failed, .. } => Some(PaneState::Failed),
            ThreadEvent::TurnFinished { reason: FinishReason::Interrupted, .. } => Some(PaneState::Interrupted),
            _ => None,
        }
    }

    /// Moves the pane of `thread` to the state the event says, if it says one that is not the pane's already.
    pub fn hear(&self, thread: &str, event: &ThreadEvent) {
        let Some(state) = Panes::state_after(event) else { return };
        let now = self.clock.now_ms();
        let mut records = self.records();
        match records.iter_mut().find(|r| r.thread == thread) {
            Some(record) if record.state == state => return,
            Some(record) => {
                record.state = state;
                record.state_started_at_ms = now;
            }
            None => records.push(PaneRecord { thread: thread.into(), state, state_started_at_ms: now, read_at_ms: None, cleared_at_ms: None }),
        }
        self.save(&records);
    }

    fn view(&self, thread: &agent_api_agent::ThreadView, record: Option<&PaneRecord>) -> PaneView {
        let state = record.map_or(if thread.status == Status::Running { PaneState::Working } else { PaneState::Idle }, |r| r.state);
        let started = record.map_or(0, |r| r.state_started_at_ms);
        let finished_news = matches!(state, PaneState::Done | PaneState::Failed);
        PaneView {
            pane: api::pane_key(&thread.id),
            thread: thread.id.clone(),
            title: thread.title.clone(),
            workspace: thread.workspace.clone(),
            state,
            listed: record.is_none_or(|r| r.cleared_at_ms.is_none_or(|cleared| r.state_started_at_ms > cleared)),
            unread: finished_news && record.is_some_and(|r| r.read_at_ms.is_none_or(|read| read < r.state_started_at_ms)),
            clearable: state.finished(),
            sort_at_ms: started,
            state_started_at_ms: started,
        }
    }

    /// Every pane of the service (of one workspace when asked), newest change first; a thread the engine no longer has is not a pane.
    pub fn list(&self, workspace: Option<&str>) -> Result<Vec<PaneView>, Failure> {
        let threads = self.engine.list().map_err(|e| Failure { code: "INTERNAL".into(), message: e.0 })?;
        let records = self.records().clone();
        let mut panes: Vec<PaneView> = threads.iter().filter(|t| workspace.is_none_or(|w| t.workspace.as_deref() == Some(w))).map(|t| self.view(t, records.iter().find(|r| r.thread == t.id))).collect();
        panes.sort_by(|a, b| b.sort_at_ms.cmp(&a.sort_at_ms).then_with(|| a.pane.cmp(&b.pane)));
        Ok(panes)
    }

    /// Marks the panes named that are panes of the service (and, for a clear, are over); how many were marked. A record is made only for a mark that
    /// applies, and the records of threads the engine no longer lists are dropped.
    pub fn mark(&self, panes: &[String], mark: Mark) -> Result<usize, Failure> {
        let known: Vec<String> = self.engine.list().map_err(|e| Failure { code: "INTERNAL".into(), message: e.0 })?.into_iter().map(|t| t.id).collect();
        let now = self.clock.now_ms();
        let mut records = self.records();
        let kept = records.len();
        records.retain(|r| known.contains(&r.thread));
        let pruned = records.len() != kept;
        let (mut marked, mut changed) = (0, pruned);
        for thread in panes.iter().filter_map(|p| thread_of(p)).filter(|t| known.iter().any(|k| k == t)) {
            let at = records.iter().position(|r| r.thread == thread);
            match (mark, at) {
                (Mark::Read, Some(at)) => records[at].read_at_ms = Some(now),
                (Mark::Read, None) => records.push(PaneRecord { thread: thread.into(), state: PaneState::Idle, state_started_at_ms: 0, read_at_ms: Some(now), cleared_at_ms: None }),
                (Mark::Unread, Some(at)) => records[at].read_at_ms = None,
                (Mark::Unread, None) => {
                    marked += 1;
                    continue;
                }
                (Mark::Clear, Some(at)) if records[at].state.finished() => records[at].cleared_at_ms = Some(now.max(records[at].state_started_at_ms)),
                (Mark::Clear, _) => continue,
            }
            marked += 1;
            changed = true;
        }
        if changed {
            self.save(&records);
        }
        Ok(marked)
    }
}

pub struct PanesGroup {
    panes: Arc<Panes>,
}

impl PanesGroup {
    pub fn new(panes: Arc<Panes>) -> PanesGroup {
        PanesGroup { panes }
    }
}

#[async_trait]
impl Group for PanesGroup {
    fn name(&self) -> GroupName {
        api::GROUP
    }

    fn methods(&self) -> &[Served] {
        &SERVED
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let Call { id, method, params, .. } = call;
        let answered = match method {
            api::LIST => match ListParams::parse(params) {
                Ok(p) => self.panes.list(p.workspace.as_deref()).map(|panes| serde_json::to_value(ListResult { panes }).unwrap_or_default()),
                Err(e) => return e.response(id),
            },
            api::MARK => match MarkParams::parse(params) {
                Ok(p) => self.panes.mark(&p.panes, p.mark).map(|marked| serde_json::to_value(MarkResult { marked }).unwrap_or_default()),
                Err(e) => return e.response(id),
            },
            other => unreachable!("the panes group does not serve {}", other.name()),
        };
        match answered {
            Ok(result) => Response::result(id, result),
            Err(failure) => Response::error(id, &failure.code, failure.message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rca_group_agent::testing::MemoryThreads;
    use rca_group_agent::NewThread;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Tick(AtomicU64);

    impl ActivityClock for Tick {
        fn now_ms(&self) -> u64 {
            self.0.fetch_add(10, Ordering::SeqCst)
        }
    }

    #[derive(Default)]
    struct Kept(Mutex<Vec<PaneRecord>>, AtomicU64);

    impl PaneStore for Kept {
        fn load(&self) -> Result<Vec<PaneRecord>, String> {
            Ok(self.0.lock().unwrap().clone())
        }

        fn save(&self, all: &[PaneRecord]) -> Result<(), String> {
            self.1.fetch_add(1, Ordering::SeqCst);
            *self.0.lock().unwrap() = all.to_vec();
            Ok(())
        }
    }

    fn rig() -> (Arc<MemoryThreads>, Arc<Kept>, Arc<Panes>) {
        let engine = Arc::new(MemoryThreads::default());
        for (id, workspace) in [("a", "w1"), ("b", "w2")] {
            engine.create(NewThread { id: id.into(), title: format!("thread {id}"), workspace: Some(workspace.into()), driver: None, model: None }).unwrap();
        }
        let store = Arc::new(Kept::default());
        let panes = Panes::new(Arc::new(Tick(AtomicU64::new(1000))), store.clone(), engine.clone());
        (engine, store, panes)
    }

    fn finish(reason: FinishReason) -> ThreadEvent {
        ThreadEvent::TurnFinished { reason, cause: None }
    }

    fn only(panes: &Panes, workspace: &str) -> PaneView {
        let mut list = panes.list(Some(workspace)).unwrap();
        assert_eq!(list.len(), 1);
        list.remove(0)
    }

    #[test]
    fn given_a_turn_that_finished_when_the_pane_is_read_then_cleared_then_changed_again_then_it_is_unread_then_read_then_unlisted_then_listed_again() {
        // Given a thread that never ran, and one whose turn started and completed
        let (_engine, _store, panes) = rig();
        let idle = only(&panes, "w2");
        assert_eq!((idle.state, idle.listed, idle.unread, idle.clearable, idle.pane.as_str()), (PaneState::Idle, true, false, true, "thread:b"));
        panes.hear("a", &ThreadEvent::TurnStarted {});
        let working = only(&panes, "w1");
        assert_eq!((working.state, working.clearable, working.unread), (PaneState::Working, false, false));
        panes.hear("a", &finish(FinishReason::Completed));
        // Then the pane is done, unread and clearable, and it sorts before the one that never ran
        let done = only(&panes, "w1");
        assert_eq!((done.state, done.unread, done.clearable, done.listed, done.workspace.as_deref(), done.title.as_str()), (PaneState::Done, true, true, true, Some("w1"), "thread a"));
        assert_eq!(panes.list(None).unwrap().iter().map(|p| p.thread.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        // And marking it read clears the unread, unread brings it back, clear unlists it, and a new turn lists it again
        assert_eq!(panes.mark(&["thread:a".into()], Mark::Read).unwrap(), 1);
        assert!(!only(&panes, "w1").unread);
        assert_eq!(panes.mark(&["thread:a".into()], Mark::Unread).unwrap(), 1);
        assert!(only(&panes, "w1").unread);
        assert_eq!(panes.mark(&["thread:a".into()], Mark::Clear).unwrap(), 1);
        assert!(!only(&panes, "w1").listed);
        panes.hear("a", &ThreadEvent::TurnStarted {});
        let again = only(&panes, "w1");
        assert_eq!((again.listed, again.state), (true, PaneState::Working));
    }

    #[test]
    fn given_panes_the_service_does_not_have_or_that_are_not_over_when_marked_then_only_the_clearable_known_ones_count_and_an_interrupt_is_not_news() {
        let (_engine, _store, panes) = rig();
        panes.hear("a", &ThreadEvent::TurnStarted {});
        // Then a running pane is not cleared, an unknown pane and a key that names no thread are ignored, and a read of a known one counts
        assert_eq!(panes.mark(&["thread:a".into(), "thread:ghost".into(), "tab:1:x".into()], Mark::Clear).unwrap(), 0);
        assert_eq!(panes.mark(&["thread:a".into(), "thread:ghost".into()], Mark::Read).unwrap(), 1);
        // And an interrupted turn leaves the pane over but not unread
        panes.hear("a", &finish(FinishReason::Interrupted));
        let stopped = only(&panes, "w1");
        assert_eq!((stopped.state, stopped.unread, stopped.clearable), (PaneState::Interrupted, false, true));
        // And a question and a permission request put the pane in the state that waits
        panes.hear("a", &ThreadEvent::InputRequested { request: "r".into(), prompt: "which?".into(), options: vec![] });
        assert_eq!(only(&panes, "w1").state, PaneState::Waiting);
        panes.hear("a", &ThreadEvent::ApprovalRequested { approval: "p".into(), tool: "bash".into(), args: serde_json::json!({}) });
        assert_eq!(only(&panes, "w1").state, PaneState::Blocked);
    }

    #[test]
    fn given_records_kept_in_a_store_when_the_service_restarts_then_the_new_panes_have_the_states_reads_and_clears() {
        // Given a pane that finished, was read and then cleared, in a store
        let (engine, store, panes) = rig();
        panes.hear("a", &finish(FinishReason::Failed));
        panes.mark(&["thread:a".into()], Mark::Read).unwrap();
        panes.mark(&["thread:a".into()], Mark::Clear).unwrap();
        // When a new Panes starts over the same store and engine
        let restarted = Panes::new(Arc::new(Tick(AtomicU64::new(9000))), store, engine);
        // Then it lists the pane as failed, read and unlisted
        let pane = only(&restarted, "w1");
        assert_eq!((pane.state, pane.unread, pane.listed), (PaneState::Failed, false, false));
    }

    #[test]
    fn given_marks_that_do_not_apply_when_marked_then_no_record_is_made_and_nothing_is_saved() {
        // Given panes that never ran and a store that has not been written
        let (_engine, store, panes) = rig();
        // When a running-or-idle pane is cleared, an unknown one is read and a pane with no record is marked unread
        assert_eq!(panes.mark(&["thread:a".into()], Mark::Clear).unwrap(), 0);
        assert_eq!(panes.mark(&["thread:ghost".into()], Mark::Read).unwrap(), 0);
        assert_eq!(panes.mark(&["thread:a".into()], Mark::Unread).unwrap(), 1);
        // Then the store holds no record and was never saved
        assert_eq!((store.0.lock().unwrap().len(), store.1.load(Ordering::SeqCst)), (0, 0));
        // And a read that applies makes the one record and saves once
        assert_eq!(panes.mark(&["thread:a".into()], Mark::Read).unwrap(), 1);
        assert_eq!((store.0.lock().unwrap().iter().map(|r| r.thread.clone()).collect::<Vec<_>>(), store.1.load(Ordering::SeqCst)), (vec!["a".to_string()], 1));
    }

    #[test]
    fn given_a_record_of_a_thread_the_engine_no_longer_lists_when_a_mark_is_made_then_that_record_is_dropped() {
        // Given a store holding a record for a thread the engine does not have and one for a thread it does
        let engine = Arc::new(MemoryThreads::default());
        engine.create(NewThread { id: "a".into(), title: "thread a".into(), workspace: None, driver: None, model: None }).unwrap();
        let store = Arc::new(Kept::default());
        let old = |thread: &str| PaneRecord { thread: thread.into(), state: PaneState::Done, state_started_at_ms: 5, read_at_ms: None, cleared_at_ms: None };
        store.save(&[old("gone"), old("a")]).unwrap();
        let panes = Panes::new(Arc::new(Tick(AtomicU64::new(1000))), store.clone(), engine);
        // When a mark that marks nothing is made
        assert_eq!(panes.mark(&["thread:ghost".into()], Mark::Read).unwrap(), 0);
        // Then the record of the missing thread is gone from the store and the other stays
        assert_eq!(store.0.lock().unwrap().iter().map(|r| r.thread.clone()).collect::<Vec<_>>(), ["a"]);
    }
}
