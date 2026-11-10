//! The blocking client. One socket, driven by whoever holds `&mut Client`: a call writes its request and reads until the answer with its id
//! (events that arrive meanwhile are queued), and `next_event` reads until an event is queued or its time is up. A dropped socket is reopened
//! the next time the client is used, and every subscription is opened again from the last `seq` it saw; events the service replays that the
//! client already has are dropped, so a caller sees each event of a topic once, in order.
use crate::connection;
use crate::error::{code, Error};
use agent_api_proto::{Event, Incoming, Method, Request, Response};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::net::TcpStream;
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket};

/// How long to wait between attempts to reopen a dropped socket, and when to give up.
#[derive(Debug, Clone, PartialEq)]
pub struct Backoff {
    /// The wait before the second attempt; it doubles for each one after, up to `max`. The first attempt is at once.
    pub initial: Duration,
    pub max: Duration,
    /// Attempts per reconnect. After them the client answers `DISCONNECTED` and tries again the next time it is used.
    pub attempts: u32,
}

impl Default for Backoff {
    fn default() -> Backoff {
        Backoff { initial: Duration::from_millis(200), max: Duration::from_secs(5), attempts: 8 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// `ws://host:port` of the service; the client opens `/agent/v1/socket` there.
    pub url: String,
    pub bearer: String,
    /// How long a call waits for its answer.
    pub call_timeout: Duration,
    /// How long opening the socket, handshake included, may take.
    pub connect_timeout: Duration,
    pub backoff: Backoff,
}

impl Config {
    pub fn new(url: impl Into<String>, bearer: impl Into<String>) -> Config {
        Config { url: url.into(), bearer: bearer.into(), call_timeout: Duration::from_secs(30), connect_timeout: Duration::from_secs(10), backoff: Backoff::default() }
    }
}

/// A stream of events a client keeps open: the request that opens it and which of its params carries the position to resume from. The
/// client knows no method: the caller names the one that subscribes (for a thread, `thread.subscribe` with `after_seq`).
#[derive(Debug, Clone, PartialEq)]
pub struct Subscribe {
    topic: String,
    method: String,
    params: Value,
    seq_param: String,
}

impl Subscribe {
    /// `topic` is the stream's name as its events carry it (`Event::topic`). `params[seq_param]`, when set, is where the first subscription
    /// starts; every later one starts after the last seq the client saw.
    pub fn new(topic: impl Into<String>, method: Method, params: Value, seq_param: &str) -> Subscribe {
        Subscribe { topic: topic.into(), method: method.name().to_string(), params, seq_param: seq_param.to_string() }
    }

    pub fn topic(&self) -> &str {
        &self.topic
    }

    fn start(&self) -> u64 {
        self.params.get(&self.seq_param).and_then(Value::as_u64).unwrap_or(0)
    }

    fn params_after(&self, seq: u64) -> Value {
        let mut params = self.params.clone();
        if let Some(object) = params.as_object_mut() {
            object.insert(self.seq_param.clone(), json!(seq));
        }
        params
    }
}

struct Open {
    spec: Subscribe,
    last_seq: u64,
}

type Socket = WebSocket<TcpStream>;

pub struct Client {
    config: Config,
    socket: Option<Socket>,
    next_id: u64,
    queue: VecDeque<Event>,
    subscriptions: Vec<Open>,
}

/// What reading the socket for a while found.
enum Read {
    Frame(Incoming),
    Quiet,
}

impl Client {
    /// Opens the socket once: an unreachable service or a refused bearer is the error, not something to retry here.
    pub fn connect(config: Config) -> Result<Client, Error> {
        let socket = connection::open(&config.url, &config.bearer, config.connect_timeout)?;
        Ok(Client { config, socket: Some(socket), next_id: 1, queue: VecDeque::new(), subscriptions: Vec::new() })
    }

    pub fn is_connected(&self) -> bool {
        self.socket.is_some()
    }

    /// The last seq the client saw of a subscribed topic.
    pub fn last_seq(&self, topic: &str) -> Option<u64> {
        self.subscriptions.iter().find(|s| s.spec.topic == topic).map(|s| s.last_seq)
    }

    /// Sends one request and returns its result, or the error the service answered with (its own code), `TIMEOUT` when no answer came within
    /// `call_timeout`, and `DISCONNECTED` when the socket dropped before the answer: the request was written and is not sent again. A socket
    /// that had dropped earlier is reopened first.
    pub fn call(&mut self, method: Method, params: Value) -> Result<Value, Error> {
        self.call_named(method.name(), params)
    }

    fn call_named(&mut self, method: &str, params: Value) -> Result<Value, Error> {
        self.ensure_open()?;
        self.exchange(method, params)
    }

    /// The call itself, on the socket as it is.
    fn exchange(&mut self, method: &str, params: Value) -> Result<Value, Error> {
        let id = self.next_id;
        self.next_id += 1;
        let request = Request { id: json!(id), method: method.to_string(), params };
        let Some(socket) = self.socket.as_mut() else { return Err(Error::new(code::DISCONNECTED, "the socket is closed")) };
        if let Err(e) = socket.send(Message::Text(request.encode().into())) {
            self.drop_socket(&format!("writing {method} failed: {e}"));
            return Err(Error::new(code::DISCONNECTED, format!("writing {method} failed, so it may have been received, and is not sent again: {e}")));
        }
        let deadline = Instant::now() + self.config.call_timeout;
        loop {
            match self.read_until(deadline)? {
                Read::Quiet => return Err(Error::new(code::TIMEOUT, format!("{method} was not answered in {:?}", self.config.call_timeout))),
                Read::Frame(Incoming::Event(event)) => self.take(event),
                // An answer to a call that timed out: nobody waits for it
                Read::Frame(Incoming::Response(answer)) if answer.id() != &json!(id) => {}
                Read::Frame(Incoming::Response(Response::Result { result, .. })) => return Ok(result),
                Read::Frame(Incoming::Response(Response::Error { error, .. })) => return Err(Error::new(&error.code, error.message)),
            }
        }
    }

    /// Opens a stream and keeps it: the service's answer is returned, and from then on the stream's events come from `next_event`, resumed
    /// after the last seq seen whenever the socket is reopened. A refused subscription is not kept.
    pub fn subscribe(&mut self, spec: Subscribe) -> Result<Value, Error> {
        self.ensure_open()?;
        self.subscriptions.retain(|s| s.spec.topic != spec.topic);
        let (method, params, start) = (spec.method.clone(), spec.params_after(spec.start()), spec.start());
        // Kept before the request goes, so the replay that may precede the answer is already counted against it
        self.subscriptions.push(Open { spec: spec.clone(), last_seq: start });
        let answer = self.exchange(&method, params);
        if answer.as_ref().is_err_and(|e| e.from_service()) {
            self.subscriptions.retain(|s| s.spec.topic != spec.topic);
        }
        answer
    }

    /// Stops following a topic and drops its queued events. The service is told by the caller's own method; false when it was not followed.
    pub fn unsubscribe(&mut self, topic: &str) -> bool {
        let before = self.subscriptions.len();
        self.subscriptions.retain(|s| s.spec.topic != topic);
        self.queue.retain(|e| e.topic().as_deref() != Some(topic));
        self.subscriptions.len() != before
    }

    /// The next event, waiting up to `wait` for it: none when none came. A dropped socket is reopened and the subscriptions resumed while
    /// waiting; when that fails for good the error is `DISCONNECTED`, and the next call tries again.
    pub fn next_event(&mut self, wait: Duration) -> Result<Option<Event>, Error> {
        let deadline = Instant::now() + wait;
        loop {
            if let Some(event) = self.queue.pop_front() {
                return Ok(Some(event));
            }
            self.ensure_open()?;
            match self.read_until(deadline) {
                Ok(Read::Quiet) => return Ok(None),
                Ok(Read::Frame(Incoming::Event(event))) => self.take(event),
                Ok(Read::Frame(Incoming::Response(_))) => {}
                // The socket dropped: the next turn of the loop reopens it
                Err(e) if e.code == code::DISCONNECTED => {}
                Err(e) => return Err(e),
            }
        }
    }

    /// Counts an event against its topic: one the client already has is a replay and is dropped.
    fn take(&mut self, event: Event) {
        if let Some(topic) = event.topic() {
            if let Some(open) = self.subscriptions.iter_mut().find(|s| s.spec.topic == topic) {
                if event.seq <= open.last_seq {
                    return;
                }
                open.last_seq = event.seq;
            }
        }
        self.queue.push_back(event);
    }

    fn drop_socket(&mut self, why: &str) {
        tracing::warn!(event = "agent_api_client.socket_dropped", url = %self.config.url, reason = %why);
        self.socket = None;
    }

    /// Reads one frame, or reports that nothing came before `deadline`. A frame that is no frame of the agent API ends the socket.
    fn read_until(&mut self, deadline: Instant) -> Result<Read, Error> {
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let Some(socket) = self.socket.as_mut() else { return Err(Error::new(code::DISCONNECTED, "the socket is closed")) };
            // A zero timeout means none to the socket
            let _ = socket.get_ref().set_read_timeout(Some(left.max(Duration::from_millis(1))));
            match socket.read() {
                Ok(Message::Text(text)) => {
                    return match Incoming::parse(&text) {
                        Ok(frame) => Ok(Read::Frame(frame)),
                        Err(e) => {
                            self.drop_socket(&format!("a frame that is no agent API frame: {e}"));
                            Err(Error::new(code::PROTOCOL, format!("the service sent a frame that is no agent API frame: {e}")))
                        }
                    };
                }
                Ok(Message::Close(_)) => {
                    self.drop_socket("the service closed the socket");
                    return Err(Error::new(code::DISCONNECTED, "the service closed the socket"));
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                    if Instant::now() >= deadline {
                        return Ok(Read::Quiet);
                    }
                }
                Err(e) => {
                    self.drop_socket(&e.to_string());
                    return Err(Error::new(code::DISCONNECTED, format!("the socket dropped: {e}")));
                }
            }
        }
    }

    /// The socket, reopened if it dropped, with every subscription resumed.
    fn ensure_open(&mut self) -> Result<(), Error> {
        if self.socket.is_some() {
            return Ok(());
        }
        let Backoff { initial, max, attempts } = self.config.backoff.clone();
        let mut wait = initial;
        let mut last = Error::new(code::DISCONNECTED, "the socket is closed");
        for attempt in 1..=attempts {
            if attempt > 1 {
                std::thread::sleep(wait);
                wait = (wait * 2).min(max);
            }
            tracing::info!(event = "agent_api_client.reconnecting", url = %self.config.url, attempt);
            match connection::open(&self.config.url, &self.config.bearer, self.config.connect_timeout) {
                Ok(socket) => self.socket = Some(socket),
                // A bearer the service refuses is refused again
                Err(e) if e.code == code::UNAUTHORIZED => return Err(e),
                Err(e) => {
                    last = e;
                    continue;
                }
            }
            match self.resume() {
                Ok(()) => return Ok(()),
                Err(e) if e.code == code::DISCONNECTED || e.code == code::TIMEOUT => {
                    // Not resumed, so not open: the next attempt starts from a socket of its own
                    self.socket = None;
                    last = e;
                }
                Err(e) => {
                    self.socket = None;
                    return Err(e);
                }
            }
        }
        Err(Error::new(code::DISCONNECTED, format!("gave up after {attempts} attempts to reopen the socket, the last: {last}")))
    }

    /// Opens every subscription again after the last seq it saw. A subscription the service now refuses is dropped and its error returned.
    fn resume(&mut self) -> Result<(), Error> {
        let topics: Vec<String> = self.subscriptions.iter().map(|s| s.spec.topic.clone()).collect();
        for topic in topics {
            let Some(open) = self.subscriptions.iter().find(|s| s.spec.topic == topic) else { continue };
            let (method, params) = (open.spec.method.clone(), open.spec.params_after(open.last_seq));
            match self.exchange(&method, params) {
                Ok(_) => {}
                Err(e) if e.from_service() => {
                    self.subscriptions.retain(|s| s.spec.topic != topic);
                    return Err(Error::new(&e.code, format!("resuming {topic}: {}", e.message)));
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}
