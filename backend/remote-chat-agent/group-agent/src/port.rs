//! What the group needs of the thread engine, in the words of the agent API. The engine decides, sequences and keeps; the group asks. The
//! service's adapter implements it over `remote-chat-agent-core`'s engine, and `testing::MemoryThreads` is the double.
use agent_api_agent::{lifecycle, queue, requests, ThreadView};
use agent_api_proto::Event;
use async_trait::async_trait;

/// The engine could not read or write its log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineError(pub String);

/// A thread to create. `workspace`, `driver` and `model` are the defaults of its turns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewThread {
    pub id: String,
    pub title: String,
    pub workspace: Option<String>,
    pub driver: Option<String>,
    pub model: Option<String>,
}

/// A page of a thread's events after a `seq`. `last_seq` is the thread's newest, so `more` is whether the page ends before it.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub events: Vec<Event>,
    pub last_seq: u64,
    pub more: bool,
}

/// The events of a thread as they happen, after the replay.
#[async_trait]
pub trait Follow: Send {
    /// The next event, in `seq` order; none when the engine has dropped the thread's stream.
    async fn next(&mut self) -> Option<Event>;
}

/// A subscription made at one instant: the events after the requested `seq`, then the live ones. No event is in both and none is between.
pub struct Subscription {
    /// The `seq` the replay ends at: that of its last event, or the requested one when it is empty (the thread's newest when that is less).
    pub last_seq: u64,
    pub replay: Vec<Event>,
    pub live: Box<dyn Follow>,
}

/// A command of `agent.requests`, `agent.queue` or `agent.lifecycle` that the engine decides against a thread's state.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    ApprovalAnswer { approval: String, decision: requests::Decision },
    InputAnswer { request: String, answer: String },
    InputDismiss { request: String },
    QueueAdd(queue::AddParams),
    QueueEdit { item: String, prompt: String },
    QueueMove { item: String, before: Option<String> },
    QueueDelete { item: String },
    QueueResume { item: Option<String> },
    /// The queue's first item leaves it to run as the turn of the same id.
    QueueStart { item: String },
    Steer { turn: Option<String>, prompt: Option<String>, item: Option<String> },
    Archive { archived: bool },
    Delete { only_if_unused: bool },
    Settle { state: lifecycle::SettleState, until_ms: Option<u64>, at_ms: Option<u64> },
    /// Decided against the new thread's id: `source` is the thread it copies.
    Fork { source: String, turn: Option<String>, title: Option<String> },
    ForceStop { turn: Option<String> },
    WorkflowStop { turn: String, workflow: Option<String> },
    /// Decided against the parent: `source` is the fork whose turns it takes as context.
    MergeBack { source: String, turn: Option<String> },
    Rollback { turn: String, restore_files: bool },
    Detach { session: String, reason: Option<String> },
    Retry { turn: String, new_turn: String },
}

/// A command the thread's state refuses, with the code and message the contract gives the refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub code: String,
    pub message: String,
}

/// What the engine made of a command: the events it recorded, in order with their `seq` (none for a command that changes nothing), or the refusal.
pub type Decided = Result<Vec<Event>, Refused>;

fn unserved<T>() -> Result<T, EngineError> {
    Err(EngineError("this thread engine keeps no lifecycle".into()))
}

pub trait ThreadEngine: Send + Sync {
    /// The thread `new.id` names, created now; the thread as it already is when one has that id, so a retried create is safe.
    fn create(&self, new: NewThread) -> Result<ThreadView, EngineError>;
    fn get(&self, thread: &str) -> Result<Option<ThreadView>, EngineError>;
    /// Every thread, the one with the newest activity first.
    fn list(&self) -> Result<Vec<ThreadView>, EngineError>;
    /// At most `limit` events of the thread after `after_seq`, oldest first; none for a thread that does not exist.
    fn history(&self, thread: &str, after_seq: u64, limit: usize) -> Result<Option<Page>, EngineError>;
    /// Follows the thread from after `after_seq`; none for a thread that does not exist.
    fn follow(&self, thread: &str, after_seq: u64) -> Result<Option<Subscription>, EngineError>;
    /// Appends an event of `turn` to the thread's log, which gives it its `seq`: the engine is the only one that does.
    fn record(&self, thread: &str, turn: Option<&str>, event: agent_api_agent::ThreadEvent) -> Result<u64, EngineError>;
    /// A fresh id, for a thread the caller did not name and for a turn.
    fn new_id(&self) -> String;

    // What follows is the thread's lifecycle (`agent.requests`, `agent.queue`, `agent.lifecycle`). An engine that keeps none, such as the double
    // of the threads group, refuses it all.

    /// Decides `command` against `thread` and records what it decides, all under the engine's one lock.
    fn decide(&self, _thread: &str, _command: Command) -> Result<Decided, EngineError> {
        unserved()
    }

    /// The thread with its lifecycle state; none for a thread that does not exist (a deleted one is still read).
    fn state(&self, _thread: &str) -> Result<Option<lifecycle::ThreadState>, EngineError> {
        unserved()
    }

    /// Whether the thread's queue is held, and its items in the order they start; none for a thread that does not exist.
    fn queue(&self, _thread: &str) -> Result<Option<(bool, Vec<queue::QueueItem>)>, EngineError> {
        unserved()
    }

    fn search(&self, _query: &str, _limit: usize) -> Result<Vec<lifecycle::SearchHit>, EngineError> {
        unserved()
    }

    fn family(&self, _thread: &str) -> Result<(Option<String>, Vec<lifecycle::ThreadState>), EngineError> {
        unserved()
    }

    fn shelf(&self, _shelf: lifecycle::Shelf, _before: Option<lifecycle::Cursor>, _limit: usize) -> Result<lifecycle::ShelfResult, EngineError> {
        unserved()
    }

    /// The fork and the context its merge back left for the thread's next prompt.
    fn pending_merge(&self, _thread: &str) -> Result<Option<(String, String)>, EngineError> {
        unserved()
    }

    /// The conversation of the thread's CLI `driver` (the CLI's id) that its next turn goes on with: none when the thread's conversation is another CLI's.
    fn provider_session(&self, _thread: &str, _driver: &str) -> Result<Option<String>, EngineError> {
        unserved()
    }

    /// Notes the conversation `session` of the thread's CLI (`driver` is the CLI's id) as announced during `turn`, which still runs.
    fn set_session(&self, _thread: &str, _driver: &str, _turn: &str, _session: &str) -> Result<(), EngineError> {
        unserved()
    }

    /// Appends an event of the lifecycle modules (`{type, ...fields}`) to the thread's log, as `record` does a `ThreadEvent`.
    fn record_value(&self, _thread: &str, _turn: Option<&str>, _event: serde_json::Value) -> Result<u64, EngineError> {
        unserved()
    }
}
