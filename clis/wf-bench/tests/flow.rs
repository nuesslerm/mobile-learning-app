//! The flow tests of `wf bench`: they enter through the thin `wf-bench` binary's argv, with `pnpm` (the app export), the VM
//! (the fake vm-ops-api of `wf_vmops::testing`) and the browser replaced as each test says; the real-Chromium measurement is `e2e.rs`.
#[path = "flow/support/mod.rs"]
mod support;

#[path = "lab/mod.rs"]
mod lab;

#[path = "flow/bench.rs"]
mod bench;
