//! `wf mimic` (docs/plans/mimic/plan.md): record a pinned reference into cassettes, replay them against a candidate, diff two sets and
//! verify a set's freshness. The behaviour is `mimic-core`; `adapters` implement its ports over the disk, a child process and a closure,
//! `config` reads `mimic.toml`, `verbs` is the command line.
pub mod adapters;
pub mod child_env;
pub mod leak_scan;
pub mod config;
pub mod provision;
pub mod proxy_shape;
pub mod real_clis;
mod help_table;
mod verbs;

pub use help_table::HELP;
pub use verbs::run;

/// What the thin binary mounts.
pub const COMMANDS: &[(&str, wf_kit::cli::Handler)] = &[("mimic", verbs::run)];
