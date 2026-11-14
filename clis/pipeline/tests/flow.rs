//! The flow tests of `wf test plan`, `wf test run --stage`, `wf test drift`, `wf test publish`, `wf ci`, `wf check --staged`,
//! `wf check --budget`: they enter through the thin `pipeline` binary's argv, with pnpm, nx, git and gh replaced by stubs, vm-ops-api by a
//! fake. The Nx cache's contract, proved against the real Nx on a copy of this repository, is its own test binary (`nx_contract.rs`): it
//! takes minutes, and only a change that can move the contract runs it.
#[path = "flow/support/mod.rs"]
mod support;

#[path = "flow/blast_radius.rs"]
mod blast_radius;
#[path = "flow/caller_drift.rs"]
mod caller_drift;
#[path = "flow/cargo_inputs.rs"]
mod cargo_inputs;
#[path = "flow/check.rs"]
mod check;
#[path = "flow/ci_comment.rs"]
mod ci_comment;
#[path = "flow/expected_revisions.rs"]
mod expected_revisions;
#[path = "flow/pipeline.rs"]
mod pipeline;
#[path = "flow/publish.rs"]
mod publish;
#[path = "flow/ts_packages.rs"]
mod ts_packages;
#[path = "flow/workspace_crossings.rs"]
mod workspace_crossings;
