//! The control surface of the VM without its edges: what a request is, who may make it, what `/health` says and what the
//! heartbeat beats. The clock, the event log and the process's memory are ports; `vm-ops-api-rs` implements them and wires them.

pub mod application;
pub mod domain;

#[cfg(any(test, feature = "testing"))]
pub mod testing;
