//! What the methods of `agent.lifecycle` answer. `seq` is that of the event the call recorded. Times are milliseconds since the epoch.
use super::params::{Cursor, Shelf};
use crate::results::{ThreadView, TranscriptItem};
use serde::{Deserialize, Serialize};

/// Where a settled-or-active choice stands: a thread a person settled, or one they brought back and keep out of automatic settling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resting {
    Settled,
    Active,
}

/// A thread's settle and snooze state; a field that is none is left out, and a thread that never settled has none of it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settle {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<Resting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settled_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsettled_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snoozed_until_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snoozed_at_ms: Option<u64>,
    /// When a snooze last ended, by `awake` or by settling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub woke_at_ms: Option<u64>,
}

/// Where a thread came from: its parent (a fork's source), how it relates to it, and the root of its family.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lineage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relationship: Option<String>,
    pub root: String,
}

/// A thread with what the lifecycle adds to its view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThreadState {
    pub thread: ThreadView,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settle: Option<Settle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineage: Option<Lineage>,
    /// How many prompts wait in the thread's queue.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub queued: u32,
    #[serde(default, skip_serializing_if = "is_false")]
    pub queue_held: bool,
    /// The fork whose merge back waits for the thread's next prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_merge_back: Option<String>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// The result of `thread.launch`: the thread and the turn that began.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LaunchResult {
    pub thread: ThreadView,
    pub turn: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArchiveResult {
    pub thread: ThreadState,
    pub seq: u64,
}

/// The result of `thread.delete`: `deleted` is false when `only_if_unused` found the thread used and kept it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeleteResult {
    pub thread: String,
    pub deleted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
}

/// The result of `thread.fork`: the new thread, the thread it came from and the turns it holds, in order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForkResult {
    pub thread: ThreadState,
    pub source: String,
    pub turns: Vec<String>,
    pub seq: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettleResult {
    pub thread: ThreadState,
    pub seq: u64,
}

/// Where a search hit was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HitSource {
    Title,
    User,
    Assistant,
}

/// A thread a search found. `snippet` is the text around the match of a message hit, and `at_ms` when that message was written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    pub thread: String,
    pub title: String,
    pub source: HitSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_ms: Option<u64>,
}

/// The result of `thread.search`: title matches ranked by how early the words start in the title, then message matches, the newest thread first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    pub matches: Vec<SearchHit>,
}

/// The result of `thread.family`: the root of the thread's family and the live threads under it (not archived, not deleted). A thread nobody
/// knows has no root and no family; a deleted one still names its root.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FamilyResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    pub threads: Vec<ThreadState>,
}

/// What a forced stop did to the CLI process. The service's CLIs are one process per turn, so it kills them: `killed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForceStopOutcome {
    Killed,
    Released,
    Releasing,
    /// The process also works for `shared_with`, so only this thread's turn ended.
    Shared,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForceStopResult {
    pub thread: String,
    pub turn: String,
    pub outcome: ForceStopOutcome,
    pub shared_with: Vec<String>,
    pub seq: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemGetResult {
    pub thread: String,
    pub item: TranscriptItem,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowStopResult {
    pub thread: String,
    pub turn: String,
    pub seq: u64,
}

/// The result of `thread.mergeBack`: the parent, with the merge waiting for its next prompt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MergeBackResult {
    pub thread: ThreadState,
    pub source: String,
    pub seq: u64,
}

/// The result of `thread.rollback`: the turns that went, and whether the files were put back (never when no turn went).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RollbackResult {
    pub thread: String,
    pub turn: String,
    pub dropped: Vec<String>,
    pub restored_files: bool,
    pub seq: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetachResult {
    pub thread: String,
    pub session: String,
    pub seq: u64,
}

/// The result of `turn.retry`: the new turn, and the failed one it runs again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetryResult {
    pub thread: String,
    pub turn: String,
    pub retry_of: String,
    pub seq: u64,
}

/// The result of `thread.shelf`: a page of the shelf, `next` (the cursor of the page after it, none when this was the last) and `total`, how many
/// threads the shelf holds after `before` (all of them when `before` is left out).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShelfResult {
    pub shelf: Shelf,
    pub threads: Vec<ThreadState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<Cursor>,
    pub total: u32,
}
