//! `Proxy`: the loopback adapter that stands between the phone and the T3 server it is configured for. It forwards every byte of an
//! HTTP exchange and of an orchestration socket (the credentials the app sends go on untouched, so the app keeps working) and reports
//! what crossed, so the recorder can export its shape.
use super::conn::dial;
use super::http1::{bodiless, body, read_head, start_line, status_of, Body, Head};
use mimic_core::cassette::Dir;
use mimic_core::wire::{close_info, FrameParser, OP_CLOSE, OP_TEXT};
pub use mimic_proxy_core::domain::traffic::{Event, Failure};
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

const POLL: Duration = Duration::from_millis(5);
/// The control frames of the protocol: they are relayed, not reported.
const OP_PING: u8 = 9;
const OP_PONG: u8 = 10;
/// How long a read of an ordinary exchange waits for the other side. A slow client is a broken one.
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a socket's read waits before the pump looks at the stop flag.
const PUMP_TIMEOUT: Duration = Duration::from_millis(100);

/// How a proxy forwards. The shape trace forwards every header as it came; a recording asks for bodies it can read.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// Drop the client's `accept-encoding`, so the server answers uncompressed and the sanitiser can read every body.
    pub readable_bodies: bool,
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
        Proxy::listen_with(addr, upstream, sink, stop, Options::default())
    }

    pub fn listen_with(addr: SocketAddr, upstream: SocketAddr, sink: Sink, stop: Arc<AtomicBool>, options: Options) -> std::io::Result<Proxy> {
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
                                if serve(client, upstream, sink.clone(), stop, exchanges, connections, options).is_err() {
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

/// A head and what follows it as one write: a peer that reads once and answers must not be left with half a message unread.
pub(super) fn whole(head: String, rest: &[u8]) -> Vec<u8> {
    let mut out = head.into_bytes();
    out.extend_from_slice(rest);
    out
}

/// One connection: an HTTP exchange, or an orchestration socket.
fn serve(
    mut client: TcpStream,
    upstream: SocketAddr,
    sink: Sink,
    stop: Arc<AtomicBool>,
    exchanges: Arc<AtomicU32>,
    connections: Arc<AtomicU32>,
    options: Options,
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
    if head.upgrades() {
        let connection = connections.fetch_add(1, Ordering::SeqCst) + 1;
        socket(head, rest, client, upstream, sink, stop, connection)
    } else {
        let exchange = exchanges.fetch_add(1, Ordering::SeqCst) + 1;
        exchange_http(head, rest, client, upstream, sink, exchange, options)
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
    options: Options,
) -> std::io::Result<()> {
    let (method, target) = start_line(&head.start);
    let request_body = body(&mut client, &head, rest, false)?;
    sink(Event::HttpRequest { exchange, method: method.clone(), target: target.clone(), headers: head.headers.clone(), body: request_body.observed().to_vec() });
    let mut up = match dial(upstream) {
        Ok(up) => up,
        Err(_) => {
            sink(Event::Failed { failure: Failure::NoAnswer });
            let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
            return Ok(());
        }
    };
    up.set_read_timeout(Some(READ_TIMEOUT))?;
    up.set_write_timeout(Some(READ_TIMEOUT))?;
    let skip = |k: &str| matches!(k, "host" | "connection" | "proxy-connection") || (options.readable_bodies && k == "accept-encoding");
    up.write_all(&whole(format!("{}Host: {upstream}\r\nConnection: close\r\n\r\n", head.text(&skip)), &request_body.wire))?;
    let (answer, early) = match read_head(&mut up) {
        Ok(pair) => pair,
        Err(_) => {
            sink(Event::Failed { failure: Failure::NoAnswerRead });
            let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
            return Ok(());
        }
    };
    let status = status_of(&answer.start);
    let res_body = if bodiless(status, &method) { Body::empty() } else { body(&mut up, &answer, early, true)? };
    sink(Event::HttpResponse { exchange, status, headers: answer.headers.clone(), body: res_body.observed().to_vec() });
    let skip = |k: &str| matches!(k, "connection" | "keep-alive");
    client.write_all(&whole(format!("{}Connection: close\r\n\r\n", answer.text(&skip)), &res_body.wire))
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
    let mut up = match dial(upstream) {
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
    up.write_all(&whole(format!("{}Host: {upstream}\r\nConnection: Upgrade\r\n\r\n", head.text(&skip)), &rest))?;
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
        return client.write_all(&whole(format!("{}Connection: close\r\n\r\n", answer.text(&skip)), &response_body.wire));
    }
    sink(Event::WsOpen { connection, target, status });
    let skip = |k: &str| matches!(k, "connection" | "keep-alive");
    client.write_all(format!("{}Connection: Upgrade\r\n\r\n", answer.text(&skip)).as_bytes())?;
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

/// Reports the messages of one direction of a socket as their bytes pass. A reply the server split into parts is reported whole.
pub(super) struct Frames {
    parser: FrameParser,
    text_parts: Vec<u8>,
    direction: Dir,
    connection: u32,
}

impl Frames {
    pub(super) fn new(direction: Dir, connection: u32) -> Frames {
        Frames { parser: FrameParser::default(), text_parts: Vec::new(), direction, connection }
    }

    pub(super) fn feed(&mut self, bytes: &[u8], sink: &Sink) {
        let (connection, direction) = (self.connection, self.direction);
        for message in self.parser.push(bytes) {
            match message.opcode {
                OP_CLOSE => {
                    let (code, _) = close_info(&message.payload);
                    sink(Event::WsClose { connection, direction: Some(direction), code });
                }
                OP_PING | OP_PONG => {}
                OP_TEXT if message.payload.first() == Some(&0x1e) && direction == Dir::S2c => {
                    self.text_parts.extend_from_slice(&message.payload[1..]);
                }
                OP_TEXT if !self.text_parts.is_empty() => {
                    self.text_parts.extend_from_slice(&message.payload);
                    sink(Event::WsFrame { connection, direction, opcode: OP_TEXT, data: std::mem::take(&mut self.text_parts) });
                }
                _ => sink(Event::WsFrame { connection, direction, opcode: message.opcode, data: message.payload }),
            }
        }
    }

    /// Whether the stream ended in the middle of a split reply.
    pub(super) fn cut_short(&self) -> bool {
        !self.text_parts.is_empty()
    }
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
    let mut frames = Frames::new(direction, connection);
    let (mut chunk, mut pending) = ([0u8; 8192], first);
    loop {
        if !pending.is_empty() {
            frames.feed(&pending, &sink);
            to.write_all(&pending)?;
            pending.clear();
        }
        if stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        match from.read(&mut chunk) {
            Ok(0) if frames.cut_short() => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(0) => return Ok(()),
            Ok(n) => pending.extend_from_slice(&chunk[..n]),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => return Err(e),
        }
    }
}
