//! What crossed a proxy, as the adapter that carried it reports it: whole HTTP messages, whole WebSocket messages, the upgrade and
//! the close. The values are the wire's own, so nothing here may reach a file before `sanitize` has seen it.
use mimic_core::cassette::Dir;

/// Why an exchange failed: a word of the proxy's own, never a value of the traffic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// What a file says: a fixed word, so no value of the traffic can ride on an error.
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

/// Header lines as the wire carried them, repeated names kept apart.
pub type Headers = Vec<(String, String)>;

/// One thing that crossed. `exchange` ties a request to its answer and `connection` the lines of one socket together.
#[derive(Clone, Debug)]
pub enum Event {
    HttpRequest { exchange: u32, method: String, target: String, headers: Headers, body: Vec<u8> },
    HttpResponse { exchange: u32, status: u16, headers: Headers, body: Vec<u8> },
    WsOpen { connection: u32, target: String, status: u16 },
    WsRefused { connection: u32, target: String, status: u16, body: Vec<u8> },
    WsFrame { connection: u32, direction: Dir, opcode: u8, data: Vec<u8> },
    WsClose { connection: u32, direction: Option<Dir>, code: Option<u16> },
    Failed { failure: Failure },
}
