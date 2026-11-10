//! `wf t3`: the T3-compatible server (`serve`) and its pairing codes (`pair`). This crate is the composition root of the
//! `t3-*` crates: `adapters` are the system implementations of the core's ports, `wiring` builds the graph, `verbs` is the
//! command line. Plan `docs/plans/t3-parity/plan.md`, wire `docs/plans/t3-parity/protocol.md`.
pub mod adapters;
mod help_table;
mod verbs;
pub mod wiring;

#[cfg(feature = "testing")]
pub mod testing;

pub use help_table::HELP;

/// What the thin binary mounts.
pub const COMMANDS: &[(&str, wf_kit::cli::Handler)] = &[("t3", verbs::run)];

pub use verbs::run;
