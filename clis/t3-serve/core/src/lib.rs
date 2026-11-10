//! The T3-compatible server's core: `domain` and `usecases` do no I/O and own the `ports` they need; adapters (`t3-store`,
//! `t3-http`, the root crate's clock, ids, log and catalog) implement them and the root crate wires them.
pub mod domain;
pub mod ports;
pub mod rpc;
pub mod usecases;

#[cfg(feature = "testing")]
pub mod testing;

pub use usecases::service::{EnvironmentInfo, Parts, Principal, Refusal, Service};
