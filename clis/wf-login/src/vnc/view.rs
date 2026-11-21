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
use super::http::{self, Reply};
use tiny_http::Request;
use serde_json::Value;
use std::collections::HashSet;
use std::net::TcpListener;
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

fn cookie_value<'a>(values: impl Iterator<Item = &'a str>, name: &str) -> Option<String> {
    values.flat_map(|v| v.split(';')).find_map(|part| {
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
    fn authorize(&self, request: &Request, query: Option<&str>) -> Option<String> {
        let mut vouched = self.vouched.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cookie) = cookie_value(http::headers(request, "cookie"), COOKIE_NAME).filter(|c| vouched.contains(c)) {
            return Some(cookie);
        }
        let token = query_param(query, "t").filter(|t| self.tokens.is_valid(t))?;
        vouched.insert(token.clone());
        Some(token)
    }
}

fn text(status: u16, body: &'static str) -> Reply {
    Reply::new(status, "text/plain", body)
}

fn viewer() -> Reply {
    Reply::new(200, "text/html; charset=utf-8", VIEWER_HTML).header("Cache-Control", "no-store").header("Referrer-Policy", "no-referrer")
}

const FRAME_WAIT: Duration = Duration::from_secs(8);
const FRAME_FIRST_WAIT: Duration = Duration::from_secs(5);

/// `GET /frame[?after=<n>]`: the page once it differs from frame `n` (the `x-frame-seq` of the one the viewer has), `204` when
/// it held still for the whole wait. Without `after`, the current frame.
fn frame(launcher: &Arc<Launcher>, query: Option<&str>) -> Reply {
    let after = query_param(query, "after").and_then(|a| a.parse::<u64>().ok());
    let wait = if after.is_some() { FRAME_WAIT } else { FRAME_FIRST_WAIT };
    match launcher.frame_after(after, wait) {
        Ok(Some((seq, frame))) => Reply::new(200, "image/jpeg", frame.jpeg.clone())
            .header("Cache-Control", "no-store")
            .header("x-editable", if frame.editable { "1" } else { "0" })
            .header("x-frame-seq", seq.to_string()),
        Ok(None) => Reply::empty(204),
        Err(e) if e == NO_BROWSER => text(503, NO_BROWSER),
        Err(e) => {
            log::error("vnc_auth.view.frame_failed", &[("err", e)]);
            text(503, "frame unavailable")
        }
    }
}

fn input(launcher: &Launcher, request: &mut Request) -> Reply {
    let Some(bytes) = http::read_body(request, MAX_INPUT_BYTES) else { return text(413, "too large") };
    let Ok(event) = serde_json::from_slice::<Value>(&bytes) else { return text(400, "invalid json") };
    match launcher.input(&event) {
        Ok(()) => Reply::empty(204),
        Err(e) if e == NO_BROWSER => text(503, NO_BROWSER),
        Err(e) => {
            log::error("vnc_auth.view.input_failed", &[("err", e)]);
            text(400, "input rejected")
        }
    }
}

fn handle(gate: &Gate, mut request: Request) {
    let url = request.url().to_string();
    let (path, query) = http::split_url(&url);
    let Some(token) = gate.authorize(&request, query) else {
        return http::respond(request, text(403, "forbidden: missing, invalid, or expired session token"));
    };
    let reply = match (request.method().as_str(), path) {
        ("GET", "/") => viewer(),
        ("GET", "/frame") => frame(&gate.launcher, query),
        ("POST", "/input") => input(&gate.launcher, &mut request),
        _ => text(404, "not found"),
    };
    http::respond(request, reply.header("Set-Cookie", format!("{COOKIE_NAME}={token}; HttpOnly; Path=/; SameSite=Lax")));
}

/// Accepts connections until the process ends.
pub fn serve(listener: TcpListener, launcher: Arc<Launcher>, tokens: Tokens) {
    let gate = Arc::new(Gate { launcher, tokens, vouched: Mutex::new(HashSet::new()) });
    if let Err(e) = http::serve(listener, move |request| handle(&gate, request)) {
        log::error("vnc_auth.gate.stopped", &[("err", e)]);
    }
}
