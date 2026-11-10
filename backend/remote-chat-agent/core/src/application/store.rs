//! The port the engine writes its event log through: events appended per thread, a snapshot and a summary per thread. The file-backed
//! adapter lives in the service crate; `MemoryStore` is the in-process one (the chat engine's, and the flow tests').

use super::super::domain::types::{Event, Summary, Thread};
use std::collections::HashMap;
use std::sync::Mutex;

pub trait Store: Send + Sync {
    fn append(&self, thread_id: &str, events: &[Event]) -> Result<(), String>;
    fn read_events(&self, thread_id: &str, after: Option<i64>) -> Result<Vec<Event>, String>;
    fn list_summaries(&self) -> Result<Vec<Summary>, String>;
    fn read_snapshot(&self, thread_id: &str) -> Result<Option<Thread>, String>;
    /// Every thread that has a log, for the restart scan.
    fn list_thread_ids(&self) -> Result<Vec<String>, String>;
}

/// A store shared between engines, as a log on disk is shared between restarts of the service.
impl<S: Store + ?Sized> Store for std::sync::Arc<S> {
    fn append(&self, thread_id: &str, events: &[Event]) -> Result<(), String> {
        (**self).append(thread_id, events)
    }

    fn read_events(&self, thread_id: &str, after: Option<i64>) -> Result<Vec<Event>, String> {
        (**self).read_events(thread_id, after)
    }

    fn list_summaries(&self) -> Result<Vec<Summary>, String> {
        (**self).list_summaries()
    }

    fn read_snapshot(&self, thread_id: &str) -> Result<Option<Thread>, String> {
        (**self).read_snapshot(thread_id)
    }

    fn list_thread_ids(&self) -> Result<Vec<String>, String> {
        (**self).list_thread_ids()
    }
}

#[derive(Default)]
pub struct MemoryStore {
    events: Mutex<HashMap<String, Vec<Event>>>,
    snapshots: Mutex<Vec<Thread>>,
}

impl Store for MemoryStore {
    fn append(&self, thread_id: &str, events: &[Event]) -> Result<(), String> {
        self.events.lock().unwrap_or_else(|e| e.into_inner()).entry(thread_id.to_string()).or_default().extend_from_slice(events);
        Ok(())
    }

    fn read_events(&self, thread_id: &str, after: Option<i64>) -> Result<Vec<Event>, String> {
        let all = self.events.lock().unwrap_or_else(|e| e.into_inner());
        Ok(all.get(thread_id).map(|v| v.iter().filter(|e| after.is_none_or(|a| e.sequence > a)).cloned().collect()).unwrap_or_default())
    }

    fn list_summaries(&self) -> Result<Vec<Summary>, String> {
        Ok(self.snapshots.lock().unwrap_or_else(|e| e.into_inner()).iter().map(Thread::summary).collect())
    }

    fn list_thread_ids(&self) -> Result<Vec<String>, String> {
        let mut ids: Vec<String> = self.events.lock().unwrap_or_else(|e| e.into_inner()).keys().cloned().collect();
        ids.sort();
        Ok(ids)
    }

    fn read_snapshot(&self, thread_id: &str) -> Result<Option<Thread>, String> {
        Ok(self.snapshots.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|t| t.id == thread_id).cloned())
    }
}
