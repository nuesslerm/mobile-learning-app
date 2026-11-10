//! The HTTP listener the CLIs serve their own pages with: `tiny_http`, one thread per request, the reply decided by the caller
//! from the request target.
use std::path::Path;
use std::sync::Arc;
use tiny_http::{Header, Response, Server};

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
    server: Arc<Server>,
    port: u16,
}

impl Listener {
    /// Binds `host:port` (0 for any free port) and serves every request with `handler(target)` until `stop`.
    pub fn bind(host: &str, port: u16, handler: impl Fn(&str) -> Reply + Send + Sync + 'static) -> Result<Self, String> {
        let server = Arc::new(Server::http((host, port)).map_err(|e| format!("cannot listen on {host}:{port}: {e}"))?);
        let port = server.server_addr().to_ip().ok_or("not an IP listener")?.port();
        let handler = Arc::new(handler);
        let accepting = Arc::clone(&server);
        std::thread::spawn(move || {
            for request in accepting.incoming_requests() {
                let handler = Arc::clone(&handler);
                std::thread::spawn(move || {
                    let reply = handler(request.url());
                    let header = |name: &str, value: &str| Header::from_bytes(name, value).expect("static header");
                    let _ = request.respond(
                        Response::from_data(reply.body)
                            .with_status_code(reply.status)
                            .with_header(header("Content-Type", &reply.content_type))
                            .with_header(header("Cache-Control", "no-store")),
                    );
                });
            }
        });
        Ok(Self { server, port })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn stop(&self) {
        self.server.unblock();
    }
}
