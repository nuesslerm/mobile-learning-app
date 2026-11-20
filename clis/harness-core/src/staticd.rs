//! The HTTP listener the CLIs serve their own pages with: one thread per connection, the reply decided by the caller from
//! the request target, always with its `Content-Length` and the connection closed after it. (tiny_http answered a large file
//! with chunked encoding on a kept-alive socket, and Chrome then held its six sockets to the server busy for a minute while
//! the page's last scripts waited, so a page sometimes never loaded.)
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub struct Reply {
    status: u16,
    content_type: String,
    body: Vec<u8>,
}

/// The content type a file's extension says, with the charset on text.
pub fn content_type(path: &Path) -> String {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let text = mime.type_() == "text" || mime.subtype() == "json" || mime.suffix().is_some_and(|s| s == "xml");
    if text {
        format!("{mime}; charset=utf-8")
    } else {
        mime.to_string()
    }
}

impl Reply {
    /// 200 with the content type the file's extension says.
    pub fn file(path: &Path, body: Vec<u8>) -> Self {
        Self { status: 200, content_type: content_type(path), body }
    }

    pub fn text(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self { status, content_type: "text/plain".into(), body: body.into() }
    }
}

pub struct Listener {
    host: String,
    port: u16,
    stopped: Arc<AtomicBool>,
}

impl Listener {
    /// Binds `host:port` (0 for any free port) and serves every request with `handler(target)` until `stop`.
    pub fn bind(host: &str, port: u16, handler: impl Fn(&str) -> Reply + Send + Sync + 'static) -> Result<Self, String> {
        let listener = TcpListener::bind((host, port)).map_err(|e| format!("cannot listen on {host}:{port}: {e}"))?;
        let port = listener.local_addr().map_err(|e| format!("not an IP listener: {e}"))?.port();
        let handler = Arc::new(handler);
        let stopped = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stopped);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if flag.load(Ordering::Acquire) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let handler = Arc::clone(&handler);
                std::thread::spawn(move || answer(stream, &*handler));
            }
        });
        Ok(Self { host: host.to_string(), port, stopped })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        // the accept loop is blocked in accept: one connection to ourselves wakes it to see the flag
        let _ = TcpStream::connect((self.host.as_str(), self.port));
    }
}

const MAX_HEAD: usize = 64 * 1024;

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Status",
    }
}

/// Reads one request's head, answers it and closes the connection.
fn answer(mut stream: TcpStream, handler: &dyn Fn(&str) -> Reply) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut head = Vec::new();
    let mut chunk = [0u8; 4096];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => head.extend_from_slice(&chunk[..n]),
        }
        if head.len() > MAX_HEAD {
            return;
        }
    }
    let text = String::from_utf8_lossy(&head);
    let mut line = text.lines().next().unwrap_or("").split_whitespace();
    let (method, target) = (line.next().unwrap_or(""), line.next().unwrap_or("/"));
    let reply = handler(target);
    let mut out = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        reply.status,
        reason(reply.status),
        reply.content_type,
        reply.body.len()
    )
    .into_bytes();
    if method != "HEAD" {
        out.extend_from_slice(&reply.body);
    }
    let _ = stream.write_all(&out);
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Write);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(port: u16, request: &str) -> Vec<u8> {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut reply = Vec::new();
        stream.read_to_end(&mut reply).unwrap();
        reply
    }

    #[test]
    fn a_large_file_is_answered_whole_with_its_length_and_the_connection_closed() {
        // Given a listener that answers every target with a 300 KB script
        let body = vec![b'x'; 300 * 1024];
        let served = body.clone();
        let listener = Listener::bind("127.0.0.1", 0, move |_| Reply::file(Path::new("a.js"), served.clone())).unwrap();

        // When a client asks for it and reads until the server closes
        let reply = get(listener.port(), "GET /a.js HTTP/1.1\r\nHost: x\r\n\r\n");
        listener.stop();

        // Then the head names the length and the close, the body follows whole, and nothing is chunked
        let split = reply.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        let head = String::from_utf8_lossy(&reply[..split]).to_lowercase();
        assert!(head.starts_with("http/1.1 200 ok"), "{head}");
        assert!(head.contains(&format!("content-length: {}", body.len())), "{head}");
        assert!(head.contains("connection: close") && !head.contains("transfer-encoding"), "{head}");
        assert_eq!(&reply[split..], &body[..]);
    }

    #[test]
    fn a_head_request_gets_the_length_and_no_body() {
        // Given a listener answering with a known body
        let listener = Listener::bind("127.0.0.1", 0, |_| Reply::text(404, "gone")).unwrap();

        // When a client sends HEAD
        let reply = get(listener.port(), "HEAD /x HTTP/1.1\r\nHost: x\r\n\r\n");
        listener.stop();

        // Then the status and length are there and the body is not
        let text = String::from_utf8_lossy(&reply).to_lowercase();
        assert!(text.starts_with("http/1.1 404 not found") && text.contains("content-length: 4"), "{text}");
        assert!(!text.contains("gone"), "{text}");
    }
}
