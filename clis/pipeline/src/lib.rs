//! The pipeline: which tests, deploy legs and gates a change needs (`wf test plan` and `wf test run --stage` through the Nx cache,
//! `wf test drift`, `wf check --staged` and `--budget`, the legs `wf ship --changed` plans, the pnpm workspace graph tier 3 plans by),
//! and what a run leaves for people to read (`wf ci comment` on the pushed commit, the run bundles `wf test publish` uploads with the
//! metrics row it posts, which `wf test results` reads back).
//! A change here reruns the crates that plan or ship from it.
mod ci_audit;
mod ci_cache_health;
mod ci_comment;
mod ci_ledger;
mod ci_live;
mod ci_preflight;
mod ci_tidy;
mod ci_insights;
pub mod blast_radius;
pub mod cargo_deps;
pub mod cargo_inputs;
pub mod copy_closure;
pub mod core_purity;
pub mod docker_ignore;
pub mod library_paths;
pub mod recorders;
pub mod pushed;
pub mod dead_refs;
pub mod device_run;
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
pub mod workspace_crossings;

use wf_kit::args::{parse_spec, Positional, Spec};

pub use ci_comment::run as ci;
pub use help_table::HELP;

/// What the thin binary mounts: the commands this crate's flow tests spawn
/// through the executable.
pub const COMMANDS: &[(&str, wf_kit::cli::Handler)] = &[("check", check), ("ci", ci), ("test", run_test)];

/// What `wf test plan` takes.
const PLAN: Spec = Spec { values: &["sha", "base", "platform", "force"], flags: &["json", "changed", "all", "github-output"], positional: Positional::Any, ..Spec::NONE };

/// What `wf test publish` takes.
const PUBLISH: Spec = Spec { values: &["ran-on"], flags: &["no-record"], positional: Positional::Any, ..Spec::NONE };

/// `wf test plan`, `wf test run --stage`, `wf test drift` and `wf test publish <dir>`: the words after `test`, for the router of `wf`
/// and the thin binary.
pub fn run_test(rest: &[String]) -> Option<i32> {
    match rest.first().map(String::as_str)? {
        "publish" => publish_verb(rest),
        "drift" => nx_drift::run(&rest[1..]),
        "run" if rest.iter().any(|a| a == "--stage") => Some(e2e_tests::run_stage(&rest[1..])),
        "plan" => Some(e2e_tests::plan(&parse_spec(rest, &PLAN)?)),
        _ => None,
    }
}

/// `check --staged`, `check --pushed` and `check --budget`, the gates that need nothing but the checkout. `wf` adds the dead-reference scan, which
/// reads its whole command surface, and tier 3; the thin binary has neither.
fn check(rest: &[String]) -> Option<i32> {
    let modes = wf_kit::args::probe(rest, &[&verify::STAGED, &verify::BUDGET, &pushed::PUSHED])?;
    if modes.flag("pushed") {
        pushed::run(rest)
    } else if modes.flag("staged") {
        verify::staged(rest, |_| None)
    } else if modes.flag("budget") || modes.flag("blast-radius") || modes.flag("unused-deps") || modes.flag("workspace-crossings") || modes.flag("cargo-inputs") {
        verify::budget(rest)
    } else {
        None
    }
}

fn publish_verb(rest: &[String]) -> Option<i32> {
    let p = parse_spec(rest, &PUBLISH)?;
    match p.positional.get(1) {
        Some(dir) => Some(test_publish::run(dir, !p.flag("no-record"), p.val("ran-on"))),
        None => {
            println!("{}", serde_json::json!({ "error": "wf test publish wants a dir", "next": "wf test publish <run dir|flow report dir|e2e run dir> [--no-record] [--ran-on <host>]" }));
            Some(1)
        }
    }
}
