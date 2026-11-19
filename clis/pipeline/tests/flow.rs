//! The flow tests of `wf ci plan` (tests, tags, mimic), `wf test run --stage`, `wf test drift`, `wf test publish`, `wf ci`, `wf check --staged`,
//! `wf check --budget`: they enter through the thin `pipeline` binary's argv, with pnpm, nx, git and gh replaced by stubs, vm-ops-api by a
//! fake. The Nx cache's contract, proved against the real Nx on a copy of this repository, is its own test binary (`nx_contract.rs`): it
//! takes minutes, and only a change that can move the contract runs it.
#[path = "flow/support/mod.rs"]
mod support;

#[path = "flow/workspace_deps.rs"]
mod workspace_deps;
#[path = "flow/ast_rules.rs"]
mod ast_rules;
#[path = "flow/caller_drift.rs"]
mod caller_drift;
#[path = "flow/cargo_inputs.rs"]
mod cargo_inputs;
#[path = "flow/check.rs"]
mod check;
#[path = "flow/ci_comment.rs"]
mod ci_comment;
#[path = "flow/ci_timings.rs"]
mod ci_timings;
#[path = "flow/ci_db.rs"]
mod ci_db;
#[path = "flow/ci_plan.rs"]
mod ci_plan;
#[path = "flow/e2e_backends.rs"]
mod e2e_backends;
#[path = "flow/expected_revisions.rs"]
mod expected_revisions;
#[path = "flow/layout.rs"]
mod layout;
#[path = "flow/pipeline.rs"]
mod pipeline;
#[path = "flow/publish.rs"]
mod publish;
#[path = "flow/ts_packages.rs"]
mod ts_packages;
#[path = "flow/workspace_crossings.rs"]
mod workspace_crossings;
