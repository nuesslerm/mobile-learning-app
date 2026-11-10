//! `rca-exec` (`wf exec`): the executor of `docs/plans/rca-client/architecture.md`. It runs at a workspace, connects out to the rca service
//! (`GET /exec/v2/executors?workspace=<id>`) and performs every action a model takes there. `domain` holds the executor port and the mapping
//! of a wire request onto it, `adapters` implement it on the local disk and carry it over the channel, `verbs` is the command line and the
//! one place that wires them.
pub mod adapters;
pub mod domain;
pub mod hosted;
mod help_table;
mod verbs;

pub use help_table::HELP;

/// What the thin binary mounts: `rca-exec exec serve` and, as the binary's own spelling, `rca-exec serve`.
pub const COMMANDS: &[(&str, wf_kit::cli::Handler)] = &[("exec", verbs::run), ("serve", verbs::serve_alone)];

pub use verbs::run;
