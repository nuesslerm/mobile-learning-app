//! `ReplayServer`: the driving adapter of a replay. It turns what a client sends on a socket into calls of the `Replayer` and
//! writes back what the cassette answers. It never connects anywhere: a request the recording lacks gets a 502 or a close 1011.
use super::forward::{absolute, host_port, ESTABLISHED, FORBIDDEN};
use super::http1::{accept_key, body, read_head, start_line, write_answer, Head};
use super::tls::{Authority, ServerSide};
use mimic_proxy_core::domain::host_policy::{HostPolicy, Route, Seen};
use mimic_core::wire::{encode_frame, FrameParser, OP_BINARY, OP_CLOSE, OP_TEXT};
use mimic_proxy_core::application::replay::{Answer, Replayer, Upgrade};
use mimic_proxy_core::domain::mismatch::Mismatch;
use mimic_proxy_core::domain::ordering::{Out, Payload};
use serde_json::json;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(5);
const OP_PING: u8 = 9;
const OP_PONG: u8 = 10;
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a socket's read waits before the cassette is asked what is due.
const TICK: Duration = Duration::from_millis(50);
const CLOSE_MISMATCH: u16 = 1011;

pub struct ReplayServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    live: Arc<AtomicU32>,
    accepting: Option<JoinHandle<()>>,
}

struct Live(Arc<AtomicU32>);

impl Drop for Live {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

type Shared = Arc<Mutex<Replayer>>;

/// A replay as an explicit proxy: approved hosts are answered from the cassette under a test CA, every other host is refused.
pub struct Explicit {
    pub policy: HostPolicy,
    pub authority: Authority,
    pub seen: Mutex<Vec<Seen>>,
}

impl Explicit {
    fn saw(&self, seen: Seen) {
        self.seen.lock().unwrap_or_else(|e| e.into_inner()).push(seen);
    }
}

/// A client's stream whose reads can be bounded, so an idle socket still lets the cassette say what is due.
trait Ticking: Read + Write {
    fn wait_at_most(&self, wait: Duration) -> io::Result<()>;
}

impl Ticking for TcpStream {
    fn wait_at_most(&self, wait: Duration) -> io::Result<()> {
        self.set_read_timeout(Some(wait))
    }
}

impl Ticking for ServerSide {
    fn wait_at_most(&self, wait: Duration) -> io::Result<()> {
        self.sock.set_read_timeout(Some(wait))
    }
}

fn with<T>(replayer: &Shared, call: impl FnOnce(&mut Replayer) -> T) -> T {
    call(&mut replayer.lock().unwrap_or_else(|e| e.into_inner()))
}

impl ReplayServer {
    pub fn listen(addr: SocketAddr, replayer: Shared, stop: Arc<AtomicBool>, explicit: Option<Arc<Explicit>>) -> io::Result<ReplayServer> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        let bound = listener.local_addr()?;
        let (live, started) = (Arc::new(AtomicU32::new(0)), Instant::now());
        let accepting = {
            let (stop, live) = (stop.clone(), live.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((client, _)) => {
                            let (replayer, stop, explicit) = (replayer.clone(), stop.clone(), explicit.clone());
                            live.fetch_add(1, Ordering::SeqCst);
                            let guard = Live(live.clone());
                            std::thread::spawn(move || {
                                let _ = guard;
                                let _ = serve(client, &replayer, &stop, started, explicit.as_deref());
                            });
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(POLL),
                        Err(_) => return,
                    }
                }
            })
        };
        Ok(ReplayServer { addr: bound, stop, live, accepting: Some(accepting) })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn live(&self) -> u32 {
        self.live.load(Ordering::SeqCst)
    }
}

impl Drop for ReplayServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(accepting) = self.accepting.take() {
            let _ = accepting.join();
        }
    }
}

fn now_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn write_miss(client: &mut impl Write, mismatch: &Mismatch, method: &str) -> io::Result<()> {
    let body = json!({"error": "mimic replay: the recording has no answer for this request", "what": mismatch.what, "reason": mismatch.reason}).to_string();
    let headers = [("content-type".to_string(), "application/json".to_string()), ("x-mimic-mismatch".to_string(), "1".to_string())];
    write_answer(client, 502, &headers, body.as_bytes(), method)
}

fn serve(mut client: TcpStream, replayer: &Shared, stop: &AtomicBool, started: Instant, explicit: Option<&Explicit>) -> io::Result<()> {
    client.set_nonblocking(false)?;
    client.set_read_timeout(Some(READ_TIMEOUT))?;
    client.set_write_timeout(Some(READ_TIMEOUT))?;
    let (head, rest) = read_head(&mut client)?;
    let (method, target) = start_line(&head.start);
    let Some(explicit) = explicit else { return answer(&head, rest, &mut client, &target, replayer, stop, started) };
    if method != "CONNECT" {
        // a plain request names its host in the target: an approved one is answered as recorded, by that full address
        let approved = absolute(&target).is_some_and(|(host, port, _)| explicit.policy.route(&host, port) == Route::Intercept);
        return if approved { answer(&head, rest, &mut client, &target, replayer, stop, started) } else { client.write_all(FORBIDDEN) };
    }
    let Some((host, port)) = host_port(&target, 443) else { return client.write_all(FORBIDDEN) };
    let name = format!("{host}:{port}");
    if explicit.policy.route(&host, port) != Route::Intercept {
        explicit.saw(Seen::Refused(name));
        return client.write_all(FORBIDDEN);
    }
    client.write_all(ESTABLISHED)?;
    let mut inner = explicit.authority.accept(&host, client).map_err(io::Error::other)?;
    let Ok((head, rest)) = read_head(&mut inner) else {
        explicit.saw(Seen::RejectedCa(name));
        return Ok(());
    };
    explicit.saw(Seen::Intercepted(name.clone()));
    let (_, path) = start_line(&head.start);
    let shown = if port == 443 { format!("https://{host}{path}") } else { format!("https://{name}{path}") };
    answer(&head, rest, &mut inner, &shown, replayer, stop, started)?;
    inner.conn.send_close_notify();
    inner.flush()
}

/// One request of a client, answered from the cassette: an exchange, or a socket.
fn answer<S: Ticking>(head: &Head, rest: Vec<u8>, client: &mut S, target: &str, replayer: &Shared, stop: &AtomicBool, started: Instant) -> io::Result<()> {
    let (method, _) = start_line(&head.start);
    if head.upgrades() {
        return socket(head, rest, client, target, replayer, stop, started);
    }
    let request = body(client, head, rest, false)?;
    match with(replayer, |r| r.http(&method, target, &head.headers, request.observed())) {
        Answer::Reply(reply) => write_answer(client, reply.status, &reply.headers, &reply.body, &method)?,
        Answer::Miss(mismatch) => write_miss(client, &mismatch, &method)?,
    }
    client.flush()
}

fn close_frame(code: Option<u16>) -> Vec<u8> {
    encode_frame(OP_CLOSE, &code.map(|c| c.to_be_bytes().to_vec()).unwrap_or_default(), None)
}

/// Writes what the cassette sends; `false` when the socket ends here.
fn send(client: &mut impl Write, out: Vec<Out>) -> io::Result<bool> {
    for item in out {
        match item {
            Out::Send(Payload::Json(value)) => client.write_all(&encode_frame(OP_TEXT, value.to_string().as_bytes(), None))?,
            Out::Send(Payload::Text(text)) => client.write_all(&encode_frame(OP_TEXT, text.as_bytes(), None))?,
            Out::Send(Payload::Binary(bytes)) => client.write_all(&encode_frame(OP_BINARY, &bytes, None))?,
            Out::Close(code) => {
                client.write_all(&close_frame(code))?;
                return Ok(false);
            }
            Out::Miss(_) => {
                client.write_all(&close_frame(Some(CLOSE_MISMATCH)))?;
                return Ok(false);
            }
        }
    }
    client.flush()?;
    Ok(true)
}

fn socket<S: Ticking>(head: &Head, rest: Vec<u8>, client: &mut S, target: &str, replayer: &Shared, stop: &AtomicBool, started: Instant) -> io::Result<()> {
    let session = match with(replayer, |r| r.upgrade(target, now_ms(started))) {
        Upgrade::Accept(session) => session,
        Upgrade::Refuse(reply) => return write_answer(client, reply.status, &reply.headers, &reply.body, "GET").and_then(|()| client.flush()),
        Upgrade::Miss(mismatch) => return write_miss(client, &mismatch, "GET").and_then(|()| client.flush()),
    };
    let protocol = head.get("sec-websocket-protocol").and_then(|offered| offered.split(',').next()).map(|p| format!("Sec-WebSocket-Protocol: {}\r\n", p.trim())).unwrap_or_default();
    let accept = accept_key(head.get("sec-websocket-key").unwrap_or_default());
    write!(client, "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n{protocol}\r\n")?;
    client.flush()?;
    client.wait_at_most(TICK)?;
    let result = pump(client, rest, session, replayer, stop, started);
    with(replayer, |r| r.closed(session));
    result
}

fn pump<S: Ticking>(client: &mut S, first: Vec<u8>, session: u64, replayer: &Shared, stop: &AtomicBool, started: Instant) -> io::Result<()> {
    let mut parser = FrameParser::default();
    let (mut chunk, mut pending) = ([0u8; 8192], first);
    loop {
        for message in parser.push(&pending) {
            let payload = match message.opcode {
                OP_CLOSE => return client.write_all(&encode_frame(OP_CLOSE, &message.payload, None)),
                OP_PING => {
                    client.write_all(&encode_frame(OP_PONG, &message.payload, None))?;
                    continue;
                }
                OP_PONG => continue,
                OP_TEXT => match serde_json::from_slice(&message.payload) {
                    Ok(value) => Payload::Json(value),
                    Err(_) => Payload::Text(String::from_utf8_lossy(&message.payload).into_owned()),
                },
                _ => Payload::Binary(message.payload),
            };
            let out = with(replayer, |r| r.frame(session, &payload, now_ms(started)));
            if !send(client, out)? {
                return Ok(());
            }
        }
        pending.clear();
        let due = with(replayer, |r| r.tick(session, now_ms(started)));
        if !send(client, due)? || stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        match client.read(&mut chunk) {
            Ok(0) => return Ok(()),
            Ok(n) => pending.extend_from_slice(&chunk[..n]),
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
            Err(e) => return Err(e),
        }
    }
}
