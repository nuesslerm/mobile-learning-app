//! Doubles of what the host tells the service: its `/proc` files and the ids it asks for.
use crate::application::ports::{HostFiles, Ids};
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A `/proc` tree in memory: a file that was not given is missing.
#[derive(Default)]
pub struct FakeHostFiles {
    files: Mutex<BTreeMap<String, String>>,
}

impl FakeHostFiles {
    pub fn with(self, name: &str, text: &str) -> Self {
        self.set(name, text);
        self
    }

    /// Replaces a file, as the kernel does between two reads.
    pub fn set(&self, name: &str, text: &str) {
        lock(&self.files).insert(name.to_string(), text.to_string());
    }
}

#[async_trait]
impl HostFiles for FakeHostFiles {
    async fn read_text(&self, name: &str) -> Option<String> {
        lock(&self.files).get(name).cloned()
    }
}

/// Ids that count up from `00000000-0000-4000-8000-000000000001`: valid version 4 UUIDs, in a known order.
#[derive(Default)]
pub struct FakeIds {
    issued: AtomicU64,
}

impl Ids for FakeIds {
    fn uuid(&self) -> String {
        let n = self.issued.fetch_add(1, Ordering::Relaxed) + 1;
        format!("00000000-0000-4000-8000-{n:012x}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
        use crate::domain::validators::is_valid_job_id;

    #[tokio::test]
    async fn given_the_fake_host_files_when_read_then_a_given_file_is_its_text_and_any_other_is_missing() {
        let files = FakeHostFiles::default().with("meminfo", "MemTotal: 1 kB\n");

        assert_eq!(files.read_text("meminfo").await.as_deref(), Some("MemTotal: 1 kB\n"));
        assert_eq!(files.read_text("loadavg").await, None);
        files.set("meminfo", "MemTotal: 2 kB\n");
        assert_eq!(files.read_text("meminfo").await.as_deref(), Some("MemTotal: 2 kB\n"));
    }

    #[test]
    fn given_the_fake_ids_when_asked_then_each_is_a_distinct_job_id_in_a_known_order() {
        let ids = FakeIds::default();

        let issued: Vec<String> = (0..3).map(|_| ids.uuid()).collect();

        assert_eq!(issued, ["00000000-0000-4000-8000-000000000001", "00000000-0000-4000-8000-000000000002", "00000000-0000-4000-8000-000000000003"]);
        assert!(issued.iter().all(|id| is_valid_job_id(id)));
    }
}
