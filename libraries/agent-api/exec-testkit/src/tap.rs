//! A recording tap between a client of the rca service and the service: a WebSocket proxy the test points the mapper (`--rca-url`) and the
//! executor (`rca-exec serve --rca-url`) at. It forwards every frame both ways untouched, so the processes talk as they would to the service,
//! and keeps what it saw as the facts a method-hit trace is made of: each agent API call with whether the service answered it, each executor
//! tool the service asked with whether the executor answered `ok`. A refused handshake reaches the client as the service's own refusal.
use serde_json::Value;
use std::collections::HashMap;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use tungstenite::client::IntoClientRequest;
use tungstenite::handshake::server::{ErrorResponse, Request as HttpRequest, Response as HttpResponse};
use tungstenite::http::{HeaderName, HeaderValue};
use tungstenite::{Error, HandshakeError, Message, WebSocket};

/// The service's agent socket and executor channel, which tell the tap which way a frame reads.
pub const AGENT_PATH: &str = "/agent/v1/socket";
pub const EXECUTOR_PATH: &str = "/exec/v2/executors";

/// How long a read waits before the tap looks at the other side.
const POLL: Duration = Duration::from_millis(5);

/// A call that went through the tap and was answered.
#[derive(Debug, Clone, PartialEq)]
pub enum Seen {
    /// An agent API method, answered with a result (`ok`) or an error (`code`).
    Agent { method: String, ok: bool, code: Option<String> },
    /// An executor tool the service asked, answered `ok` or with an error (`code`).
    Executor { tool: String, ok: bool, code: Option<String> },
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Channel {
    Agent,
    Executor,
    Other,
}

impl Channel {
    fn of(path: &str) -> Channel {
        match path {
            AGENT_PATH => Channel::Agent,
            EXECUTOR_PATH => Channel::Executor,
            _ => Channel::Other,
        }
    }
}

/// Which direction a frame travels, from the connection's client.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Way {
    ToService,
    ToClient,
}

/// Pairs the frames of one connection: a request waits for its answer by id, and the pair is what was seen.
struct Observer {
    channel: Channel,
    pending: HashMap<String, String>,
}

impl Observer {
    fn new(channel: Channel) -> Observer {
        Observer { channel, pending: HashMap::new() }
    }

    fn see(&mut self, way: Way, text: &str) -> Option<Seen> {
        let frame: Value = serde_json::from_str(text).ok()?;
        let id = frame.get("id")?.to_string();
        let code = |frame: &Value| frame["error"]["code"].as_str().map(str::to_string);
        match (self.channel, way) {
            // the agent API: the mapper asks `{id, method, params}`, the service answers `{id, result}` or `{id, error}`
            (Channel::Agent, Way::ToService) => {
                self.pending.insert(id, frame.get("method")?.as_str()?.to_string());
                None
            }
            (Channel::Agent, Way::ToClient) if frame.get("result").is_some() || frame.get("error").is_some() => {
                let method = self.pending.remove(&id)?;
                Some(Seen::Agent { method, ok: frame.get("result").is_some() && frame.get("error").is_none(), code: code(&frame) })
            }
            // the executor channel: the service asks `{id, tool, args}`, the executor streams output and ends with `{id, ok, ...}`
            (Channel::Executor, Way::ToClient) => {
                self.pending.insert(id, frame.get("tool")?.as_str()?.to_string());
                None
            }
            (Channel::Executor, Way::ToService) => {
                let ok = frame.get("ok")?.as_bool()?;
                let tool = self.pending.remove(&id)?;
                Some(Seen::Executor { tool, ok, code: code(&frame) })
            }
            _ => None,
        }
    }
}

pub struct Tap {
    addr: SocketAddr,
    log: Arc<Mutex<Vec<Seen>>>,
    stop: Arc<AtomicBool>,
    accepting: Option<JoinHandle<()>>,
}

impl Tap {
    /// Listens on a free loopback port and forwards every WebSocket connection to the service at `service`.
    pub fn to(service: SocketAddr) -> Tap {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port for the tap");
        listener.set_nonblocking(true).expect("a non-blocking listener");
        let addr = listener.local_addr().expect("the tap's address");
        let (log, stop) = (Arc::new(Mutex::new(Vec::new())), Arc::new(AtomicBool::new(false)));
        let (seen, halt) = (log.clone(), stop.clone());
        let accepting = std::thread::spawn(move || {
            while !halt.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((client, _)) => {
                        let (seen, halt) = (seen.clone(), halt.clone());
                        std::thread::spawn(move || {
                            // a connection the service refuses or that breaks ends here; the client sees the same
                            let _ = relay(client, service, &seen, &halt);
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(POLL),
                    Err(_) => return,
                }
            }
        });
        Tap { addr, log, stop, accepting: Some(accepting) }
    }

    /// `http://127.0.0.1:<port>`: what `--rca-url` takes.
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Where the log stands now; `since` gives what came after.
    pub fn mark(&self) -> usize {
        self.log.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn since(&self, mark: usize) -> Vec<Seen> {
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

type Socket = WebSocket<TcpStream>;

/// The headers of a client's handshake that belong to the connection and not to the request: the proxy's own handshake sets them again.
fn is_connection_header(name: &str) -> bool {
    matches!(name, "host" | "connection" | "upgrade" | "content-length") || name.starts_with("sec-websocket-")
}

/// Opens the connection to the service as the client asked (its path and headers), or the service's refusal as the client's answer.
// tungstenite's handshake callback fixes the error type: a whole HTTP response
#[allow(clippy::result_large_err)]
fn upstream(service: SocketAddr, request: &HttpRequest) -> Result<Socket, ErrorResponse> {
    let refuse = |status: u16, reason: String| {
        let mut response = ErrorResponse::new(Some(reason));
        *response.status_mut() = tungstenite::http::StatusCode::from_u16(status).unwrap_or(tungstenite::http::StatusCode::BAD_GATEWAY);
        response
    };
    let target = request.uri().path_and_query().map_or("/", |p| p.as_str());
    let mut forwarded = format!("ws://{service}{target}").into_client_request().map_err(|e| refuse(502, e.to_string()))?;
    for (name, value) in request.headers() {
        if !is_connection_header(name.as_str()) {
            forwarded.headers_mut().append(HeaderName::from(name), HeaderValue::from(value));
        }
    }
    let stream = TcpStream::connect(service).map_err(|e| refuse(502, format!("the service is not reachable: {e}")))?;
    match tungstenite::client(forwarded, stream) {
        Ok((socket, _)) => Ok(socket),
        Err(HandshakeError::Failure(Error::Http(response))) => Err(response.map(|body| body.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()))),
        Err(other) => Err(refuse(502, other.to_string())),
    }
}

// the handshake callback below returns tungstenite's `ErrorResponse`
#[allow(clippy::result_large_err)]
fn relay(client: TcpStream, service: SocketAddr, log: &Mutex<Vec<Seen>>, stop: &AtomicBool) -> Result<(), String> {
    client.set_nonblocking(false).map_err(|e| e.to_string())?;
    let (mut service_side, mut channel) = (None, Channel::Other);
    let mut down = tungstenite::accept_hdr(client, |request: &HttpRequest, response: HttpResponse| -> Result<HttpResponse, ErrorResponse> {
        channel = Channel::of(request.uri().path());
        service_side = Some(upstream(service, request)?);
        Ok(response)
    })
    .map_err(|e| e.to_string())?;
    let mut up = service_side.ok_or("the service connection is gone")?;
    for socket in [&down, &up] {
        socket.get_ref().set_read_timeout(Some(POLL)).map_err(|e| e.to_string())?;
    }
    let mut observer = Observer::new(channel);
    while !stop.load(Ordering::SeqCst) {
        let moved_up = pass(&mut down, &mut up, |text| observer.see(Way::ToService, text), log);
        let moved_down = pass(&mut up, &mut down, |text| observer.see(Way::ToClient, text), log);
        if moved_up == Moved::Closed || moved_down == Moved::Closed {
            break;
        }
    }
    Ok(())
}

#[derive(Debug, PartialEq)]
enum Moved {
    Nothing,
    Frame,
    Closed,
}

/// Moves the frame that is waiting on `from`, if one is, to `to`, and keeps what `see` makes of a text frame.
fn pass(from: &mut Socket, to: &mut Socket, mut see: impl FnMut(&str) -> Option<Seen>, log: &Mutex<Vec<Seen>>) -> Moved {
    let message = match from.read() {
        Ok(message) => message,
        Err(Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => return Moved::Nothing,
        Err(_) => return Moved::Closed,
    };
    match message {
        Message::Text(text) => {
            if let Some(seen) = see(text.as_str()) {
                log.lock().unwrap_or_else(|e| e.into_inner()).push(seen);
            }
            send(to, Message::Text(text))
        }
        Message::Binary(bytes) => send(to, Message::Binary(bytes)),
        Message::Close(frame) => {
            let _ = to.send(Message::Close(frame));
            Moved::Closed
        }
        // each side answers the other's pings itself
        Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => Moved::Frame,
    }
}

fn send(to: &mut Socket, message: Message) -> Moved {
    match to.send(message) {
        Ok(()) => Moved::Frame,
        Err(_) => Moved::Closed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn seen(channel: Channel, frames: &[(Way, Value)]) -> Vec<Seen> {
        let mut observer = Observer::new(channel);
        frames.iter().filter_map(|(way, frame)| observer.see(*way, &frame.to_string())).collect()
    }

    #[test]
    fn given_agent_calls_when_they_are_answered_then_each_is_seen_once_with_its_outcome_and_events_and_unanswered_calls_are_not() {
        // Given a call answered with a result, one answered with an error, an event, and a call nobody answered
        let frames = [
            (Way::ToService, json!({"id": 1, "method": "thread.create", "params": {}})),
            (Way::ToService, json!({"id": 2, "method": "turn.start", "params": {}})),
            (Way::ToClient, json!({"id": 2, "error": {"code": "TURN_RUNNING", "message": "busy"}})),
            (Way::ToClient, json!({"event": "turn.started", "topic": "thread/t1", "seq": 3})),
            (Way::ToClient, json!({"id": 1, "result": {"thread": "t1"}})),
            (Way::ToService, json!({"id": 3, "method": "thread.get", "params": {}})),
        ];
        // When the frames pass
        let log = seen(Channel::Agent, &frames);
        // Then the two answered calls are seen in the order they were answered, and the third is not
        assert_eq!(log, [Seen::Agent { method: "turn.start".into(), ok: false, code: Some("TURN_RUNNING".into()) }, Seen::Agent { method: "thread.create".into(), ok: true, code: None }]);
    }

    #[test]
    fn given_executor_tools_when_the_executor_answers_then_the_tool_is_seen_with_ok_and_its_output_frames_are_not_results() {
        // Given a tool the service asks, its output, its result, and a tool that fails
        let frames = [
            (Way::ToClient, json!({"id": "a", "tool": "write_file", "args": {"path": "n.txt"}})),
            (Way::ToService, json!({"id": "a", "stream": "stdout", "data": "x"})),
            (Way::ToService, json!({"id": "a", "ok": true, "result": {}})),
            (Way::ToClient, json!({"id": "b", "tool": "read_file", "args": {}})),
            (Way::ToService, json!({"id": "b", "ok": false, "error": {"code": "NOT_FOUND", "message": "no"}})),
        ];
        // When the frames pass
        let log = seen(Channel::Executor, &frames);
        // Then each tool is seen once, at its result frame
        assert_eq!(log, [Seen::Executor { tool: "write_file".into(), ok: true, code: None }, Seen::Executor { tool: "read_file".into(), ok: false, code: Some("NOT_FOUND".into()) }]);
    }

    #[test]
    fn given_a_connection_that_is_neither_channel_when_frames_pass_then_nothing_is_seen() {
        let frames = [(Way::ToService, json!({"id": 1, "method": "x"})), (Way::ToClient, json!({"id": 1, "result": {}}))];
        assert!(seen(Channel::Other, &frames).is_empty());
    }
}
