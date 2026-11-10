//! The HTTP head of a connection: `/health`, or the WebSocket upgrade the phone's transport opens.
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;

const MAX_HEAD: usize = 16 * 1024;

pub struct Head {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
}

impl Head {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

pub fn read_head(stream: &mut TcpStream) -> Option<Head> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    let end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_HEAD {
            return None;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, target) = (first.next()?.to_string(), first.next().unwrap_or("/"));
    let path = target.split('?').next().unwrap_or("/").to_string();
    let headers = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    Some(Head { method, path, headers })
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Internal Server Error",
    }
}

pub fn respond(stream: &mut TcpStream, status: u16, body: &Value) {
    let text = body.to_string();
    let head = format!("HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", reason(status), text.len());
    let _ = stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(text.as_bytes())).and_then(|()| stream.flush());
}

pub fn health() -> Value {
    json!({"ok": true, "service": "orca-serve", "version": env!("CARGO_PKG_VERSION"), "commit": std::env::var("GIT_COMMIT").ok().filter(|c| !c.is_empty()).unwrap_or_else(|| "unknown".into())})
}
