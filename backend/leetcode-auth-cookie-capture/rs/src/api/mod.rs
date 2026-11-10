//! The front door: `POST /trigger` behind a per-caller rate limit and the shared trigger secret. Everything else is a 404 with no body.
//! The handler starts the attempt in the background and answers at once, so a caller never waits for a person to sign in.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use capture_core::application::Capture;
use capture_core::domain::outcome::Outcome;
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use service_kit::{rate_limit, trace};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use subtle::ConstantTimeEq;

/// How many triggers one caller may send a minute, before the secret is even looked at.
const TRIGGERS_PER_MINUTE: u32 = 30;

#[derive(Clone)]
struct AppState {
    capture: Arc<Capture>,
    trigger_secret: SecretString,
    running: Arc<AtomicBool>,
}

/// The listener over `capture`, whose ports `main` fills with the real adapters and a flow test with doubles.
pub fn app(capture: Arc<Capture>, trigger_secret: SecretString) -> Router {
    let state = AppState { capture, trigger_secret, running: Arc::new(AtomicBool::new(false)) };
    Router::new()
        .route("/trigger", post(trigger).layer(rate_limit::per_minute_json(TRIGGERS_PER_MINUTE)))
        .fallback(not_found)
        .method_not_allowed_fallback(not_found)
        .layer(axum::middleware::from_fn(trace::log_request))
        .layer(trace::layer())
        .with_state(state)
}

async fn not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}

fn secret_matches(provided: &str, expected: &SecretString) -> bool {
    provided.as_bytes().ct_eq(expected.expose_secret().as_bytes()).into()
}

async fn trigger(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let provided = headers.get("x-capture-trigger-secret").and_then(|v| v.to_str().ok()).unwrap_or("");
    if !secret_matches(provided, &state.trigger_secret) {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "unauthorized" }))).into_response();
    }
    if state.running.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return (StatusCode::OK, Json(json!({ "accepted": true, "alreadyRunning": true }))).into_response();
    }
    let running = state.running.clone();
    let capture = state.capture.clone();
    tokio::spawn(async move {
        match capture.run().await {
            Ok(Outcome::Failed { message }) => tracing::warn!(message = %message, "capture.attempt.failed"),
            Ok(outcome) => tracing::info!(outcome = ?outcome, "capture.attempt.finished"),
            Err(error) => tracing::error!(error = %error, "capture.attempt.not_started"),
        }
        running.store(false, Ordering::SeqCst);
    });
    (StatusCode::ACCEPTED, Json(json!({ "accepted": true }))).into_response()
}
