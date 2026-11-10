//! The flow tests of `wf cache`: they enter through the thin `wf-bucket` binary's argv, with `oci` replaced by a stub and the bucket
//! by a loopback fake behind a PAR URL.
#[path = "flow/support/mod.rs"]
mod support;

#[path = "flow/cache.rs"]
mod cache;

#[path = "flow/guard.rs"]
mod guard;

#[path = "flow/metrics.rs"]
mod metrics;

#[path = "flow/why.rs"]
mod why;
