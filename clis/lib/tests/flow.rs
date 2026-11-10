//! The flow tests of `wf lib`: they enter through the thin `wf-lib` binary's argv, in a throwaway git repository, with the bucket
//! as an in-memory double behind a link on loopback (`MemBucket`), `oci` replaced by a stub that talks to it, and real `cargo`.
#[path = "flow/support/mod.rs"]
mod support;

#[path = "flow/prune.rs"]
mod prune;
#[path = "flow/release.rs"]
mod release;
#[path = "flow/retarget.rs"]
mod retarget;
#[path = "flow/status.rs"]
mod status;
