//! The loopback server `sccache` talks to. sccache's WebDAV backend uses one endpoint for reads and writes, but the bucket hands out
//! a read link and a write link that each do one thing; this answers sccache's `GET`, `PUT`, `PROPFIND` and `MKCOL` from a `Cache`
//! over any `BlobStore` (kind `sccache`, one object per compile, named by its hash). Only success statuses and 404 are sent for a
//! cache that fails: sccache then compiles as it would with no cache, and a failed write is lost, never red.
use crate::domain::Kind;
use crate::nx_server::{read_body, Served};
use crate::ports::BlobStore;
use crate::Cache;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;
use tiny_http::{Header, Method, Request, Response, Server};

const WORKERS: usize = 16;
const POLL: Duration = Duration::from_millis(100);
/// sccache's start-up check reads and writes this name; it is answered without touching the bucket.
const CHECK: &str = ".sccache_check";

#[derive(Default)]
struct Counters {
    gets: AtomicU64,
    puts: AtomicU64,
}

struct Shared<S> {
    cache: Arc<Cache<S>>,
    token: String,
    counters: Arc<Counters>,
}

pub struct SccacheServer {
    port: u16,
    token: String,
    stop: Arc<AtomicBool>,
    workers: Vec<JoinHandle<()>>,
    counters: Arc<Counters>,
}

impl SccacheServer {
    /// Listens on a random loopback port with a random bearer token, until `stop` or drop.
    pub fn start<S: BlobStore + 'static>(cache: Arc<Cache<S>>) -> std::io::Result<SccacheServer> {
        let server = Arc::new(Server::http("127.0.0.1:0").map_err(|e| std::io::Error::other(e.to_string()))?);
        let port = server.server_addr().to_ip().map(|a| a.port()).ok_or_else(|| std::io::Error::other("the server has no IP address"))?;
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
        let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let counters = Arc::new(Counters::default());
        let shared = Arc::new(Shared { cache, token: token.clone(), counters: counters.clone() });
        let stop = Arc::new(AtomicBool::new(false));
        let workers = (0..WORKERS)
            .map(|_| {
                let (server, shared, stop) = (server.clone(), shared.clone(), stop.clone());
                std::thread::spawn(move || work(&server, &shared, &stop))
            })
            .collect();
        Ok(SccacheServer { port, token, stop, workers, counters })
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn served(&self) -> Served {
        Served { gets: self.counters.gets.load(Ordering::SeqCst), puts: self.counters.puts.load(Ordering::SeqCst) }
    }

    pub fn stop(mut self) {
        self.shut_down();
    }

    fn shut_down(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

impl Drop for SccacheServer {
    fn drop(&mut self) {
        self.shut_down();
    }
}

fn work<S: BlobStore>(server: &Server, shared: &Shared<S>, stop: &AtomicBool) {
    while !stop.load(Ordering::SeqCst) {
        match server.recv_timeout(POLL) {
            Ok(Some(request)) => answer(shared, request),
            Ok(None) => {}
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// The last path segment, which is the compile's hash: sccache nests its objects in `a/b/c/<hash>` directories the bucket has no use for.
fn name_of(url: &str) -> Option<&str> {
    let path = url.split('?').next().unwrap_or_default();
    path.rsplit('/').next().filter(|n| !n.is_empty())
}

/// A WebDAV multistatus that says the requested path is a directory, which is all sccache's directory checks read.
fn directory_listing(href: &str) -> String {
    let href = href.split('?').next().unwrap_or("/").replace('&', "&amp;").replace('<', "&lt;");
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?><D:multistatus xmlns:D=\"DAV:\"><D:response><D:href>{href}</D:href><D:propstat><D:prop><D:displayname/><D:getlastmodified>Mon, 26 Oct 2026 12:00:00 GMT</D:getlastmodified><D:getcontentlength>0</D:getcontentlength><D:getcontenttype>httpd/unix-directory</D:getcontenttype><D:getetag>\"0\"</D:getetag><D:resourcetype><D:collection/></D:resourcetype></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response></D:multistatus>"
    )
}

fn answer<S: BlobStore>(shared: &Shared<S>, mut request: Request) {
    let trusted = request.headers().iter().find(|h| h.field.equiv("Authorization")).is_some_and(|h| h.value.as_str().strip_prefix("Bearer ") == Some(shared.token.as_str()));
    if !trusted {
        let _ = read_body(&mut request, 0);
        let _ = request.respond(Response::empty(401));
        return;
    }
    let url = request.url().to_string();
    let method = request.method().clone();
    let name = name_of(&url).map(str::to_string);
    let _ = match (method, name) {
        (Method::Get, Some(name)) if name != CHECK => {
            shared.counters.gets.fetch_add(1, Ordering::SeqCst);
            let body = shared.cache.key(Kind::Sccache, &name).ok().and_then(|key| shared.cache.get(&key));
            match body {
                Some(body) => request.respond(Response::from_data(body).with_header(Header::from_bytes("Content-Type", "application/octet-stream").expect("a static header"))),
                None => request.respond(Response::empty(404)),
            }
        }
        (Method::Put, Some(name)) => {
            let cap = shared.cache.max_entry_bytes(Kind::Sccache);
            let body = read_body(&mut request, cap);
            if let (Some(body), true, Ok(key)) = (body, name != CHECK, shared.cache.key(Kind::Sccache, &name)) {
                shared.counters.puts.fetch_add(1, Ordering::SeqCst);
                // sccache writes only after its read missed, so the write needs no existence check of its own
                let _ = shared.cache.put(&key, &body, false);
            }
            request.respond(Response::empty(201))
        }
        (method, _) if method.as_str() == "PROPFIND" => {
            let _ = read_body(&mut request, 1 << 20);
            let kind = Header::from_bytes("Content-Type", "application/xml; charset=utf-8").expect("a static header");
            request.respond(Response::from_string(directory_listing(&url)).with_status_code(207).with_header(kind))
        }
        (method, _) if method.as_str() == "MKCOL" => request.respond(Response::empty(201)),
        _ => request.respond(Response::empty(404)),
    };
}
