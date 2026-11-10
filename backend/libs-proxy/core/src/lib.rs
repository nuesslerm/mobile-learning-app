//! The proxy's rules, pure: which requests are served, which bucket object each one reads, and what a read result becomes.
//! It owns the `ObjectRead` port; the adapters live in `libs-proxy-rs`.
pub mod application;
pub mod domain;
#[cfg(feature = "testing")]
pub mod testing;

pub use application::ports::{Object, ObjectRead, ReadResult};
pub use application::reply::{respond, Reply};
pub use domain::routing::{route, Rejection, Route};
