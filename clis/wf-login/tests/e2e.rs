//! The e2e tests of `wf secret login`: the listener driving a real Chromium, which the flow tests (`flow.rs`) double.
#[allow(dead_code)]
#[path = "flow/support/mod.rs"]
mod support;
#[allow(dead_code)]
#[path = "rig/mod.rs"]
mod rig;

#[path = "e2e/auth.rs"]
mod auth;
