//! The pool ledger file (`plugins/cooldowns/fileStore.ts`), the ladder's cooldown store on disk.
use crate::pool_ledger::{parse_ledger, serialize_ledger, Ledger};
use std::path::PathBuf;

/// A missing or corrupt file reads as empty. Writes go through a tmp file and a rename, so a crash never truncates it.
pub struct FileLedgerStore {
    path: PathBuf,
}

impl FileLedgerStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn read(&self) -> Ledger {
        std::fs::read_to_string(&self.path).map_or_else(|_| Ledger::new(), |t| parse_ledger(&t))
    }

    pub fn write(&self, ledger: &Ledger) -> std::io::Result<()> {
        let tmp = PathBuf::from(format!("{}.tmp", self.path.display()));
        if let Some(dir) = self.path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&tmp, serialize_ledger(ledger))?;
        std::fs::rename(tmp, &self.path)
    }
}
