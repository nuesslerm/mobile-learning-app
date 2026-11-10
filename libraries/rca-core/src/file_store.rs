//! The run log file (`plugins/ledger/fileStore.ts`).
use crate::run_log::{parse_run_log, serialize_run_log_entry, RunLogEntry};
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct FileRunLog {
    path: PathBuf,
}

impl FileRunLog {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&self, entry: &RunLogEntry) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&self.path)?;
        f.write_all(serialize_run_log_entry(entry).as_bytes())
    }

    pub fn read_all(&self) -> Vec<RunLogEntry> {
        std::fs::read_to_string(&self.path).map_or_else(|_| Vec::new(), |t| parse_run_log(&t))
    }
}
