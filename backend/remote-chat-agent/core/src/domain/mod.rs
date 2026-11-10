//! The rules of the thread view: what a command does to a thread, what an event does to the read model, how what an agent reports
//! becomes commands, and the legacy chat frames cut from a thread's items. Nothing here does I/O.

pub mod accumulator;
pub mod agent;
pub mod decider;
pub mod ids;
pub mod ingest;
pub mod lifecycle;
pub mod mcp;
pub mod projector;
pub mod queries;
pub mod translate;
pub mod types;
