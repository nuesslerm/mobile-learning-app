//! The read API: GET only, JSON, no auth and no CORS (the service is reachable on the tailnet only). Routing is by path in one
//! handler so that every non-GET and every unknown path answers `404 {"error":"not_found"}`, as the service always has.
use axum::extract::{Query, State};
use axum::http::{Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::{Json, Router};
use percent_encoding::percent_decode_str;
use reporting_core::application::{Reporting, SeriesError};
use reporting_core::domain::sample::{Sample, SeriesPoint};
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

pub mod wiring;

pub use wiring::{App, Ports};

#[derive(Clone)]
pub struct AppState {
    pub reporting: Arc<Reporting>,
    pub started: Instant,
    /// The commit the image was built from, as `/health` reports it.
    pub commit: String,
}

pub fn app(state: AppState) -> Router {
    Router::new().fallback(any(handle)).with_state(state).layer(axum::middleware::from_fn(service_kit::trace::log_request)).layer(service_kit::trace::layer())
}

#[derive(Serialize)]
struct Series {
    window: String,
    points: Vec<SeriesPoint>,
}

#[derive(Serialize)]
struct Current {
    samples: Vec<Sample>,
}

fn json_response<T: Serialize>(status: StatusCode, body: &T) -> Response {
    (status, Json(body)).into_response()
}

fn not_found() -> Response {
    json_response(StatusCode::NOT_FOUND, &json!({ "error": "not_found" }))
}

fn internal(error: impl std::fmt::Display) -> Response {
    tracing::error!(error = %error, "reporting.http.failed");
    json_response(StatusCode::INTERNAL_SERVER_ERROR, &json!({ "error": "internal" }))
}

fn segment(path: &str, prefix: &str) -> Option<String> {
    let rest = path.strip_prefix(prefix)?;
    (!rest.is_empty() && !rest.contains('/')).then(|| percent_decode_str(rest).decode_utf8_lossy().into_owned())
}

async fn handle(State(state): State<AppState>, method: Method, uri: Uri, Query(query): Query<HashMap<String, String>>) -> Response {
    route(&state, &method, uri.path(), &query).await
}

async fn route(state: &AppState, method: &Method, path: &str, query: &HashMap<String, String>) -> Response {
    if method != Method::GET {
        return not_found();
    }
    let reporting = &state.reporting;
    match path {
        "/health" => match reporting.rows().await {
            Ok(rows) => json_response(StatusCode::OK, &json!({ "status": "ok", "uptime": (state.started.elapsed().as_secs_f64()).round() as u64, "rows": rows, "commit": state.commit })),
            Err(error) => internal(error),
        },
        "/api/metrics/current" => match reporting.current().await {
            Ok(samples) => json_response(StatusCode::OK, &Current { samples }),
            Err(error) => internal(error),
        },
        "/api/metrics/series" => match reporting.series(query.get("window").map(String::as_str), &series_query(query)).await {
            Ok((window, points)) => json_response(StatusCode::OK, &Series { window, points }),
            Err(SeriesError::InvalidWindow { accepted }) => json_response(StatusCode::BAD_REQUEST, &json!({ "error": "invalid_window", "accepted": accepted })),
            Err(SeriesError::Store(error)) => internal(error),
        },
        "/api/headroom" => match reporting.headroom().await {
            Ok(report) => json_response(StatusCode::OK, &report),
            Err(error) => internal(error),
        },
        "/api/plans" => json_response(StatusCode::OK, &json!({ "plans": reporting.plan_summaries().await })),
        _ => {
            if let Some(slug) = segment(path, "/api/plans/") {
                return match reporting.plan(&slug).await {
                    Some(detail) => json_response(StatusCode::OK, &detail),
                    None => not_found(),
                };
            }
            if let Some(run_id) = segment(path, "/api/runs/") {
                return match reporting.run(&run_id).await {
                    Some(report) => json_response(StatusCode::OK, &report),
                    None => not_found(),
                };
            }
            not_found()
        }
    }
}

/// `scope`, `metric` (comma separated) and `since` (epoch milliseconds) of a series request.
fn series_query(query: &std::collections::HashMap<String, String>) -> reporting_core::domain::sample::SeriesQuery {
    reporting_core::domain::sample::SeriesQuery {
        scope: query.get("scope").filter(|s| !s.is_empty()).cloned(),
        metrics: query.get("metric").map(|m| m.split(',').filter(|m| !m.is_empty()).map(String::from).collect()).unwrap_or_default(),
        since_ms: query.get("since").and_then(|s| s.parse().ok()),
    }
}
