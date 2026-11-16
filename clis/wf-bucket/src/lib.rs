//! The buckets `wf` stores in: `wf cache` and the loopback shim that gives Nx the shared build cache, the pnpm store, the read and
//! write links of the verification-artifacts bucket, and the `refs/wf/state` ref that keeps small cross-run files in the repository. A change here reruns every crate that publishes, plans or ships.
pub mod artifacts;
pub mod cache;
pub mod cache_runs;
pub mod ci_metrics;
mod help_table;
mod state_verb;
pub mod job_name;
pub mod nx_why;
pub mod pins;
pub mod serve;
pub mod state;

pub use help_table::HELP;

/// What the thin binary mounts.
pub const COMMANDS: &[(&str, wf_kit::cli::Handler)] = &[("cache", cache::run), ("bucket", bucket_run)];

pub fn bucket_run(args: &[String]) -> Option<i32> {
    serve::run(args).or_else(|| state_verb::run(args))
}
