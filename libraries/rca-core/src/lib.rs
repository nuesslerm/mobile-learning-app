//! The generic core of remote-chat-agent's model runs: run log, stats, savings, the opencode stream parser and the file
//! stores. Pure logic with no I/O, so `wf` and the service can share it. The ladder lives in the `rca-ladder` plugin.
//! Behaviour is pinned by `tests/flow/golden/rca-core.json`, whose outputs the TypeScript implementation produced.
pub mod time;
pub mod run_log;
pub mod stats;
pub mod file_store;
pub mod savings;
pub mod opencode;
pub mod text;
