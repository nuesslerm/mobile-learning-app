//! One span per request, as the child of the caller's W3C `traceparent` when it sent one (and sampled out when the caller's trace
//! is not sampled), named `METHOD path` with the route, method and status as attributes; and one `http.request` line in it.

use axum::extract::MatchedPath;
use axum::http::{Request, Response};
use opentelemetry_http::HeaderExtractor;
use std::time::{Duration, Instant};
use tower_http::classify::{ServerErrorsAsFailures, SharedClassifier};
use tower_http::trace::{DefaultOnBodyChunk, DefaultOnEos, DefaultOnFailure, DefaultOnRequest, MakeSpan, OnResponse, TraceLayer};
use tracing::{field, Span};
use tracing_opentelemetry::OpenTelemetrySpanExt;

/// A route whose last segment is a `{token}` carries a credential in its URL: log the route, never the path.
pub fn loggable_path<'a>(route: &'a str, path: &'a str) -> &'a str {
    if route.ends_with("/{token}") {
        route
    } else {
        path
    }
}

#[derive(Clone, Copy)]
pub struct RequestSpan;

impl<B> MakeSpan<B> for RequestSpan {
    fn make_span(&mut self, request: &Request<B>) -> Span {
        let path = request.uri().path();
        let route = request.extensions().get::<MatchedPath>().map_or(path, MatchedPath::as_str);
        let span = tracing::info_span!(
            "request",
            otel.name = %format_args!("{} {}", request.method(), loggable_path(route, path)),
            http.method = %request.method(),
            http.route = %route,
            http.status_code = field::Empty,
            otel.status_code = field::Empty,
        );
        let parent = opentelemetry::global::get_text_map_propagator(|propagator| propagator.extract(&HeaderExtractor(request.headers())));
        span.set_parent(parent);
        span
    }
}

#[derive(Clone, Copy)]
pub struct RecordStatus;

impl<B> OnResponse<B> for RecordStatus {
    fn on_response(self, response: &Response<B>, _latency: Duration, span: &Span) {
        span.record("http.status_code", response.status().as_u16());
        if response.status().is_server_error() {
            span.record("otel.status_code", "ERROR");
        }
    }
}

pub type HttpTraceLayer =
    TraceLayer<SharedClassifier<ServerErrorsAsFailures>, RequestSpan, DefaultOnRequest, RecordStatus, DefaultOnBodyChunk, DefaultOnEos, DefaultOnFailure>;

/// Add it with `Router::layer` so the matched route is known when the span is made.
pub fn layer() -> HttpTraceLayer {
    TraceLayer::new_for_http().make_span_with(RequestSpan).on_response(RecordStatus)
}

/// One `http.request` line per request, inside its span so it carries the trace. Add it with `Router::layer` before
/// `layer()`, so the trace layer wraps it: `.layer(from_fn(trace::log_request)).layer(trace::layer())`.
pub async fn log_request(request: axum::extract::Request, next: axum::middleware::Next) -> axum::response::Response {
    let method = request.method().clone();
    let path = {
        let raw = request.uri().path();
        let route = request.extensions().get::<MatchedPath>().map_or(raw, MatchedPath::as_str);
        loggable_path(route, raw).to_string()
    };
    let started = Instant::now();
    let response = next.run(request).await;
    let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    tracing::info!(method = %method, path = %path, status = response.status().as_u16(), latencyMs = latency_ms, "http.request");
    response
}
