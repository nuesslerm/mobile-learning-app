//! The control channel: `POST /auth/start|done|credential|logout|launch-url`, guarded by the shared trigger secret (401) and the owner
//! allowlist (403).
//!
//! The tokened `linkUrl` is returned to the authorized caller and never logged (it is a bearer secret). There is no
//! `/auth/persist`: a captured credential goes back to agent-shell as a file through vm-ops-api (see `relay`), and the
//! caller that started the login may fetch the same files with `/auth/credential` (the sandbox is no agent-shell).
use super::descriptor::{self, assemble_credential, builtin, env_or_vault, Descriptor};
use super::desktop::Launcher;
use super::engine::Engine;
use super::log;
use axum::body::Bytes;
use base64::Engine as _;
use axum::extract::State;
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::serve::ListenerExt;
use axum::Router;
use serde_json::{json, Value};
use std::sync::Arc;
use tower_http::limit::RequestBodyLimitLayer;

const MAX_BODY: usize = 1024 * 1024;

/// Origins `/auth/launch-url` may navigate the shared desktop to: each login flow redirects through its provider's own
/// sign-in page, Google OAuth, or the magic-link completion page. Deliberately not an open-any-URL primitive.
const ALLOWED_LAUNCH_ORIGINS: [&str; 6] = [
    "https://claude.ai",
    "https://claude.com",
    "https://platform.claude.com",
    "https://console.anthropic.com",
    "https://auth.openai.com",
    "https://chatgpt.com",
];
const GOOGLE_ORIGIN: &str = "https://accounts.google.com";

pub struct Deps {
    pub trigger_secret: String,
    pub owners: Vec<String>,
    pub engine: Engine,
    pub launcher: Arc<Launcher>,
}

fn origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if scheme.is_empty() || !scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?.rsplit('@').next()?.to_lowercase();
    let authority = match (scheme.to_lowercase().as_str(), authority.rsplit_once(':')) {
        ("http", Some((h, "80"))) | ("https", Some((h, "443"))) => h.to_string(),
        _ => authority,
    };
    (!authority.is_empty()).then(|| format!("{}://{authority}", scheme.to_lowercase()))
}

fn is_allowed_launch_url(url: &str) -> bool {
    origin(url).is_some_and(|o| o == GOOGLE_ORIGIN || ALLOWED_LAUNCH_ORIGINS.contains(&o.as_str()))
}

/// Constant-time: a wrong secret must not be guessable one byte at a time.
fn same_secret(given: &str, expected: &str) -> bool {
    let (a, b) = (given.as_bytes(), expected.as_bytes());
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= usize::from(a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0));
    }
    diff == 0
}

fn string_field(body: &Value, key: &str) -> Option<String> {
    body.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string)
}

/// The cookies a spec's `capture` asks for, then the credential delivered where `deliver` says; the failure text is the
/// adapter's own and never contains the credential.
fn persist(descriptor: &Descriptor, launcher: &Launcher) -> Result<(), String> {
    let (Some(capture), Some(deliver)) = (&descriptor.capture, &descriptor.deliver) else { return Ok(()) };
    let credential = assemble_credential(capture, &launcher.cookies(&capture.urls))?;
    descriptor::deliver(deliver, &credential, &|name| env_or_vault(name))?;
    log::info("vnc_auth.spec_adapter.persisted", &[("tool", descriptor.id.clone())]);
    Ok(())
}

/// Whether the login finished (`ready`, judged by the descriptor's own validity check in this container) and, when the
/// caller asks to `copy`, the files the descriptor declares copyable, each as `{path, content}` (base64, `~` unexpanded).
/// Only declared paths are ever read, so a caller cannot name a file. Never logged.
fn credential_state(descriptor: &Descriptor, copy: bool, user: &str) -> (u16, Value) {
    if copy && descriptor.copy.is_empty() {
        return error(400, "this login declares nothing to copy");
    }
    if !crate::auth::is_authenticated(descriptor, Some(user)) {
        let check = crate::auth::check_output(descriptor, Some(user));
        return (200, if check.is_empty() { json!({ "ready": false }) } else { json!({ "ready": false, "check": check }) });
    }
    if !copy {
        return (200, json!({ "ready": true }));
    }
    let mut files = Vec::new();
    for path in &descriptor.copy {
        match std::fs::read(crate::vnc::expand_tilde(path)) {
            Ok(bytes) => files.push(json!({ "path": path, "content": base64::engine::general_purpose::STANDARD.encode(bytes) })),
            Err(_) => return (200, json!({ "ready": false })),
        }
    }
    (200, json!({ "ready": true, "files": files }))
}

/// Logs the tool out of this container: revokes it where the tool can (gcloud), deletes every credential file its descriptor
/// names, and clears the login browser so the next sign-in asks for the account again. Reports paths, never contents.
fn logout(descriptor: &Descriptor, launcher: &Launcher) -> (u16, Value) {
    if descriptor.id == "gcloud" {
        // As the login user, like the login itself; then the whole config dir goes, so nothing a past root-run command left in
        // it (a root-owned directory the login user cannot write to) survives into the next login.
        let _ = std::process::Command::new("s6-setuidgid").args([launcher.user(), "gcloud", "auth", "revoke", "--all"]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
        let _ = std::fs::remove_dir_all(crate::vnc::expand_tilde("~/.config/gcloud"));
    }
    let mut paths: Vec<String> = descriptor.credential_paths.iter().chain(descriptor.valid_path.iter()).chain(descriptor.copy.iter()).cloned().collect();
    paths.sort();
    paths.dedup();
    let removed: Vec<&String> = paths.iter().filter(|p| std::fs::remove_file(crate::vnc::expand_tilde(p)).is_ok()).collect();
    let browser = launcher.clear_browser();
    log::info("vnc_auth.logout", &[("tool", descriptor.id.clone()), ("removed", removed.len().to_string()), ("browser", browser.to_string())]);
    (200, json!({ "ok": true, "removed": removed, "browser": browser }))
}

fn error(status: u16, message: &str) -> (u16, Value) {
    (status, json!({ "error": message }))
}

/// What `control` needs of a request: its method, path and headers.
struct Call {
    method: Method,
    path: String,
    headers: HeaderMap,
}

impl Call {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name)?.to_str().ok()
    }
}

fn control(call: &Call, body: &Value, deps: &Deps) -> (u16, Value) {
    if call.method != Method::POST {
        return error(404, "not found");
    }
    if !call.header("x-auth-trigger-secret").is_some_and(|s| same_secret(s, &deps.trigger_secret)) {
        return error(401, "unauthorized");
    }
    if !call.header("x-auth-owner-id").is_some_and(|o| deps.owners.iter().any(|a| a == o)) {
        return error(403, "forbidden");
    }
    match call.path.as_str() {
        "/auth/start" => {
            // Either name a tool this deployment ships, or hand over the whole descriptor: a login flow nobody anticipated
            // needs no code change and no redeploy, only a JSON spec.
            let descriptor = if let Some(spec) = body.get("spec") {
                match Descriptor::parse(spec) {
                    Ok(d) => d,
                    Err(issues) => return (400, json!({ "error": "invalid spec", "detail": issues })),
                }
            } else {
                let Some(tool) = string_field(body, "tool") else { return error(400, "missing tool or spec") };
                match builtin(&tool) {
                    Some(d) => d,
                    None => return error(404, &format!("unknown tool: {tool}")),
                }
            };
            let session = deps.engine.start(&descriptor);
            (202, json!({ "sessionId": session.session_id, "tool": session.tool, "status": session.status, "linkUrl": session.link_url }))
        }
        "/auth/done" => {
            let Some(session_id) = string_field(body, "sessionId") else { return error(400, "missing sessionId") };
            let Some(session) = deps.engine.get(&session_id) else { return error(404, "unknown session") };
            // Harvest before marking complete. For a credential that lives only in the browser this is the one moment it
            // can be taken. A failure is reported, never swallowed: ok:true for a credential that was never stored is the
            // worst outcome available.
            let persisted = session.descriptor.capture.is_some() && session.descriptor.deliver.is_some();
            if let Err(detail) = persist(&session.descriptor, &deps.launcher) {
                return (502, json!({ "error": "capture completed but the credential could not be stored", "detail": detail }));
            }
            deps.engine.mark_complete(&session_id);
            (200, json!({ "ok": true, "persisted": persisted }))
        }
        "/auth/credential" => {
            let Some(session_id) = string_field(body, "sessionId") else { return error(400, "missing sessionId") };
            let Some(session) = deps.engine.get(&session_id) else { return error(404, "unknown session") };
            credential_state(&session.descriptor, body.get("copy").and_then(Value::as_bool).unwrap_or(false), deps.launcher.user())
        }
        "/auth/logout" => {
            let Some(tool) = string_field(body, "tool") else { return error(400, "missing tool") };
            let Some(descriptor) = builtin(&tool) else { return error(404, &format!("unknown tool: {tool}")) };
            deps.engine.forget_tool(&tool);
            logout(&descriptor, &deps.launcher)
        }
        "/auth/launch-url" => {
            let Some(url) = string_field(body, "url") else { return error(400, "missing url") };
            if !is_allowed_launch_url(&url) {
                return error(400, "url origin not allowlisted");
            }
            deps.launcher.navigate(&url);
            (200, json!({ "ok": true }))
        }
        _ => error(404, "not found"),
    }
}

fn json_response(status: u16, body: &Value) -> Response {
    (StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR), [(CONTENT_TYPE, "application/json")], body.to_string()).into_response()
}

/// The blocking engine and launcher (they wait on the desktop and on Chromium) run off the async workers.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tokio::task::spawn_blocking(work).await.ok()
}

async fn control_route(State(deps): State<Arc<Deps>>, method: Method, uri: Uri, headers: HeaderMap, body: Bytes) -> Response {
    let call = Call { method, path: uri.path().to_string(), headers };
    let parsed = serde_json::from_slice::<Value>(&body).unwrap_or_else(|_| json!({}));
    match blocking(move || control(&call, &parsed, &deps)).await {
        Some((status, out)) => json_response(status, &out),
        None => json_response(500, &json!({ "error": "internal error" })),
    }
}

pub fn router(deps: Arc<Deps>) -> Router {
    Router::new()
        .fallback(control_route)
        .layer(RequestBodyLimitLayer::new(MAX_BODY))
        .with_state(deps)
}

/// Serves requests until the process ends.
pub async fn serve(listener: tokio::net::TcpListener, deps: Arc<Deps>) {
    let listener = listener.tap_io(|tcp| {
        let _ = tcp.set_nodelay(true);
    });
    if let Err(e) = axum::serve(listener, router(deps)).await {
        log::error("vnc_auth.listener.stopped", &[("err", e.to_string())]);
    }
}
