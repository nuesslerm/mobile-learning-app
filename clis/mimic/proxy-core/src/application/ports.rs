//! What the core needs from outside, in its own words.
use crate::domain::ordering::Payload;

#[derive(Clone, Debug, PartialEq)]
pub struct HttpAnswer {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// A server under check: it is sent what a recorded client sent, and says what it answers.
pub trait Candidate {
    fn http(&mut self, method: &str, target: &str, headers: &[(String, String)], body: &[u8]) -> Result<HttpAnswer, String>;
    /// Asks for a socket; the status the server answered with (101 when it is open).
    fn open(&mut self, target: &str) -> Result<u16, String>;
    fn send(&mut self, message: &Payload) -> Result<(), String>;
    /// The next message of the open socket, or `None` when none comes within `wait_ms`.
    fn receive(&mut self, wait_ms: u64) -> Result<Option<Payload>, String>;
    fn close(&mut self);
}
