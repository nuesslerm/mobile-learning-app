//! The host adapters of the service: what spawns processes and touches the disk behind the ports of `rca-service-core` and
//! `remote-chat-agent-core`: `System` (`Commands`), `Processes` (the CLI driver's `Spawner`), `HostFiles` (`WorkspaceFiles`), `FileStore`
//! (the thread event log), `Documents` (`DocumentText`) and `Hosts`, the ladder's `RunHost` for a one-shot run.

mod docs;
mod exec;
mod file_store;
mod files;
mod proc;
mod runs;
#[cfg(test)]
mod tests;

pub use docs::Documents;
pub use exec::{inherited, provider_command, supervise, Processes, Supervised};
pub use file_store::FileStore;
pub use files::HostFiles;
pub use proc::System;
pub use runs::Hosts;
