//! The HTTP front door: routes under `/api/leetcode/*` behind the bearer token (the session routes also behind the admin secret),
//! `/health` open, and the edge layers around them (CORS, rate limit, trace, body limit). Handlers parse, call a use case of
//! `leetcode-backend-core` and map the result to a response; they hold no logic of their own.

mod auth;
mod error;
mod handlers;

use axum::http::Method;
use axum::middleware::from_fn_with_state;
use axum::routing::{get, post};
use axum::Router;
use leetcode_backend_core::application::ports::Ports;
use leetcode_backend_core::application::Service;
use secrecy::SecretString;
use service_kit::{rate_limit, trace};
use std::sync::Arc;
use tower_http::cors::{AllowHeaders, Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;

/// One megabyte, the largest request body.
const BODY_LIMIT: usize = 1024 * 1024;
/// Requests one caller may make per minute in production.
pub const REQUESTS_PER_MINUTE: u32 = 120;

/// What the service is told at start, apart from its ports.
pub struct Settings {
    pub commit: String,
    pub bearer: SecretString,
    pub admin: SecretString,
    pub requests_per_minute: u32,
}

#[derive(Clone)]
struct AppState {
    service: Arc<Service>,
    commit: Arc<str>,
    bearer: SecretString,
    admin: SecretString,
}

/// The routes and their authorisation, without the edge layers.
fn routes(state: AppState) -> Router {
    let leetcode = Router::new()
        .route("/user-status", get(handlers::user_status))
        .route("/login-session", get(handlers::login_session).post(handlers::start_login))
        .route("/test", post(handlers::test))
        .route("/submit", post(handlers::submit))
        .route("/poll/{id}", get(handlers::poll))
        .route("/run", post(handlers::run))
        .route("/format", post(handlers::format))
        .route("/session", post(handlers::set_session))
        .fallback(handlers::admin_then_not_found)
        .layer(from_fn_with_state(state.clone(), auth::require_bearer));
    Router::new().route("/health", get(handlers::health)).nest("/api/leetcode", leetcode).with_state(state)
}

/// The whole service over `ports`: `routes` inside CORS (any origin), a per-caller rate limit, the request trace and the body limit.
/// `main` passes the real adapters, a flow test its doubles.
pub fn app(ports: Ports, settings: Settings) -> Router {
    let state = AppState {
        service: Arc::new(Service::new(ports)),
        commit: settings.commit.into(),
        bearer: settings.bearer,
        admin: settings.admin,
    };
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::HEAD, Method::PUT, Method::PATCH, Method::POST, Method::DELETE])
        .allow_headers(AllowHeaders::mirror_request());
    routes(state)
        .layer(RequestBodyLimitLayer::new(BODY_LIMIT))
        .layer(axum::middleware::from_fn(trace::log_request))
        .layer(trace::layer())
        .layer(rate_limit::per_minute(settings.requests_per_minute))
        .layer(cors)
}
