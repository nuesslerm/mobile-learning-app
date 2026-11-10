//! What a call or a wait for an event can fail with. The code is one of the five below, or the service's own (`THREAD_NOT_FOUND`, `NO_EXECUTOR`,
//! `INVALID_PARAMS`, ...) when the service answered the request with an error.
use std::fmt;

pub mod code {
    /// The socket could not be opened: the address, the scheme, the connection or the handshake.
    pub const CONNECT: &str = "CONNECT";
    /// The service refused the bearer.
    pub const UNAUTHORIZED: &str = "UNAUTHORIZED";
    /// The socket dropped, or the client gave up reconnecting. A request written before it dropped may have been received: it is not sent again.
    pub const DISCONNECTED: &str = "DISCONNECTED";
    /// No answer came in time. The socket is still open and a late answer is dropped.
    pub const TIMEOUT: &str = "TIMEOUT";
    /// The service sent a frame that is no frame of the agent API.
    pub const PROTOCOL: &str = "PROTOCOL";

    /// The five codes the client itself answers with.
    pub const ALL: [&str; 5] = [CONNECT, UNAUTHORIZED, DISCONNECTED, TIMEOUT, PROTOCOL];
}

#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    pub code: String,
    pub message: String,
}

impl Error {
    pub(crate) fn new(code: &str, message: impl Into<String>) -> Error {
        Error { code: code.to_string(), message: message.into() }
    }

    /// Whether the service answered with this error, and not the client.
    pub fn from_service(&self) -> bool {
        !code::ALL.contains(&self.code.as_str())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for Error {}
