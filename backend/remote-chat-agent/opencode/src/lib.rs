//! The opencode adapters of the service: `Http` is the transport behind the `Opencode` port, `OpencodeBackend` the thread engine's `Backend`
//! and the routes' `Gateway`. The `testing` feature holds `Loopback`, an opencode server on a loopback port.

mod backend;
mod http;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
#[cfg(test)]
mod tests;

pub use backend::{EventMapper, OpencodeBackend};
pub use http::Http;
