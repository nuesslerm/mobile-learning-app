//! Per-caller rate limit as a tower layer (`tower_governor`, a token bucket). The key is the peer's IP address unless the service
//! names its own extractor. The server must be started with `into_make_service_with_connect_info::<SocketAddr>()` so the layer can
//! see who is calling.

use axum::body::Body;
use axum::http::{header, Response, StatusCode};
use governor::middleware::NoOpMiddleware;
use std::sync::Arc;
use std::time::Duration;
use tower_governor::errors::GovernorError;
use tower_governor::governor::{GovernorConfig, GovernorConfigBuilder};
use tower_governor::key_extractor::{KeyExtractor, PeerIpKeyExtractor};
use tower_governor::GovernorLayer;

const CLEAN_EVERY: Duration = Duration::from_secs(60);

/// `max` requests per minute per caller: a burst of `max`, refilled evenly over the minute.
pub fn per_minute(max: u32) -> GovernorLayer<PeerIpKeyExtractor, NoOpMiddleware> {
    per_minute_by(max, PeerIpKeyExtractor)
}

/// `per_minute` with the caller identified by `key` (for a service behind a proxy that forwards the address).
pub fn per_minute_by<K: KeyExtractor>(max: u32, key: K) -> GovernorLayer<K, NoOpMiddleware>
where
    K::Key: Send + Sync + 'static,
{
    layer(max, key, false)
}

/// `per_minute` answering a refusal with `429 {"error":"rate_limited"}` and a `Retry-After` header instead of the library's text.
pub fn per_minute_json(max: u32) -> GovernorLayer<PeerIpKeyExtractor, NoOpMiddleware> {
    layer(max, PeerIpKeyExtractor, true)
}

fn json_refusal(error: GovernorError) -> Response<Body> {
    let wait = match &error {
        GovernorError::TooManyRequests { wait_time, .. } => *wait_time,
        _ => 1,
    };
    let (status, body) = match error {
        GovernorError::TooManyRequests { .. } => (StatusCode::TOO_MANY_REQUESTS, r#"{"error":"rate_limited"}"#),
        _ => (StatusCode::INTERNAL_SERVER_ERROR, r#"{"error":"rate_limit_failed"}"#),
    };
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::RETRY_AFTER, wait.to_string())
        .body(Body::from(body))
        .expect("a static response")
}

fn layer<K: KeyExtractor>(max: u32, key: K, json: bool) -> GovernorLayer<K, NoOpMiddleware>
where
    K::Key: Send + Sync + 'static,
{
    let max = max.max(1);
    let mut builder = GovernorConfigBuilder::default();
    let mut builder = builder.key_extractor(key);
    builder.per_millisecond(u64::from(60_000 / max).max(1)).burst_size(max);
    if json {
        builder.error_handler(json_refusal);
    }
    let config: GovernorConfig<K, NoOpMiddleware> = builder.finish().expect("a non-zero quota");
    let config = Arc::new(config);
    let limiter = config.limiter().clone();
    std::thread::Builder::new()
        .name("rate-limit-clean".into())
        .spawn(move || loop {
            std::thread::sleep(CLEAN_EVERY);
            limiter.retain_recent();
        })
        .ok();
    GovernorLayer { config }
}
