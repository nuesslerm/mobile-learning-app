//! `Proxy`: the loopback adapter that stands between the phone and the T3 server it is configured for. It forwards every byte of an
//! HTTP exchange and of an orchestration socket (the credentials the app sends go on untouched, so the app keeps working) and reports
//! what crossed, so the recorder can export its shape.
use mimic_core::cassette::Dir;
use mimic_core::wire::{close_info, FrameParser, OP_CLOSE, OP_TEXT};
use std::io::{self, BufRead, BufReader, Cursor, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

const POLL: Duration = Duration::from_millis(5);
/// The control frames of the protocol: they are relayed, not reported.
const OP_PING: u8 = 9;
const OP_PONG: u8 = 10;
const MAX_HEAD: usize = 64 * 1024;
/// How long a read of an ordinary exchange waits for the other side. A slow client is a broken one.
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a socket's read waits before the pump looks at the stop flag.
const PUMP_TIMEOUT: Duration = Duration::from_millis(100);

/// Why an exchange failed: a word of the proxy's own, never a value of the traffic.
#[derive(Clone, Copy, Debug)]
pub enum Failure {
    /// The head of the request could not be read.
    RequestHead,
    Incomplete,
    /// The server did not answer.
    NoAnswer,
    /// The answer of the server could not be read.
    NoAnswerRead,
    /// The server did not answer the socket.
    NoSocketAnswer,
    /// The handshake of the socket could not be read.
    NoHandshake,
}

impl Failure {
    /// What the file says: a fixed word, so no value of the traffic can ride on an error.
    pub fn label(self) -> &'static str {
        match self {
            Failure::Incomplete => "an exchange ended before the complete message",
            Failure::RequestHead => "the request head could not be read",
            Failure::NoAnswer => "the server did not answer",
            Failure::NoAnswerRead => "the server's answer could not be read",
            Failure::NoSocketAnswer => "the server did not answer the socket",
            Failure::NoHandshake => "the server's handshake could not be read",
        }
    }
}

/// What crossed the proxy.
#[derive(Clone, Debug)]
pub enum Event {
    HttpRequest { exchange: u32, method: String, target: String, headers: Vec<(String, String)>, body: Vec<u8> },
    HttpResponse { exchange: u32, status: u16, headers: Vec<(String, String)>, body: Vec<u8> },
    WsOpen { connection: u32, target: String, status: u16 },
    WsRefused { connection: u32, target: String, status: u16, body: Vec<u8> },
    WsFrame { connection: u32, direction: Dir, opcode: u8, data: Vec<u8> },
    WsClose { connection: u32, direction: Option<Dir>, code: Option<u16> },
    Failed { failure: Failure },
}

/// Where the proxy reports what crossed.
pub type Sink = Arc<dyn Fn(Event) + Send + Sync>;

/// A listening proxy: it forwards to one upstream and never to anything else.
pub struct Proxy {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    live: Arc<AtomicU32>,
    accepting: Option<JoinHandle<()>>,
}

/// Counts a connection that is still open, so the caller can wait for the last one before it writes the file's end.
struct Live(Arc<AtomicU32>);

impl Drop for Live {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Proxy {
    /// Listens on `addr` and forwards every connection to `upstream`.
    pub fn listen(addr: SocketAddr, upstream: SocketAddr, sink: Sink, stop: Arc<AtomicBool>) -> std::io::Result<Proxy> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        let bound = listener.local_addr()?;
        let exchanges = Arc::new(AtomicU32::new(0));
        let connections = Arc::new(AtomicU32::new(0));
        let live = Arc::new(AtomicU32::new(0));
        let accepting = {
            let (stop, live) = (stop.clone(), live.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((client, _)) => {
                            let (sink, stop) = (sink.clone(), stop.clone());
                            let (exchanges, connections) = (exchanges.clone(), connections.clone());
                            live.fetch_add(1, Ordering::SeqCst);
                            let guard = Live(live.clone());
                            std::thread::spawn(move || {
                                let _ = guard;
                                if serve(client, upstream, sink.clone(), stop, exchanges, connections).is_err() {
                                    sink(Event::Failed { failure: Failure::Incomplete });
                                }
                            });
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(POLL),
                        Err(_) => return,
                    }
                }
            })
        };
        Ok(Proxy { addr: bound, stop, live, accepting: Some(accepting) })
    }

    /// The address the proxy listens on.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// How many connections are still open.
    pub fn live(&self) -> u32 {
        self.live.load(Ordering::SeqCst)
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(accepting) = self.accepting.take() {
            let _ = accepting.join();
        }
    }
}

/// The head of a message: its start line and its header lines.
struct Head {
    start: String,
    headers: Vec<(String, String)>,
}

impl Head {
    fn get(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// The head as it goes on the wire, without the blank line that ends it.
    fn text(&self, skip: &dyn Fn(&str) -> bool) -> String {
        let mut out = format!("{}\r\n", self.start);
        for (name, value) in &self.headers {
            if !skip(&name.to_lowercase()) {
                out.push_str(&format!("{name}: {value}\r\n"));
            }
        }
        out
    }
}

/// The start line's method and target.
fn start_line(start: &str) -> (String, String) {
    let mut parts = start.split_whitespace();
    (parts.next().unwrap_or("GET").to_string(), parts.next().unwrap_or("/").to_string())
}

/// The status code of a start line.
fn status_of(start: &str) -> u16 {
    start.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0)
}

/// Reads the head of a message; what came after it is returned beside it.
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
        if buf.len() > MAX_HEAD {
            return Err(std::io::ErrorKind::InvalidData.into());
        }
        match stream.read(&mut chunk)? {
            0 => return Err(std::io::ErrorKind::UnexpectedEof.into()),
            n => buf.extend_from_slice(&chunk[..n]),
        }
    }
}

// Preserve chunk framing and trailers on the wire. Only the decoded entity goes to the recorder.
struct Body {
    wire: Vec<u8>,
    decoded: Option<Vec<u8>>,
}

impl Body {
    fn observed(&self) -> &[u8] { self.decoded.as_deref().unwrap_or(&self.wire) }
}

fn body(stream: &mut TcpStream, head: &Head, initial: Vec<u8>, to_eof: bool) -> io::Result<Body> {
    let mut reader = BufReader::new(Cursor::new(initial).chain(stream));
    let mut wire = Vec::new();
    if head.get("transfer-encoding").is_some_and(|v| v.split(',').any(|part| part.trim().eq_ignore_ascii_case("chunked"))) {
        let mut decoded = Vec::new();
        loop {
            let start = wire.len();
            reader.by_ref().take(MAX_HEAD as u64).read_until(b'\n', &mut wire)?;
            let line = wire[start..].strip_suffix(b"\r\n").ok_or(io::ErrorKind::InvalidData)?;
            let size = std::str::from_utf8(line.split(|c| *c == b';').next().unwrap_or_default())
                .ok().and_then(|s| u64::from_str_radix(s.trim(), 16).ok()).ok_or(io::ErrorKind::InvalidData)?;
            if size == 0 {
                loop {
                    let start = wire.len();
                    reader.by_ref().take(MAX_HEAD as u64).read_until(b'\n', &mut wire)?;
                    let trailer = wire[start..].strip_suffix(b"\r\n").ok_or(io::ErrorKind::InvalidData)?;
                    if trailer.is_empty() { return Ok(Body { wire, decoded: Some(decoded) }); }
                }
            }
            let start = wire.len();
            if reader.by_ref().take(size).read_to_end(&mut wire)? as u64 != size {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            decoded.extend_from_slice(&wire[start..]);
            let mut crlf = [0; 2];
            reader.read_exact(&mut crlf)?;
            if crlf != *b"\r\n" { return Err(io::ErrorKind::InvalidData.into()); }
            wire.extend_from_slice(&crlf);
        }
    }
    if let Some(length) = head.get("content-length") {
        let length: u64 = length.parse().map_err(|_| io::ErrorKind::InvalidData)?;
        if reader.take(length).read_to_end(&mut wire)? as u64 != length {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
    } else if to_eof { reader.read_to_end(&mut wire)?; }
    Ok(Body { wire, decoded: None })
}

/// One connection: an HTTP exchange, or an orchestration socket.
fn serve(
    mut client: TcpStream,
    upstream: SocketAddr,
    sink: Sink,
    stop: Arc<AtomicBool>,
    exchanges: Arc<AtomicU32>,
    connections: Arc<AtomicU32>,
) -> std::io::Result<()> {
    client.set_nonblocking(false)?;
    client.set_read_timeout(Some(READ_TIMEOUT))?;
    client.set_write_timeout(Some(READ_TIMEOUT))?;
    let (head, rest) = match read_head(&mut client) {
        Ok(pair) => pair,
        Err(_) => {
            sink(Event::Failed { failure: Failure::RequestHead });
            return Ok(());
        }
    };
    let upgrade = head.get("upgrade").is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    if upgrade {
        let connection = connections.fetch_add(1, Ordering::SeqCst) + 1;
        socket(head, rest, client, upstream, sink, stop, connection)
    } else {
        let exchange = exchanges.fetch_add(1, Ordering::SeqCst) + 1;
        exchange_http(head, rest, client, upstream, sink, exchange)
    }
}

/// One HTTP exchange, forwarded with the client's own headers: the credentials the app sent go on untouched.
fn exchange_http(
    head: Head,
    rest: Vec<u8>,
    mut client: TcpStream,
    upstream: SocketAddr,
    sink: Sink,
    exchange: u32,
) -> std::io::Result<()> {
    let (method, target) = start_line(&head.start);
    let request_body = body(&mut client, &head, rest, false)?;
    sink(Event::HttpRequest { exchange, method: method.clone(), target: target.clone(), headers: head.headers.clone(), body: request_body.observed().to_vec() });
    let mut up = match TcpStream::connect(upstream) {
        Ok(up) => up,
        Err(_) => {
            sink(Event::Failed { failure: Failure::NoAnswer });
            let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
            return Ok(());
        }
    };
    up.set_read_timeout(Some(READ_TIMEOUT))?;
    up.set_write_timeout(Some(READ_TIMEOUT))?;
    let skip = |k: &str| matches!(k, "host" | "connection" | "proxy-connection");
    write!(up, "{}Host: {upstream}\r\nConnection: close\r\n\r\n", head.text(&skip))?;
    up.write_all(&request_body.wire)?;
    let (answer, early) = match read_head(&mut up) {
        Ok(pair) => pair,
        Err(_) => {
            sink(Event::Failed { failure: Failure::NoAnswerRead });
            let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
            return Ok(());
        }
    };
    let status = status_of(&answer.start);
    let bodiless = status == 204 || status == 304 || (100..200).contains(&status) || method == "HEAD";
    let res_body = if bodiless { Body { wire: Vec::new(), decoded: None } } else { body(&mut up, &answer, early, true)? };
    sink(Event::HttpResponse { exchange, status, headers: answer.headers.clone(), body: res_body.observed().to_vec() });
    let skip = |k: &str| matches!(k, "connection" | "keep-alive");
    write!(client, "{}Connection: close\r\n\r\n", answer.text(&skip))?;
    client.write_all(&res_body.wire)
}

/// An orchestration socket: the handshake goes through (without the compression offer, so the frames stay readable), then the frames
/// are relayed in both directions and reported.
#[allow(clippy::too_many_arguments)]
fn socket(
    head: Head,
    rest: Vec<u8>,
    mut client: TcpStream,
    upstream: SocketAddr,
    sink: Sink,
    stop: Arc<AtomicBool>,
    connection: u32,
) -> std::io::Result<()> {
    let (_, target) = start_line(&head.start);
    let mut up = match TcpStream::connect(upstream) {
        Ok(up) => up,
        Err(_) => {
            sink(Event::Failed { failure: Failure::NoSocketAnswer });
            let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
            return Ok(());
        }
    };
    up.set_read_timeout(Some(PUMP_TIMEOUT))?;
    up.set_write_timeout(Some(READ_TIMEOUT))?;
    let skip = |k: &str| matches!(k, "host" | "connection" | "sec-websocket-extensions");
    write!(up, "{}Host: {upstream}\r\nConnection: Upgrade\r\n\r\n", head.text(&skip))?;
    up.write_all(&rest)?;
    let (answer, early) = match read_head(&mut up) {
        Ok(pair) => pair,
        Err(_) => {
            sink(Event::Failed { failure: Failure::NoHandshake });
            let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
            return Ok(());
        }
    };
    let status = status_of(&answer.start);
    if status != 101 {
        let response_body = body(&mut up, &answer, early, true)?;
        sink(Event::WsRefused { connection, target, status, body: response_body.observed().to_vec() });
        let skip = |k: &str| matches!(k, "connection" | "keep-alive");
        write!(client, "{}Connection: close\r\n\r\n", answer.text(&skip))?;
        return client.write_all(&response_body.wire);
    }
    sink(Event::WsOpen { connection, target, status });
    let skip = |k: &str| matches!(k, "connection" | "keep-alive");
    write!(client, "{}Connection: Upgrade\r\n\r\n", answer.text(&skip))?;
    client.set_read_timeout(Some(PUMP_TIMEOUT))?;
    client.set_write_timeout(Some(READ_TIMEOUT))?;
    let (mut client_in, mut up_in) = (client.try_clone()?, up.try_clone()?);
    let (to_service, to_client) = (sink.clone(), sink.clone());
    let to_service_stop = stop.clone();
    let service = std::thread::spawn(move || {
        let _ = pump(&mut client_in, &mut up_in, rest, Dir::C2s, to_service, to_service_stop, connection);
        let _ = up_in.shutdown(Shutdown::Write);
    });
    let _ = pump(&mut up, &mut client, early, Dir::S2c, to_client, stop, connection);
    sink(Event::WsClose { connection, direction: None, code: None });
    let _ = service.join();
    Ok(())
}

/// Copies `from` to `to` until `from` ends, reporting the frames that pass; `first` is what was read past the handshake.
#[allow(clippy::too_many_arguments)]
fn pump(
    from: &mut TcpStream,
    to: &mut TcpStream,
    first: Vec<u8>,
    direction: Dir,
    sink: Sink,
    stop: Arc<AtomicBool>,
    connection: u32,
) -> std::io::Result<()> {
    let mut parser = FrameParser::default();
    let mut text_parts = Vec::new();
    let (mut chunk, mut pending) = ([0u8; 8192], first);
    loop {
        if !pending.is_empty() {
            for message in parser.push(&pending) {
                match message.opcode {
                    OP_CLOSE => {
                        let (code, _) = close_info(&message.payload);
                        sink(Event::WsClose { connection, direction: Some(direction), code });
                    }
                    OP_PING | OP_PONG => {}
                    OP_TEXT if message.payload.first() == Some(&0x1e) && direction == Dir::S2c => {
                        text_parts.extend_from_slice(&message.payload[1..]);
                    }
                    OP_TEXT if !text_parts.is_empty() => {
                        text_parts.extend_from_slice(&message.payload);
                        sink(Event::WsFrame { connection, direction, opcode: OP_TEXT, data: std::mem::take(&mut text_parts) });
                    }
                    _ => sink(Event::WsFrame { connection, direction, opcode: message.opcode, data: message.payload }),
                }
            }
            to.write_all(&pending)?;
            pending.clear();
        }
        if stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        match from.read(&mut chunk) {
            Ok(0) if !text_parts.is_empty() => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(0) => return Ok(()),
            Ok(n) => pending.extend_from_slice(&chunk[..n]),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => return Err(e),
        }
    }
}
