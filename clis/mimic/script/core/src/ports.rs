//! The ports the core owns, synchronous and free of I/O. Time reaches the core only through `Clock`, the wire only through `Transport`.
use std::fmt;

/// What arrived on the socket, in arrival order.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A whole data frame: opcode 1 text, 2 binary.
    Frame { opcode: u8, data: Vec<u8> },
    /// The peer's close frame.
    Close { code: Option<u16>, reason: String },
    /// The connection went away without a close frame.
    Dropped,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HttpCall {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HttpReply {
    pub status: u16,
    /// As the server sent them; the interpreter lower-cases and joins repeated names.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// A transport failure is recorded behaviour (`transport_error` in the steps), carrying the cause code or message.
#[derive(Clone, Debug, PartialEq)]
pub struct TransportError(pub String);

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One endpoint, one HTTP client and at most one socket.
pub trait Transport {
    /// Never follows a redirect; the whole body is read.
    fn http(&mut self, call: &HttpCall) -> Result<HttpReply, TransportError>;
    /// The status of the upgrade: 101 is an open socket, anything else leaves none. A second open replaces the first.
    fn ws_open(&mut self, path: &str) -> Result<u16, TransportError>;
    /// `Err("no socket")` when there is none.
    fn ws_send(&mut self, text: &str) -> Result<(), TransportError>;
    /// Everything that has arrived since the last call, without waiting.
    fn ws_drain(&mut self) -> Vec<Event>;
    fn ws_close(&mut self, code: u16, reason: &str) -> Result<(), TransportError>;
}

/// The server a case runs against: its workspace folder and one credential per run.
pub trait Target {
    /// The server's workspace folder, masked as `<workspace>` in every step.
    fn workspace(&self) -> String;
    /// A fresh credential, asked for only when a script names `{credential}`.
    fn mint(&mut self) -> Result<String, String>;
}

pub trait Clock {
    fn now_ms(&self) -> u64;
    fn sleep(&mut self, ms: u64);
}
