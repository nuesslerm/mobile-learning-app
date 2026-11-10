//! Doubles of the ports: a blob store in memory (that can be made to fail), a clock and counting ids.
use crate::ports::{BlobStore, Clock, Ids, Put};
use agent_api_attachments::AttachmentView;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Default)]
pub struct MemoryBlobs {
    kept: Mutex<HashMap<String, (AttachmentView, Vec<u8>)>>,
    failing: Mutex<Option<String>>,
}

impl MemoryBlobs {
    /// From now on every call fails with `why`; none when `why` is none.
    pub fn fail_with(&self, why: Option<&str>) {
        *self.failing.lock().unwrap() = why.map(str::to_string);
    }

    pub fn ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.kept.lock().unwrap().keys().cloned().collect();
        ids.sort();
        ids
    }

    fn check(&self) -> Result<(), String> {
        self.failing.lock().unwrap().clone().map_or(Ok(()), Err)
    }
}

impl BlobStore for MemoryBlobs {
    fn put(&self, view: &AttachmentView, bytes: &[u8]) -> Result<Put, String> {
        self.check()?;
        let mut kept = self.kept.lock().unwrap();
        if let Some((held, _)) = kept.get(&view.id) {
            return Ok(Put::Taken(held.clone()));
        }
        kept.insert(view.id.clone(), (view.clone(), bytes.to_vec()));
        Ok(Put::Created)
    }

    fn meta(&self, id: &str) -> Result<Option<AttachmentView>, String> {
        self.check()?;
        Ok(self.kept.lock().unwrap().get(id).map(|(view, _)| view.clone()))
    }

    fn read(&self, id: &str, offset: u64, length: u64) -> Result<Option<(AttachmentView, Vec<u8>)>, String> {
        self.check()?;
        Ok(self.kept.lock().unwrap().get(id).map(|(view, bytes)| {
            let from = (offset as usize).min(bytes.len());
            let to = from.saturating_add(length.min(usize::MAX as u64) as usize).min(bytes.len());
            (view.clone(), bytes[from..to].to_vec())
        }))
    }

    fn delete(&self, id: &str) -> Result<bool, String> {
        self.check()?;
        Ok(self.kept.lock().unwrap().remove(id).is_some())
    }
}

pub struct FixedClock(pub u64);

impl Clock for FixedClock {
    fn now_ms(&self) -> u64 {
        self.0
    }
}

#[derive(Default)]
pub struct CountingIds(AtomicU64);

impl Ids for CountingIds {
    fn new_id(&self) -> String {
        format!("att-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}
