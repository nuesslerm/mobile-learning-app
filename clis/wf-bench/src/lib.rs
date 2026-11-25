//! `wf bench`: the cold start and footprint of a CLI binary (`cli`), and the app benchmarks (build speed, memory, runtime
//! speed, startup, the Android cold start) with the scenarios and baselines committed in `clis/app-bench`. A change here reruns
//! the crate above it, which is `wf` only.
mod agent_proof;
mod app_bench;
mod bench;
mod help_table;
mod reports;

pub use help_table::HELP;

/// What the thin binary mounts: `bench`, and `browser-daemon`, which the suites that drive a page start again through the executable.
pub const COMMANDS: &[(&str, wf_kit::cli::Handler)] = &[("bench", run), ("browser-daemon", wf_harness::browser_daemon::run)];

/// Runs the selected benchmark with the words after `bench`.
pub fn run(args: &[String]) -> Option<i32> {
    match args.split_first() {
        Some((kind, more)) if kind == "drivers" => {
            let script = wf_kit::root::repo_root().join("clis/wf-bench/src/drivers/benchmark.py");
            let status = std::env::current_exe().and_then(|wf| std::process::Command::new("python3")
                .arg(script).arg("--wf").arg(wf).args(more).status());
            Some(status.map_or_else(|error| { println!("{}", serde_json::json!({"ok":false,"error":error.to_string()})); 1 }, |s| s.code().unwrap_or(1)))
        }
        Some((kind, more)) if kind == "reports" => reports::run(more),
        Some((kind, more)) if kind == "cli" => bench::run(more),
        Some((kind, more)) if kind == "agent-proof" => agent_proof::run(more),
        _ => app_bench::run(args),
    }
}
