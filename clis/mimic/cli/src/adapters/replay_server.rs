//! `ReplayServer`: the driving adapter of a replay. It turns what a client sends on a socket into calls of the `Replayer` and
//! writes back what the cassette answers. It never connects anywhere: a request the recording lacks gets a 502 or a close 1011.
use super::http1::{accept_key, body, read_head, start_line, write_answer, Head};
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

fn with<T>(replayer: &Shared, call: impl FnOnce(&mut Replayer) -> T) -> T {
    call(&mut replayer.lock().unwrap_or_else(|e| e.into_inner()))
}

impl ReplayServer {
    pub fn listen(addr: SocketAddr, replayer: Shared, stop: Arc<AtomicBool>) -> io::Result<ReplayServer> {
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
                            let (replayer, stop) = (replayer.clone(), stop.clone());
                            live.fetch_add(1, Ordering::SeqCst);
                            let guard = Live(live.clone());
                            std::thread::spawn(move || {
                                let _ = guard;
                                let _ = serve(client, &replayer, &stop, started);
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

fn write_miss(client: &mut TcpStream, mismatch: &Mismatch, method: &str) -> io::Result<()> {
    let body = json!({"error": "mimic replay: the recording has no answer for this request", "what": mismatch.what, "reason": mismatch.reason}).to_string();
    let headers = [("content-type".to_string(), "application/json".to_string()), ("x-mimic-mismatch".to_string(), "1".to_string())];
    write_answer(client, 502, &headers, body.as_bytes(), method)
}

fn serve(mut client: TcpStream, replayer: &Shared, stop: &AtomicBool, started: Instant) -> io::Result<()> {
    client.set_nonblocking(false)?;
    client.set_read_timeout(Some(READ_TIMEOUT))?;
    client.set_write_timeout(Some(READ_TIMEOUT))?;
    let (head, rest) = read_head(&mut client)?;
    let (method, target) = start_line(&head.start);
    if head.upgrades() {
        return socket(&head, rest, client, &target, replayer, stop, started);
    }
    let request = body(&mut client, &head, rest, false)?;
    match with(replayer, |r| r.http(&method, &target, &head.headers, request.observed())) {
        Answer::Reply(reply) => write_answer(&mut client, reply.status, &reply.headers, &reply.body, &method),
        Answer::Miss(mismatch) => write_miss(&mut client, &mismatch, &method),
    }
}

fn close_frame(code: Option<u16>) -> Vec<u8> {
    encode_frame(OP_CLOSE, &code.map(|c| c.to_be_bytes().to_vec()).unwrap_or_default(), None)
}

/// Writes what the cassette sends; `false` when the socket ends here.
fn send(client: &mut TcpStream, out: Vec<Out>) -> io::Result<bool> {
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
    Ok(true)
}

fn socket(head: &Head, rest: Vec<u8>, mut client: TcpStream, target: &str, replayer: &Shared, stop: &AtomicBool, started: Instant) -> io::Result<()> {
    let session = match with(replayer, |r| r.upgrade(target, now_ms(started))) {
        Upgrade::Accept(session) => session,
        Upgrade::Refuse(reply) => return write_answer(&mut client, reply.status, &reply.headers, &reply.body, "GET"),
        Upgrade::Miss(mismatch) => return write_miss(&mut client, &mismatch, "GET"),
    };
    let protocol = head.get("sec-websocket-protocol").and_then(|offered| offered.split(',').next()).map(|p| format!("Sec-WebSocket-Protocol: {}\r\n", p.trim())).unwrap_or_default();
    let accept = accept_key(head.get("sec-websocket-key").unwrap_or_default());
    write!(client, "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n{protocol}\r\n")?;
    client.set_read_timeout(Some(TICK))?;
    let result = pump(&mut client, rest, session, replayer, stop, started);
    with(replayer, |r| r.closed(session));
    result
}

fn pump(client: &mut TcpStream, first: Vec<u8>, session: u64, replayer: &Shared, stop: &AtomicBool, started: Instant) -> io::Result<()> {
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
