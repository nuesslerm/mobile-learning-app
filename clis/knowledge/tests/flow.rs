//! The flow tests of `wf knowledge`: they enter through the thin `knowledge` binary's argv; git, gcloud and the Google APIs are
//! replaced as each test says.
#[path = "flow/support/mod.rs"]
mod support;

#[path = "flow/knowledge.rs"]
mod knowledge;
