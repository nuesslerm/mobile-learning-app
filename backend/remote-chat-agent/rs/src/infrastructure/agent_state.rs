//! The agent API's per-device state as one JSON file (`{scope: {key: value}}`), written beside the target and renamed over it.
use rca_service_core::domain::util::random_hex;
use rca_service_core::application::agent_groups::StateStore;
use serde_json::{Map, Value};
use std::path::PathBuf;
use std::sync::Mutex;

pub struct StateFile {
    path: PathBuf,
    lock: Mutex<()>,
}

impl StateFile {
    pub fn at(path: PathBuf) -> Self {
        Self { path, lock: Mutex::new(()) }
    }

    fn read(&self) -> Map<String, Value> {
        std::fs::read_to_string(&self.path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }
}

impl StateStore for StateFile {
    fn get(&self, scope: &str, key: &str) -> Option<Value> {
        let _held = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        self.read().get(scope)?.get(key).cloned()
    }

    fn scope(&self, scope: &str) -> Map<String, Value> {
        let _held = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        match self.read().remove(scope) {
            Some(Value::Object(keys)) => keys,
            _ => Map::new(),
        }
    }

    fn set(&self, scope: &str, key: &str, value: Value) -> Result<(), String> {
        let _held = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut all = self.read();
        let entry = all.entry(scope.to_string()).or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(keys) = entry {
            keys.insert(key.to_string(), value);
        }
        let dir = self.path.parent().ok_or("the state file has no directory")?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let tmp = dir.join(format!("agent-state-{}.json.tmp", random_hex(4)));
        std::fs::write(&tmp, Value::Object(all).to_string()).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &self.path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            e.to_string()
        })
    }
}
