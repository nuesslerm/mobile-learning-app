//! The e2e tests of `wf bench`: the app measured in a real Chromium, which the flow tests (`flow.rs`) double.
#[allow(dead_code)]
#[path = "flow/support/mod.rs"]
mod support;
#[allow(dead_code)]
#[path = "lab/mod.rs"]
mod lab;

#[path = "e2e/bench.rs"]
mod bench;
