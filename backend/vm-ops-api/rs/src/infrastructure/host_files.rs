use async_trait::async_trait;
use std::path::{Path, PathBuf};
use vm_ops_api_core::application::ports::HostFiles;

/// The text files of a `/proc` tree: the host's, or the one `PROC_ROOT` names.
pub struct ProcHostFiles {
    root: PathBuf,
}

impl ProcHostFiles {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

#[async_trait]
impl HostFiles for ProcHostFiles {
    async fn read_text(&self, name: &str) -> Option<String> {
        read_text(&self.root.join(name.trim_start_matches('/'))).await
    }
}

/// The text of a file as Node's `readFile(file, 'utf8')` decodes it, invalid bytes as U+FFFD; none when it cannot be read.
pub(super) async fn read_text(file: &Path) -> Option<String> {
    let bytes = tokio::fs::read(file).await.ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}
