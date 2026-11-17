//! The e2e tests of `wf rca ask`: the CLI against a real `rca-service` and `opencode serve` started on loopback by the run (`rca.rs`),
//! which the flow tests (`flow.rs`) double. The `ask` group has the scripted `fake-model` behind opencode (no network, no key); the compare
//! groups compare wording with the real model and need `OPENCODE_GO_API_KEY`. They check that the options `ask` takes from `opencode run` do what opencode's do.
#[allow(dead_code)]
#[path = "flow/support/mod.rs"]
mod support;

#[path = "e2e/rca.rs"]
mod rca;

#[path = "e2e/ask.rs"]
mod ask;

#[path = "e2e/compare_support.rs"]
mod compare_support;

#[path = "e2e/compare.rs"]
mod compare;

#[path = "e2e/compare_features.rs"]
mod compare_features;

#[path = "e2e/compare_capabilities.rs"]
mod compare_capabilities;
