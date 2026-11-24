//! The executor's side of an opencode session: `opencode serve` listens on a loopback port under a password this process made and keeps, and the service
//! reaches it only through the session's two actions. `session_send` carries an HTTP request (`exec_proc::SessionHttp`) that is made here with the
//! credential, and the session's output carries the answers and the server's event stream, as one JSON line each. The password is never printed, sent or
//! put in an error: every text that leaves is scrubbed of it.
use base64::Engine;
use exec_proc::SessionHttp;
use exec_proto::{Code, ExecError, ExecResult};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Where a line for the service goes: the session's output.
pub type Sink = Arc<dyn Fn(String) + Send + Sync>;

const USER: &str = "rca";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// A request waits as long as the server takes (the first call in a directory starts its services); the event stream is quiet at most for the heartbeat.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const STREAM_TIMEOUT: Duration = Duration::from_secs(45);
const START_WAIT: Duration = Duration::from_secs(60);
const BODY_MAX: usize = 64 << 20;

pub struct Bridge {
    password: String,
    port: AtomicU16,
    sink: Mutex<Option<Sink>>,
    stream: Mutex<Option<TcpStream>>,
    closed: AtomicBool,
}

impl Bridge {
    pub fn new() -> ExecResult<Arc<Bridge>> {
        let mut raw = [0u8; 24];
        std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut raw)).map_err(|e| ExecError::new(Code::Io, format!("no random password for the server: {e}")))?;
        let password: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        Ok(Arc::new(Bridge { password, port: AtomicU16::new(0), sink: Mutex::new(None), stream: Mutex::new(None), closed: AtomicBool::new(false) }))
    }

    /// The environment the server is started with: its credential and no self-update.
    pub fn env(&self) -> [(&'static str, String); 3] {
        [("OPENCODE_SERVER_USERNAME", USER.into()), ("OPENCODE_SERVER_PASSWORD", self.password.clone()), ("OPENCODE_DISABLE_AUTOUPDATE", "true".into())]
    }

    pub fn scrub(&self, text: &str) -> String {
        text.replace(&self.password, "***")
    }

    fn say(&self, line: Value) {
        let sink = self.sink.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(sink) = sink {
            sink(self.scrub(&line.to_string()));
        }
    }

    fn authorization(&self) -> String {
        format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{USER}:{}", self.password)))
    }

    /// Reads what the server prints until it closes its output, and once it says where it listens opens the event stream and tells the service the
    /// server is ready. Nothing the server prints goes further.
    pub fn watch(self: &Arc<Self>, stdout: impl Read + Send + 'static, sink: Sink) {
        *self.sink.lock().unwrap_or_else(|e| e.into_inner()) = Some(sink);
        let bridge = Arc::clone(self);
        std::thread::spawn(move || {
            let mut events = false;
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let Some(port) = line.split_once("listening on ").and_then(|(_, url)| url.trim().rsplit(':').next().and_then(|p| p.trim_end_matches('/').parse::<u16>().ok())) else { continue };
                if !events {
                    events = true;
                    bridge.port.store(port, Ordering::SeqCst);
                    let bridge = Arc::clone(&bridge);
                    std::thread::spawn(move || bridge.follow_events());
                }
            }
        });
    }

    /// Ends the bridge's own connections: the event stream closes, and no request is made any more.
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.sink.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(stream) = self.stream.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }

    /// Makes the request of `line` on a thread of its own and answers on the session's output; a request the executor will not make is refused here.
    pub fn request(self: &Arc<Self>, line: &str) -> ExecResult<()> {
        let call = SessionHttp::parse(line)?;
        if self.port.load(Ordering::SeqCst) == 0 || self.closed.load(Ordering::SeqCst) {
            return Err(ExecError::new(Code::Io, "the opencode server is not listening"));
        }
        let bridge = Arc::clone(self);
        std::thread::spawn(move || {
            let answer = match bridge.call(&call.method, &call.path, call.body.as_ref()) {
                Ok((status, body)) => json!({"id": call.id, "status": status, "body": body}),
                Err(why) => json!({"id": call.id, "status": 0, "error": why}),
            };
            bridge.say(answer);
        });
        Ok(())
    }

    fn connect(&self, timeout: Duration) -> Result<TcpStream, String> {
        let addr = SocketAddr::from(([127, 0, 0, 1], self.port.load(Ordering::SeqCst)));
        let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(|e| format!("the opencode server does not answer: {e}"))?;
        stream.set_read_timeout(Some(timeout)).and_then(|()| stream.set_write_timeout(Some(REQUEST_TIMEOUT))).map_err(|e| e.to_string())?;
        Ok(stream)
    }

    fn send_head(&self, stream: &mut TcpStream, method: &str, path: &str, body: Option<&str>, accept: &str, close: bool) -> Result<(), String> {
        let port = self.port.load(Ordering::SeqCst);
        let mut head = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: {}\r\nAccept: {accept}\r\n", self.authorization());
        if close {
            head.push_str("Connection: close\r\n");
        }
        if let Some(body) = body {
            head.push_str(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n", body.len()));
        }
        head.push_str("\r\n");
        head.push_str(body.unwrap_or_default());
        stream.write_all(head.as_bytes()).map_err(|e| format!("the request could not be sent: {e}"))
    }

    /// One request to the server and its answer: the status and the body as text.
    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<(u16, String), String> {
        let _span = tracing::info_span!("opencode.http").entered();
        let mut stream = self.connect(REQUEST_TIMEOUT)?;
        self.send_head(&mut stream, method, path, body.map(Value::to_string).as_deref(), "application/json", true)?;
        let mut reader = BufReader::new(stream);
        let (status, chunked, length) = read_head(&mut reader)?;
        let mut payload = Vec::new();
        Body { reader: &mut reader, chunked, length, left: 0, done: false }.take(BODY_MAX as u64).read_to_end(&mut payload).map_err(|e| format!("the answer could not be read: {e}"))?;
        Ok((status, self.scrub(&String::from_utf8_lossy(&payload))))
    }

    /// Waits for the server to answer, opens its event stream, says it is ready, and passes every frame on until the stream ends.
    fn follow_events(&self) {
        let until = Instant::now() + START_WAIT;
        let opened = loop {
            match self.open_stream() {
                Ok(open) => break Ok(open),
                Err(why) if Instant::now() >= until || self.closed.load(Ordering::SeqCst) => break Err(why),
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        };
        let mut reader = match opened {
            Ok(reader) => reader,
            Err(why) => return self.say(json!({"closed": format!("the event stream did not open: {why}")})),
        };
        self.say(json!({"ready": true}));
        let ended = loop {
            let mut data = String::new();
            let more = loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break Err("the server closed it".to_string()),
                    Err(e) => break Err(e.to_string()),
                    Ok(_) => {}
                }
                let line = line.trim_end_matches(['\r', '\n']);
                if line.is_empty() {
                    break Ok(());
                }
                if let Some(text) = line.strip_prefix("data:") {
                    data.push_str(text.trim_start());
                }
            };
            if let Err(why) = more {
                break why;
            }
            if let Ok(frame) = serde_json::from_str::<Value>(&data) {
                if frame["payload"]["type"] != "server.heartbeat" {
                    self.say(json!({"event": frame}));
                }
            }
        };
        if !self.closed.load(Ordering::SeqCst) {
            self.say(json!({"closed": format!("the event stream ended: {ended}")}));
        }
    }

    /// `GET /global/event`, answered `200`; the reader of its decoded body.
    fn open_stream(&self) -> Result<BufReader<Body<BufReader<TcpStream>>>, String> {
        let _span = tracing::info_span!("opencode.events.open").entered();
        let mut stream = self.connect(STREAM_TIMEOUT)?;
        self.send_head(&mut stream, "GET", "/global/event", None, "text/event-stream", false)?;
        let kept = stream.try_clone().map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(stream);
        let (status, chunked, length) = read_head(&mut reader)?;
        if status != 200 {
            return Err(format!("it answered {status}"));
        }
        *self.stream.lock().unwrap_or_else(|e| e.into_inner()) = Some(kept);
        if self.closed.load(Ordering::SeqCst) {
            self.close();
        }
        Ok(BufReader::new(Body { reader, chunked, length, left: 0, done: false }))
    }
}

/// The status line and the headers of a response: the status, whether the body is chunked, and its length when it has one.
fn read_head<R: BufRead>(reader: &mut R) -> Result<(u16, bool, Option<usize>), String> {
    let mut status = String::new();
    reader.read_line(&mut status).map_err(|e| format!("no answer: {e}"))?;
    let code = status.split_whitespace().nth(1).and_then(|c| c.parse::<u16>().ok()).ok_or_else(|| format!("the answer is no HTTP: {:?}", status.trim_end()))?;
    let (mut chunked, mut length) = (false, None);
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).map_err(|e| format!("no answer: {e}"))? == 0 {
            return Err("the answer ended in its headers".into());
        }
        let header = header.trim_end().to_ascii_lowercase();
        if header.is_empty() {
            return Ok((code, chunked, length));
        }
        if let Some(value) = header.strip_prefix("content-length:") {
            length = value.trim().parse().ok();
        }
        chunked |= header.starts_with("transfer-encoding:") && header.contains("chunked");
    }
}

/// A response body as the bytes it carries: de-chunked, cut at its length, or read to the end of the connection.
struct Body<R> {
    reader: R,
    chunked: bool,
    length: Option<usize>,
    left: usize,
    done: bool,
}

impl<R: BufRead> Read for Body<R> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.done || out.is_empty() {
            return Ok(0);
        }
        if self.chunked {
            if self.left == 0 {
                let mut size = String::new();
                if self.reader.read_line(&mut size)? == 0 {
                    self.done = true;
                    return Ok(0);
                }
                let size = size.trim();
                if size.is_empty() {
                    // the CRLF that ends the previous chunk
                    return self.read(out);
                }
                self.left = usize::from_str_radix(size.split(';').next().unwrap_or("0"), 16).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                if self.left == 0 {
                    self.done = true;
                    return Ok(0);
                }
            }
            let take = out.len().min(self.left);
            let read = self.reader.read(&mut out[..take])?;
            self.left -= read;
            if read == 0 {
                self.done = true;
            }
            return Ok(read);
        }
        let take = match self.length {
            Some(length) => {
                if length == 0 {
                    self.done = true;
                    return Ok(0);
                }
                out.len().min(length)
            }
            None => out.len(),
        };
        let read = self.reader.read(&mut out[..take])?;
        if let Some(length) = &mut self.length {
            *length -= read;
        }
        if read == 0 {
            self.done = true;
        }
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_server_that_has_not_said_where_it_listens_when_a_request_on_a_route_is_made_then_it_is_refused_as_not_listening() {
        // Given a bridge whose server has printed no port
        let bridge = Bridge::new().unwrap();

        // When a request on a route the adapter drives is made
        let refused = bridge.request(r#"{"id":1,"method":"GET","path":"/global/health"}"#).unwrap_err();

        // Then it is refused as the server not listening, and nothing was sent
        assert_eq!(refused.code, Code::Io);
        assert!(refused.message.contains("not listening"), "{}", refused.message);
    }
}
