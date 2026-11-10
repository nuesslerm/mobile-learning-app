//! The double of the group's port: a thread engine that keeps its log in memory, with the sequence an engine gives (one counter for every
//! thread, so a thread's `seq` has the gaps the others leave) and the same view of a thread's turn (it runs from `turn.started` to `turn.finished`).
use crate::port::{EngineError, Follow, NewThread, Page, Subscription, ThreadEngine};
use agent_api_agent::{Status, ThreadEvent, ThreadView};
use agent_api_proto::Event;
use async_trait::async_trait;
use std::sync::Mutex;
use tokio::sync::mpsc;

const TIME: &str = "2026-11-04T10:00:00Z";

struct Row {
    new: NewThread,
    turn: Option<String>,
    last_seq: u64,
    touched: u64,
    events: Vec<Event>,
    listeners: Vec<mpsc::UnboundedSender<Event>>,
}

impl Row {
    fn view(&self) -> ThreadView {
        let running = self.turn.is_some();
        ThreadView { id: self.new.id.clone(), title: self.new.title.clone(), workspace: self.new.workspace.clone(), driver: self.new.driver.clone(), model: self.new.model.clone(), status: if running { Status::Running } else { Status::Idle }, turn: self.turn.clone(), last_seq: self.last_seq, created_at: TIME.into(), updated_at: TIME.into() }
    }
}

#[derive(Default)]
struct Inner {
    clock: u64,
    ids: u64,
    threads: Vec<Row>,
    broken: Option<String>,
}

/// Threads in memory. `broken` makes every call fail, as an engine whose log cannot be written does.
#[derive(Default)]
pub struct MemoryThreads {
    inner: Mutex<Inner>,
}

impl MemoryThreads {
    fn held(&self) -> Result<std::sync::MutexGuard<'_, Inner>, EngineError> {
        let inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        match &inner.broken {
            Some(why) => Err(EngineError(why.clone())),
            None => Ok(inner),
        }
    }

    /// From now on every call fails with `why`.
    pub fn break_with(&self, why: &str) {
        self.inner.lock().unwrap_or_else(|p| p.into_inner()).broken = Some(why.to_string());
    }

    /// Every event of the thread, in the order recorded.
    pub fn events_of(&self, thread: &str) -> Vec<Event> {
        let inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.threads.iter().find(|row| row.new.id == thread).map(|row| row.events.clone()).unwrap_or_default()
    }
}

struct Live(mpsc::UnboundedReceiver<Event>);

#[async_trait]
impl Follow for Live {
    async fn next(&mut self) -> Option<Event> {
        self.0.recv().await
    }
}

impl ThreadEngine for MemoryThreads {
    fn create(&self, new: NewThread) -> Result<ThreadView, EngineError> {
        let mut inner = self.held()?;
        if let Some(row) = inner.threads.iter().find(|row| row.new.id == new.id) {
            return Ok(row.view());
        }
        inner.clock += 1;
        let touched = inner.clock;
        let row = Row { new, turn: None, last_seq: 0, touched, events: Vec::new(), listeners: Vec::new() };
        let view = row.view();
        inner.threads.push(row);
        Ok(view)
    }

    fn get(&self, thread: &str) -> Result<Option<ThreadView>, EngineError> {
        Ok(self.held()?.threads.iter().find(|row| row.new.id == thread).map(Row::view))
    }

    fn list(&self) -> Result<Vec<ThreadView>, EngineError> {
        let inner = self.held()?;
        let mut rows: Vec<&Row> = inner.threads.iter().collect();
        rows.sort_by_key(|row| std::cmp::Reverse(row.touched));
        Ok(rows.into_iter().map(Row::view).collect())
    }

    fn history(&self, thread: &str, after_seq: u64, limit: usize) -> Result<Option<Page>, EngineError> {
        let inner = self.held()?;
        Ok(inner.threads.iter().find(|row| row.new.id == thread).map(|row| {
            let after: Vec<Event> = row.events.iter().filter(|e| e.seq > after_seq).cloned().collect();
            Page { more: after.len() > limit, events: after.into_iter().take(limit).collect(), last_seq: row.last_seq }
        }))
    }

    fn follow(&self, thread: &str, after_seq: u64) -> Result<Option<Subscription>, EngineError> {
        let mut inner = self.held()?;
        let Some(row) = inner.threads.iter_mut().find(|row| row.new.id == thread) else { return Ok(None) };
        let replay: Vec<Event> = row.events.iter().filter(|e| e.seq > after_seq).cloned().collect();
        let (tx, rx) = mpsc::unbounded_channel();
        row.listeners.push(tx);
        let last_seq = replay.last().map_or(after_seq.min(row.last_seq), |e| e.seq);
        Ok(Some(Subscription { last_seq, replay, live: Box::new(Live(rx)) }))
    }

    fn record(&self, thread: &str, turn: Option<&str>, event: ThreadEvent) -> Result<u64, EngineError> {
        let mut inner = self.held()?;
        inner.clock += 1;
        let seq = inner.clock;
        let Some(row) = inner.threads.iter_mut().find(|row| row.new.id == thread) else { return Err(EngineError(format!("no thread {thread}"))) };
        match &event {
            ThreadEvent::TurnStarted {} => row.turn = turn.map(String::from),
            ThreadEvent::TurnFinished { .. } if row.turn.as_deref() == turn => row.turn = None,
            _ => {}
        }
        let wire = event.into_event(thread, turn, seq);
        row.listeners.retain(|tx| tx.send(wire.clone()).is_ok());
        row.events.push(wire);
        (row.last_seq, row.touched) = (seq, seq);
        Ok(seq)
    }

    fn new_id(&self) -> String {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.ids += 1;
        format!("id-{}", inner.ids)
    }
}
