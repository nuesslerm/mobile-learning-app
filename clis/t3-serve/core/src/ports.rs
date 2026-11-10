//! What the core needs from outside, in its own words.
use serde_json::Value;

pub type Millis = i64;

pub trait Clock: Send + Sync {
    fn now_ms(&self) -> Millis;
}

pub trait Ids: Send + Sync {
    /// Cryptographically random bytes: pairing codes, bearer tokens and tickets are made from them.
    fn random_bytes(&self, n: usize) -> Vec<u8>;
}

/// Static event names with variables as fields; a secret is never a field.
pub trait Log: Send + Sync {
    fn event(&self, name: &'static str, fields: &[(&'static str, String)]);
}

#[derive(Debug, Clone)]
pub struct CatalogModel {
    pub slug: String,
    pub name: String,
}

/// The models the `rca` provider lists in `ServerConfig`.
pub trait ModelCatalog: Send + Sync {
    fn models(&self) -> Vec<CatalogModel>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredEvent {
    pub sequence: u64,
    /// `event:thread:<thread>:command:<command>:<uuid>` for a thread's event: the id the thread stream carries.
    pub event_id: String,
    pub kind: String,
    pub thread_id: Option<String>,
    pub project_id: Option<String>,
    pub occurred_at: Millis,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct EventDraft {
    pub event_id: String,
    pub kind: String,
    pub thread_id: Option<String>,
    pub project_id: Option<String>,
    pub occurred_at: Millis,
    pub payload: Value,
}

/// The sequence a command's events ended at, remembered by `commandId`.
#[derive(Debug, Clone, PartialEq)]
pub struct Receipt {
    pub command_type: String,
    pub sequence: u64,
}

#[derive(Debug)]
pub struct StoreError(pub String);

pub type StoreResult<T> = Result<T, StoreError>;

/// One global sequence across every aggregate. `append` writes the events and the command's receipt atomically.
pub trait EventStore: Send + Sync {
    fn append(&self, drafts: Vec<EventDraft>, receipt: Option<(&str, &str)>) -> StoreResult<Vec<StoredEvent>>;
    fn read_after(&self, after: u64) -> StoreResult<Vec<StoredEvent>>;
    fn last_sequence(&self) -> StoreResult<u64>;
    fn receipt(&self, command_id: &str) -> StoreResult<Option<Receipt>>;
}

#[derive(Debug, Clone)]
pub struct GrantRecord {
    pub scopes: Vec<String>,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GrantRefusal {
    Unknown,
    Expired,
    Used,
}

#[derive(Debug, Clone)]
pub struct TokenRecord {
    pub id: String,
    pub scopes: Vec<String>,
    pub label: String,
    pub expires_at: Millis,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TicketRecord {
    pub token_id: String,
    pub scopes: Vec<String>,
    pub use_: String,
}

/// Pairing codes, bearer tokens and tickets, each stored by the hash of the secret only.
pub trait AuthStore: Send + Sync {
    fn put_grant(&self, hash: &str, grant: &GrantRecord, expires_at: Millis) -> StoreResult<()>;
    /// Single use: the second call for one hash is `Used`.
    fn consume_grant(&self, hash: &str, now: Millis) -> StoreResult<Result<GrantRecord, GrantRefusal>>;
    fn put_token(&self, hash: &str, token: &TokenRecord) -> StoreResult<()>;
    fn find_token(&self, hash: &str) -> StoreResult<Option<TokenRecord>>;
    fn put_ticket(&self, hash: &str, ticket: &TicketRecord, expires_at: Millis) -> StoreResult<()>;
    /// Single use; a ticket past its expiry or already redeemed is `None`.
    fn consume_ticket(&self, hash: &str, now: Millis) -> StoreResult<Option<TicketRecord>>;
}

/// Small durable values: the environment id.
pub trait MetaStore: Send + Sync {
    fn get(&self, key: &str) -> StoreResult<Option<String>>;
    fn set(&self, key: &str, value: &str) -> StoreResult<()>;
}

/// The codes of the agent API's refusals that t3 tells apart: the contract's own, which a flow asserts they still are.
pub mod agent_codes {
    pub const PATH_OUTSIDE_ROOT: &str = "PATH_OUTSIDE_ROOT";
    pub const NOT_FOUND: &str = "NOT_FOUND";
    pub const ALREADY_EXISTS: &str = "ALREADY_EXISTS";
    pub const WORKSPACE_EXISTS: &str = "WORKSPACE_EXISTS";
    pub const UNKNOWN_WORKSPACE: &str = "UNKNOWN_WORKSPACE";
}

/// Why a call to the agent API did not do what was asked.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentError {
    /// The agent API has no such thread: nothing has run on it there.
    NoThread,
    /// Anything else, in words an operator can act on.
    Failed(String),
    /// The agent API refused with a code of its own (`UNKNOWN_TERMINAL`, `PATH_OUTSIDE_ROOT`), which the caller maps to the app's error.
    Refused { code: String, message: String },
}

impl AgentError {
    pub fn message(&self) -> String {
        match self {
            AgentError::NoThread => "the agent API has no such thread".into(),
            AgentError::Failed(message) | AgentError::Refused { message, .. } => message.clone(),
        }
    }
}

/// A thread t3 serve asks the agent API to hold under t3's own thread id.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentThread {
    pub thread_id: String,
    pub instance_id: String,
    pub model: Option<String>,
}

/// One turn to run on a thread: the message and the thread's modes.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentTurn {
    pub thread_id: String,
    pub instance_id: String,
    pub model: Option<String>,
    pub prompt: String,
    pub runtime_mode: String,
    pub interaction_mode: String,
    /// Ids of the attachments kept by the agent API (`upload.put`) that the turn's workspace is given before the CLI starts.
    pub attachments: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Finish {
    Completed,
    Interrupted,
    Failed,
}

/// What a thread's log says happened, neutral of any provider's own wire. `Other` is what t3 shows nothing of yet (usage, plans, file
/// changes, requests for an answer).
#[derive(Debug, Clone, PartialEq)]
pub enum AgentEventKind {
    TurnStarted,
    /// The user's own message, which t3 recorded itself when it was sent.
    UserText(String),
    Text(String),
    Reasoning(String),
    ToolStarted { id: String, name: String, input: Value },
    ToolOutput { id: String, data: String },
    ToolFinished { id: String, ok: bool },
    Failure(String),
    TurnFinished(Finish),
    Other,
}

/// `seq` is the thread's own event number; `turn` the turn the event belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentEvent {
    pub seq: u64,
    pub turn: Option<String>,
    pub kind: AgentEventKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct History {
    pub events: Vec<AgentEvent>,
    pub last_seq: u64,
    pub more: bool,
}

/// Whether a followed thread is still wanted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Follow {
    Continue,
    Stop,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DirEntry {
    pub name: String,
    pub directory: bool,
    pub size: u64,
}

/// What a client reports about itself: the contract's `service.clientActivity` params, with the scopes as the strings the service keeps.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientActivity {
    pub client: String,
    pub kind: String,
    pub visible: bool,
    pub focused: bool,
    pub recently_interacted: bool,
    pub app_state: Option<String>,
    pub low_power_mode: Option<String>,
    pub scopes: Vec<String>,
    pub ttl_ms: Option<u64>,
    pub observed_at_ms: u64,
}

/// A request to the agent API beyond a turn's own life: the answers, the queue, the lifecycle and the device state, in the contract's words
/// (snake_case, times in milliseconds). `run` answers the contract's result, or the refusal in the message T3 shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    ApprovalAnswer { thread: String, approval: String, decision: String },
    InputAnswer { thread: String, request: String, answer: String },
    InputDismiss { thread: String, request: String },
    QueueAdd { thread: String, item: String, prompt: String },
    QueueEdit { thread: String, item: String, prompt: String },
    QueueMove { thread: String, item: String, before: Option<String> },
    QueueDelete { thread: String, item: String },
    QueueResume { thread: String, item: Option<String> },
    TurnSteer { thread: String, turn: Option<String>, item: Option<String> },
    ThreadArchive { thread: String, archived: bool },
    ThreadDelete { thread: String, only_if_unused: bool },
    ThreadFork { thread: String, fork: String, turn: Option<String>, title: Option<String> },
    /// `state` is `settled`, `active`, `snoozed` or `awake`.
    ThreadSettle { thread: String, state: &'static str, until_ms: Option<u64>, at_ms: Option<u64> },
    ThreadSearch { query: String, limit: u32 },
    ThreadFamily { thread: String },
    ThreadForceStop { thread: String, turn: Option<String> },
    ItemGet { thread: String, seq: u64 },
    WorkflowStop { thread: String, turn: String, workflow: Option<String> },
    ThreadMergeBack { thread: String, into: String, turn: Option<String> },
    ThreadRollback { thread: String, turn: String, restore_files: bool },
    ThreadDetach { thread: String, session: String },
    TurnRetry { thread: String, turn: String },
    /// `shelf` is `archived` or `settled`; `before` is the (time in ms, thread) a page starts after.
    ThreadShelf { shelf: &'static str, before: Option<(u64, Option<String>)>, limit: u32 },
    StateGet { scope: String, key: String },
    StateSet { scope: String, key: String, value: Value },
    ClientActivity(ClientActivity),
    /// The files and worktrees of a project: `root` is the project's directory (a worktree's path, or the project's own), `path` is under it
    /// unless it is absolute (`FileBrowse`).
    FileStat { root: String, path: String },
    FileWrite { root: String, path: String, content: String },
    FileBrowse { root: String, path: String, show_hidden: bool },
    /// A directory made in the executor's tree (`files.edit`): `path` is under the executor's root, relative or absolute. `parents` makes the
    /// missing ones and accepts a directory that exists; without it the name is claimed and a taken one is refused (`ALREADY_EXISTS`).
    FileMkdir { path: String, parents: bool },
    /// A repository made in the directory `path` below the executor's root (`git` group), with `files` (path, content) written as new files and
    /// committed under `message` when there are any. The answer says why a commit did not happen (`commit_error`).
    GitInit { path: String, initial_branch: Option<String>, files: Vec<(String, String)>, message: Option<String> },
    /// The directory `path` below the executor's root made a workspace of the agent API named `workspace`, and forgotten again (`workspace`
    /// group).
    WorkspaceCreate { workspace: String, path: String },
    WorkspaceRemove { workspace: String },
    FileSearch { root: String, query: String, limit: u32 },
    /// The changes under `root` since `base`.
    TurnDiff { root: String, base: String },
    /// The changes under `root` since the revision it is at.
    ThreadDiff { root: String },
    WorktreeCreate { root: String, worktree: String, branch: String, base: Option<String> },
    WorktreeSetupCancel { root: String },
    /// The terminals of a project's directory: `terminal` is the name in the agent API (T3's thread and terminal ids joined), `cwd` the
    /// directory the shell starts in.
    TerminalOpen { root: String, terminal: String, cwd: String, cols: Option<u16>, rows: Option<u16>, env: Vec<(String, String)> },
    TerminalWrite { root: String, terminal: String, data: String },
    TerminalResize { root: String, terminal: String, cols: u16, rows: u16 },
    TerminalClear { root: String, terminal: String },
    TerminalClose { root: String, terminal: String },
    /// The repository of a project's directory (`git` group). `kind` of `GitRefs` is `branch` or `remote`, none for every kind.
    GitStatus { root: String },
    GitUpstream { root: String },
    GitDiff { root: String, base: Option<String>, head: Option<String>, path: Option<String> },
    GitRefs { root: String, kind: Option<&'static str>, limit: u32 },
    GitHistory { root: String, rev: Option<String>, limit: u32 },
    GitCheckout { root: String, target: String, track: bool },
    /// `from` is the revision the new branch starts at (the checkout's head when none).
    GitCreateRef { root: String, name: String, from: Option<String>, checkout: bool },
    /// Fetch pull request heads and branches of the remote into the repository's own `refs/rca/` refs: each refspec is `+source:destination`; the
    /// answer's `refs` name the commit each destination holds.
    GitFetch { root: String, refspecs: Vec<String> },
    /// How `head` differs from `base`: the merge base, and the commits and files of the change (at most `limit` commits).
    GitCompare { root: String, base: String, head: String, limit: u32 },
    /// The worktrees of a project's repository.
    WorktreeList { root: String },
    GitPull { root: String },
    /// `text` of the answer is the file as text (the adapter decodes the contract's base64).
    GitBlob { root: String, rev: String, path: String },
    GitStage { root: String, paths: Vec<String> },
    GitCommit { root: String, message: String },
    GitPush { root: String, set_upstream: bool },
    /// The provider CLIs and their logins (`accounts` group). A reply names a status and a login prompt, never a token.
    AccountsList,
    AccountsRefresh { provider: Option<String> },
    AccountsSelect { provider: String },
    AuthStart { provider: String, method: Option<String> },
    AuthRespond { auth: String, text: String },
    AuthComplete { auth: String, timeout_ms: u64 },
    AuthCancel { auth: String },
    AccountsLogout { provider: String },
    UsageSummary,
    ResetCredit { provider: String },
    /// A request of the hosted source control groups (`pr.*`, `clone.*`, `sourceControl.*`): `method` is the agent API method, `root` the
    /// project's directory (it picks the workspace) and `params` the method's params without the workspace.
    Hosted { method: &'static str, root: String, params: Value },
    /// A request of the `schedules` group (`schedule.*`): `method` is the agent API method and `params` its params. `root` is the project's
    /// directory when the schedule runs in its workspace (it picks the `workspace` the service is told).
    Schedule { method: &'static str, root: Option<String>, params: Value },
    /// A request of the `attachments` group (`upload.put`, `attachment.get`, `attachment.delete`): `method` is the agent API method.
    Attachment { method: &'static str, params: Value },
    /// A request of the `preview.sessions` group (`preview.open`, `preview.resize`, ..., `preview.subscribe` for where the stream stands): `method` is
    /// the agent API method.
    Preview { method: &'static str, params: Value },
}

impl Op {
    /// The agent API method the request is, which a double and a trace name it by.
    pub fn method(&self) -> &'static str {
        match self {
            Op::ApprovalAnswer { .. } => "approval.answer",
            Op::InputAnswer { .. } => "input.answer",
            Op::InputDismiss { .. } => "input.dismiss",
            Op::QueueAdd { .. } => "queue.add",
            Op::QueueEdit { .. } => "queue.edit",
            Op::QueueMove { .. } => "queue.move",
            Op::QueueDelete { .. } => "queue.delete",
            Op::QueueResume { .. } => "queue.resume",
            Op::TurnSteer { .. } => "turn.steer",
            Op::ThreadArchive { .. } => "thread.archive",
            Op::ThreadDelete { .. } => "thread.delete",
            Op::ThreadFork { .. } => "thread.fork",
            Op::ThreadSettle { .. } => "thread.settle",
            Op::ThreadSearch { .. } => "thread.search",
            Op::ThreadFamily { .. } => "thread.family",
            Op::ThreadForceStop { .. } => "thread.forceStop",
            Op::ItemGet { .. } => "item.get",
            Op::WorkflowStop { .. } => "workflow.stop",
            Op::ThreadMergeBack { .. } => "thread.mergeBack",
            Op::ThreadRollback { .. } => "thread.rollback",
            Op::ThreadDetach { .. } => "thread.detach",
            Op::TurnRetry { .. } => "turn.retry",
            Op::ThreadShelf { .. } => "thread.shelf",
            Op::StateGet { .. } => "state.get",
            Op::StateSet { .. } => "state.set",
            Op::ClientActivity(_) => "service.clientActivity",
            Op::FileStat { .. } => "file.stat",
            Op::FileWrite { .. } => "file.write",
            Op::FileBrowse { .. } => "file.browse",
            Op::FileMkdir { .. } => "file.mkdir",
            Op::GitInit { .. } => "git.init",
            Op::WorkspaceCreate { .. } => "workspace.create",
            Op::WorkspaceRemove { .. } => "workspace.remove",
            Op::FileSearch { .. } => "file.search",
            Op::TurnDiff { .. } => "turn.diff",
            Op::ThreadDiff { .. } => "thread.diff",
            Op::WorktreeCreate { .. } => "worktree.create",
            Op::WorktreeSetupCancel { .. } => "worktree.setup.cancel",
            Op::TerminalOpen { .. } => "terminal.open",
            Op::TerminalWrite { .. } => "terminal.write",
            Op::TerminalResize { .. } => "terminal.resize",
            Op::TerminalClear { .. } => "terminal.clear",
            Op::TerminalClose { .. } => "terminal.close",
            Op::GitStatus { .. } => "git.status",
            Op::GitUpstream { .. } => "git.upstreamStatus",
            Op::GitDiff { .. } => "git.diff",
            Op::GitRefs { .. } => "git.refs",
            Op::GitHistory { .. } => "git.history",
            Op::GitCheckout { .. } => "git.checkout",
            Op::GitCreateRef { .. } => "git.createRef",
            Op::GitFetch { .. } => "git.fetch",
            Op::GitCompare { .. } => "git.compare",
            Op::WorktreeList { .. } => "worktree.list",
            Op::GitPull { .. } => "git.pull",
            Op::GitBlob { .. } => "git.blob",
            Op::GitStage { .. } => "git.stage",
            Op::GitCommit { .. } => "git.commit",
            Op::GitPush { .. } => "git.push",
            Op::AccountsList => "provider.list",
            Op::AccountsRefresh { .. } => "provider.refresh",
            Op::AccountsSelect { .. } => "provider.select",
            Op::AuthStart { .. } => "provider.auth.start",
            Op::AuthRespond { .. } => "provider.auth.respond",
            Op::AuthComplete { .. } => "provider.auth.complete",
            Op::AuthCancel { .. } => "provider.auth.cancel",
            Op::AccountsLogout { .. } => "provider.logout",
            Op::UsageSummary => "usage.summary",
            Op::ResetCredit { .. } => "provider.resetCredit",
            Op::Hosted { method, .. } | Op::Schedule { method, .. } | Op::Attachment { method, .. } | Op::Preview { method, .. } => method,
        }
    }
}

/// What the machine t3 serve runs on has, as `server.getHostResources` shows it. A reading the machine cannot give is none.
#[derive(Debug, Clone, PartialEq)]
pub struct HostSample {
    pub cpu_count: u64,
    /// The share of the processors busy over the last look, 0 to 1.
    pub cpu_utilization: Option<f64>,
    pub available_memory_bytes: u64,
    pub total_memory_bytes: u64,
}

pub trait HostProbe: Send + Sync {
    fn sample(&self) -> Result<HostSample, String>;
}

/// What t3 serve's own database holds, as `server.getDatabaseStorage` shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct StorageSample {
    pub database_bytes: u64,
    /// Free pages inside the file that a vacuum returns to disk.
    pub reclaimable_bytes: u64,
    pub incremental_vacuum: bool,
    pub path: String,
}

pub trait StorageProbe: Send + Sync {
    fn sample(&self) -> Result<StorageSample, String>;
}

/// Everything t3 serve asks of the agent API (rca's `agent.threads`, `agent.requests`, `agent.queue`, `agent.lifecycle`, `state` and `files`
/// groups): turns, the threads' logs, the requests and queues of a thread, its lifecycle, device state and the project files in the workspace
/// the executor works in.
pub trait AgentApi: Send + Sync {
    /// Whether there is an agent API to ask. A server started without one keeps its rows to itself: a project or a thread it makes is not asked
    /// of anything, and what needs the agent API says so when it is asked for.
    fn configured(&self) -> bool {
        true
    }
    /// One request of `Op`: the contract's result.
    fn run(&self, op: &Op) -> Result<Value, AgentError>;
    /// The thread under t3's id, made when rca has none (a thread that exists is left as it is): the number of its newest event.
    fn ensure_thread(&self, thread: &AgentThread) -> Result<u64, AgentError>;
    /// Starts a turn on an idle thread: the id of the turn that began.
    fn start_turn(&self, turn: &AgentTurn) -> Result<String, AgentError>;
    /// Asks the running turn to stop; a thread with nothing to stop is not an error.
    fn interrupt(&self, thread_id: &str) -> Result<(), AgentError>;
    /// Calls `sink` with each event after `after_seq`, those the thread already has first, until it answers `Stop`.
    fn follow(&self, thread_id: &str, after_seq: u64, sink: &mut dyn FnMut(AgentEvent) -> Follow) -> Result<(), AgentError>;
    /// Up to `limit` events after `after_seq`.
    fn history(&self, thread_id: &str, after_seq: u64, limit: u32) -> Result<History, AgentError>;
    fn list_dir(&self, root: &str, path: &str) -> Result<Vec<DirEntry>, AgentError>;
    fn read_file(&self, root: &str, path: &str) -> Result<String, AgentError>;
    /// The setup events of the worktree at `root` after `after_seq`, those it has first, waiting up to `wait` for the setup to finish.
    fn setup_events(&self, root: &str, after_seq: u64, wait: std::time::Duration) -> Result<SetupBatch, AgentError>;
    /// What the terminal `terminal` of the directory `root` did after the byte offset `after_offset` (a snapshot of what is kept when none is
    /// given, or when the offset is older than what is kept), waiting up to `wait` for something to happen.
    fn terminal_events(&self, root: &str, terminal: &str, after_offset: Option<u64>, wait: std::time::Duration) -> Result<TerminalBatch, AgentError>;
    /// Every terminal of the project's workspace and its worktrees.
    fn terminals(&self) -> Result<Vec<TerminalSummary>, AgentError>;
    /// Calls `sink` with the status of the repository at `root` (the executor's `git status` result, as the contract has it) at once and each
    /// time it changes, and with `None` about every second while it does not, until the sink answers `Stop`.
    fn git_follow(&self, root: &str, sink: &mut dyn FnMut(Option<Value>) -> Follow) -> Result<(), AgentError>;
    /// Calls `sink` with the login session `auth` (the contract's `AuthView`) as it stood at each change, from the first, until the sink answers
    /// `Stop` or the session has ended (its ending is the last call); null is a call about every second while nothing changes, so a sink can stop.
    fn auth_follow(&self, auth: &str, sink: &mut dyn FnMut(Value) -> Follow) -> Result<(), AgentError>;
    /// Calls `sink` with each state of the clones of the workspace `root` is in (`{clone, state, repo, detail, seq}`), those the log has first,
    /// and with `None` about every second while nothing changes, until the sink answers `Stop`.
    fn clone_follow(&self, root: &str, sink: &mut dyn FnMut(Option<Value>) -> Follow) -> Result<(), AgentError>;
    /// Calls `sink` with each event of the `schedules` stream (`{seq, type, ...}`: a change to a schedule, a run, a webhook request) after the
    /// subscription, and with `None` about every second while nothing happens, until the sink answers `Stop`.
    fn schedule_follow(&self, sink: &mut dyn FnMut(Option<Value>) -> Follow) -> Result<(), AgentError>;
    /// Calls `sink` with each event of the `preview` stream after number `after_seq` of the run `epoch` (`{seq, type, thread, tab, at_ms, epoch,
    /// revision, ...}`), and with `None` about every second while nothing happens, until the sink answers `Stop`.
    fn preview_follow(&self, epoch: &str, after_seq: u64, sink: &mut dyn FnMut(Option<Value>) -> Follow) -> Result<(), AgentError>;
}

/// One thing a terminal's stream said. Offsets count the bytes of output the terminal produced.
#[derive(Debug, Clone, PartialEq)]
pub enum TerminalEvent {
    /// The kept output from `start` to `offset` and the terminal as it stands; `reset` when the reader's offset was older than what is kept.
    Snapshot { start: u64, offset: u64, data: Vec<u8>, reset: bool, cols: u16, rows: u16, running: bool, exit_code: Option<i32> },
    Output { offset: u64, end: u64, data: Vec<u8> },
    Exited { exit_code: Option<i32> },
    Closed,
    Cleared,
}

impl TerminalEvent {
    /// The offset the stream stands at after the event, for the one that moves it.
    pub fn end(&self) -> Option<u64> {
        match self {
            TerminalEvent::Snapshot { offset, .. } => Some(*offset),
            TerminalEvent::Output { end, .. } => Some(*end),
            _ => None,
        }
    }
}

/// What `terminal_events` found. `found` is false when the agent API has no such terminal.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TerminalBatch {
    pub found: bool,
    pub events: Vec<TerminalEvent>,
}

/// A terminal as the agent API lists it: its name there, the directory its workspace is (a worktree's path, none for the project's own) and
/// how it stands.
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalSummary {
    pub name: String,
    pub cwd: String,
    pub worktree_path: Option<String>,
    pub title: String,
    pub running: bool,
    pub exit_code: Option<i32>,
    pub pid: Option<u32>,
    pub updated_at_ms: u64,
}

/// One progress event of a worktree's setup: the agent API's own kind (`setup.started`, `step.started`, `step.output`, `step.finished`,
/// `setup.finished`) and its fields.
#[derive(Debug, Clone, PartialEq)]
pub struct SetupEvent {
    pub seq: u64,
    pub kind: String,
    pub data: Value,
}

/// What `setup_events` found. `ran` is false when no setup ran in the worktree.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SetupBatch {
    pub ran: bool,
    pub events: Vec<SetupEvent>,
}
