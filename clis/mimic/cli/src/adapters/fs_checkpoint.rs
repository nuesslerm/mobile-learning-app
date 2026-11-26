//! The checkpoint of a recording on disk: `<set>/raw/checkpoint/record.checkpoint.jsonl` (under `raw/`, which is never committed: the entries can hold unmasked captures), one line per finished batch entry, appended and synced, so a
//! killed run loses at most the batch in flight. A torn last line (the kill came mid-write) is dropped; any other bad line is an error.
use mimic_core::ports::{Checkpoint, CheckpointEntry, StoreError};
use std::io::Write;
use std::path::PathBuf;

pub const CHECKPOINT_DIR: &str = "raw/checkpoint";
pub const CHECKPOINT_FILE: &str = "record.checkpoint.jsonl";

pub struct FsCheckpoint {
    path: PathBuf,
}

impl FsCheckpoint {
    pub fn new(set_dir: &std::path::Path) -> Self {
        Self { path: set_dir.join(CHECKPOINT_DIR).join(CHECKPOINT_FILE) }
    }

    pub fn exists(&self) -> bool {
        self.path.is_file()
    }
}

fn io(path: &std::path::Path, e: std::io::Error) -> StoreError {
    StoreError::Io(format!("{}: {e}", path.display()))
}

impl Checkpoint for FsCheckpoint {
    fn load(&self) -> Result<Vec<CheckpointEntry>, StoreError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io(&self.path, e)),
        };
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        let mut out = Vec::with_capacity(lines.len());
        for (n, line) in lines.iter().enumerate() {
            match serde_json::from_str(line) {
                Ok(e) => out.push(e),
                Err(_) if n + 1 == lines.len() && !text.ends_with('\n') => {}
                Err(e) => return Err(StoreError::Invalid(format!("{} line {}: {e}", self.path.display(), n + 1))),
            }
        }
        Ok(out)
    }

    fn append(&mut self, entries: &[CheckpointEntry]) -> Result<(), StoreError> {
        if let Some(dir) = self.path.parent() {
            let ignore = dir.parent().map(|raw| raw.join(".gitignore"));
            std::fs::create_dir_all(dir).and_then(|()| ignore.filter(|g| !g.exists()).map_or(Ok(()), |g| std::fs::write(g, "*\n"))).map_err(|e| io(dir, e))?;
        }
        let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&self.path).map_err(|e| io(&self.path, e))?;
        let mut text = String::new();
        for e in entries {
            text.push_str(&serde_json::to_string(e).expect("an entry serialises"));
            text.push('\n');
        }
        file.write_all(text.as_bytes()).and_then(|()| file.sync_data()).map_err(|e| io(&self.path, e))
    }

    fn clear(&mut self) -> Result<(), StoreError> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => {
                if let Some(dir) = self.path.parent() {
                    let _ = std::fs::remove_dir(dir);
                }
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io(&self.path, e)),
        }
    }
}
