//! The flow tests of `wf parity`: they enter through the thin `wf-parity` binary (or `WF_BIN`) on fixture trees whose references
//! are real git checkouts, and on the repository's own tables.
#[path = "flow/support/mod.rs"]
mod support;

#[path = "flow/orca.rs"]
mod orca;
#[path = "flow/recorded.rs"]
mod recorded;
#[path = "flow/repo.rs"]
mod repo;
#[path = "flow/t3.rs"]
mod t3;
#[path = "flow/track.rs"]
mod track;
