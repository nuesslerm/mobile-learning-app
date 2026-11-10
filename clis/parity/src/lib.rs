//! `wf parity` (`docs/plans/rca-parity/plan.md` §5): the coverage tables of the phone apps over the agent API. The reference scans,
//! the table and its checks are `parity-core`; `adapters` implement its ports over the disk and git, `verbs` is the command line.
pub mod adapters;
mod help_table;
mod verbs;

pub use help_table::HELP;
pub use verbs::run;

/// What the thin binary mounts.
pub const COMMANDS: &[(&str, wf_kit::cli::Handler)] = &[("parity", verbs::run)];
