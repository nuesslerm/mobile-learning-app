//! Client for vm-ops-api, the bearer-token-gated HTTP control surface on the VM. Every call is signed
//! (a rotating HMAC token plus an Ed25519 signature over method, path, timestamp and body hash) and goes
//! over tailscaled's userspace SOCKS5 listener, as the TypeScript client does.
use wf_kit::time::now_ms;
use rca_core::time::utc_date;
use base64::Engine;
use ed25519_dalek::pkcs8::DecodePrivateKey;
use ed25519_dalek::{Signer, SigningKey};
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::{Digest, Sha256};
use cli_http::{Request, Via};
use std::time::Duration;

const HOST: &str = "vm-ops-api-main";
/// The admin port: every route, the admin seed.
pub const PORT: u16 = 8080;
/// The spare port a smoke container's CI listener takes, beside `services::SMOKE_PORT` for its admin one.
pub const CI_SMOKE_PORT: u16 = 18998;
const DEFAULT_SOCKS: &str = "localhost:1055";

fn hmac_hex(seed: &str, message: &str) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(seed.as_bytes()).expect("hmac accepts any key length");
    mac.update(message.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

fn rotating_token(seed: &str, date: &str) -> String {
    hmac_hex(seed, date)
}

/// The three headers of one authenticated request. A GET hashes the empty body, as the verifier does.
pub fn sign_request(
    seed: &str,
    private_key_pem: &str,
    method: &str,
    path_with_query: &str,
    body: Option<&str>,
    now: i64,
) -> Result<Vec<(String, String)>, String> {
    let timestamp = now.to_string();
    let body_hash = hex::encode(Sha256::digest(body.unwrap_or("").as_bytes()));
    let canonical = format!("{method}\n{path_with_query}\n{timestamp}\n{body_hash}");
    let key = SigningKey::from_pkcs8_pem(private_key_pem)
        .map_err(|e| format!("cannot read the signing key: {e}"))?;
    let signature =
        base64::engine::general_purpose::STANDARD.encode(key.sign(canonical.as_bytes()).to_bytes());
    Ok(vec![
        (
            "Authorization".into(),
            format!("Bearer {}", rotating_token(seed, &utc_date(now))),
        ),
        ("X-Signature".into(), signature),
        ("X-Signature-Timestamp".into(), timestamp),
    ])
}

/// The routes the CI port serves (the `Shared` and `CiOnly` rows of `backend/vm-ops-api/core/src/domain/exposure.rs`, which a flow test compares
/// with this list); `{app}` is one path segment.
pub const CI_ROUTES: &[(&str, &str)] = &[
    ("GET", "/health"),
    ("GET", "/services/{app}/ps"),
    ("GET", "/stats"),
    ("GET", "/redeploy/status"),
    ("POST", "/services/{app}/redeploy"),
    ("POST", "/services/{app}/smoke"),
    ("POST", "/services/{app}/smoke/stop"),
    ("POST", "/redeploy"),
    ("POST", "/redeploy/smoke"),
    ("POST", "/redeploy/smoke/stop"),
    ("POST", "/android/device/start"),
    ("POST", "/android/device/stop"),
    ("POST", "/android/device/list"),
    ("GET", "/android/device/jobs/{job}"),
];

/// Which port and seed a call goes to: a CI job's own, or the admin's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    Admin,
    Ci,
}

impl Lane {
    pub fn port(self) -> u16 {
        match self {
            Lane::Ci => ci_port().unwrap_or(PORT),
            Lane::Admin => PORT,
        }
    }

    /// The port a smoke container of vm-ops-api answers this lane on.
    pub fn smoke_port(self) -> u16 {
        match self {
            Lane::Ci => CI_SMOKE_PORT,
            Lane::Admin => crate::services::SMOKE_PORT,
        }
    }
}

/// The CI port, when this process is a CI job: `WF_VM_CI_PORT` names it. Unset, empty or no port number, the admin port serves.
pub fn ci_port() -> Option<u16> {
    std::env::var("WF_VM_CI_PORT").ok().and_then(|text| text.trim().parse().ok())
}

pub fn is_ci_route(method: &str, path_with_query: &str) -> bool {
    let path = path_with_query.split('?').next().unwrap_or_default();
    CI_ROUTES.iter().any(|(m, pattern)| {
        let (mut want, mut got) = (pattern.split('/'), path.split('/'));
        *m == method && loop {
            match (want.next(), got.next()) {
                (None, None) => break true,
                (Some(w), Some(g)) if (w.starts_with('{') && !g.is_empty()) || w == g => {}
                _ => break false,
            }
        }
    })
}

/// The lane of a call: the CI port for a route it serves when this process is a CI job, the admin port for everything else (the
/// sandbox, agent-shell, and every route only the admin port serves).
pub fn lane_for(method: &str, path_with_query: &str) -> Lane {
    if ci_port().is_some() && is_ci_route(method, path_with_query) {
        Lane::Ci
    } else {
        Lane::Admin
    }
}

/// Read once per process. A deploy polls every 10 s for minutes, and resolving per request put the vault on that path: one
/// Bitwarden 503 stalled a 24-minute deploy for 13 of them.
static CREDENTIALS: std::sync::OnceLock<(String, String)> = std::sync::OnceLock::new();
static CI_CREDENTIALS: std::sync::OnceLock<(String, String)> = std::sync::OnceLock::new();

/// The value in `cell`, else what `load` returns; a failure is not kept, so the next call tries again.
fn cached<T: Clone>(cell: &std::sync::OnceLock<T>, load: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    if let Some(found) = cell.get() {
        return Ok(found.clone());
    }
    let loaded = load()?;
    Ok(cell.get_or_init(|| loaded).clone())
}

/// The credential pair vm-ops-api requires: `VM_OPS_API_AUTH_TOKEN` (not the gateway seed) and the signing key.
pub fn resolve_credentials() -> Result<(String, String), String> {
    cached(&CREDENTIALS, load_credentials)
}

/// The CI port's pair: its own seed, `VM_OPS_API_CI_AUTH_TOKEN`, and the same signing key.
pub fn resolve_ci_credentials() -> Result<(String, String), String> {
    cached(&CI_CREDENTIALS, || load_credentials_of("VM_OPS_API_CI_AUTH_TOKEN"))
}

fn load_credentials() -> Result<(String, String), String> {
    load_credentials_of("VM_OPS_API_AUTH_TOKEN")
}

fn load_credentials_of(seed_key: &str) -> Result<(String, String), String> {
    let loaded = secrets::load_values(&[seed_key, "VM_GATEWAY_REQUEST_SIGNING_PRIVATE_KEY"]);
    let seed = loaded[seed_key].clone().ok_or_else(|| format!("{seed_key} could not be resolved. It is a vault secret (`wf secret get {seed_key}`)."))?;
    let raw_key = loaded["VM_GATEWAY_REQUEST_SIGNING_PRIVATE_KEY"].clone().ok_or(
        "VM_GATEWAY_REQUEST_SIGNING_PRIVATE_KEY could not be resolved. vm-ops-api rejects an unsigned request even with a valid token (`wf secret get VM_GATEWAY_REQUEST_SIGNING_PRIVATE_KEY`).",
    )?;
    // The vault stores the PEM on one line with literal backslash-n sequences, which OpenSSL rejects.
    Ok((seed, raw_key.replace("\\n", "\n")))
}

pub struct Opts {
    pub post: bool,
    pub body: Option<String>,
    pub timeout_sec: u32,
    pub ok_statuses: &'static [i64],
}

impl Opts {
    pub fn get() -> Opts {
        Opts {
            post: false,
            body: None,
            timeout_sec: 10,
            ok_statuses: &[200],
        }
    }
    pub fn post() -> Opts {
        Opts {
            post: true,
            ..Opts::get()
        }
    }
}

/// How a request reaches the tailnet: through the userspace SOCKS5 listener (`WF_TAILNET_SOCKS`, default `localhost:1055`), or by the
/// environment's own routing when that is set empty.
pub fn tailnet_via() -> Via {
    let address = std::env::var("WF_TAILNET_SOCKS").unwrap_or_else(|_| DEFAULT_SOCKS.into());
    if address.is_empty() {
        Via::Environment
    } else {
        Via::Socks5(address)
    }
}

/// A script that failed answers with its whole output: an agent-shell mkdir as `output`, an app redeploy or a retire as
/// its sibling's `stdout` and `stderr` beside its `exitCode`; show it in full. Any other body, such
/// as `/redeploy/status`'s `{log, stderr, history}`, is shown verbatim.
fn describe_error_body(body: &str) -> String {
    let Ok(Value::Object(m)) = serde_json::from_str::<Value>(body) else {
        return body.to_string();
    };
    let text = |key: &str| m.get(key).and_then(Value::as_str);
    let sibling = m.contains_key("exitCode") && (text("stdout").is_some() || text("stderr").is_some());
    let sections: Vec<(&str, &str)> = match (text("output"), sibling) {
        (Some(output), _) => vec![("script output", output)],
        (None, false) => return body.to_string(),
        (None, true) => [("script stdout", text("stdout")), ("script stderr", text("stderr"))]
            .into_iter()
            .filter_map(|(heading, t)| t.filter(|t| !t.is_empty()).map(|t| (heading, t)))
            .collect(),
    };
    let code = m.get("exitCode").map_or("?".to_string(), |v| match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    });
    let error = text("error").map_or(String::new(), |e| format!(" error={e}"));
    let mut described = format!("exitCode={code}{error}");
    for (heading, t) in sections {
        described.push_str(&format!("\n--- {heading} ---\n{}", t.trim_end_matches('\n')));
    }
    described
}

/// An answer as it came: any HTTP status is one, a transport failure is the `Err` of `send`.
pub struct Reply {
    pub status: i64,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// One request to `vm-ops-api-main:<port>`, signed with the admin seed.
pub fn send(port: u16, method: &str, path: &str, body: Option<&str>, timeout: Duration) -> Result<Reply, String> {
    send_on(Lane::Admin, port, method, path, body, timeout)
}

/// One request to the port the route's lane names, signed with that lane's seed.
pub fn send_routed(method: &str, path: &str, body: Option<&str>, timeout: Duration) -> Result<Reply, String> {
    let lane = lane_for(method, path);
    send_on(lane, lane.port(), method, path, body, timeout)
}

/// One signed request to `vm-ops-api-main:<port>`, with the seed of `lane`.
pub fn send_on(lane: Lane, port: u16, method: &str, path: &str, body: Option<&str>, timeout: Duration) -> Result<Reply, String> {
    let (seed, pem) = match lane {
        Lane::Admin => resolve_credentials()?,
        Lane::Ci => resolve_ci_credentials()?,
    };
    let mut request = Request::new(method, &format!("http://{HOST}:{port}{path}")).via(tailnet_via()).timeout(timeout);
    for (name, value) in sign_request(&seed, &pem, method, path, body, now_ms())? {
        request = request.header(&name, &value);
    }
    if let Some(body) = body {
        request = request.body(body.as_bytes().to_vec());
    }
    let response = request.send().map_err(|e| format!("vm-ops-api request to {path} failed: {e}"))?;
    Ok(Reply { status: i64::from(response.status), body: response.body })
}

/// One signed request; returns the response body verbatim.
pub fn request_raw(path: &str, opts: &Opts) -> Result<String, String> {
    let method = if opts.post { "POST" } else { "GET" };
    let reply = send_routed(method, path, opts.body.as_deref(), Duration::from_secs(u64::from(opts.timeout_sec)))?;
    let status = reply.status;
    let body = reply.text();
    if !opts.ok_statuses.contains(&status) {
        return Err(format!("vm-ops-api {path} returned HTTP {status}: {}", describe_error_body(&body)));
    }
    Ok(body)
}

pub fn request_json(path: &str, opts: &Opts) -> Result<Value, String> {
    let body = request_raw(path, opts)?;
    serde_json::from_str(&body).map_err(|_| {
        let head: String = body.chars().take(500).collect();
        format!("vm-ops-api {path} returned non-JSON body: {head}")
    })
}

pub use cli_http::{encode_uri_component, form_encode as encode_form};
