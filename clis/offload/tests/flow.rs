//! The flow tests of `wf offload ask` and the git-state worker: they enter through the thin `offload` binary's argv, with opencode, git, pnpm
//! and the ladder's pricing source replaced by stubs, and vm-ops-api by the fake of `wf_vmops::testing` behind a fake tailnet.
#[path = "flow/support/mod.rs"]
mod support;

#[path = "flow/ask.rs"]
mod ask;

#[path = "flow/worker.rs"]
mod worker;
