//! What the gateway needs from outside. The service implements them over memory, the process environment and HTTP; `testing` has the doubles.
use crate::domain::{Grant, Outbound, Provider};
use async_trait::async_trait;
use futures_util::Stream;
use std::pin::Pin;

/// The grants of the live tokens, by the digest of the token: a token itself is never stored.
pub trait TokenStore: Send + Sync {
    fn put(&self, digest: String, grant: Grant);
    fn get(&self, digest: &str) -> Option<Grant>;
    /// Drops every token of the thread (its turn is over, or a new turn mints afresh).
    fn remove_thread(&self, thread: &str);
}

/// The provider keys the service holds.
pub trait KeySource: Send + Sync {
    fn key(&self, provider: &Provider) -> Option<String>;
}

/// Where a token's randomness comes from.
pub trait Ids: Send + Sync {
    fn token(&self) -> String;
}

pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Vec<u8>, String>> + Send>>;

/// The provider's answer: the head, and the body as it arrives. A body that fails part way ends with an `Err` item and is never closed as a complete one.
pub struct UpstreamReply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: ByteStream,
}

#[async_trait]
pub trait Upstream: Send + Sync {
    /// `Err` is why no answer came (the provider is unreachable, the connection broke before the head).
    async fn send(&self, request: Outbound) -> Result<UpstreamReply, String>;
}

/// The compression sidecar: shorter text with the same meaning, or why it could not.
#[async_trait]
pub trait Compressor: Send + Sync {
    async fn compress(&self, text: &str) -> Result<String, String>;
}
