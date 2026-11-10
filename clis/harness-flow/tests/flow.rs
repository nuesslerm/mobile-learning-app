//! The flow tests of `wf flow`: they enter through the thin `harness-flow` binary's argv, with `pnpm` (vitest) replaced by a stub.
//! The frozen screens painted in a real Chromium are `e2e.rs`.
#[path = "flow/support/mod.rs"]
mod support;

#[path = "flow/run.rs"]
mod run;
