//! The model gateway (plan G2): the CLIs at the workspace host call it with a per-turn token, and it calls the model providers with the key it
//! holds. `Gateway::handle` runs the request stages whose rule matches; the token store, the keys, the upstream and the compression sidecar are ports.
pub mod domain;
pub mod egress;
pub mod gateway;
pub mod ports;
pub mod stages;

#[cfg(any(test, feature = "testing"))]
pub mod testing;
#[cfg(test)]
mod tests;

pub use domain::{provider, provider_for_driver, Auth, Facts, Grant, Inbound, Outbound, Protocol, Provider, Refusal, Reroute, Rule, PROVIDERS};
pub use egress::{Destination, EgressPolicy};
pub use gateway::{Gateway, Minted, Reply, TOKEN_PREFIX};
pub use ports::{ByteStream, Compressor, Ids, KeySource, TokenStore, Upstream, UpstreamReply};
pub use stages::{Context, Headroom, InjectCredential, ApplyReroute, Stage};
