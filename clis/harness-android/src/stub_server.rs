//! The Android harness's network stubs: a small HTTP server in the run's own process that answers the requests a script
//! stubbed and forwards every other request to the host its `Host` header names. The device reaches it through an
//! iptables DNAT rule on the app's backend ports (`device/android.rs`); the app's backends speak plain HTTP on the
//! tailnet, so the server sees each request as sent. Matching is the browser harness's (`cdp::attached::matching_stub`).
use cdp::attached::{matching_stub, Stub};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct StubServer {
    pub port: u16,
    stubs: Arc<Mutex<Vec<Stub>>>,
    hits: Arc<Mutex<Vec<Value>>>,
}

/// A stub as `add_network_stub` takes it (`commands::stub_spec`): `json_body`, `body` or `body_file`, `status`, `headers`, `delay`.
pub fn stub_from_spec(spec: &Value) -> Result<Stub, String> {
    let pattern = spec["pattern"].as_str().ok_or("stub needs a pattern")?.replace("**", "*");
    let method = spec["method"].as_str().map(str::to_ascii_uppercase).filter(|m| m != "*" && m != "ANY");
    let mut headers: Vec<(String, String)> = spec["headers"].as_object().map(|h| h.iter().map(|(k, v)| (k.clone(), v.as_str().map_or_else(|| v.to_string(), str::to_string))).collect()).unwrap_or_default();
    let body = if !spec["json_body"].is_null() {
        if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type")) {
            headers.push(("Content-Type".into(), "application/json".into()));
        }
        spec["json_body"].to_string().into_bytes()
    } else if let Some(f) = spec["body_file"].as_str() {
        std::fs::read(f).map_err(|e| format!("cannot read the stub body {f}: {e}"))?
    } else {
        spec["body"].as_str().unwrap_or_default().as_bytes().to_vec()
    };
    Ok(Stub { method, pattern, status: spec["status"].as_i64().unwrap_or(200), headers, body, delay_ms: spec["delay"].as_u64().unwrap_or(0) })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn reason(status: i64) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

struct Request {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    fn url(&self) -> String {
        if self.target.starts_with("http://") {
            self.target.clone()
        } else {
            format!("http://{}{}", self.header("host").unwrap_or("localhost"), self.target)
        }
    }
}

fn read_request(r: &mut BufReader<TcpStream>) -> Result<Request, String> {
    let mut line = String::new();
    r.read_line(&mut line).map_err(|e| e.to_string())?;
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else { return Err(format!("not an HTTP request: {}", line.trim())) };
    let (method, target) = (method.to_string(), target.to_string());
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).map_err(|e| e.to_string())? == 0 || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.trim_end().split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let mut req = Request { method, target, headers, body: Vec::new() };
    let len: usize = req.header("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    req.body.resize(len, 0);
    r.read_exact(&mut req.body).map_err(|e| e.to_string())?;
    Ok(req)
}

fn answer(out: &mut TcpStream, stub: &Stub) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n", stub.status, reason(stub.status), stub.body.len());
    for (k, v) in stub.headers.iter().filter(|(k, _)| !k.eq_ignore_ascii_case("content-length") && !k.eq_ignore_ascii_case("connection")) {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    out.write_all(head.as_bytes())?;
    out.write_all(&stub.body)
}

/// Sends the request on to the host it names, as one request on a fresh connection, and copies the answer back.
fn forward(out: &mut TcpStream, req: &Request) -> Result<(), String> {
    let host = req.header("host").ok_or("no Host header to forward to")?;
    let addr = if host.contains(':') { host.to_string() } else { format!("{host}:80") };
    let mut up = TcpStream::connect(&addr).map_err(|e| format!("cannot reach {addr}: {e}"))?;
    let path = req.target.strip_prefix("http://").and_then(|t| t.find('/').map(|i| &t[i..])).unwrap_or(&req.target);
    let mut head = format!("{} {} HTTP/1.1\r\n", req.method, path);
    for (k, v) in req.headers.iter().filter(|(k, _)| !k.eq_ignore_ascii_case("connection") && !k.eq_ignore_ascii_case("keep-alive")) {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("Connection: close\r\n\r\n");
    up.write_all(head.as_bytes()).and_then(|_| up.write_all(&req.body)).map_err(|e| e.to_string())?;
    std::io::copy(&mut up, out).map(|_| ()).map_err(|e| e.to_string())
}

fn serve(conn: TcpStream, stubs: &Mutex<Vec<Stub>>, hits: &Mutex<Vec<Value>>) {
    let _ = conn.set_read_timeout(Some(Duration::from_secs(60)));
    let Ok(mut out) = conn.try_clone() else { return };
    let mut r = BufReader::new(conn);
    let Ok(req) = read_request(&mut r) else { return };
    let url = req.url();
    let stub = stubs.lock().ok().and_then(|s| matching_stub(&s, &req.method, &url));
    match stub {
        Some(stub) => {
            if stub.delay_ms > 0 {
                std::thread::sleep(Duration::from_millis(stub.delay_ms));
            }
            let _ = answer(&mut out, &stub);
            if let Ok(mut h) = hits.lock() {
                h.push(json!({ "method": req.method, "url": url, "status": stub.status, "pattern": stub.pattern, "at": now_ms() }));
            }
        }
        None => {
            if let Err(e) = forward(&mut out, &req) {
                let body = format!("wf stub server: {e}");
                let _ = answer(&mut out, &Stub { method: None, pattern: String::new(), status: 502, headers: vec![], body: body.into_bytes(), delay_ms: 0 });
            }
        }
    }
}

impl StubServer {
    /// Listens on `bind` (port 0 picks one) and serves each connection on its own thread for the life of the process.
    pub fn start(bind: &str) -> Result<StubServer, String> {
        let listener = TcpListener::bind(bind).map_err(|e| format!("cannot listen on {bind}: {e}"))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let (stubs, hits) = (Arc::new(Mutex::new(Vec::new())), Arc::new(Mutex::new(Vec::new())));
        let (s, h) = (stubs.clone(), hits.clone());
        std::thread::spawn(move || {
            for conn in listener.incoming().flatten() {
                let (s, h) = (s.clone(), h.clone());
                std::thread::spawn(move || serve(conn, &s, &h));
            }
        });
        Ok(StubServer { port, stubs, hits })
    }

    pub fn add(&self, stub: Stub) {
        if let Ok(mut s) = self.stubs.lock() {
            s.push(stub);
        }
    }

    pub fn hits(&self) -> Vec<Value> {
        self.hits.lock().map(|h| h.clone()).unwrap_or_default()
    }
}
