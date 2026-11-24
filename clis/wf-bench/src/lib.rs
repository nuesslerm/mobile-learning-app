//! `wf bench`: the cold start and footprint of a CLI binary (`cli`), and the app benchmarks (build speed, memory, runtime
//! speed, startup, the Android cold start) with the scenarios and baselines committed in `clis/app-bench`. A change here reruns
//! the crate above it, which is `wf` only.
mod agent_proof;
mod app_bench;
mod bench;
mod help_table;

pub use help_table::HELP;

/// What the thin binary mounts: `bench`, and `browser-daemon`, which the suites that drive a page start again through the executable.
pub const COMMANDS: &[(&str, wf_kit::cli::Handler)] = &[("bench", run), ("browser-daemon", wf_harness::browser_daemon::run)];

/// Runs the selected benchmark with the words after `bench`.
pub fn run(args: &[String]) -> Option<i32> {
    match args.split_first() {
        Some((kind, more)) if kind == "trace" => {
            let status = std::process::Command::new("python3")
                .args(["-c", include_str!("trace.py")])
                .args(more)
                .env("WF_BENCH_ROOT", wf_kit::root::repo_root())
                .status();
            Some(match status {
                Ok(status) => status.code().unwrap_or(1),
                Err(error) => {
                    eprintln!("cannot run trace receiver: {error}");
                    1
                }
            })
        }
        Some((kind, more)) if kind == "cli" => bench::run(more),
        Some((kind, more)) if kind == "agent-proof" => agent_proof::run(more),
        _ => app_bench::run(args),
    }
}
