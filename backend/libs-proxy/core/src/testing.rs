//! The in-memory bucket the flow tests serve from.
use crate::application::ports::{Object, ObjectRead, ReadResult};
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::Mutex;

#[derive(Default)]
pub struct InMemoryObjects {
    objects: Mutex<BTreeMap<String, Vec<u8>>>,
    reads: Mutex<Vec<String>>,
    failing: Mutex<bool>,
}

impl InMemoryObjects {
    pub fn put(&self, key: &str, bytes: &[u8]) {
        self.objects.lock().unwrap().insert(key.to_string(), bytes.to_vec());
    }

    pub fn fail_reads(&self) {
        *self.failing.lock().unwrap() = true;
    }

    /// The keys read so far, in order.
    pub fn reads(&self) -> Vec<String> {
        self.reads.lock().unwrap().clone()
    }

    pub fn etag_of(bytes: &[u8]) -> String {
        let sum = bytes.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100000001b3));
        format!("\"{sum:016x}\"")
    }
}

#[async_trait]
impl ObjectRead for InMemoryObjects {
    async fn get(&self, key: &str, if_none_match: Option<&str>) -> ReadResult {
        self.reads.lock().unwrap().push(key.to_string());
        if *self.failing.lock().unwrap() {
            return ReadResult::Unavailable;
        }
        match self.objects.lock().unwrap().get(key) {
            None => ReadResult::Missing,
            Some(bytes) => {
                let etag = Self::etag_of(bytes);
                if if_none_match == Some(etag.as_str()) {
                    ReadResult::NotModified
                } else {
                    ReadResult::Found(Object { bytes: bytes.clone(), etag: Some(etag) })
                }
            }
        }
    }
}
