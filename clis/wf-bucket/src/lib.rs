//! The buckets `wf` stores in: `wf cache` and the loopback shim that gives Nx the shared build cache, the pnpm store, and the read and
//! write links of the verification-artifacts bucket. A change here reruns every crate that publishes, plans or ships.
pub mod artifacts;
pub mod cache;
pub mod cache_runs;
pub mod ci_metrics;
mod help_table;
pub mod job_name;
pub mod nx_why;
pub mod pins;

pub use help_table::HELP;

/// What the thin binary mounts.
pub const COMMANDS: &[(&str, wf_kit::cli::Handler)] = &[("cache", cache::run)];
