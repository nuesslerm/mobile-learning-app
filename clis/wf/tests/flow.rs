//! The flow tests of `wf`: long caller journeys that enter through the built binary's argv, with every external dependency (adb,
//! docker, git, gh, pnpm, opencode, bws, oci, vm-ops-api, the bucket, ORDS, BetterStack) replaced by a stub or a fake service.
#[path = "flow/support/mod.rs"]
mod support;

#[path = "flow/agents.rs"]
mod agents;
#[path = "flow/check.rs"]
mod check;
#[path = "flow/harness.rs"]
mod harness;
#[path = "flow/pipeline.rs"]
mod pipeline;
#[path = "flow/report.rs"]
mod report;
#[path = "flow/surface.rs"]
mod surface;
#[path = "flow/vm.rs"]
mod vm;
