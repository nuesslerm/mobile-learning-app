//! Where the pool cooldown ledger lives: the port the run loop's host reads and writes through, with the file and the shared
//! database as its two adapters.
use crate::pool_ledger::Ledger;

pub trait CooldownStore {
    /// An unreadable store reads as empty, so a run never waits on it; `last_error` says why it was empty.
    fn read(&self) -> Ledger;
    fn write(&self, ledger: &Ledger) -> std::io::Result<()>;
    fn last_error(&self) -> Option<String> {
        None
    }
}

impl CooldownStore for crate::file_store::FileLedgerStore {
    fn read(&self) -> Ledger {
        crate::file_store::FileLedgerStore::read(self)
    }

    fn write(&self, ledger: &Ledger) -> std::io::Result<()> {
        crate::file_store::FileLedgerStore::write(self, ledger)
    }
}
