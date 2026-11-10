//! A blocking client of the rca agent API (`GET /agent/v1/socket`, `docs/plans/rca-client/architecture.md`), the one thing every mapper
//! (`t3 serve`, an Orca mapper) reaches the service through. It knows the envelopes (`agent-api-proto`) and no method: a caller names the
//! method of each call and the one that opens each subscription, in the words of the group crate it uses.
//!
//! - `Client::call` returns a request's result, or an `Error` whose code is the service's own, or one of `error::code::ALL`.
//! - `Client::subscribe` keeps a stream (`Subscribe`) that `Client::next_event` delivers once and in order, across socket drops.
//! - ws:// only: there is no TLS here, the service is reached over loopback or the tailnet.
pub mod client;
mod connection;
pub mod error;

pub use client::{Backoff, Client, Config, Subscribe};
pub use connection::SOCKET_PATH;
pub use error::{code, Error};
