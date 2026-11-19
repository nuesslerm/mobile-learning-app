//! The pipeline: which tests, deploy legs and gates a change needs (`wf ci plan tests` and `wf test run --stage` through the Nx cache,
//! `wf test drift`, `wf check --staged` and `--budget`, the legs `wf ci plan legs` plans, the pnpm workspace graph tier 3 plans by),
//! and what a run leaves for people to read (`wf ci comment` on the pushed commit, the run bundles `wf test publish` uploads with the
//! metrics row it posts, which `wf test results` reads back).
//! A change here reruns the crates that plan or ship from it.
mod ci_audit;
mod ci_cache_health;
mod ci_comment;
mod ci_db_verb;
mod ci_ledger;
mod ci_live;
mod ci_plan;
mod ci_preflight;
mod ci_tidy;
mod ci_insights;
pub mod arch_rules;
pub mod ast_globs;
pub mod workspace_deps;
pub mod cargo_inputs;
pub mod copy_closure;
pub mod core_purity;
pub mod docker_ignore;
pub mod layout;
pub mod library_paths;
pub mod recorders;
pub mod pushed;
pub mod dead_refs;
pub mod device_run;
pub mod e2e_backends;
pub mod e2e_run;
pub mod export_site;
pub mod test_results;
pub mod visual;
pub mod eas;
pub mod e2e_tests;
mod help_table;
pub mod image_hash;
pub mod legs;
pub mod lock_closure;
mod nx;
mod nx_ledger;
mod nx_drift;
mod test_plan;
pub mod test_metrics;
pub mod test_publish;
pub mod verify;
pub mod workflow_hygiene;
pub mod ts_packages;
pub mod workspace_crossings;

use wf_kit::args::{parse_spec, Positional, Spec};

pub use ci_comment::run as ci;
pub use help_table::HELP;

/// What the thin binary mounts: the commands this crate's flow tests spawn
/// through the executable.
pub const COMMANDS: &[(&str, wf_kit::cli::Handler)] = &[("check", check), ("ci", ci), ("test", run_test)];

/// What `wf test publish` takes.
const PUBLISH: Spec = Spec { values: &["ran-on", "out"], flags: &["no-record"], positional: Positional::Any, ..Spec::NONE };

/// `wf test run --stage`, `wf test drift` and `wf test publish <dir>`: the words after `test`, for the router of `wf`
/// and the thin binary.
pub fn run_test(rest: &[String]) -> Option<i32> {
    match rest.first().map(String::as_str)? {
        "publish" => publish_verb(rest),
        "drift" => nx_drift::run(&rest[1..]),
        "backends" => Some(e2e_backends::run(&rest[1..])),
        "run" if rest.iter().any(|a| a == "--stage") => Some(e2e_tests::run_stage(&rest[1..])),
        _ => None,
    }
}

/// Every first word the router of `wf` routes, for the dead-reference scan in the thin binary (a flow test of `wf` keeps it equal to its table).
pub const ROUTER_WORDS: &[&str] = &[
    "check", "ship", "ask", "plan", "run", "flow", "test", "ci", "android", "browser", "vm", "secret", "repo", "bench", "knowledge", "agent-config", "rca", "cache", "bucket", "lib", "t3", "exec", "mimic",
    "help", "log", "worker", "browser-daemon", "mcp-guard", "build-sha",
];

/// `check --staged`, `--pushed`, `--budget` and `--dead-refs`, spelled and ordered as the router of `wf` does, so the thin binary answers the same.
fn check(rest: &[String]) -> Option<i32> {
    dead_refs::set_commands(|w| ROUTER_WORDS.contains(&w));
    if let Some(at) = rest.iter().position(|a| a == "--dead-refs") {
        let mut left = rest.to_vec();
        left.remove(at);
        if left.is_empty() || left == ["--staged"] {
            return Some(dead_refs::run(&wf_kit::root::repo_root(), !left.is_empty()));
        }
    }
    let modes = wf_kit::args::probe(rest, &[&verify::STAGED, &verify::BUDGET, &pushed::PUSHED, &layout::LAYOUT])?;
    if modes.flag("layout") {
        layout::run(rest)
    } else if modes.flag("pushed") {
        pushed::run(rest)
    } else if modes.flag("staged") {
        verify::staged(rest, |root| dead_refs::gate(root).or_else(|| arch_rules::gate(root)))
    } else if modes.flag("budget") || modes.flag("unused-workspace-deps") || modes.flag("unused-deps") || modes.flag("workspace-crossings") || modes.flag("ts-packages") || modes.flag("cargo-inputs") {
        verify::budget(rest)
    } else {
        None
    }
}

fn publish_verb(rest: &[String]) -> Option<i32> {
    let p = parse_spec(rest, &PUBLISH)?;
    match p.positional.get(1) {
        Some(dir) => Some(test_publish::run(dir, !p.flag("no-record"), p.val("ran-on"), p.val("out"))),
        None => {
            println!("{}", serde_json::json!({ "error": "wf test publish wants a dir", "next": "wf test publish <run dir|flow report dir|e2e run dir> [--no-record] [--ran-on <host>] [--out <folder>]" }));
            Some(1)
        }
    }
}
