//! What every backend service of this repo shares: logging and OTLP export with secrets scrubbed, the request trace layer,
//! the heartbeat, graceful shutdown and the rate limit. Everything else is the standard crate, used directly by the service.

#[cfg(feature = "blackbox")]
pub mod blackbox;
#[cfg(feature = "e2e")]
pub mod e2e;
pub mod heartbeat;
pub mod rate_limit;
pub mod secrets;
pub mod shutdown;
pub mod trace;

/// Logging and export, shared with the CLIs: `telemetry::init!("<service>", extra_secrets)` once at start.
pub use telemetry;
pub use telemetry::redact;
