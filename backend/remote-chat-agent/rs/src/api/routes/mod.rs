//! The HTTP front door: the route table (`router`), the edge layers around it (`app`) and the few helpers every handler shares.
//! CORS answers a preflight first, then the rate limit, the body limit and the routes; a WebSocket upgrade is outside all of those.

use crate::api::app::App;
use crate::api::{auth, cors, ws};
use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, DefaultBodyLimit, FromRequest, Request};
use axum::http::header::{HeaderName, HeaderValue, CONTENT_TYPE};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::Router;
use serde_json::{json, Value};
use service_kit::{rate_limit, trace};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};

pub mod agent;
pub mod bearers;
pub mod caller;
pub mod control;
pub mod convert;
pub mod edit_stats;
pub mod exec_remote;
pub mod file_preview;
pub mod inventory;
pub mod plan;
pub mod question;
pub mod runs;
pub mod schedules;
pub mod telegram;
pub mod source_control;
pub mod tamil;
pub mod upload;
pub mod voice;

/// Express's `json({ limit: '30mb' })`: 30 * 1024 * 1024 bytes.
pub const BODY_LIMIT: usize = 30 * 1024 * 1024;
/// Requests one caller may make per minute in production.
pub const REQUESTS_PER_MINUTE: u32 = 120;

pub fn respond(status: u16, body: &Value) -> Response {
    text(status, "application/json; charset=utf-8", body.to_string())
}

pub fn text(status: u16, content_type: &str, body: impl Into<Vec<u8>>) -> Response {
    let mut response = Response::new(Body::from(body.into()));
    *response.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    if let Ok(value) = HeaderValue::from_str(content_type) {
        response.headers_mut().insert(CONTENT_TYPE, value);
    }
    response
}

pub fn empty(status: u16) -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    response
}

pub trait ResponseExt {
    fn with_header(self, name: &'static str, value: &str) -> Self;
}

impl ResponseExt for Response {
    fn with_header(mut self, name: &'static str, value: &str) -> Self {
        if let (Ok(name), Ok(value)) = (HeaderName::from_bytes(name.as_bytes()), HeaderValue::from_str(value)) {
            self.headers_mut().append(name, value);
        }
        self
    }
}

/// What a handler reads of the request besides its body: the method and path, the raw query and the headers.
pub struct Req {
    pub method: Method,
    pub path: String,
    pub query: String,
    pub headers: HeaderMap,
}

impl Req {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }

    /// The query's `key=value` pairs, percent-decoded, `+` read as a space.
    pub fn query_pairs(&self) -> impl Iterator<Item = (std::borrow::Cow<'_, str>, std::borrow::Cow<'_, str>)> {
        url::form_urlencoded::parse(self.query.as_bytes())
    }

    /// The first query parameter called `name`.
    pub fn query_param(&self, name: &str) -> Option<String> {
        self.query_pairs().find(|(key, _)| key == name).map(|(_, value)| value.into_owned())
    }
}

/// The JSON body when the request says it is JSON and has one (`Ok(None)` for no body or another type).
fn json_body(headers: &HeaderMap, bytes: &[u8]) -> Result<Option<Value>, serde_json::Error> {
    let is_json = headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|t| {
            let t = t.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
            t == "application/json" || (t.starts_with("application/") && t.ends_with("+json"))
        })
        .unwrap_or(false);
    if !is_json || bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    serde_json::from_slice(bytes).map(Some)
}

/// What most handlers take: the shared state, the request head and the parsed JSON body. A body that says it is JSON and is not is a 400
/// before the handler runs.
pub struct Call {
    pub app: Arc<App>,
    pub req: Req,
    pub body: Option<Value>,
}

impl FromRequest<Arc<App>> for Call {
    type Rejection = Response;

    async fn from_request(request: Request, app: &Arc<App>) -> Result<Self, Response> {
        let (parts, body) = request.into_parts();
        let req = Req { method: parts.method.clone(), path: parts.uri.path().to_string(), query: parts.uri.query().unwrap_or_default().to_string(), headers: parts.headers.clone() };
        let bytes = Bytes::from_request(Request::from_parts(parts, body), app).await.map_err(IntoResponse::into_response)?;
        let body = json_body(&req.headers, &bytes).map_err(|_| respond(400, &json!({ "error": "invalid_json" })))?;
        Ok(Self { app: Arc::clone(app), req, body })
    }
}

async fn health(Call { app, .. }: Call) -> Response {
    respond(200, &json!({ "status": "ok", "wsProtocolVersion": 1, "commit": app.config.commit }))
}

async fn not_found(_call: Call) -> Response {
    respond(404, &json!({ "error": "not_found" }))
}

/// The upgrade: 401 without the secret, else whatever axum makes of the handshake, else the connection.
async fn upgrade(
    axum::extract::State(app): axum::extract::State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Req,
    upgrade: Result<axum::extract::WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
) -> Response {
    let ctx = app.ws_context();
    if !auth::socket_authorised(ctx.secret.as_deref(), &req) {
        rca_service_core::application::log::warn!("ws.upgrade.unauthorized", pathname = req.path, remoteAddress = peer.to_string());
        return text(401, "text/plain; charset=utf-8", "unauthorized");
    }
    match upgrade {
        // The first protocol the client offers is the one chosen, as `ws` does when nothing else decides.
        Ok(upgrade) => {
            let offered = req.header("sec-websocket-protocol").and_then(|offered| offered.split(',').map(str::trim).find(|p| !p.is_empty())).map(str::to_string);
            let upgrade = match offered {
                Some(protocol) => upgrade.protocols([protocol]),
                None => upgrade,
            };
            upgrade.on_upgrade(move |connection| ws::run(ctx, connection))
        }
        Err(rejection) => rejection.into_response(),
    }
}

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for Req {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut axum::http::request::Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self { method: parts.method.clone(), path: parts.uri.path().to_string(), query: parts.uri.query().unwrap_or_default().to_string(), headers: parts.headers.clone() })
    }
}

/// The routes and their handlers, without the edge layers.
pub fn router(app: Arc<App>) -> Router {
    let http = Router::new()
        .route("/health", get(health))
        .route("/api/source-control/pull-requests", get(source_control::list).post(source_control::create))
        .route("/api/source-control/pull-requests/{number}", get(source_control::get))
        .route("/api/agent-clis", get(inventory::agent_clis))
        .route("/api/models/preferences", get(inventory::preferences))
        .route("/api/models/recent", get(inventory::models_recent))
        .route("/api/models", get(inventory::models))
        .route("/api/mcp", get(inventory::mcp_list))
        .route("/api/mcp/{name}/connect", post(inventory::mcp_connect))
        .route("/api/mcp/{name}/disconnect", post(inventory::mcp_disconnect))
        .route("/api/skills", get(inventory::skills))
        .route("/api/commands", get(inventory::commands))
        .route("/api/plan/{id}", get(plan::get))
        .route("/api/plan/{id}/annotate", post(plan::annotate))
        .route("/api/convert", post(convert::post))
        .route("/api/tamil/check", post(tamil::check))
        .route("/api/voice-transcribe", post(voice::transcribe))
        .route("/api/voice-plan", post(voice::plan))
        .route("/api/voice-edit", post(voice::edit))
        .route("/api/edit-stats", post(edit_stats::post))
        .route("/agent/v1/bearers", post(bearers::mint).get(bearers::list))
        .route("/agent/v1/bearers/{id}", delete(bearers::revoke))
        .route("/hooks/v1/schedules/{id}/{token}", post(schedules::webhook).layer(DefaultBodyLimit::max(schedules::BODY_LIMIT)))
        .route("/runs", post(runs::post))
        .route("/runs/log", get(runs::log))
        .route("/runs/sessions", delete(runs::purge))
        .route("/runs/cooldowns/clear", post(runs::clear_cooldown))
        .route("/api/control/stop", post(control::stop))
        .route("/api/control/queue", post(control::queue))
        .route("/api/control/steer", post(control::steer))
        .route("/api/question/{request_id}/reply", post(question::reply))
        .route("/api/file/{*path}", get(file_preview::get).post(file_preview::annotate))
        .fallback(not_found)
        .with_state(Arc::clone(&app));
    http.layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(rate_limit::per_minute_by(app.config.requests_per_minute, caller::Caller))
        .layer(cors_layer(app.config.allowed_origins.clone()))
        .merge(Router::new().route(ws::PATH, get(upgrade)).with_state(Arc::clone(&app)))
        // The channel and the MCP server are called by machines (a long-lived socket, a CLI making many calls), so no rate limit; a tool call may carry a big file.
        .merge(Router::new().route("/agent/v1/socket", get(agent::socket)).route("/exec/v2/executors", get(exec_remote::channel)).route("/mcp/{thread}", post(exec_remote::mcp_server)).layer(DefaultBodyLimit::max(BODY_LIMIT)).with_state(app))
        .layer(CatchPanicLayer::custom(|_| respond(500, &json!({ "error": "internal_error" }))))
        .layer(axum::middleware::from_fn(trace::log_request))
        .layer(trace::layer())
}

fn cors_layer(allowed: Vec<String>) -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(move |origin, _| origin.to_str().is_ok_and(|o| cors::is_origin_allowed(Some(o), &allowed))))
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE, Method::OPTIONS])
        .allow_headers([CONTENT_TYPE, HeaderName::from_static("authorization"), HeaderName::from_static("x-bot-secret"), HeaderName::from_static("accept")])
        .max_age(Duration::from_secs(86_400))
}

pub use rca_service_core::domain::util::truthy;
