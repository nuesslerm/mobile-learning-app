//! `wf flow` in a real Chromium: the frozen screens of a flow run painted into goldens. The flow tests (`flow.rs`) double it.
#[allow(dead_code)]
#[path = "flow/support/mod.rs"]
mod support;

#[path = "e2e/goldens.rs"]
mod goldens;
