//! The flow tests of `wf vm exec` and `wf vm net resolve`: they enter through the thin `wf-vmops` binary's argv, with vm-ops-api
//! replaced by the fake of `wf_vmops::testing` behind a fake tailnet.
#[path = "flow/support/mod.rs"]
mod support;

#[path = "flow/tailnet.rs"]
mod tailnet;

#[path = "flow/vm.rs"]
mod vm;

#[path = "flow/lanes.rs"]
mod lanes;
