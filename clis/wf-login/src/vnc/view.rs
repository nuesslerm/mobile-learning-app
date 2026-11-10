//! The token gate and the viewer, behind `tailscale serve`: the one ingress to the login browser. `tailscale serve`'s
//! backend is one fixed target with no request-level hook, so the per-session `?t=` link token is checked here, on every
//! request. The link is reusable for its whole TTL (the same link opens from more than one device); the first valid request
//! is handed an httpOnly cookie as well, and either one is accepted afterwards. What the cookie vouches for lives in this
//! process only, so a listener restart invalidates every outstanding link, TTL notwithstanding.
//!
//! Three routes: `GET /` the viewer page, `GET /frame` a JPEG of the browser's page (long-polled: it answers when the page
//! changed), `POST /input` a tap, scroll or key.
use super::desktop::Launcher;
use super::engine::Tokens;
use super::log;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, COOKIE, REFERRER_POLICY, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::serve::ListenerExt;
use axum::Router;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const COOKIE_NAME: &str = "vnc_auth_t";
const VIEWER_HTML: &str = include_str!("js/viewer.html");
const MAX_INPUT_BYTES: usize = 8 * 1024;
const NO_BROWSER: &str = "no browser";

struct Gate {
    launcher: Arc<Launcher>,
    tokens: Tokens,
    vouched: Mutex<HashSet<String>>,
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get_all(COOKIE).iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(';')).find_map(|part| {
        let (k, v) = part.split_once('=')?;
        (k.trim() == name).then(|| cli_http::decode_uri_component(v.trim()))
    })
}

fn query_param(query: Option<&str>, key: &str) -> Option<String> {
    let query = query?.split('#').next()?;
    form_urlencoded::parse(query.as_bytes()).find(|(k, _)| k == key).map(|(_, v)| v.into_owned())
}

impl Gate {
    /// The token this request is authorized by: a cookie this gate already vouched for, else a valid `?t=`.
    fn authorize(&self, headers: &HeaderMap, query: Option<&str>) -> Option<String> {
        let mut vouched = self.vouched.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cookie) = cookie_value(headers, COOKIE_NAME).filter(|c| vouched.contains(c)) {
            return Some(cookie);
        }
        let token = query_param(query, "t").filter(|t| self.tokens.is_valid(t))?;
        vouched.insert(token.clone());
        Some(token)
    }
}

fn text(status: StatusCode, body: &'static str) -> Response {
    (status, [(CONTENT_TYPE, "text/plain")], body).into_response()
}

fn viewer() -> Response {
    (StatusCode::OK, [(CONTENT_TYPE, "text/html; charset=utf-8"), (CACHE_CONTROL, "no-store"), (REFERRER_POLICY, "no-referrer")], VIEWER_HTML).into_response()
}

/// The blocking launcher (it waits on Chromium) runs off the async workers.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tokio::task::spawn_blocking(work).await.ok()
}

const FRAME_WAIT: Duration = Duration::from_secs(8);
const FRAME_FIRST_WAIT: Duration = Duration::from_secs(5);

/// `GET /frame[?after=<n>]`: the page once it differs from frame `n` (the `x-frame-seq` of the one the viewer has), `204` when
/// it held still for the whole wait. Without `after`, the current frame.
async fn frame(launcher: Arc<Launcher>, query: Option<String>) -> Response {
    let after = query_param(query.as_deref(), "after").and_then(|a| a.parse::<u64>().ok());
    let wait = if after.is_some() { FRAME_WAIT } else { FRAME_FIRST_WAIT };
    match blocking(move || launcher.frame_after(after, wait)).await {
        Some(Ok(Some((seq, frame)))) => (
            StatusCode::OK,
            [
                (CONTENT_TYPE, "image/jpeg"),
                (CACHE_CONTROL, "no-store"),
                (axum::http::HeaderName::from_static("x-editable"), if frame.editable { "1" } else { "0" }),
                (axum::http::HeaderName::from_static("x-frame-seq"), &seq.to_string()),
            ],
            frame.jpeg.clone(),
        )
            .into_response(),
        Some(Ok(None)) => StatusCode::NO_CONTENT.into_response(),
        Some(Err(e)) if e == NO_BROWSER => text(StatusCode::SERVICE_UNAVAILABLE, NO_BROWSER),
        Some(Err(e)) => {
            log::error("vnc_auth.view.frame_failed", &[("err", e)]);
            text(StatusCode::SERVICE_UNAVAILABLE, "frame unavailable")
        }
        None => text(StatusCode::INTERNAL_SERVER_ERROR, "internal error"),
    }
}

async fn input(launcher: Arc<Launcher>, body: Body) -> Response {
    let Ok(bytes) = axum::body::to_bytes(body, MAX_INPUT_BYTES).await else { return text(StatusCode::PAYLOAD_TOO_LARGE, "too large") };
    let Ok(event) = serde_json::from_slice::<Value>(&bytes) else { return text(StatusCode::BAD_REQUEST, "invalid json") };
    match blocking(move || launcher.input(&event)).await {
        Some(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Some(Err(e)) if e == NO_BROWSER => text(StatusCode::SERVICE_UNAVAILABLE, NO_BROWSER),
        Some(Err(e)) => {
            log::error("vnc_auth.view.input_failed", &[("err", e)]);
            text(StatusCode::BAD_REQUEST, "input rejected")
        }
        None => text(StatusCode::INTERNAL_SERVER_ERROR, "internal error"),
    }
}

async fn handle(State(gate): State<Arc<Gate>>, request: Request) -> Response {
    let Some(token) = gate.authorize(request.headers(), request.uri().query()) else {
        return text(StatusCode::FORBIDDEN, "forbidden: missing, invalid, or expired session token");
    };
    let (method, path) = (request.method().as_str().to_string(), request.uri().path().to_string());
    let mut response = match (method.as_str(), path.as_str()) {
        ("GET", "/") => viewer(),
        ("GET", "/frame") => frame(gate.launcher.clone(), request.uri().query().map(str::to_string)).await,
        ("POST", "/input") => input(gate.launcher.clone(), request.into_body()).await,
        _ => text(StatusCode::NOT_FOUND, "not found"),
    };
    if let Ok(value) = HeaderValue::from_str(&format!("{COOKIE_NAME}={token}; HttpOnly; Path=/; SameSite=Lax")) {
        response.headers_mut().append(SET_COOKIE, value);
    }
    response
}

pub fn router(launcher: Arc<Launcher>, tokens: Tokens) -> Router {
    Router::new().fallback(handle).with_state(Arc::new(Gate { launcher, tokens, vouched: Mutex::new(HashSet::new()) }))
}

/// Accepts connections until the process ends.
pub async fn serve(listener: tokio::net::TcpListener, launcher: Arc<Launcher>, tokens: Tokens) {
    let listener = listener.tap_io(|tcp| {
        let _ = tcp.set_nodelay(true);
    });
    if let Err(e) = axum::serve(listener, router(launcher, tokens)).await {
        log::error("vnc_auth.gate.stopped", &[("err", e.to_string())]);
    }
}
