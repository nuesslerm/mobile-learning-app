//! The `terminal` group of the agent API (`libraries/agent-api/agent-api-terminal`) over the executor's ptys (`exec-pty`). The service keeps for
//! each terminal a bounded, non-persisted tail of its output, numbered by the pty's byte offset, and a pump that attaches to the executor's pty
//! again from the offset it reached whenever the executor's channel is replaced; a reader older than the tail is handed a snapshot. Terminals
//! the service lost (a restart) are adopted from the executor's own list.
mod group;
mod meta;
mod terminal;

pub use group::{TerminalGroup, SERVED};
pub use terminal::Clock;

#[cfg(test)]
mod tests;
