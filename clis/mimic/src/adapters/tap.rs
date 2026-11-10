//! `Tap`: the `Wire` adapter, a recording reverse proxy for a `ws+http` reference (ported from exec-testkit's tap and the T3 recorder's
//! proxy.ts). It forwards bytes untouched, so the client and the target talk as they would to each other, and reports what crossed:
//! each HTTP exchange whole, each WebSocket frame whole (no truncation, fragments joined), the upgrade and the close. `Carry::Http` and
//! `Carry::Ws` are the plan's `HttpTap` and `WsTap`: a tap refuses the kind it does not carry. Ping and pong frames are forwarded, not recorded.
use mimic_core::cassette::Dir;
use mimic_core::wire::{close_info, FrameParser, Headers, Wire, WireEvent, OP_CLOSE};
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

const POLL: Duration = Duration::from_millis(5);
const OP_PING: u8 = 9;
const OP_PONG: u8 = 10;

/// Which kinds of connection a tap carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Carry {
    Http,
    Ws,
    Both,
}

type Log = Arc<Mutex<Vec<WireEvent>>>;

fn push(log: &Log, event: WireEvent) {
    log.lock().unwrap_or_else(|e| e.into_inner()).push(event);
}

pub struct Tap {
    addr: SocketAddr,
    log: Log,
    stop: Arc<AtomicBool>,
    accepting: Option<JoinHandle<()>>,
}

impl Tap {
    /// Listens on a free loopback port and forwards every connection to `upstream`.
    pub fn to(upstream: SocketAddr, carry: Carry) -> std::io::Result<Tap> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let addr = listener.local_addr()?;
        let (log, stop): (Log, _) = (Arc::default(), Arc::new(AtomicBool::new(false)));
        let (seen, halt) = (log.clone(), stop.clone());
        let accepting = std::thread::spawn(move || {
            while !halt.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((client, _)) => {
                        let (seen, halt) = (seen.clone(), halt.clone());
                        std::thread::spawn(move || {
                            // a connection that breaks ends here; the client sees the same
                            let _ = connection(client, upstream, carry, &seen, &halt);
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(POLL),
                    Err(_) => return,
                }
            }
        });
        Ok(Tap { addr, log, stop, accepting: Some(accepting) })
    }
}

impl Wire for Tap {
    fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn mark(&self) -> usize {
        self.log.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    fn since(&self, mark: usize) -> Vec<WireEvent> {
        self.log.lock().unwrap_or_else(|e| e.into_inner()).iter().skip(mark).cloned().collect()
    }
}

impl Drop for Tap {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(accepting) = self.accepting.take() {
            let _ = accepting.join();
        }
    }
}

struct Head {
    start: String,
    headers: Headers,
}

impl Head {
    fn get(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    fn text(&self, skip: &dyn Fn(&str) -> bool) -> String {
        let lines: String = self.headers.iter().filter(|(k, _)| !skip(&k.to_lowercase())).map(|(k, v)| format!("{k}: {v}\r\n")).collect();
        format!("{}\r\n{lines}", self.start)
    }
}

/// Reads up to the blank line that ends a message head; what came after it is returned beside it.
fn read_head(stream: &mut TcpStream) -> std::io::Result<(Head, Vec<u8>)> {
    let (mut buf, mut chunk) = (Vec::new(), [0u8; 4096]);
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let text = String::from_utf8_lossy(&buf[..end]).into_owned();
            let mut lines = text.split("\r\n");
            let start = lines.next().unwrap_or_default().to_string();
            let headers = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect();
            return Ok((Head { start, headers }, buf[end + 4..].to_vec()));
        }
        match stream.read(&mut chunk)? {
            0 => return Err(std::io::ErrorKind::UnexpectedEof.into()),
            n => buf.extend_from_slice(&chunk[..n]),
        }
    }
}

/// Repeated header names joined with ", " as the recorder saw them.
fn joined(headers: &Headers) -> Headers {
    let mut out: Headers = Vec::new();
    for (k, v) in headers {
        match out.iter_mut().find(|(name, _)| name.eq_ignore_ascii_case(k)) {
            Some((_, have)) => *have = format!("{have}, {v}"),
            None => out.push((k.clone(), v.clone())),
        }
    }
    out
}

fn dechunk(raw: &[u8]) -> Vec<u8> {
    let (mut out, mut at) = (Vec::new(), 0);
    while let Some(eol) = raw[at..].windows(2).position(|w| w == b"\r\n") {
        let size = usize::from_str_radix(String::from_utf8_lossy(&raw[at..at + eol]).split(';').next().unwrap_or("").trim(), 16).unwrap_or(0);
        at += eol + 2;
        if size == 0 || raw.len() < at + size {
            break;
        }
        out.extend_from_slice(&raw[at..at + size]);
        at += size + 2;
    }
    out
}

fn is_chunked(head: &Head) -> bool {
    head.get("transfer-encoding").is_some_and(|v| v.to_lowercase().contains("chunked"))
}

/// The body that follows a head: by length, chunked, or (a response) to the end of the connection.
fn read_body(stream: &mut TcpStream, head: &Head, mut have: Vec<u8>, to_eof: bool) -> std::io::Result<Vec<u8>> {
    let want = head.get("content-length").and_then(|v| v.parse::<usize>().ok());
    let chunked = is_chunked(head);
    let mut chunk = [0u8; 4096];
    loop {
        let done = if chunked { have.ends_with(b"0\r\n\r\n") } else { want.is_some_and(|n| have.len() >= n) || (want.is_none() && !to_eof) };
        if done {
            break;
        }
        match stream.read(&mut chunk)? {
            0 => break,
            n => have.extend_from_slice(&chunk[..n]),
        }
    }
    Ok(if chunked { dechunk(&have) } else { have })
}

fn connection(mut client: TcpStream, upstream: SocketAddr, carry: Carry, log: &Log, stop: &Arc<AtomicBool>) -> std::io::Result<()> {
    client.set_nonblocking(false)?;
    let (head, rest) = read_head(&mut client)?;
    let upgrade = head.get("upgrade").is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    if (upgrade && carry == Carry::Http) || (!upgrade && carry == Carry::Ws) {
        return client.write_all(b"HTTP/1.1 501 Not Implemented\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
    }
    let mut up = match TcpStream::connect(upstream) {
        Ok(up) => up,
        Err(_) => return client.write_all(b"HTTP/1.1 502 Bad Gateway\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"),
    };
    if upgrade {
        socket(head, rest, client, up, log, stop)
    } else {
        exchange(head, rest, &mut client, &mut up, log)
    }
}

/// One HTTP exchange, forwarded with `Connection: close` so the answer is read to its end.
fn exchange(head: Head, rest: Vec<u8>, client: &mut TcpStream, up: &mut TcpStream, log: &Log) -> std::io::Result<()> {
    let body = read_body(client, &head, rest, false)?;
    let skip = |k: &str| matches!(k, "host" | "connection" | "accept-encoding" | "transfer-encoding" | "content-length");
    let host = up.peer_addr()?;
    write!(up, "{}Host: {host}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n", head.text(&skip), body.len())?;
    up.write_all(&body)?;
    let (answer, early) = read_head(up)?;
    let status: u16 = answer.start.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let bodiless = status == 204 || status == 304 || head.start.starts_with("HEAD ");
    let res_body = if bodiless { Vec::new() } else { read_body(up, &answer, early, true)? };
    let (method, path) = {
        let mut parts = head.start.split_whitespace();
        (parts.next().unwrap_or("GET").to_string(), parts.next().unwrap_or("/").to_string())
    };
    push(
        log,
        WireEvent::Http {
            method,
            path,
            req_headers: joined(&head.headers),
            req_body: String::from_utf8_lossy(&body).into_owned(),
            status,
            res_headers: joined(&answer.headers),
            res_body: String::from_utf8_lossy(&res_body).into_owned(),
        },
    );
    let relay_skip = |k: &str| matches!(k, "transfer-encoding" | "content-length" | "connection");
    write!(client, "{}Content-Length: {}\r\nConnection: close\r\n\r\n", answer.text(&relay_skip), res_body.len())?;
    client.write_all(&res_body)
}

/// An upgrade: the handshake passes through (without compression, which would make frames unreadable), then frames are parsed on the way.
fn socket(head: Head, rest: Vec<u8>, mut client: TcpStream, mut up: TcpStream, log: &Log, stop: &Arc<AtomicBool>) -> std::io::Result<()> {
    let skip = |k: &str| matches!(k, "host" | "sec-websocket-extensions");
    let host = up.peer_addr()?;
    write!(up, "{}Host: {host}\r\n\r\n", head.text(&skip))?;
    up.write_all(&rest)?;
    let (answer, early) = read_head(&mut up)?;
    let status: u16 = answer.start.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    write!(client, "{}\r\n", answer.text(&|_| false))?;
    if status != 101 {
        // a refused upgrade is an ordinary HTTP answer: recorded as one
        let body = read_body(&mut up, &answer, early, true)?;
        client.write_all(&body)?;
        let path = head.start.split_whitespace().nth(1).unwrap_or("/").to_string();
        let res_body = String::from_utf8_lossy(&body).into_owned();
        let method = head.start.split_whitespace().next().unwrap_or("GET").to_string();
        push(log, WireEvent::Http { method, path, req_headers: joined(&head.headers), req_body: String::new(), status, res_headers: joined(&answer.headers), res_body });
        return Ok(());
    }
    let path = head.start.split_whitespace().nth(1).unwrap_or("/").to_string();
    push(log, WireEvent::WsOpen { path, status });
    let (mut client_in, mut up_in) = (client.try_clone()?, up.try_clone()?);
    for s in [&client, &up] {
        s.set_read_timeout(Some(Duration::from_millis(50)))?;
    }
    let (seen, halt) = (log.clone(), stop.clone());
    let to_service = std::thread::spawn(move || {
        let _ = pump(&mut client_in, &mut up_in, rest, Dir::C2s, &seen, &halt);
        // the client is done sending: the service sees the end and closes, whose last frames are still read
        let _ = up_in.shutdown(Shutdown::Write);
    });
    let _ = pump(&mut up, &mut client, early, Dir::S2c, log, stop);
    push(log, WireEvent::WsClose { dir: None, code: None, reason: String::new() });
    let _ = client.shutdown(Shutdown::Both);
    let _ = to_service.join();
    Ok(())
}

/// Copies `from` to `to` until `from` ends, recording the frames that pass; `first` is what was already read past the handshake.
fn pump(from: &mut TcpStream, to: &mut TcpStream, first: Vec<u8>, dir: Dir, log: &Log, stop: &AtomicBool) -> std::io::Result<()> {
    let mut parser = FrameParser::default();
    let (mut chunk, mut pending) = ([0u8; 8192], first);
    loop {
        if !pending.is_empty() {
            // recorded before it is forwarded, so what a frame causes is never logged ahead of it
            for m in parser.push(&pending) {
                match m.opcode {
                    OP_CLOSE => {
                        let (code, reason) = close_info(&m.payload);
                        push(log, WireEvent::WsClose { dir: Some(dir), code, reason });
                    }
                    OP_PING | OP_PONG => {}
                    opcode => push(log, WireEvent::Ws { dir, opcode, data: m.payload }),
                }
            }
            to.write_all(&pending)?;
            pending.clear();
        }
        if stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        match from.read(&mut chunk) {
            Ok(0) => return Ok(()),
            Ok(n) => pending.extend_from_slice(&chunk[..n]),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => return Err(e),
        }
    }
}
