//! The two servers' shared plumbing over `tiny_http`: a thread per request (the viewer long-polls and the control channel
//! waits on Chromium) and the few helpers both handlers use.
use std::io::Read;
use std::net::TcpListener;
use std::sync::Arc;
use tiny_http::{Header, Request, Response, Server};

const DRAIN_LIMIT: u64 = 16 * 1024 * 1024;

pub struct Reply {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn new(status: u16, content_type: &str, body: impl Into<Vec<u8>>) -> Reply {
        Reply { status, headers: vec![("Content-Type", content_type.to_string())], body: body.into() }
    }

    pub fn empty(status: u16) -> Reply {
        Reply { status, headers: Vec::new(), body: Vec::new() }
    }

    pub fn header(mut self, name: &'static str, value: impl Into<String>) -> Reply {
        self.headers.push((name, value.into()));
        self
    }
}

pub fn header<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str())
}

pub fn headers<'a>(request: &'a Request, name: &'a str) -> impl Iterator<Item = &'a str> {
    request.headers().iter().filter(move |h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str())
}

/// The body, or `None` when it is larger than `max` bytes.
pub fn read_body(request: &mut Request, max: usize) -> Option<Vec<u8>> {
    if request.body_length().is_some_and(|n| n > max) {
        // A client still sending would never read the refusal; swallow a bounded amount of it first.
        let _ = std::io::copy(&mut request.as_reader().take(DRAIN_LIMIT), &mut std::io::sink());
        return None;
    }
    let mut body = Vec::new();
    request.as_reader().take(max as u64 + 1).read_to_end(&mut body).ok()?;
    (body.len() <= max).then_some(body)
}

pub fn split_url(url: &str) -> (&str, Option<&str>) {
    match url.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (url, None),
    }
}

pub fn respond(request: Request, reply: Reply) {
    let mut response = Response::from_data(reply.body).with_status_code(reply.status);
    for (name, value) in reply.headers {
        if let Ok(h) = Header::from_bytes(name.as_bytes(), value.as_bytes()) {
            response.add_header(h);
        }
    }
    let _ = request.respond(response);
}

/// Accepts connections until the process ends; each request runs on its own thread.
pub fn serve<F>(listener: TcpListener, handle: F) -> Result<(), String>
where
    F: Fn(Request) + Send + Sync + 'static,
{
    let server = Server::from_listener(listener, None).map_err(|e| e.to_string())?;
    let handle = Arc::new(handle);
    for request in server.incoming_requests() {
        let handle = handle.clone();
        std::thread::spawn(move || handle(request));
    }
    Ok(())
}
