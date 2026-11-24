//! The system implementations of the core's ports.
mod clock;
mod command_crawler;
mod coverage;
mod fs_checkpoint;
mod fs_store;
mod in_process;
pub mod script_driver;
mod stdio;
mod tap;

pub use command_crawler::CommandCrawler;
pub use coverage::{sources_from_env, FsTree, GitHistory, HttpUpstream, MimicEvidence, RunTraces};
pub use clock::{entropy, SystemClock};
pub use fs_checkpoint::{FsCheckpoint, CHECKPOINT_DIR};
pub use fs_store::FsStore;
pub use in_process::InProcessDriver;
pub use script_driver::open_driver;
pub use stdio::{serve, StdioDriver, StdioSpec};
pub use tap::{Carry, Tap};
