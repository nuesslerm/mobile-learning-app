//! The loopback server Nx talks to. Nx 23's remote cache speaks one protocol, `GET|PUT {server}/v1/cache/<hash>` with a bearer
//! token; this answers it from a `Cache` over any `BlobStore`. Only 200 and 404 are ever sent: Nx 23.2.1 skips the task and fails
//! the run on any other status, so a cache that fails must look like a cache that is empty.
use crate::domain::Kind;
use crate::ports::BlobStore;
use crate::Cache;
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;
use tiny_http::Request;

/// A connection that stays silent this long is dropped.
const IDLE: Duration = Duration::from_secs(120);
const MAX_HEAD: u64 = 64 * 1024;
const ROUTE: &str = "/v1/cache/";

/// What the server received, whatever it answered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Served {
    pub gets: u64,
    pub puts: u64,
}

#[derive(Default)]
struct Counters {
    gets: AtomicU64,
    puts: AtomicU64,
}

struct Shared<S> {
    cache: Arc<Cache<S>>,
    token: String,
    /// Hashes this run just answered 404 for: Nx stores the result of exactly these next, so the PUT needs no existence check.
    missed: Mutex<HashSet<String>>,
    counters: Arc<Counters>,
}

pub struct NxServer {
    port: u16,
    token: String,
    stop: Arc<AtomicBool>,
    acceptor: Option<JoinHandle<()>>,
    counters: Arc<Counters>,
}

impl NxServer {
    /// Listens on a random loopback port with a random token, until `stop` or drop.
    pub fn start<S: BlobStore + 'static>(cache: Arc<Cache<S>>) -> std::io::Result<NxServer> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
        let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let counters = Arc::new(Counters::default());
        let shared = Arc::new(Shared { cache, token: token.clone(), missed: Mutex::new(HashSet::new()), counters: counters.clone() });
        let stop = Arc::new(AtomicBool::new(false));
        let acceptor = Some({
            let stop = stop.clone();
            std::thread::spawn(move || accept_loop(listener, shared, stop))
        });
        Ok(NxServer { port, token, stop, acceptor, counters })
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

    /// What the Nx child needs: the server and its token, and the loopback kept out of any proxy (the sandbox sets one, which
    /// would otherwise swallow the call). `proxy_bypass` is the `NO_PROXY` the child would inherit.
    pub fn env(&self, proxy_bypass: Option<&str>) -> Vec<(&'static str, String)> {
        let bypass = [proxy_bypass.unwrap_or_default().trim_end_matches(','), "127.0.0.1,localhost"].into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(",");
        vec![("NX_SELF_HOSTED_REMOTE_CACHE_SERVER", self.url()), ("NX_SELF_HOSTED_REMOTE_CACHE_ACCESS_TOKEN", self.token.clone()), ("NO_PROXY", bypass.clone()), ("no_proxy", bypass)]
    }

    pub fn stop(mut self) {
        self.shut_down();
    }

    fn shut_down(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(acceptor) = self.acceptor.take() {
            let _ = TcpStream::connect(("127.0.0.1", self.port));
            let _ = acceptor.join();
        }
    }
}

impl Drop for NxServer {
    fn drop(&mut self) {
        self.shut_down();
    }
}

/// A connection that sends nothing holds only its own thread, never a request's place.
fn accept_loop<S: BlobStore + 'static>(listener: TcpListener, shared: Arc<Shared<S>>, stop: Arc<AtomicBool>) {
    for stream in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        if let Ok(stream) = stream {
            let shared = shared.clone();
            std::thread::spawn(move || {
                let _ = stream.set_read_timeout(Some(IDLE));
                let _ = stream.set_write_timeout(Some(IDLE));
                let _ = answer(&shared, stream);
            });
        }
    }
}

/// Reads one request (a line, headers, a `Content-Length` or chunked body for PUT), answers it and closes.
fn answer<S: BlobStore>(shared: &Shared<S>, mut stream: TcpStream) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.by_ref().take(MAX_HEAD).read_line(&mut line)?;
    let (method, target) = {
        let mut parts = line.split_whitespace();
        (parts.next().unwrap_or_default().to_string(), parts.next().unwrap_or_default().to_string())
    };
    let (mut bearer, mut length, mut chunked, mut expect) = (None, 0u64, false, false);
    loop {
        let mut header = String::new();
        if reader.by_ref().take(MAX_HEAD).read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            let value = value.trim();
            match name.trim().to_ascii_lowercase().as_str() {
                "authorization" => bearer = value.strip_prefix("Bearer ").map(str::to_string),
                "content-length" => length = value.parse().unwrap_or(0),
                "transfer-encoding" => chunked = value.eq_ignore_ascii_case("chunked"),
                "expect" => expect = value.eq_ignore_ascii_case("100-continue"),
                _ => {}
            }
        }
    }
    let started = std::time::Instant::now();
    let trace = std::env::var_os("WF_NX_TRACE").is_some_and(|v| !v.is_empty());
    let hash = hash_of(&target);
    if trace {
        eprintln!("wf: nx cache request {method} {}", hash.unwrap_or("-"));
    }
    let trusted = bearer.as_deref() == Some(shared.token.as_str());
    let key = hash.filter(|_| trusted).and_then(|h| shared.cache.key(Kind::Nx, h).ok().map(|k| (h.to_string(), k)));
    let mut read_body = |cap: u64| {
        if expect {
            let _ = stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n");
        }
        read_body_of(&mut reader, length, chunked, cap)
    };
    let (status, body) = match (method.as_str(), key) {
        ("GET", Some((hash, key))) => {
            shared.counters.gets.fetch_add(1, Ordering::SeqCst);
            match shared.cache.get(&key) {
                Some(body) => (200, body),
                None => {
                    shared.missed.lock().unwrap_or_else(PoisonError::into_inner).insert(hash);
                    (404, Vec::new())
                }
            }
        }
        ("PUT", Some((hash, key))) => {
            shared.counters.puts.fetch_add(1, Ordering::SeqCst);
            if let Some(body) = read_body(shared.cache.max_entry_bytes(Kind::Nx)) {
                let just_missed = shared.missed.lock().unwrap_or_else(PoisonError::into_inner).remove(&hash);
                let _ = shared.cache.put(&key, &body, !just_missed);
            }
            (200, Vec::new())
        }
        ("PUT", None) => {
            let _ = read_body(0);
            (404, Vec::new())
        }
        _ => (404, Vec::new()),
    };
    let (reason, kind) = if status == 200 { ("OK", "Content-Type: application/octet-stream\r\n") } else { ("Not Found", "") };
    write!(stream, "HTTP/1.1 {status} {reason}\r\n{kind}Content-Length: {}\r\nConnection: close\r\n\r\n", body.len())?;
    if method != "HEAD" {
        stream.write_all(&body)?;
    }
    stream.flush()?;
    if trace {
        eprintln!("wf: nx cache answered in {} ms", started.elapsed().as_millis());
    }
    Ok(())
}

/// `/v1/cache/<hash>`, with any query dropped.
fn hash_of(url: &str) -> Option<&str> {
    let path = url.split('?').next().unwrap_or_default();
    path.strip_prefix(ROUTE).filter(|h| !h.is_empty() && !h.contains('/'))
}

/// The body, or `None` when it is over `cap` bytes or cannot be read; either way none of it is left on the connection.
pub(crate) fn read_body(request: &mut Request, cap: u64) -> Option<Vec<u8>> {
    let declared = request.body_length();
    let reader = request.as_reader();
    if declared.is_some_and(|n| n as u64 > cap) {
        let _ = std::io::copy(reader, &mut std::io::sink());
        return None;
    }
    let mut body = Vec::with_capacity(declared.unwrap_or(0));
    let read = Read::take(&mut *reader, cap.saturating_add(1)).read_to_end(&mut body);
    if read.is_ok() && body.len() as u64 > cap {
        let _ = std::io::copy(reader, &mut std::io::sink());
    }
    (read.is_ok() && body.len() as u64 <= cap).then_some(body)
}

/// The body of a std-parsed request, or `None` when it is over `cap` bytes or cannot be read; the rest is drained so the reply is not reset.
fn read_body_of(reader: &mut impl BufRead, length: u64, chunked: bool, cap: u64) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    if chunked {
        loop {
            let mut size = String::new();
            reader.read_line(&mut size).ok()?;
            let size = u64::from_str_radix(size.trim().split(';').next().unwrap_or_default(), 16).ok()?;
            if size == 0 {
                break;
            }
            Read::take(&mut *reader, size).read_to_end(&mut body).ok()?;
            reader.read_line(&mut String::new()).ok()?;
            if body.len() as u64 > cap {
                return None;
            }
        }
        return (body.len() as u64 <= cap).then_some(body);
    }
    if length > cap {
        let _ = std::io::copy(&mut Read::take(reader, length), &mut std::io::sink());
        return None;
    }
    Read::take(reader, length).read_to_end(&mut body).ok().filter(|n| *n as u64 == length).map(|_| body)
}
