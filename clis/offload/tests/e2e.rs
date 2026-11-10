//! The e2e tests of `wf offload ask`: the CLI against the deployed rca service with a real model behind it, which the flow tests
//! (`flow.rs`) double. They check that the options `ask` takes from `opencode run` do what opencode's do.
#[allow(dead_code)]
#[path = "flow/support/mod.rs"]
mod support;

#[path = "e2e/ask.rs"]
mod ask;

#[path = "e2e/parity.rs"]
mod parity;

#[path = "e2e/parity_features.rs"]
mod parity_features;

#[path = "e2e/parity_capabilities.rs"]
mod parity_capabilities;
