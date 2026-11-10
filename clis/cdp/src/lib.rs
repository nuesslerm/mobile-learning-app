//! A Chrome DevTools Protocol client: a tungstenite WebSocket to the browser, JSON-RPC request/response matching with events
//! queued. No async runtime; DevTools endpoints are loopback.
pub mod tiled;
pub mod attached;
pub mod launch;
pub mod record;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use base64::engine::DecodePaddingMode;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{http::Uri, Message, WebSocket};

/// Standard base64 that reads with or without padding, which is how the browser's payloads and the callers' text arrive.
pub const BASE64: GeneralPurpose = GeneralPurpose::new(&base64::alphabet::STANDARD, GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent));

/// Messages the browser sent that are not replies to a command (events), oldest first. Bounded: a long-lived
/// connection that nobody drains does not grow without limit.
const EVENT_QUEUE_LIMIT: usize = 10_000;

/// Answers a `Fetch.requestPaused` event on its own thread: the page is blocked until it does.
pub type PausedHandler = Arc<dyn Fn(Value, Fire) + Send + Sync>;
/// Receives each `Debugger.scriptParsed` event inline on the reader thread (it must only record) and each `Debugger.paused`
/// event on its own thread (a paused page needs commands answered).
pub type DebugHandler = Arc<dyn Fn(Value) + Send + Sync>;

struct Shared {
    pending: Mutex<HashMap<u64, mpsc::Sender<Value>>>,
    events: Mutex<VecDeque<Value>>,
    paused: Mutex<Option<PausedHandler>>,
    debug: Mutex<Option<DebugHandler>>,
}

/// One end of the socket. The reader and the writer each hold a clone of the stream, and everything either writes goes through one
/// lock, so a command and the pong tungstenite answers a ping with are never interleaved.
struct Wire {
    stream: TcpStream,
    writes: Arc<Mutex<()>>,
}

impl Read for Wire {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.stream.read(buf)
    }
}

impl Write for Wire {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _turn = self.writes.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        self.stream.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.stream.flush()
    }
}

type Writer = Arc<Mutex<WebSocket<Wire>>>;

/// Sends commands without waiting for the reply, from any thread (the answer to a paused request).
#[derive(Clone)]
pub struct Fire {
    writer: Writer,
    next_id: Arc<AtomicU64>,
}

impl Fire {
    pub fn send(&self, method: &str, params: Value, session: Option<&str>) -> Result<(), String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let mut request = json!({ "id": id, "method": method, "params": params });
        if let Some(s) = session {
            request["sessionId"] = json!(s);
        }
        send_frame(&self.writer, &request.to_string())
    }
}

fn send_frame(writer: &Mutex<WebSocket<Wire>>, text: &str) -> Result<(), String> {
    writer.lock().map_err(|_| "connection poisoned".to_string())?.send(Message::text(text)).map_err(|e| e.to_string())
}

/// A DevTools connection. A reader thread owns the receiving side, so replies reach the caller that is waiting for them
/// and events are queued whether or not a command is in flight (page bindings, execution contexts).
#[derive(Clone)]
pub struct Cdp {
    writer: Writer,
    next_id: Arc<AtomicU64>,
    shared: Arc<Shared>,
}

fn http_get(host: &str, port: u16, path: &str) -> Result<String, String> {
    let response = cli_http::Request::get(&format!("http://{host}:{port}{path}")).direct().timeout(Duration::from_secs(10)).send().map_err(|e| format!("cannot reach {host}:{port}: {e}"))?;
    if !response.ok() {
        return Err(format!("{path} answered HTTP {}", response.status));
    }
    Ok(response.text())
}

/// `/json/version` and `/json/list` of a DevTools endpoint, parsed.
pub fn json_endpoint(host: &str, port: u16, path: &str) -> Result<Value, String> {
    serde_json::from_str(&http_get(host, port, path)?).map_err(|e| format!("{path} is not JSON: {e}"))
}

/// The host and port of a `ws://` or `http://` URL.
pub(crate) fn host_and_port(url: &str) -> Result<(String, u16), String> {
    let uri: Uri = url.parse().map_err(|e| format!("not a URL: {url} ({e})"))?;
    let host = uri.host().ok_or_else(|| format!("no host in {url}"))?;
    let port = uri.port_u16().ok_or_else(|| format!("no port in {url}"))?;
    Ok((host.to_string(), port))
}

impl Cdp {
    pub fn connect(ws_url: &str) -> Result<Cdp, String> {
        if !ws_url.starts_with("ws://") {
            return Err(format!("not a ws:// URL: {ws_url}"));
        }
        let (host, port) = host_and_port(ws_url)?;
        let stream = TcpStream::connect((host.as_str(), port)).map_err(|e| format!("cannot reach {ws_url}: {e}"))?;
        stream.set_read_timeout(Some(Duration::from_secs(30))).ok();
        let writes = Arc::new(Mutex::new(()));
        let wire = |stream: &TcpStream| -> Result<Wire, String> { Ok(Wire { stream: stream.try_clone().map_err(|e| e.to_string())?, writes: Arc::clone(&writes) }) };
        let config = WebSocketConfig::default().max_message_size(None).max_frame_size(None);
        let (mut reader, _) = tungstenite::client::client_with_config(ws_url, wire(&stream)?, Some(config)).map_err(|e| format!("WebSocket upgrade refused: {e}"))?;
        let writer: Writer = Arc::new(Mutex::new(WebSocket::from_raw_socket(wire(&stream)?, tungstenite::protocol::Role::Client, Some(config))));
        let shared = Arc::new(Shared { pending: Mutex::new(HashMap::new()), events: Mutex::new(VecDeque::new()), paused: Mutex::new(None), debug: Mutex::new(None) });
        let next_id = Arc::new(AtomicU64::new(1));
        stream.set_read_timeout(None).ok();
        let reader_shared = Arc::clone(&shared);
        let fire = Fire { writer: Arc::clone(&writer), next_id: Arc::clone(&next_id) };
        std::thread::spawn(move || {
            while let Ok(message) = Cdp::read_message(&mut reader) {
                let Ok(value) = serde_json::from_str::<Value>(&message) else { continue };
                match value["id"].as_u64() {
                    Some(id) => {
                        let sender = reader_shared.pending.lock().ok().and_then(|mut p| p.remove(&id));
                        if let Some(tx) = sender {
                            let _ = tx.send(value);
                        }
                    }
                    None => {
                        if value["method"] == "Fetch.requestPaused" || value["method"] == "Fetch.authRequired" {
                            let handler = reader_shared.paused.lock().ok().and_then(|h| h.clone());
                            if let Some(handler) = handler {
                                let fire = fire.clone();
                                std::thread::spawn(move || handler(value, fire));
                                continue;
                            }
                        }
                        if matches!(value["method"].as_str(), Some("Debugger.scriptParsed" | "Debugger.paused")) {
                            let handler = reader_shared.debug.lock().ok().and_then(|h| h.clone());
                            if let Some(handler) = handler {
                                let value = value.clone();
                                if value["method"] == "Debugger.scriptParsed" {
                                    // Registration only (no commands): inline keeps the order, so a caller that has seen
                                    // `Debugger.enable` answered knows every earlier script is already recorded.
                                    handler(value);
                                } else {
                                    std::thread::spawn(move || handler(value));
                                }
                            }
                        }
                        if let Ok(mut q) = reader_shared.events.lock() {
                            if q.len() >= EVENT_QUEUE_LIMIT {
                                q.pop_front();
                            }
                            q.push_back(value);
                        }
                    }
                }
            }
            // Wake every caller still waiting: the connection is gone.
            if let Ok(mut p) = reader_shared.pending.lock() {
                p.clear();
            }
        });
        Ok(Cdp { writer, next_id, shared })
    }

    fn read_message(reader: &mut WebSocket<Wire>) -> Result<String, String> {
        loop {
            match reader.read().map_err(|e| format!("connection lost: {e}"))? {
                Message::Text(text) => return Ok(text.to_string()),
                Message::Binary(bytes) => return String::from_utf8(bytes.to_vec()).map_err(|e| e.to_string()),
                Message::Close(_) => return Err("connection closed by the browser".into()),
                _ => {}
            }
        }
    }

    fn send_text(&self, text: &str) -> Result<(), String> {
        send_frame(&self.writer, text)
    }

    /// Feeds the debugger: its handler sees `Debugger.scriptParsed` and `Debugger.paused` (the events still reach the queue).
    pub fn on_debugger(&self, handler: DebugHandler) {
        if let Ok(mut h) = self.shared.debug.lock() {
            *h = Some(handler);
        }
    }

    /// A handle that sends commands from other threads, and routes `Fetch.requestPaused` events to `handler`.
    pub fn on_paused(&self, handler: PausedHandler) {
        if let Ok(mut h) = self.shared.paused.lock() {
            *h = Some(handler);
        }
    }

    /// Everything the browser sent that was not a reply, oldest first; the queue is emptied.
    pub fn take_events(&self) -> Vec<Value> {
        self.shared.events.lock().map(|mut q| q.drain(..).collect()).unwrap_or_default()
    }

    /// One command on the browser connection.
    pub fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        self.call_on(method, params, None)
    }

    /// Like `call`, addressed to an attached target session (`Target.attachToTarget` with `flatten`).
    pub fn call_on(&self, method: &str, params: Value, session: Option<&str>) -> Result<Value, String> {
        self.call_within(method, params, session, Duration::from_secs(60))
    }

    /// Like `call_on`, waiting at most `wait` for the answer; a late answer is dropped.
    pub fn call_within(&self, method: &str, params: Value, session: Option<&str>, wait: Duration) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let mut request = json!({ "id": id, "method": method, "params": params });
        if let Some(s) = session {
            request["sessionId"] = json!(s);
        }
        let (tx, rx) = mpsc::channel();
        self.shared.pending.lock().map_err(|_| "connection poisoned".to_string())?.insert(id, tx);
        if let Err(e) = self.send_text(&request.to_string()) {
            if let Ok(mut p) = self.shared.pending.lock() {
                p.remove(&id);
            }
            return Err(e);
        }
        let reply = rx.recv_timeout(wait).map_err(|e| {
            if let Ok(mut p) = self.shared.pending.lock() {
                p.remove(&id);
            }
            match e {
                mpsc::RecvTimeoutError::Timeout => format!("{method} timed out"),
                mpsc::RecvTimeoutError::Disconnected => "connection lost".to_string(),
            }
        })?;
        match reply.get("error") {
            Some(e) => Err(format!("{method} failed: {}", e["message"].as_str().unwrap_or("unknown error"))),
            None => Ok(reply["result"].clone()),
        }
    }
}
