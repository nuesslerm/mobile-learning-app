//! The e2e tests of `wf offload ask`: the CLI against a real `rca-service` and `opencode serve` started on loopback by the run (`rca.rs`),
//! which the flow tests (`flow.rs`) double. The `ask` group has the scripted `fake-model` behind opencode (no network, no key); the parity
//! groups compare wording with the real model and need `OPENCODE_GO_API_KEY`. They check that the options `ask` takes from `opencode run` do what opencode's do.
#[allow(dead_code)]
#[path = "flow/support/mod.rs"]
mod support;

#[path = "e2e/rca.rs"]
mod rca;

#[path = "e2e/ask.rs"]
mod ask;

#[path = "e2e/parity_support.rs"]
mod parity_support;

#[path = "e2e/parity.rs"]
mod parity;

#[path = "e2e/parity_features.rs"]
mod parity_features;

#[path = "e2e/parity_capabilities.rs"]
mod parity_capabilities;
