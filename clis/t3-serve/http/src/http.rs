//! Request parsing, routing and responses.
use crate::socket;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use t3_contracts::environment::ORCHESTRATION_PROTOCOL_HEADER;
use t3_contracts::wire;
use t3_core::usecases::attachments::{ASSET_PREFIX, MAX_UPLOAD_BYTES, UPLOAD_PREFIX};
use t3_core::usecases::history::EnvironmentRequest;
use t3_core::{Refusal, Service};

const MAX_HEAD: usize = 16 * 1024;
const MAX_BODY: usize = 64 * 1024;
/// The upload route takes the attachment's bytes: as much as the agent API keeps, with longer to read them in.
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(120);
/// `GET /api/orchestration/threads/:threadId/history` and `.../transcript`.
const THREADS: &str = "/api/orchestration/threads/";

pub struct Request {
    pub method: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if bytes.get(i + 1..i + 3).is_some_and(|h| h.iter().all(u8::is_ascii_hexdigit)) => {
                out.push(u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("00"), 16).unwrap_or(0));
                i += 2;
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn pairs(text: &str) -> Vec<(String, String)> {
    text.split('&').filter(|p| !p.is_empty()).map(|p| p.split_once('=').map_or((percent_decode(p), String::new()), |(k, v)| (percent_decode(k), percent_decode(v)))).collect()
}

fn read_request(stream: &mut TcpStream) -> Result<Request, &'static str> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_HEAD {
            return Err("request head too large");
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return Err("connection closed"),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let (method, target) = (first.next().unwrap_or("").to_string(), first.next().unwrap_or("/"));
    let (path, query) = target.split_once('?').map_or((target, ""), |(p, q)| (p, q));
    let headers: Vec<(String, String)> = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    let length: usize = headers.iter().find(|(k, _)| k == "content-length").and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
    let upload = method == "POST" && path.starts_with(&format!("{UPLOAD_PREFIX}/"));
    if length > if upload { MAX_UPLOAD_BYTES as usize } else { MAX_BODY } {
        return Err("request body too large");
    }
    if upload {
        let _ = stream.set_read_timeout(Some(UPLOAD_TIMEOUT));
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return Err("connection closed"),
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    body.truncate(length);
    Ok(Request { method, path: path.to_string(), query: pairs(query), headers, body })
}

const CORS_PREFLIGHT: &str = "Access-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: authorization, content-type, b3, traceparent, x-t3-orchestration-protocol\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\n";
/// The headers the recorded server sends on every answer.
const COMMON: &str = "Access-Control-Allow-Origin: *\r\nVary: Accept-Encoding\r\n";
const NO_STORE: &str = "Cache-Control: no-store\r\nPragma: no-cache\r\n";

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        409 => "Conflict",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        426 => "Upgrade Required",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    }
}

pub fn respond(stream: &mut TcpStream, status: u16, body: &Value) {
    respond_with(stream, status, body, "")
}

/// A plain text answer, as the pin's attachment and asset routes give.
fn respond_text(stream: &mut TcpStream, status: u16, text: &str) {
    let head = format!("HTTP/1.1 {status} {}\r\n{COMMON}Content-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", reason(status), text.len());
    let _ = stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(text.as_bytes())).and_then(|()| stream.flush());
}

fn respond_empty(stream: &mut TcpStream, status: u16) {
    let head = format!("HTTP/1.1 {status} {}\r\n{COMMON}Content-Length: 0\r\nConnection: close\r\n\r\n", reason(status));
    let _ = stream.write_all(head.as_bytes()).and_then(|()| stream.flush());
}

/// `GET /api/assets/<token>/<name>`: the headers from what the attachment is, then its bytes chunk by chunk as the agent API gives them.
fn send_asset(stream: &mut TcpStream, svc: &Service, token: &str) {
    let asset = match svc.asset_open(token) {
        Ok(asset) => asset,
        Err(failure) => return respond_text(stream, failure.status, &failure.text),
    };
    let name: String = asset.name.chars().map(|c| if (c.is_ascii_graphic() && !matches!(c, '"' | '\\' | '/')) || c == ' ' { c } else { '_' }).collect();
    let disposition = if asset.inline { "inline" } else { "attachment" };
    let head = format!(
        "HTTP/1.1 200 OK\r\n{COMMON}Content-Type: {}\r\nContent-Length: {}\r\nCache-Control: private, max-age=3600\r\nX-Content-Type-Options: nosniff\r\nContent-Disposition: {disposition}; filename=\"{name}\"\r\nConnection: close\r\n\r\n",
        asset.mime_type.replace(['\r', '\n'], ""),
        asset.size
    );
    if stream.write_all(head.as_bytes()).is_err() || stream.write_all(&asset.first).is_err() {
        return;
    }
    let (mut offset, mut done) = (asset.first.len() as u64, asset.eof);
    while !done {
        // A failure part way ends the connection short of its Content-Length, which a client reads as a failed download
        let Ok((bytes, eof)) = svc.asset_read(&asset.id, offset) else { return };
        if bytes.is_empty() || stream.write_all(&bytes).is_err() {
            return;
        }
        offset += bytes.len() as u64;
        done = eof;
    }
    let _ = stream.flush();
}

fn respond_with(stream: &mut TcpStream, status: u16, body: &Value, extra: &str) {
    let text = if status == 204 { String::new() } else { body.to_string() };
    let cors = if status == 204 { CORS_PREFLIGHT } else { COMMON };
    let head = format!("HTTP/1.1 {status} {}\r\n{cors}{extra}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", reason(status), text.len());
    let _ = stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(text.as_bytes())).and_then(|()| stream.flush());
}

fn refused(stream: &mut TcpStream, r: Refusal, extra: &str) {
    respond_with(stream, r.status, &r.body, extra);
}

pub fn handle(mut stream: TcpStream, svc: &Service, stop: &AtomicBool) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let request = match read_request(&mut stream) {
        Ok(r) => r,
        Err(m) => return respond(&mut stream, 400, &wire::bad_request(m)),
    };
    let auth = request.header("authorization");
    match (request.method.as_str(), request.path.as_str()) {
        ("OPTIONS", _) => respond(&mut stream, 204, &Value::Null),
        ("GET", "/health") => respond(&mut stream, 200, &json!({"ok": true, "service": "t3-serve", "version": env!("CARGO_PKG_VERSION"), "commit": std::env::var("GIT_COMMIT").ok().filter(|c| !c.is_empty()).unwrap_or_else(|| "unknown".into())})),
        ("GET", "/.well-known/t3/environment") => respond(&mut stream, 200, &svc.descriptor()),
        ("GET", "/api/auth/session") => respond(&mut stream, 200, &svc.session(auth)),
        ("POST", "/oauth/token") => {
            let form = if request.header("content-type").is_some_and(|t| t.contains("json")) {
                serde_json::from_slice::<Value>(&request.body).ok().and_then(|v| v.as_object().cloned()).map(|o| o.into_iter().filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string()))).collect()).unwrap_or_default()
            } else {
                pairs(&String::from_utf8_lossy(&request.body))
            };
            match svc.exchange(&form) {
                Ok(body) => respond_with(&mut stream, 200, &body, NO_STORE),
                Err(r) => refused(&mut stream, r, NO_STORE),
            }
        }
        ("POST", path) if path == "/api/auth/websocket-ticket" || path.starts_with("/api/auth/websocket-ticket/") => {
            let use_ = path.strip_prefix("/api/auth/websocket-ticket/").unwrap_or("rpc");
            match svc.ticket(auth, use_) {
                Ok(body) => respond_with(&mut stream, 200, &body, NO_STORE),
                Err(r) => refused(&mut stream, r, NO_STORE),
            }
        }
        ("POST", "/api/auth/pairing-token") => {
            let body = if request.body.is_empty() { Ok(json!({})) } else { serde_json::from_slice::<Value>(&request.body) };
            match body {
                Ok(body) if body.is_object() => match svc.pairing_credential(auth, &body) {
                    Ok(answer) => respond_with(&mut stream, 200, &answer, NO_STORE),
                    Err(r) => refused(&mut stream, r, NO_STORE),
                },
                _ => respond_with(&mut stream, 400, &wire::bad_request("the body is a JSON object"), NO_STORE),
            }
        }
        ("GET", "/ws") => match svc.connect(&request.query) {
            Ok(connection) => socket::upgrade(stream, &request, connection, stop),
            Err(r) => refused(&mut stream, r, ""),
        },
        ("GET", "/api/orchestration/shell") => {
            let env = EnvironmentRequest { authorization: auth, protocol: request.header(ORCHESTRATION_PROTOCOL_HEADER) };
            match svc.shell_snapshot_http(&env) {
                Ok(body) => respond(&mut stream, 200, &body),
                Err(r) => refused(&mut stream, r, ""),
            }
        }
        ("POST", "/api/pull-requests/diff") => {
            let env = EnvironmentRequest { authorization: auth, protocol: request.header(ORCHESTRATION_PROTOCOL_HEADER) };
            match svc.pull_request_diff_http(&env, &request.body) {
                Ok(body) => respond(&mut stream, 200, &body),
                Err(r) => refused(&mut stream, r, ""),
            }
        }
        ("GET", path) if path.starts_with(THREADS) => {
            let env = EnvironmentRequest { authorization: auth, protocol: request.header(ORCHESTRATION_PROTOCOL_HEADER) };
            let answer = match path[THREADS.len()..].split_once('/') {
                Some((thread, "history")) => svc.thread_history_page(&env, &percent_decode(thread), &request.query),
                Some((thread, "transcript")) => svc.thread_transcript(&env, &percent_decode(thread)),
                Some((thread, "bounded")) => svc.thread_bounded_snapshot(&env, &percent_decode(thread)),
                _ => return respond(&mut stream, 404, &json!({"code": "not_found", "message": "No such route."})),
            };
            match answer {
                Ok(body) => respond(&mut stream, 200, &body),
                Err(r) => refused(&mut stream, r, ""),
            }
        }
        ("POST", path) if path.starts_with(&format!("{UPLOAD_PREFIX}/")) => {
            let length = request.header("content-length").and_then(|v| v.parse::<u64>().ok());
            match svc.attachment_upload(&path[UPLOAD_PREFIX.len() + 1..], &request.body, length) {
                Ok(()) => respond_empty(&mut stream, 204),
                Err(failure) => respond_text(&mut stream, failure.status, &failure.text),
            }
        }
        ("GET", path) if path.starts_with(&format!("{ASSET_PREFIX}/")) => match path[ASSET_PREFIX.len() + 1..].split_once('/') {
            Some((token, _name)) if !token.is_empty() => send_asset(&mut stream, svc, token),
            _ => respond_text(&mut stream, 404, "Not Found"),
        },
        _ => respond(&mut stream, 404, &json!({"code": "not_found", "message": "No such route."})),
    }
}
