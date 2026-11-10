//! The flow tests of `wf secret`: they enter through the thin `secrets` binary's argv, with `bws` and `cargo` replaced by stubs.
#[path = "flow/support/mod.rs"]
mod support;

#[path = "flow/secrets.rs"]
mod secrets;
