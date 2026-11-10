use async_trait::async_trait;
use leetcode_backend_core::application::ports::{PortResult, SessionStore};
use leetcode_backend_core::application::session::Session;
use serde_json::Value;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;

/// The session in one JSON file, rewritten atomically (a private temporary file renamed over it, readable by the owner only).
pub struct FileSessionStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileSessionStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), lock: Mutex::new(()) }
    }

    async fn read(&self) -> PortResult<Option<Session>> {
        let raw = match tokio::fs::read_to_string(&self.path).await {
            Ok(raw) => raw,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        match serde_json::from_str::<Value>(&raw).map_err(|error| error.to_string())? {
            Value::Null => Ok(None),
            value => Session::from_value(value).map(Some).ok_or_else(|| "session file is not a JSON object".to_string()),
        }
    }

    async fn write(&self, session: &Session) -> PortResult<()> {
        let path = self.path.clone();
        let text = session.to_value().to_string();
        tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            let dir = path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or(Path::new("."));
            std::fs::create_dir_all(dir)?;
            let mut file = tempfile::NamedTempFile::new_in(dir)?;
            file.write_all(text.as_bytes())?;
            file.as_file().sync_all()?;
            file.persist(&path).map(|_| ()).map_err(|error| error.error)
        })
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
    }
}

#[async_trait]
impl SessionStore for FileSessionStore {
    async fn get(&self) -> PortResult<Option<Session>> {
        let _guard = self.lock.lock().await;
        self.read().await
    }

    async fn set(&self, session: &Session) -> PortResult<()> {
        let _guard = self.lock.lock().await;
        self.write(session).await
    }

    async fn mark_expired(&self) -> PortResult<()> {
        let _guard = self.lock.lock().await;
        match self.read().await? {
            Some(current) => self.write(&current.expired()).await,
            None => Ok(()),
        }
    }
}
