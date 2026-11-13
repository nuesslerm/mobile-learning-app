//! The thread engine as the `agent.threads` group needs it (`rca_group_agent::ThreadEngine`): the adapter over `remote-chat-agent-core`'s
//! engine, which decides every command, gives every event its sequence number and keeps the log. The agent API's `seq` is that number: a thread's
//! events carry the numbers the engine gave them, so the numbers of one thread have the gaps the events of others leave. Nothing here counts.
use agent_api_agent::{Status, ThreadEvent, ThreadView};
use agent_api_proto::Event as Wire;
use async_trait::async_trait;
use super::engine_lifecycle::{body, cursor, engine_shelf, hit_of, queue_item, shelf_result, state_of};
use agent_api_agent::lifecycle::{self, ThreadState};
use agent_api_agent::queue::QueueItem;
use rca_group_agent::{Command as Lifecycle, Decided, EngineError, Follow, NewThread, Page, Refused, Subscription, ThreadEngine};
use remote_chat_agent_core::application::engine::{Engine, NeutralView, NEUTRAL_EVENT};
use remote_chat_agent_core::domain::types::{Body, Command, Event, Session, ThreadItem};
use serde_json::{Map, Value};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

/// Who the engine's events say made them.
const ACTOR: &str = "agent-api";

type SettledHook = Arc<dyn Fn(&str) + Send + Sync>;

pub struct EngineThreads {
    engine: Arc<Engine>,
    settled: Mutex<Option<SettledHook>>,
}

impl EngineThreads {
    pub fn new(engine: Arc<Engine>) -> EngineThreads {
        EngineThreads { engine, settled: Mutex::new(None) }
    }
}

impl EngineThreads {
    /// Brings the engine up from its log, and settles what a crash left running: a turn that was running ends `interrupted` in the thread's events
    /// (the engine settles it in the thread's view already), so whoever follows the thread hears that it ended. Returns how many turns it settled.
    pub fn start(&self) -> Result<usize, String> {
        let mut settled = 0;
        let hook = match self.settled.lock() {
            Ok(hook) => hook.clone(),
            Err(poisoned) => {
                tracing::warn!(event = "agent_threads.settled_hook_poisoned");
                poisoned.into_inner().clone()
            }
        };
        for stale in self.engine.start()? {
            let Some(latest) = self.engine.neutral_thread(&stale.thread_id)?.and_then(|view| view.thread.latest_turn) else { continue };
            self.record(&stale.thread_id, Some(&latest.turn_id), ThreadEvent::TurnFinished { reason: agent_api_agent::FinishReason::Interrupted }).map_err(|e| e.0)?;
            settled += 1;
            if let Some(hook) = &hook {
                hook(&stale.thread_id);
            }
        }
        Ok(settled)
    }
}

fn failed(why: impl std::fmt::Display) -> EngineError {
    EngineError(why.to_string())
}

pub(super) fn view(thread: &NeutralView) -> ThreadView {
    let running = thread.thread.latest_turn.as_ref().filter(|turn| turn.state == "running");
    ThreadView {
        id: thread.thread.id.clone(),
        title: thread.thread.title.clone(),
        workspace: thread.neutral.workspace.clone(),
        driver: thread.neutral.driver.clone(),
        model: thread.neutral.model.clone(),
        status: if running.is_some() { Status::Running } else { Status::Idle },
        turn: running.map(|turn| turn.turn_id.clone()),
        last_seq: u64::try_from(thread.neutral.last_seq).unwrap_or(0),
        created_at: thread.thread.created_at.clone(),
        updated_at: thread.thread.updated_at.clone(),
    }
}

/// The agent API's event an engine event holds, or none for an event of another kind (a thread's creation, a restart's interruption).
pub(super) fn wire(event: &Event) -> Option<Wire> {
    if event.kind != NEUTRAL_EVENT {
        return None;
    }
    let neutral = event.payload.get("event")?.as_object()?;
    let kind = neutral.get("type")?.as_str()?.to_string();
    let data: Map<String, Value> = neutral.iter().filter(|(name, _)| name.as_str() != "type").map(|(name, value)| (name.clone(), value.clone())).collect();
    Some(Wire { seq: u64::try_from(event.sequence).unwrap_or(0), thread: Some(event.thread_id.clone()), turn: event.payload.get("turnId").and_then(Value::as_str).map(String::from), kind, data })
}

/// The live events of a thread: what the engine commits after the subscription, whichever kind, narrowed to the agent API's.
struct Live(UnboundedReceiver<ThreadItem>);

#[async_trait]
impl Follow for Live {
    async fn next(&mut self) -> Option<Wire> {
        loop {
            match self.0.recv().await? {
                ThreadItem::Event(event) => {
                    if let Some(wire) = wire(&event) {
                        return Some(wire);
                    }
                }
                ThreadItem::Snapshot(_) | ThreadItem::Synchronized => {}
            }
        }
    }
}

impl ThreadEngine for EngineThreads {
    fn create(&self, new: NewThread) -> Result<ThreadView, EngineError> {
        if let Some(existing) = self.engine.neutral_thread(&new.id).map_err(failed)? {
            return Ok(view(&existing));
        }
        let body = Body::NeutralCreate { title: new.title, workspace: new.workspace, driver: new.driver, model: new.model };
        let receipt = self.engine.dispatch(Command { command_id: format!("agent-api:create:{}", self.engine.new_id()), thread_id: new.id.clone(), body }, ACTOR).map_err(failed)?;
        // A refusal is a create that lost the race to another of the same id: the thread is there
        match self.engine.neutral_thread(&new.id).map_err(failed)? {
            Some(created) => Ok(view(&created)),
            None => Err(failed(receipt.error.map_or_else(|| format!("thread {} was not created", new.id), |(code, message)| format!("{code}: {message}")))),
        }
    }

    fn get(&self, thread: &str) -> Result<Option<ThreadView>, EngineError> {
        Ok(self.engine.neutral_thread(thread).map_err(failed)?.as_ref().map(view))
    }

    fn list(&self) -> Result<Vec<ThreadView>, EngineError> {
        Ok(self.engine.neutral_threads().map_err(failed)?.iter().map(view).collect())
    }

    fn history(&self, thread: &str, after_seq: u64, limit: usize) -> Result<Option<Page>, EngineError> {
        let Some((events, last)) = self.engine.neutral_events(thread, i64::try_from(after_seq).unwrap_or(i64::MAX)).map_err(failed)? else { return Ok(None) };
        let events: Vec<Wire> = events.iter().filter_map(wire).collect();
        Ok(Some(Page { more: events.len() > limit, events: events.into_iter().take(limit).collect(), last_seq: u64::try_from(last).unwrap_or(0) }))
    }

    fn follow(&self, thread: &str, after_seq: u64) -> Result<Option<Subscription>, EngineError> {
        let Some(known) = self.engine.neutral_thread(thread).map_err(failed)? else { return Ok(None) };
        let (tx, mut rx) = unbounded_channel();
        // The engine puts the events after `after_seq` on the channel, then `Synchronized`, and only then lets live ones in, all under its lock
        self.engine.subscribe_thread_with(thread, Some(i64::try_from(after_seq).unwrap_or(i64::MAX)), tx).map_err(failed)?;
        let mut replay = Vec::new();
        while let Ok(item) = rx.try_recv() {
            match item {
                ThreadItem::Event(event) => replay.extend(wire(&event)),
                ThreadItem::Synchronized => break,
                ThreadItem::Snapshot(_) => {}
            }
        }
        let newest = u64::try_from(known.neutral.last_seq).unwrap_or(0);
        let last_seq = replay.last().map_or(after_seq.min(newest), |event| event.seq);
        Ok(Some(Subscription { last_seq, replay, live: Box::new(Live(rx)) }))
    }

    fn record(&self, thread: &str, turn: Option<&str>, event: ThreadEvent) -> Result<u64, EngineError> {
        let body = Body::NeutralEvent { turn_id: turn.map(String::from), event: serde_json::to_value(&event).map_err(failed)? };
        let receipt = self.engine.dispatch(Command { command_id: format!("agent-api:event:{}", self.engine.new_id()), thread_id: thread.to_string(), body }, ACTOR).map_err(failed)?;
        receipt.events.first().and_then(|event| u64::try_from(event.sequence).ok()).ok_or_else(|| failed(format!("the engine recorded no event for thread {thread}")))
    }

    fn new_id(&self) -> String {
        self.engine.new_id()
    }

    fn on_settled(&self, hook: SettledHook) {
        if let Ok(mut slot) = self.settled.lock() {
            *slot = Some(hook);
        }
    }

    fn decide(&self, thread: &str, command: Lifecycle) -> Result<Decided, EngineError> {
        let receipt = self.engine.dispatch(Command { command_id: format!("agent-api:command:{}", self.engine.new_id()), thread_id: thread.to_string(), body: body(command) }, ACTOR).map_err(failed)?;
        Ok(match receipt.error {
            Some((code, message)) => Err(Refused { code, message }),
            None => Ok(receipt.events.iter().filter_map(wire).collect()),
        })
    }

    fn state(&self, thread: &str) -> Result<Option<ThreadState>, EngineError> {
        Ok(self.engine.neutral_thread(thread).map_err(failed)?.as_ref().map(state_of))
    }

    fn queue(&self, thread: &str) -> Result<Option<(bool, Vec<QueueItem>)>, EngineError> {
        Ok(self.engine.neutral_thread(thread).map_err(failed)?.map(|t| (t.lifecycle.queue_held, t.lifecycle.queue.iter().map(queue_item).collect())))
    }

    fn search(&self, query: &str, limit: usize) -> Result<Vec<lifecycle::SearchHit>, EngineError> {
        Ok(self.engine.search_threads(query, limit).map_err(failed)?.into_iter().map(hit_of).collect())
    }

    fn family(&self, thread: &str) -> Result<(Option<String>, Vec<ThreadState>), EngineError> {
        let (root, members) = self.engine.thread_family(thread).map_err(failed)?;
        Ok((root, members.iter().map(state_of).collect()))
    }

    fn shelf(&self, which: lifecycle::Shelf, before: Option<lifecycle::Cursor>, limit: usize) -> Result<lifecycle::ShelfResult, EngineError> {
        let which = engine_shelf(which);
        let (page, threads) = self.engine.thread_shelf(which, before.map(cursor).as_ref(), limit).map_err(failed)?;
        Ok(shelf_result(which, page, &threads))
    }

    fn pending_merge(&self, thread: &str) -> Result<Option<(String, String)>, EngineError> {
        Ok(self.engine.neutral_thread(thread).map_err(failed)?.and_then(|t| t.lifecycle.pending_merge_back).map(|p| (p.source, p.context)))
    }

    fn provider_session(&self, thread: &str, driver: &str) -> Result<Option<String>, EngineError> {
        Ok(self.engine.neutral_thread(thread).map_err(failed)?.and_then(|t| t.thread.session).filter(|s| s.cli == driver).and_then(|s| s.provider_session_id))
    }

    fn set_session(&self, thread: &str, driver: &str, turn: &str, session: &str) -> Result<(), EngineError> {
        if self.provider_session(thread, driver)?.as_deref() == Some(session) {
            return Ok(());
        }
        // `running` for the turn that announced it: the thread's turn is settled by its own finish, which comes next
        let session = Session { cli: driver.to_string(), provider_session_id: Some(session.to_string()), status: "running".into(), active_turn_id: Some(turn.to_string()), last_error: None };
        self.engine.dispatch(Command { command_id: format!("agent-api:session:{}", self.engine.new_id()), thread_id: thread.to_string(), body: Body::SessionSet { session } }, ACTOR).map_err(failed)?;
        Ok(())
    }

    fn record_value(&self, thread: &str, turn: Option<&str>, event: Value) -> Result<u64, EngineError> {
        let body = Body::NeutralEvent { turn_id: turn.map(String::from), event };
        let receipt = self.engine.dispatch(Command { command_id: format!("agent-api:event:{}", self.engine.new_id()), thread_id: thread.to_string(), body }, ACTOR).map_err(failed)?;
        receipt.events.first().and_then(|event| u64::try_from(event.sequence).ok()).ok_or_else(|| failed(format!("the engine recorded no event for thread {thread}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_api_agent::{FinishReason, Role};
    use remote_chat_agent_core::application::store::MemoryStore;
    use remote_chat_agent_core::testing::{CountingIds, FixedClock};

    fn threads_over(store: Arc<MemoryStore>) -> EngineThreads {
        EngineThreads::new(Arc::new(Engine::new(Box::new(store), "/work".into(), Arc::new(FixedClock(1_700_000_000_000)), Arc::new(CountingIds::default()))))
    }

    fn new(id: &str) -> NewThread {
        NewThread { id: id.into(), title: format!("title of {id}"), workspace: Some("w1".into()), driver: Some("claudeAgent".into()), model: None }
    }

    fn say(threads: &EngineThreads, thread: &str, turn: &str, text: &str) -> u64 {
        threads.record(thread, Some(turn), ThreadEvent::MessageDelta { role: Role::Assistant, text: text.into() }).unwrap()
    }

    #[tokio::test]
    async fn given_two_threads_whose_events_interleave_when_one_is_followed_then_it_gets_its_own_events_with_the_engine_s_numbers_and_each_once_across_a_resume() {
        // Given threads a and b, each with a turn, whose events are recorded alternately
        let threads = threads_over(Arc::default());
        threads.create(new("a")).unwrap();
        threads.create(new("b")).unwrap();
        let started = threads.record("a", Some("u1"), ThreadEvent::TurnStarted {}).unwrap();
        say(&threads, "b", "u2", "other");
        let first = say(&threads, "a", "u1", "one");
        // When a client follows a from the start, then more of both are recorded
        let mut subscription = threads.follow("a", 0).unwrap().expect("a is a thread");
        let second = say(&threads, "a", "u1", "two");
        say(&threads, "b", "u2", "more of the other");
        let third = threads.record("a", Some("u1"), ThreadEvent::TurnFinished { reason: FinishReason::Completed }).unwrap();
        // Then the replay is a's two events and the live ones are a's later two, all with the numbers the engine gave them and none of b's
        assert_eq!(subscription.replay.iter().map(|e| e.seq).collect::<Vec<_>>(), [started, first]);
        assert_eq!(subscription.last_seq, first);
        let mut live = Vec::new();
        for _ in 0..2 {
            live.push(subscription.live.next().await.unwrap().seq);
        }
        assert_eq!(live, [second, third]);
        assert!(first - started > 1 && third - second > 1, "b's events sit between a's: {started} {first} {second} {third}");
        // When the client resumes after the first event it had, Then it gets the events after it and the first is not among them
        let resumed = threads.follow("a", first).unwrap().unwrap();
        assert_eq!(resumed.replay.iter().map(|e| e.seq).collect::<Vec<_>>(), [second, third]);
        assert_eq!(resumed.last_seq, third);
    }

    #[tokio::test]
    async fn given_a_client_that_is_ahead_of_the_thread_or_at_its_newest_when_it_follows_then_the_replay_is_empty_and_the_stream_says_where_the_thread_is() {
        // Given a thread with one turn's start
        let threads = threads_over(Arc::default());
        threads.create(new("a")).unwrap();
        let started = threads.record("a", Some("u1"), ThreadEvent::TurnStarted {}).unwrap();
        // When a client follows from the newest, from far beyond it, and for a thread that does not exist
        let at_newest = threads.follow("a", started).unwrap().unwrap();
        let beyond = threads.follow("a", started + 100).unwrap().unwrap();
        // Then neither replays anything, the first ends at the newest and the second is told the newest, and the unknown thread has no stream
        assert!(at_newest.replay.is_empty() && beyond.replay.is_empty());
        assert_eq!((at_newest.last_seq, beyond.last_seq), (started, started));
        assert!(threads.follow("nope", 0).unwrap().is_none() && threads.history("nope", 0, 10).unwrap().is_none());
    }

    #[test]
    fn given_threads_created_when_asked_for_them_again_then_a_create_of_an_existing_id_changes_nothing_and_the_view_follows_the_turn() {
        // Given a thread, created twice with other defaults the second time
        let threads = threads_over(Arc::default());
        let first = threads.create(new("a")).unwrap();
        let again = threads.create(NewThread { title: "other".into(), workspace: None, ..new("a") }).unwrap();
        // Then the second is the first, idle with no turn and nothing recorded
        assert_eq!(again, first);
        assert_eq!((first.status, first.turn.clone(), first.last_seq, first.workspace.as_deref()), (Status::Idle, None, 0, Some("w1")));
        // When a turn starts and then ends
        let started = threads.record("a", Some("u1"), ThreadEvent::TurnStarted {}).unwrap();
        let running = threads.get("a").unwrap().unwrap();
        let finished = threads.record("a", Some("u1"), ThreadEvent::TurnFinished { reason: FinishReason::Failed }).unwrap();
        let idle = threads.get("a").unwrap().unwrap();
        // Then the view runs it with the start's seq as newest, and is idle again with the finish's
        assert_eq!((running.status, running.turn.as_deref(), running.last_seq), (Status::Running, Some("u1"), started));
        assert_eq!((idle.status, idle.turn, idle.last_seq), (Status::Idle, None, finished));
        assert_eq!(threads.list().unwrap().iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["a"]);
    }

    #[test]
    fn given_a_turn_a_crash_left_running_when_the_service_starts_again_then_the_thread_s_events_end_it_as_interrupted_and_the_thread_is_idle() {
        // Given a thread whose turn was running when the first engine went away, on a store that outlives it
        let store: Arc<MemoryStore> = Arc::default();
        let before = threads_over(store.clone());
        before.create(new("a")).unwrap();
        before.create(new("quiet")).unwrap();
        before.record("a", Some("u1"), ThreadEvent::TurnStarted {}).unwrap();
        let said = say(&before, "a", "u1", "working");
        assert_eq!(before.get("a").unwrap().unwrap().status, Status::Running);
        // When the service starts over the same log
        let after = threads_over(store);
        let settled = after.start().unwrap();
        // Then one turn was settled, the thread's events end with the turn's finish as interrupted, and the thread is idle and keeps counting
        assert_eq!(settled, 1);
        let events = after.history("a", 0, 10).unwrap().unwrap().events;
        let last = events.last().unwrap();
        assert_eq!((last.kind.as_str(), last.data["reason"].clone(), last.turn.as_deref()), ("turn.finished", serde_json::json!("interrupted"), Some("u1")));
        assert!(last.seq > said);
        let view = after.get("a").unwrap().unwrap();
        assert_eq!((view.status, view.turn, view.last_seq), (Status::Idle, None, last.seq));
        // And the thread that ran nothing had nothing to settle, and a second start settles nothing more
        assert_eq!(after.history("quiet", 0, 10).unwrap().unwrap().events.len(), 0);
        assert_eq!(after.start().unwrap(), 0);
    }

    #[test]
    fn given_a_log_that_outlives_the_engine_when_another_opens_it_then_the_history_is_the_same_events_with_the_same_numbers() {
        // Given a thread with events on a store that outlives its engine
        let store: Arc<MemoryStore> = Arc::default();
        let before = threads_over(store.clone());
        before.create(new("a")).unwrap();
        before.record("a", Some("u1"), ThreadEvent::TurnStarted {}).unwrap();
        let said = say(&before, "a", "u1", "hello");
        let page = before.history("a", 0, 10).unwrap().unwrap();
        // When another engine opens the log
        let after = threads_over(store);
        let reread = after.history("a", 0, 10).unwrap().unwrap();
        // Then the history is the same, down to the fields of each event, and it ends at the newest
        assert_eq!(reread, page);
        assert_eq!((reread.events.len(), reread.last_seq, reread.more), (2, said, false));
        assert_eq!(reread.events[1].data["text"], serde_json::json!("hello"));
        // And a page smaller than the log says more follow
        assert!(after.history("a", 0, 1).unwrap().unwrap().more);
    }
}
