//! `wf t3 <verb>`: parse the line, wire the adapters, run the use case, print one JSON object on stdout. A failure prints
//! `{ok: false, error, next}` and exits 1; a malformed line exits 2. No verb prompts. `serve` prints one line when it listens
//! and then runs until it is stopped; `pair` prints the one-time code, the only place a secret is ever written out.
use crate::adapters::{OsIds, SystemClock};
use crate::wiring::{self, Settings, DEFAULT_BIND, DEFAULT_PORT};
use serde_json::{json, Value};
use std::path::PathBuf;
use t3_contracts::time::iso;
use t3_core::usecases::pairing::{self, PairingRequest};
use wf_kit::args::{parse_spec, Parsed, Positional, Spec};

const SERVE: Spec = Spec { values: &["state-dir", "bind", "port", "label", "catalog", "rca-url", "rca-workspace"], flags: &["healthcheck"], positional: Positional::None, ..Spec::NONE };
const PAIR: Spec = Spec { values: &["state-dir", "base-url", "ttl", "label", "scope"], positional: Positional::None, ..Spec::NONE };

pub fn run(args: &[String]) -> Option<i32> {
    let (verb, rest) = args.split_first()?;
    match verb.as_str() {
        "serve" => serve_verb(&parse_spec(rest, &SERVE)?),
        "pair" => pair_verb(&parse_spec(rest, &PAIR)?),
        _ => None,
    }
}

fn failure(verb: &str, error: impl Into<String>, next: &str) -> Option<i32> {
    let error = error.into();
    wf_kit::logfmt::mcp("error", "t3", &format!("wf t3 {verb} failed"), &[("error", error.clone())]);
    println!("{}", json!({"ok": false, "error": error, "next": next}));
    Some(1)
}

fn usage(message: String) -> Option<i32> {
    eprintln!("wf t3: {message}");
    Some(2)
}

/// A flag, else the variable, else nothing.
fn setting(p: &Parsed, flag: &str, env: &str) -> Option<String> {
    p.val(flag).map(str::to_string).or_else(|| std::env::var(env).ok().filter(|v| !v.is_empty()))
}

fn state_dir(p: &Parsed) -> PathBuf {
    setting(p, "state-dir", "T3_STATE_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".local/share/t3-compat"))
}

fn serve_verb(p: &Parsed) -> Option<i32> {
    let port = match setting(p, "port", "T3_PORT").map(|v| v.parse::<u16>()) {
        None => DEFAULT_PORT,
        Some(Ok(port)) => port,
        Some(Err(_)) => return usage("--port takes a port number (0 picks a free one)".into()),
    };
    let bind = setting(p, "bind", "T3_BIND").unwrap_or_else(|| DEFAULT_BIND.into());
    if p.flag("healthcheck") {
        return Some(healthcheck(&bind, port));
    }
    let settings = Settings {
        state_dir: state_dir(p),
        label: setting(p, "label", "T3_LABEL").unwrap_or_else(|| "rca".into()),
        catalog_file: setting(p, "catalog", "T3_CATALOG_FILE").map(PathBuf::from),
        catalog_inline: std::env::var("T3_MODELS").ok().filter(|v| !v.is_empty()),
        rca_url: setting(p, "rca-url", "T3_RCA_URL"),
        rca_bearer: std::env::var("T3_RCA_BEARER").ok().filter(|v| !v.is_empty()),
        rca_workspace: setting(p, "rca-workspace", "T3_RCA_WORKSPACE"),
    };
    let started = wiring::open_store(&settings.state_dir).and_then(|store| wiring::service(&settings, store)).and_then(|wired| {
        let running = t3_http::serve((bind.as_str(), port), wired.service).map_err(|e| format!("cannot listen on {bind}:{port}: {e}"))?;
        Ok((running, wired.environment_id))
    });
    match started {
        Err(error) => failure("serve", error, "free the port or pick another with --port, and check --state-dir is writable"),
        Ok((running, environment_id)) => {
            let addr = running.addr();
            println!("{}", json!({"ok": true, "event": "listening", "address": addr.to_string(), "port": addr.port(), "environmentId": environment_id, "stateDir": settings.state_dir.display().to_string(), "orchestrationProtocolVersion": 2}));
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
        eprintln!("wf t3: no healthy server answered at {host}:{port}");
        1
    }
}

fn pair_verb(p: &Parsed) -> Option<i32> {
    let ttl_seconds = match p.val("ttl").map(str::parse::<i64>) {
        None => t3_contracts::wire::PAIRING_TTL_SECONDS,
        Some(Ok(ttl)) if ttl > 0 => ttl,
        Some(_) => return usage("--ttl takes a positive number of seconds".into()),
    };
    let mut request = PairingRequest { ttl_seconds, ..PairingRequest::default() };
    if let Some(label) = p.val("label") {
        request.label = label.to_string();
    }
    let scopes: Vec<String> = p.values("scope").iter().flat_map(|v| v.split(',')).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    if !scopes.is_empty() {
        request.scopes = scopes;
    }
    let base = setting(p, "base-url", "T3_BASE_URL").unwrap_or_else(|| format!("http://{DEFAULT_BIND}:{DEFAULT_PORT}")).trim_end_matches('/').to_string();
    let dir = state_dir(p);
    let outcome = wiring::open_store(&dir).and_then(|store| {
        let environment_id = wiring::environment_id(store.as_ref(), &OsIds)?;
        pairing::mint(store.as_ref(), &SystemClock, &OsIds, request).map(|pairing| (pairing, environment_id))
    });
    match outcome {
        Err(error) => failure("pair", error, "pass scopes from the 17 (see wf t3 pair --help) and a writable --state-dir"),
        Ok((pairing, environment_id)) => {
            let url: Value = format!("{base}/pair#token={}", pairing.code).into();
            println!(
                "{}",
                json!({"ok": true, "url": url, "code": pairing.code, "baseUrl": base, "expiresAt": iso(pairing.expires_at), "ttlSeconds": ttl_seconds, "scopes": pairing.scopes, "label": pairing.label, "environmentId": environment_id})
            );
            Some(0)
        }
    }
}
