//! A store in memory, for tests of this crate and of the services built on it.
use crate::StateStore;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// Keeps values in memory; `full()` refuses every write with "disk full".
#[derive(Default)]
pub struct MemoryStore {
    values: Mutex<BTreeMap<(String, String), Value>>,
    full: bool,
}

impl MemoryStore {
    pub fn full() -> MemoryStore {
        MemoryStore { full: true, ..MemoryStore::default() }
    }
}

impl StateStore for MemoryStore {
    fn get(&self, scope: &str, key: &str) -> Option<Value> {
        self.values.lock().unwrap_or_else(|p| p.into_inner()).get(&(scope.into(), key.into())).cloned()
    }

    fn scope(&self, scope: &str) -> Map<String, Value> {
        let values = self.values.lock().unwrap_or_else(|p| p.into_inner());
        values.iter().filter(|((s, _), _)| s == scope).map(|((_, k), v)| (k.clone(), v.clone())).collect()
    }

    fn set(&self, scope: &str, key: &str, value: Value) -> Result<(), String> {
        if self.full {
            return Err("disk full".into());
        }
        self.values.lock().unwrap_or_else(|p| p.into_inner()).insert((scope.into(), key.into()), value);
        Ok(())
    }
}
