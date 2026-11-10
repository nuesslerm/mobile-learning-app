//! What the methods of `agent.queue` answer. `seq` is that of the event the call recorded. Times are milliseconds since the epoch.
use serde::{Deserialize, Serialize};

/// A prompt that waits. Each field but `item`, `prompt` and `created_at_ms` is what the turn runs with when the item set it, and none when it left
/// it to the thread's own defaults.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QueueItem {
    pub item: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction_mode: Option<String>,
    pub created_at_ms: u64,
}

/// The result of `queue.list`: the items in the order they start, and whether a forced stop holds them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListResult {
    pub thread: String,
    pub held: bool,
    pub items: Vec<QueueItem>,
}

/// The result of `queue.add`: the item as queued and its place (1 is next).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AddResult {
    pub thread: String,
    pub item: QueueItem,
    pub position: u32,
    pub seq: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditResult {
    pub thread: String,
    pub item: String,
    pub seq: u64,
}

/// The result of `queue.move`: the ids in the order they now start.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoveResult {
    pub thread: String,
    pub order: Vec<String>,
    pub seq: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeleteResult {
    pub thread: String,
    pub item: String,
    pub seq: u64,
}

/// The result of `queue.resume`: the queue is no longer held; `started` is the turn the call started (the id of the item that ran), none when
/// the queue was empty or a turn was running.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResumeResult {
    pub thread: String,
    pub held: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<String>,
    pub seq: u64,
}

/// The result of `turn.steer`: the turn that goes on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SteerResult {
    pub thread: String,
    pub turn: String,
    pub seq: u64,
}
