//! The WebSocket adapter. A socket is unkeyed until the phone's `e2ee_hello` is answered, unauthenticated until its first sealed message names
//! a paired device, and only then the core's `Connection`. The close codes and reasons are the reference's (`e2ee-channel.ts`): 4001 for a
//! handshake or auth failure, 4002 for the handshake deadline, 4003 for too many frames that do not open.
use crate::http::{self, Head};
use orca_core::{Connection, Host};
use orca_crypto::auth::{authenticate, Authenticated};
use orca_crypto::legacy::LegacyChannel;
use orca_crypto::ports::DeviceRegistry;
use orca_crypto::v2::{DesktopSession, Transport};
use orca_crypto::{Entropy, KeyPair};
use serde_json::{json, Value};
use std::io::{self, ErrorKind, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tungstenite::handshake::derive_accept_key;
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::protocol::{CloseFrame, Role, WebSocketConfig};
use tungstenite::{Error, Message, WebSocket};

pub const HANDSHAKE_DEADLINE: Duration = Duration::from_secs(10);
const MAX_DECRYPT_FAILURES: u32 = 5;
/// A frame of up to 16 MiB: the WebSocket limit the reference's own transport sets.
const MAX_FRAME: usize = 16 << 20;

pub type SharedEntropy = Arc<dyn Entropy + Send + Sync>;
pub type SharedDevices = Arc<dyn DeviceRegistry + Send + Sync>;

/// Everything a socket needs, shared by all of them.
#[derive(Clone)]
pub struct Shared {
    pub host: Arc<Host>,
    pub keys: Arc<KeyPair>,
    pub devices: SharedDevices,
    pub entropy: SharedEntropy,
    pub handshake_deadline: Duration,
}

pub struct Running {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
}

impl Running {
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Blocks while the server runs (until `stop` from another thread).
    pub fn wait(mut self) {
        if let Some(handle) = self.accept.take() {
            let _ = handle.join();
        }
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.accept.take() {
            let _ = handle.join();
        }
    }
}

pub fn serve(bind: impl ToSocketAddrs, shared: Shared) -> io::Result<Running> {
    let listener = TcpListener::bind(bind)?;
    listener.set_nonblocking(true)?;
    let addr = listener.local_addr()?;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let accept = std::thread::spawn(move || {
        while !flag.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let (shared, stop) = (shared.clone(), flag.clone());
                    std::thread::spawn(move || handle(stream, &shared, &stop));
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(10)),
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        }
    });
    Ok(Running { addr, stop, accept: Some(accept) })
}

fn handle(mut stream: TcpStream, shared: &Shared, stop: &AtomicBool) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let Some(head) = http::read_head(&mut stream) else { return };
    let upgrading = head.header("upgrade").is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    match (head.method.as_str(), head.path.as_str(), upgrading) {
        ("GET", "/health", false) => http::respond(&mut stream, 200, &http::health()),
        ("GET", _, true) => upgrade(stream, &head, shared, stop),
        _ => http::respond(&mut stream, 404, &json!({"code": "not_found", "message": "No such route."})),
    }
}

fn upgrade(mut stream: TcpStream, head: &Head, shared: &Shared, stop: &AtomicBool) {
    let Some(key) = head.header("sec-websocket-key") else {
        return http::respond(&mut stream, 400, &json!({"code": "bad_request", "message": "A WebSocket upgrade is required."}));
    };
    let accept = derive_accept_key(key.as_bytes());
    let reply = format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n");
    if stream.write_all(reply.as_bytes()).and_then(|()| stream.flush()).is_err() {
        return;
    }
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_millis(20)));
    let mut ws = WebSocket::from_raw_socket(stream, Role::Server, Some(WebSocketConfig::default().max_message_size(Some(MAX_FRAME)).max_frame_size(Some(MAX_FRAME))));
    let mut socket = Socket::new(shared.clone());
    let started = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        let step = match ws.read() {
            Ok(Message::Text(text)) => socket.on_text(text.as_str()),
            Ok(Message::Binary(bytes)) => socket.on_binary(&bytes),
            Ok(Message::Close(_)) => break,
            Ok(_) => Ok(vec![]),
            Err(Error::Io(e)) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => Ok(vec![]),
            Err(_) => break,
        };
        // Any traffic, pings included, must not keep an unauthenticated socket past the deadline.
        let step = match step {
            Ok(_) if socket.keyed_late(started) => Err(Close::new(4002, "E2EE handshake timeout")),
            other => other,
        };
        let step = step.map(|mut out| {
            out.extend(socket.poll());
            out
        });
        match step {
            Ok(out) => {
                if out.into_iter().any(|m| ws.send(m).is_err()) {
                    break;
                }
                let _ = ws.flush();
            }
            Err(close) => {
                for m in close.send {
                    let _ = ws.send(m);
                }
                let _ = ws.close(Some(CloseFrame { code: CloseCode::from(close.code), reason: close.reason.into() }));
                let _ = ws.flush();
                break;
            }
        }
    }
    socket.finish();
}

/// What ends a socket: messages to send first, then the close frame.
struct Close {
    send: Vec<Message>,
    code: u16,
    reason: &'static str,
}

impl Close {
    fn new(code: u16, reason: &'static str) -> Self {
        Self { send: vec![], code, reason }
    }
}

type Step = Result<Vec<Message>, Close>;

/// The key schedule of the socket, whichever framing the phone's hello chose.
enum Keyed {
    V2(Box<DesktopSession>),
    Legacy(LegacyChannel),
}

enum Stage {
    Hello,
    Auth(Keyed),
    Ready { keyed: Keyed, connection: Connection, failures: u32 },
}

struct Socket {
    shared: Shared,
    stage: Stage,
}

impl Socket {
    fn new(shared: Shared) -> Self {
        Self { shared, stage: Stage::Hello }
    }

    fn keyed_late(&self, started: Instant) -> bool {
        !matches!(self.stage, Stage::Ready { .. }) && started.elapsed() > self.shared.handshake_deadline
    }

    fn finish(&mut self) {
        if let Stage::Ready { connection, .. } = &mut self.stage {
            connection.close();
        }
    }

    fn seal(&mut self, text: &str) -> Option<Message> {
        let entropy = self.shared.entropy.clone();
        let keyed = match &mut self.stage {
            Stage::Auth(k) | Stage::Ready { keyed: k, .. } => k,
            Stage::Hello => return None,
        };
        let sealed = match keyed {
            Keyed::V2(s) => s.seal_text(text)?,
            Keyed::Legacy(c) => c.seal_text(entropy.as_ref(), text),
        };
        Some(Message::text(sealed))
    }

    fn on_text(&mut self, text: &str) -> Step {
        match self.stage {
            Stage::Hello => self.hello(text),
            Stage::Auth(_) => self.auth(text),
            Stage::Ready { .. } => self.request(text),
        }
    }

    fn on_binary(&mut self, bytes: &[u8]) -> Step {
        let (keyed, failures) = match &mut self.stage {
            Stage::Ready { keyed, failures, .. } => (keyed, failures),
            Stage::Auth(keyed) => {
                let opens = match keyed {
                    Keyed::V2(s) => s.open_binary(bytes).is_ok(),
                    Keyed::Legacy(c) => c.open_bytes(bytes).is_some(),
                };
                return Err(if opens { Close::new(4001, "Invalid binary message before authentication") } else { Close::new(4001, "Unauthorized") });
            }
            Stage::Hello => return Err(Close::new(4001, "Unauthorized")),
        };
        let opened = match keyed {
            Keyed::V2(s) => s.open_binary(bytes).ok(),
            Keyed::Legacy(c) => c.open_bytes(bytes),
        };
        match opened {
            // No area takes binary streams yet: the frame is opened, so the counter stays in step, and dropped.
            Some(_) => *failures = 0,
            None => return Self::failure(failures),
        }
        Ok(vec![])
    }

    fn failure(failures: &mut u32) -> Step {
        *failures += 1;
        if *failures >= MAX_DECRYPT_FAILURES {
            Err(Close::new(4003, "Too many decryption failures"))
        } else {
            Ok(vec![])
        }
    }

    fn hello(&mut self, text: &str) -> Step {
        let hello: Value = match serde_json::from_str(text) {
            Ok(v @ Value::Object(_)) => v,
            _ => return Err(Close::new(4001, "Invalid handshake message")),
        };
        if hello["type"] == "e2ee_hello" && hello["v"] == 2 {
            let session = DesktopSession::accept(&hello, &self.shared.keys, &Transport::Direct, self.shared.entropy.as_ref()).map_err(|_| Close::new(4001, "Invalid e2ee_hello v2"))?;
            let ready = Message::text(session.ready.to_string());
            self.stage = Stage::Auth(Keyed::V2(Box::new(session)));
            return Ok(vec![ready]);
        }
        let Some(public) = hello["publicKeyB64"].as_str().filter(|_| hello["type"] == "e2ee_hello") else {
            return Err(Close::new(4001, "Invalid e2ee_hello"));
        };
        let channel = LegacyChannel::new(&self.shared.keys.secret, public).ok_or_else(|| Close::new(4001, "Invalid public key"))?;
        self.stage = Stage::Auth(Keyed::Legacy(channel));
        Ok(vec![Message::text(json!({"type": "e2ee_ready"}).to_string())])
    }

    fn auth(&mut self, text: &str) -> Step {
        let Stage::Auth(keyed) = &mut self.stage else { return Ok(vec![]) };
        let (plain, hash) = match keyed {
            Keyed::V2(s) => (s.open_text(text).ok(), Some(s.transcript_hash_b64().to_string())),
            Keyed::Legacy(c) => (c.open_text(text), None),
        };
        // A frame that does not open cannot recover on this socket: the phone's bounded retry reconnects.
        let plain = plain.ok_or_else(|| Close::new(4001, "Unauthorized"))?;
        match authenticate(&plain, hash.as_deref(), self.shared.devices.as_ref()) {
            Err(failure) => {
                let sealed = self.seal(&failure.sealed_error().to_string());
                Err(Close { send: sealed.into_iter().collect(), code: 4001, reason: failure.close_reason() })
            }
            Ok(Authenticated { device, client_capabilities }) => {
                let reply = match &hash {
                    Some(h) => json!({"type": "e2ee_authenticated", "v": 2, "transcriptHashB64": h}),
                    None => json!({"type": "e2ee_authenticated"}),
                };
                let connection = self.shared.host.connection(&device.id, &device.token, capabilities_of(client_capabilities));
                let Stage::Auth(keyed) = std::mem::replace(&mut self.stage, Stage::Hello) else { return Ok(vec![]) };
                self.stage = Stage::Ready { keyed, connection, failures: 0 };
                Ok(self.seal(&reply.to_string()).into_iter().collect())
            }
        }
    }

    fn request(&mut self, text: &str) -> Step {
        let Stage::Ready { keyed, failures, .. } = &mut self.stage else { return Ok(vec![]) };
        let plain = match keyed {
            Keyed::V2(s) => s.open_text(text).ok(),
            Keyed::Legacy(c) => c.open_text(text),
        };
        let Some(plain) = plain else { return Self::failure(failures) };
        *failures = 0;
        let Stage::Ready { connection, .. } = &mut self.stage else { return Ok(vec![]) };
        let frames = connection.on_message(&plain);
        self.seal_all(frames)
    }

    fn poll(&mut self) -> Vec<Message> {
        let Stage::Ready { connection, .. } = &mut self.stage else { return vec![] };
        let frames = connection.poll();
        self.seal_all(frames).unwrap_or_default()
    }

    // A spent counter cannot seal: the socket is re-keyed by reconnecting.
    fn seal_all(&mut self, frames: Vec<String>) -> Step {
        frames.iter().map(|f| self.seal(f).ok_or_else(|| Close::new(1013, "Outbound reply buffer overflow"))).collect()
    }
}

/// Capabilities a legacy auth names: a list of at most 64 strings, else none (the reference's parse).
fn capabilities_of(value: Option<Value>) -> Vec<String> {
    let Some(Value::Array(list)) = value.filter(|v| v.as_array().is_some_and(|a| a.len() <= 64)) else { return vec![] };
    let strings: Vec<String> = list.iter().filter_map(|c| c.as_str().filter(|s| !s.is_empty() && s.len() <= 128).map(str::to_string)).collect();
    if strings.len() == list.len() {
        strings
    } else {
        vec![]
    }
}
