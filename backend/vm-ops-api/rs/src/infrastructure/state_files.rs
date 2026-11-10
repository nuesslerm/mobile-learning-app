use super::state_io::{make_dir, object_of, read_text, replace_file};
use async_trait::async_trait;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use vm_ops_api_core::application::ports::{CooldownLedger, Event, EventLog, LedgerState, LedgerWrite, Object};
use vm_ops_api_core::domain::js::stringify_pretty;

async fn read_object(file: &Path) -> Object {
    read_text(file).await.as_deref().and_then(object_of).unwrap_or_default()
}

/// `JSON.stringify(object, null, 2)` and a newline, put in `file` whole, its directory made first.
async fn persist(file: &Path, object: Object) -> Result<(), String> {
    if let Some(dir) = file.parent() {
        make_dir(dir).await?;
    }
    let mut text = stringify_pretty(&Value::Object(object));
    text.push('\n');
    replace_file(file, text.as_bytes()).await
}

/// The cooldown ledger in one JSON file. The version is the count of writes since this process started, and is not in the file.
pub struct FileCooldownLedger {
    file: PathBuf,
    version: Mutex<i64>,
    log: Arc<dyn EventLog>,
}

impl FileCooldownLedger {
    pub fn new(file: impl Into<PathBuf>, log: Arc<dyn EventLog>) -> Self {
        Self { file: file.into(), version: Mutex::new(0), log }
    }
}

#[async_trait]
impl CooldownLedger for FileCooldownLedger {
    async fn read(&self) -> LedgerState {
        let version = self.version.lock().await;
        LedgerState { ledger: read_object(&self.file).await, version: *version }
    }

    async fn write(&self, ledger: Object, expected: i64) -> LedgerWrite {
        let mut version = self.version.lock().await;
        if expected != *version {
            return LedgerWrite { ok: false, version: *version };
        }
        match persist(&self.file, ledger).await {
            Ok(()) => {
                *version += 1;
                LedgerWrite { ok: true, version: *version }
            }
            Err(message) => {
                self.log.emit(Event::error("cooldowns.persist_failed").with("err", format!("Error: {message}")));
                LedgerWrite { ok: false, version: *version }
            }
        }
    }
}
