//! `orca-serve <verb>`: parse the line, wire the adapters, run the use case, print one JSON object on stdout. A failure prints
//! `{ok: false, error, next}` and exits 1; a malformed line exits 2. No verb prompts. `serve` prints one line when it listens and then runs
//! until it is stopped; `pair` prints the pairing link, the only place a device token is ever written out.
use crate::adapters::FileDevices;
use crate::transport;
use crate::wiring::{self, Settings, DEFAULT_BIND, DEFAULT_PORT};
use orca_crypto::pairing::PairingOffer;
use orca_crypto::ports::{load_or_create_keypair, mint_device, DeviceRegistry};
use orca_crypto::SystemEntropy;
use serde_json::{json, Value};
use std::path::PathBuf;
use wf_kit::args::{parse_spec, Parsed, Positional, Spec};

const SERVE: Spec = Spec { values: &["state-dir", "bind", "port", "label", "rca-url", "mobile-web-dir"], flags: &["healthcheck"], positional: Positional::None, ..Spec::NONE };
const PAIR: Spec = Spec { values: &["state-dir", "endpoint"], positional: Positional::None, ..Spec::NONE };
const API: Spec = Spec { positional: Positional::None, ..Spec::NONE };

fn failure(verb: &str, error: impl Into<String>, next: &str) -> Option<i32> {
    let error = error.into();
    wf_kit::logfmt::mcp("error", "orca-serve", &format!("orca-serve {verb} failed"), &[("error", error.clone())]);
    println!("{}", json!({"ok": false, "error": error, "next": next}));
    Some(1)
}

fn usage(message: String) -> Option<i32> {
    eprintln!("orca-serve: {message}");
    Some(2)
}

/// A flag, else the variable, else nothing.
fn setting(p: &Parsed, flag: &str, env: &str) -> Option<String> {
    p.val(flag).map(str::to_string).or_else(|| std::env::var(env).ok().filter(|v| !v.is_empty()))
}

fn state_dir(p: &Parsed) -> PathBuf {
    setting(p, "state-dir", "ORCA_STATE_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".local/share/orca-serve"))
}

pub fn serve(args: &[String]) -> Option<i32> {
    let p = parse_spec(args, &SERVE)?;
    let port = match setting(&p, "port", "ORCA_PORT").map(|v| v.parse::<u16>()) {
        None => DEFAULT_PORT,
        Some(Ok(port)) => port,
        Some(Err(_)) => return usage("--port takes a port number (0 picks a free one)".into()),
    };
    let bind = setting(&p, "bind", "ORCA_BIND").unwrap_or_else(|| DEFAULT_BIND.into());
    if p.flag("healthcheck") {
        return Some(healthcheck(&bind, port));
    }
    let settings = Settings {
        state_dir: state_dir(&p),
        label: setting(&p, "label", "ORCA_LABEL").unwrap_or_else(|| "rca".into()),
        rca_url: setting(&p, "rca-url", "ORCA_RCA_URL"),
        rca_bearer: std::env::var("ORCA_RCA_BEARER").ok().filter(|v| !v.is_empty()),
        web_dir: setting(&p, "mobile-web-dir", "ORCA_MOBILE_WEB_DIR").map(PathBuf::from),
    };
    let started = wiring::wire(&settings).and_then(|wired| {
        let runtime_id = wired.shared.host.runtime_id.clone();
        let running = transport::serve((bind.as_str(), port), wired.shared).map_err(|e| format!("cannot listen on {bind}:{port}: {e}"))?;
        Ok((running, runtime_id))
    });
    match started {
        Err(error) => failure("serve", error, "check --rca-url and ORCA_RCA_BEARER, free the port or pick another with --port, and check --state-dir is writable"),
        Ok((running, runtime_id)) => {
            let addr = running.addr();
            println!("{}", json!({"ok": true, "event": "listening", "address": addr.to_string(), "port": addr.port(), "runtimeId": runtime_id, "stateDir": settings.state_dir.display().to_string()}));
            running.wait();
            Some(0)
        }
    }
}

/// Asks the server of this configuration for `/health` and exits 0 when it answers 200: the container's healthcheck, which has no `curl`.
fn healthcheck(bind: &str, port: u16) -> i32 {
    use std::io::{Read, Write};
    let host = if bind == "0.0.0.0" { "127.0.0.1" } else { bind };
    let answered = std::net::TcpStream::connect((host, port)).ok().and_then(|mut stream| {
        stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).ok()?;
        stream.write_all(format!("GET /health HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes()).ok()?;
        let mut head = String::new();
        stream.read_to_string(&mut head).ok()?;
        head.starts_with("HTTP/1.1 200").then_some(())
    });
    if answered.is_some() {
        0
    } else {
        eprintln!("orca-serve: no healthy server answered at {host}:{port}");
        1
    }
}

/// Mints a device and prints the `orca://pair` link a phone scans. The device is known to a running server at once (its file is read per lookup).
pub fn pair(args: &[String]) -> Option<i32> {
    let p = parse_spec(args, &PAIR)?;
    let Some(endpoint) = setting(&p, "endpoint", "ORCA_ENDPOINT") else {
        return usage("--endpoint (ORCA_ENDPOINT) is required: the ws:// address the phone reaches this server at".into());
    };
    let dir = state_dir(&p);
    let entropy = SystemEntropy;
    let outcome = load_or_create_keypair(&crate::adapters::FileKeyPair(dir.join("host-key.json")), &entropy).map_err(|e| e.0).and_then(|keys| {
        let device = mint_device(&entropy);
        FileDevices::new(dir.join("devices.json")).register(device.clone()).map_err(|e| e.0)?;
        Ok((keys, device))
    });
    match outcome {
        Err(error) => failure("pair", error, "check --state-dir is writable"),
        Ok((keys, device)) => {
            let offer = PairingOffer::direct(&endpoint, &device, &keys.public);
            println!("{}", json!({"ok": true, "url": offer.to_uri(), "deviceId": device.id, "endpoint": endpoint}));
            Some(0)
        }
    }
}

/// The method table as JSON: every method with its state, from the areas the server is built with. It needs no rca and no state directory.
pub fn api(args: &[String]) -> Option<i32> {
    parse_spec(args, &API)?;
    let agent: std::sync::Arc<dyn orca_core::AgentApi> = std::sync::Arc::new(NoAgent);
    match orca_core::Host::new("api".into(), wiring::areas(orca_map_mobile::MobileWebArea::new(Box::new(orca_map_mobile::NoBundle))), agent, std::sync::Arc::new(NoPrincipals), wiring::host_info("api")) {
        Ok(host) => {
            println!("{}", host.api());
            Some(0)
        }
        Err(error) => failure("api", error, "two areas claim one method: fix the area crates"),
    }
}

/// The `api` verb answers for no running service.
struct NoAgent;

impl orca_core::AgentApi for NoAgent {
    fn status(&self) -> Result<orca_core::ServiceStatus, orca_core::AgentError> {
        Err(orca_core::AgentError::Failed("no rca".into()))
    }

    fn state_get(&self, _scope: &str, _key: &str) -> Result<Option<Value>, orca_core::AgentError> {
        Err(orca_core::AgentError::Failed("no rca".into()))
    }

    fn request(&self, _method: &'static str, _params: Value) -> Result<Value, orca_core::AgentError> {
        Err(orca_core::AgentError::Failed("no rca".into()))
    }
}

struct NoPrincipals;

impl orca_core::Principals for NoPrincipals {
    fn still_paired(&self, _device_id: &str, _token: &str) -> bool {
        false
    }
}
