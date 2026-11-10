//! What the attachments group stands on. The service's adapters implement them; `testing` has the doubles.
use agent_api_attachments::AttachmentView;

/// Where the bytes live: the adapter keeps them on the service volume across restarts.
pub trait BlobStore: Send + Sync {
    /// Keeps `bytes` under `view.id` unless the id is taken, whole or not at all; a taken id is answered with what it holds.
    fn put(&self, view: &AttachmentView, bytes: &[u8]) -> Result<Put, String>;
    /// What the id holds, without its bytes.
    fn meta(&self, id: &str) -> Result<Option<AttachmentView>, String>;
    /// `length` bytes of the id's attachment from `offset` (fewer at the end), and what it holds.
    fn read(&self, id: &str, offset: u64, length: u64) -> Result<Option<(AttachmentView, Vec<u8>)>, String>;
    /// True when the id held an attachment.
    fn delete(&self, id: &str) -> Result<bool, String>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum Put {
    Created,
    Taken(AttachmentView),
}

pub trait Clock: Send + Sync {
    /// Milliseconds since the epoch.
    fn now_ms(&self) -> u64;
}

pub trait Ids: Send + Sync {
    /// A fresh attachment id that `agent_api_attachments::valid_id` accepts.
    fn new_id(&self) -> String;
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

    pub fn unavailable(why: impl std::fmt::Display) -> Refusal {
        Refusal::new(agent_api_attachments::code::ATTACHMENTS_UNAVAILABLE, format!("the attachments could not be read or written: {why}"))
    }
}
