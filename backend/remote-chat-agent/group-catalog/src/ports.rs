//! What the catalog group stands on besides the executors. The service's adapter implements it; `testing` has a double.

pub trait Clock: Send + Sync {
    /// Milliseconds since the epoch.
    fn now_ms(&self) -> u64;
}

/// A refusal in the words of the contract: its code and message go on the wire as they are.
#[derive(Debug, Clone, PartialEq)]
pub struct Refusal {
    pub code: String,
    pub message: String,
}

impl Refusal {
    pub fn new(code: &str, message: impl Into<String>) -> Refusal {
        Refusal { code: code.into(), message: message.into() }
    }
}

/// The service's speech-to-text engine: what `speech.status` names and what `speech.transcribe` asks. The service's adapter implements it over the
/// engine its environment picks.
#[async_trait::async_trait]
pub trait Speech: Send + Sync {
    /// The engine's name, or none when the service has none.
    fn engine(&self) -> Option<String>;
    /// The words in one recording (`format` is its file extension); the error is the engine's own words.
    async fn transcribe(&self, audio: Vec<u8>, format: &str) -> Result<String, String>;
}
