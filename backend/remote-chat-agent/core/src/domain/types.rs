//! The orchestration wire shapes (`@app/remote-chat-agent-contracts`: orchestrationCore, orchestrationEvents, orchestrationCommands):
//! what the event log stores, what a thread looks like, and the commands the engine decides. Events keep their payload as JSON,
//! built the way the TypeScript decider builds it, so what is stored and sent is the same bytes in the same order.

use super::lifecycle::{Lifecycle, SettleTo};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub const MAX_THREAD_MESSAGES: usize = 2_000;
pub const MAX_THREAD_ACTIVITIES: usize = 5_000;
pub const MAX_THREAD_PROPOSED_PLANS: usize = 200;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelSelection {
    pub cli: String,
    #[serde(rename = "providerID", default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(rename = "modelID", default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
}

impl ModelSelection {
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<Value>>,
    #[serde(rename = "turnId", default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposedPlan {
    pub id: String,
    #[serde(rename = "turnId", default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(rename = "planMarkdown")]
    pub plan_markdown: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub cli: String,
    #[serde(rename = "providerSessionId", default, skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    pub status: String,
    #[serde(rename = "activeTurnId", default, skip_serializing_if = "Option::is_none")]
    pub active_turn_id: Option<String>,
    #[serde(rename = "lastError", default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Activity {
    pub id: String,
    pub tone: String,
    pub kind: String,
    pub summary: String,
    #[serde(default)]
    pub payload: Value,
    #[serde(rename = "turnId", default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LatestTurn {
    #[serde(rename = "turnId")]
    pub turn_id: String,
    pub state: String,
    #[serde(rename = "requestedAt")]
    pub requested_at: String,
    #[serde(rename = "startedAt")]
    pub started_at: Option<String>,
    #[serde(rename = "completedAt")]
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Thread {
    pub id: String,
    pub title: String,
    pub cwd: String,
    #[serde(rename = "modelSelection")]
    pub model_selection: ModelSelection,
    #[serde(rename = "runtimeMode")]
    pub runtime_mode: String,
    #[serde(rename = "interactionMode")]
    pub interaction_mode: String,
    #[serde(rename = "latestTurn", default)]
    pub latest_turn: Option<LatestTurn>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    #[serde(rename = "archivedAt", default)]
    pub archived_at: Option<String>,
    #[serde(rename = "deletedAt", default)]
    pub deleted_at: Option<String>,
    pub messages: Vec<Message>,
    #[serde(rename = "proposedPlans")]
    pub proposed_plans: Vec<ProposedPlan>,
    pub activities: Vec<Activity>,
    #[serde(default)]
    pub session: Option<Session>,
    #[serde(rename = "tokenUsage", default, skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(rename = "worktreePath", default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    /// What the agent API's lifecycle keeps of the thread: every field of it is left out while it is empty, so a thread that has none serialises as
    /// it did before.
    #[serde(flatten)]
    pub lifecycle: Lifecycle,
}

/// The thread without its heavy collections, what the shell list ships on every tick.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub id: String,
    pub title: String,
    pub cwd: String,
    #[serde(rename = "modelSelection")]
    pub model_selection: ModelSelection,
    #[serde(rename = "runtimeMode")]
    pub runtime_mode: String,
    #[serde(rename = "interactionMode")]
    pub interaction_mode: String,
    #[serde(rename = "latestTurn", default)]
    pub latest_turn: Option<LatestTurn>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    #[serde(rename = "archivedAt", default)]
    pub archived_at: Option<String>,
    #[serde(rename = "deletedAt", default)]
    pub deleted_at: Option<String>,
    #[serde(default)]
    pub session: Option<Session>,
    #[serde(rename = "tokenUsage", default, skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(rename = "worktreePath", default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
}

impl Thread {
    pub fn summary(&self) -> Summary {
        Summary {
            id: self.id.clone(),
            title: self.title.clone(),
            cwd: self.cwd.clone(),
            model_selection: self.model_selection.clone(),
            runtime_mode: self.runtime_mode.clone(),
            interaction_mode: self.interaction_mode.clone(),
            latest_turn: self.latest_turn.clone(),
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
            archived_at: self.archived_at.clone(),
            deleted_at: self.deleted_at.clone(),
            session: self.session.clone(),
            token_usage: self.token_usage.clone(),
            branch: self.branch.clone(),
            worktree_path: self.worktree_path.clone(),
        }
    }
}

/// One line of a thread's event log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    #[serde(rename = "eventId")]
    pub event_id: String,
    pub sequence: i64,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "threadId")]
    pub thread_id: String,
    #[serde(rename = "commandId", default, skip_serializing_if = "Option::is_none")]
    pub command_id: Option<String>,
    pub actor: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub payload: Value,
}

impl Event {
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

/// What a dispatch answers: the events it produced, or why it was refused.
#[derive(Debug, Clone)]
pub struct Receipt {
    pub command_id: String,
    pub accepted: bool,
    pub events: Vec<Event>,
    pub error: Option<(String, String)>,
}

impl Receipt {
    pub fn to_json(&self) -> Value {
        let mut out = json!({
            "commandId": self.command_id,
            "status": if self.accepted { "accepted" } else { "rejected" },
            "events": self.events.iter().map(Event::to_json).collect::<Vec<_>>(),
        });
        if let Some((code, message)) = &self.error {
            out["error"] = json!({ "code": code, "message": message });
        }
        out
    }
}

#[derive(Debug, Clone)]
pub struct Bootstrap {
    pub title: String,
    pub cwd: Option<String>,
    pub model_selection: ModelSelection,
    pub runtime_mode: String,
    pub interaction_mode: String,
}

/// Client commands, then the runtime-only ones the ingestion produces. A command is built, decided and dropped, never stored, so
/// the size of its largest variant costs nothing.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum Body {
    Create { title: String, cwd: Option<String>, model_selection: ModelSelection, runtime_mode: String, interaction_mode: String, branch: Option<String>, worktree_path: Option<String> },
    Delete,
    Archive,
    Unarchive,
    MetaUpdate { title: Option<String>, model_selection: Option<ModelSelection>, cwd: Option<String>, branch: Option<String>, worktree_path: Option<String> },
    RuntimeModeSet { runtime_mode: String },
    InteractionModeSet { interaction_mode: String },
    TurnStart {
        message_id: String,
        text: String,
        attachments: Option<Vec<Value>>,
        model_selection: Option<ModelSelection>,
        runtime_mode: String,
        interaction_mode: String,
        source_proposed_plan: Option<Value>,
        bootstrap: Option<Bootstrap>,
    },
    TurnInterrupt { turn_id: Option<String>, created_at: String },
    ApprovalRespond { request_id: String, decision: String, created_at: String },
    UserInputRespond { request_id: String, answers: Map<String, Value>, created_at: String },
    SessionStop { created_at: String },
    SessionSet { session: Session },
    ActivityAppend { activity: Activity },
    ProposedPlanUpsert { plan: ProposedPlan },
    TurnDiffUpdate { turn_id: String, unified_diff: String },
    MessageAppend { message_id: String, delta: String, turn_id: Option<String> },
    /// A thread of the agent API: its `workspace`, `driver` and `model` are the defaults of its turns. Decided like `Create`.
    NeutralCreate { title: String, workspace: Option<String>, driver: Option<String>, model: Option<String> },
    /// One neutral event of the agent API (`{type, ...fields}`, `agent-api-agent`'s `ThreadEvent`) kept in the thread's log, so the engine's
    /// sequence is the one the agent API's `seq` is. The engine reads only `type` and `reason` of it, to follow the turns.
    NeutralEvent { turn_id: Option<String>, event: Value },
    // The commands of the agent API's `agent.requests`, `agent.queue` and `agent.lifecycle`, decided by `domain::lifecycle`. The legacy `Archive`,
    // `Unarchive` and `Delete` above stay as permissive as the thread view needs them; the lifecycle's own refusals are `LifecycleArchive` and
    // `LifecycleDelete`.
    ApprovalAnswer { approval: String, decision: String },
    InputAnswer { request: String, answer: String },
    InputDismiss { request: String },
    QueueAdd { item: Option<String>, prompt: String, attachments: Vec<String>, workspace: Option<String>, driver: Option<String>, model: Option<String>, runtime_mode: Option<String>, interaction_mode: Option<String> },
    QueueEdit { item: String, prompt: String },
    QueueMove { item: String, before: Option<String> },
    QueueDelete { item: String },
    QueueResume { item: Option<String> },
    /// The queue's first item leaves it to run as the turn of the same id.
    QueueStart { item: String },
    Steer { turn: Option<String>, prompt: Option<String>, item: Option<String> },
    LifecycleArchive { archive: bool },
    LifecycleDelete { only_if_unused: bool },
    SettleSet { to: SettleTo, until_ms: Option<u64>, at_ms: Option<u64> },
    /// Decided against `thread_id`, the new thread: `source` is the thread it copies, whose log the engine loads before deciding.
    Fork { source: String, at_turn: Option<String>, title: Option<String> },
    ForceStop { turn: Option<String> },
    WorkflowStop { turn: String, workflow: Option<String> },
    /// Decided against `thread_id`, the parent: `source` is the fork whose turns it takes as context.
    MergeBack { source: String, turn: Option<String> },
    Rollback { turn: String, restore_files: bool },
    Detach { session: String, reason: Option<String> },
    Retry { turn: String, new_turn: String },
}

#[derive(Debug, Clone)]
pub struct Command {
    pub command_id: String,
    pub thread_id: String,
    pub body: Body,
}

impl Command {
    pub fn is_internal(&self) -> bool {
        matches!(self.body, Body::SessionSet { .. } | Body::ActivityAppend { .. } | Body::ProposedPlanUpsert { .. } | Body::TurnDiffUpdate { .. } | Body::MessageAppend { .. } | Body::NeutralEvent { .. })
    }
}

/// An item of a thread subscription. Items pass through a channel once, so the size of the largest variant costs nothing.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum ThreadItem {
    Synchronized,
    Snapshot(Box<Thread>),
    Event(Event),
}

/// An item of the shell (thread list) subscription.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum ShellItem {
    Synchronized,
    Snapshot(Vec<Summary>),
    Event(Event),
}

impl ThreadItem {
    pub fn to_json(&self) -> Value {
        match self {
            Self::Synchronized => json!({ "kind": "synchronized" }),
            Self::Snapshot(thread) => json!({ "kind": "snapshot", "snapshot": thread }),
            Self::Event(event) => json!({ "kind": "event", "event": event }),
        }
    }
}

impl ShellItem {
    pub fn to_json(&self) -> Value {
        match self {
            Self::Synchronized => json!({ "kind": "synchronized" }),
            Self::Snapshot(list) => json!({ "kind": "snapshot", "snapshot": list }),
            Self::Event(event) => json!({ "kind": "event", "event": event }),
        }
    }
}
