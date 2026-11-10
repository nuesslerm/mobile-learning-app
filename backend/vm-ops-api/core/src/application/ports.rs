//! What the service needs from outside, in its own words. `vm-ops-api-rs` implements each; `testing` holds a double of each.
use async_trait::async_trait;
use serde_json::Value;
use std::time::Duration;

#[async_trait]
pub trait Clock: Send + Sync {
    /// Milliseconds since the Unix epoch.
    fn now_ms(&self) -> i64;
    async fn sleep(&self, duration: Duration);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }
}

/// One named event with its fields, in the order they are written.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub level: Level,
    pub message: &'static str,
    pub attributes: Vec<(&'static str, Value)>,
}

impl Event {
    pub fn info(message: &'static str) -> Self {
        Self { level: Level::Info, message, attributes: Vec::new() }
    }

    pub fn warn(message: &'static str) -> Self {
        Self { level: Level::Warn, message, attributes: Vec::new() }
    }

    pub fn error(message: &'static str) -> Self {
        Self { level: Level::Error, message, attributes: Vec::new() }
    }

    pub fn with(mut self, key: &'static str, value: impl Into<Value>) -> Self {
        self.attributes.push((key, value.into()));
        self
    }
}

/// Where events go. It never fails and never waits: a lost event is the sink's to report.
pub trait EventLog: Send + Sync {
    fn emit(&self, event: Event);
}

pub trait ProcessMemory: Send + Sync {
    /// Resident memory of this process in whole megabytes, 0 when it cannot be read.
    fn rss_mb(&self) -> u64;
}

/// The host's `/proc` text files, named relative to its root (`meminfo`, `loadavg`).
#[async_trait]
pub trait HostFiles: Send + Sync {
    /// The file's text, read afresh; none when it is missing or cannot be read, which a caller treats as empty.
    async fn read_text(&self, name: &str) -> Option<String>;
}

pub trait Ids: Send + Sync {
    /// A random version 4 UUID in lowercase hyphenated text, as `crypto.randomUUID()` writes it.
    fn uuid(&self) -> String;
}

/// A JSON object with its keys in the order they were written.
pub type Object = serde_json::Map<String, Value>;

/// The state store could not write. The message is Node's `Error#message` for the failed call, as `ENOENT: no such file or
/// directory, open '<path>'`: a reply's `detail` carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateError(pub String);

impl std::fmt::Display for StateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for StateError {}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LedgerState {
    pub ledger: Object,
    pub version: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerWrite {
    pub ok: bool,
    pub version: i64,
}

/// The hosted pool-cooldown ledger with optimistic concurrency. Its version counts the writes since this process started,
/// so a caller that holds a version from before a restart is told once that it is stale, then converges.
#[async_trait]
pub trait CooldownLedger: Send + Sync {
    /// The stored ledger, empty when the store is missing, unreadable or not an object, at the current version.
    async fn read(&self) -> LedgerState;

    /// Stores `ledger` and moves the version on by one when `expected` is the current version. A stale write, and one that
    /// cannot be stored (which the adapter reports as the `cooldowns.persist_failed` event), are not ok and leave the version.
    async fn write(&self, ledger: Object, expected: i64) -> LedgerWrite;
}

/// A run's meta: `runId` and `startedAt` among the fields its writers chose, in the order they wrote them.
pub type RunMeta = Object;

/// The opencode run log, one directory per run. A run id is validated by the caller: the store takes it as it is.
#[async_trait]
pub trait RunLog: Send + Sync {
    /// The run's meta; none when the run has none, or its meta is not a JSON object.
    async fn read_meta(&self, run_id: &str) -> Option<RunMeta>;

    /// Stores the run's meta whole.
    async fn write_meta(&self, run_id: &str, meta: RunMeta) -> Result<(), StateError>;

    /// Appends each line, in order, to the run's lines; no line stores nothing.
    async fn append_lines(&self, run_id: &str, lines: &[String]) -> Result<(), StateError>;

    /// The run's last 5000 non-empty lines.
    async fn read_lines(&self, run_id: &str) -> Vec<String>;

    /// The meta of every run that has one, newest `startedAt` first.
    async fn list_runs(&self) -> Vec<RunMeta>;
}

/// Why a call to the container engine gave nothing: the Engine refused it, or could not be reached or read.
/// The message is Node's `Error#message`, a JavaScript string: a reply's `detail` carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainersError {
    Refused { status: u16, message: crate::domain::js::JsString },
    Failed { message: crate::domain::js::JsString },
}

impl ContainersError {
    /// `<what> returned <status>: ` and the first `units` UTF-16 units of the body, so a cut can leave half a surrogate pair.
    pub fn refused(what: &str, status: u16, body: &str, units: usize) -> Self {
        let lead = format!("{what} returned {status}: ");
        let cut = lead.encode_utf16().count() + units;
        Self::Refused { status, message: crate::domain::js::JsString::from(format!("{lead}{body}").as_str()).first(cut) }
    }

    pub fn failed(message: &str) -> Self {
        Self::Failed { message: crate::domain::js::JsString::from(message) }
    }

    pub fn message(&self) -> &crate::domain::js::JsString {
        match self {
            Self::Refused { message, .. } | Self::Failed { message } => message,
        }
    }
}

impl std::fmt::Display for ContainersError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message().to_text_lossy())
    }
}

impl std::error::Error for ContainersError {}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecOptions {
    /// Return once the command has started, without waiting for it or reading its output.
    pub detach: bool,
    /// `NAME=value` entries added to the command's environment.
    pub env: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecResult {
    /// None for a detached exec, and when the Engine's inspect names no exit code.
    pub exit_code: Option<i64>,
    /// Both streams with their frames joined; empty for a detached exec.
    pub output: String,
    /// Set for a detached exec only.
    pub exec_id: Option<String>,
}

impl ExecResult {
    pub fn attached(exit_code: Option<i64>, output: String) -> Self {
        Self { exit_code, output, exec_id: None }
    }

    pub fn detached(exec_id: String) -> Self {
        Self { exit_code: None, output: String::new(), exec_id: Some(exec_id) }
    }
}

/// The Engine's answer to a call whose status the caller reads for itself; the body is its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineReply {
    pub status: u16,
    pub body: String,
}

/// A container's logs as they arrive. Dropping it ends the follow and closes the connection to the Engine.
#[async_trait]
pub trait LogFollow: Send {
    /// The next frame's text, in order; none once the Engine ends the stream, which it does when the container stops.
    async fn next_text(&mut self) -> Result<Option<String>, ContainersError>;
}

/// The container engine: no call has a time limit, because a follow never ends and an exec lasts as long as its command.
#[async_trait]
pub trait Containers: Send + Sync {
    /// The containers in any state that carry one of the app's exact names, in the Engine's order; an app with no names has none.
    async fn list_by_app(&self, app: &str) -> Result<Vec<crate::domain::containers::DockerContainer>, ContainersError>;

    /// Every running container, whatever app it belongs to.
    async fn list_running(&self) -> Result<Vec<crate::domain::containers::DockerContainer>, ContainersError>;

    /// Runs `cmd`, a literal argv with no shell, in the container; the Engine refuses to create it with an `Err`.
    async fn exec(&self, container_id: &str, cmd: &[String], options: &ExecOptions) -> Result<ExecResult, ContainersError>;

    /// Puts the tar archive into `target_dir` of the container; whatever the Engine answers is the reply, refusals included.
    async fn put_file(&self, container_id: &str, target_dir: &str, tar: &[u8]) -> Result<EngineReply, ContainersError>;

    /// The last `tail` lines of both streams as text.
    async fn logs(&self, container_id: &str, tail: u32) -> Result<String, ContainersError>;

    /// The last `tail` lines, then everything the container writes. A refusal is the `Err` here, not of the first read.
    async fn follow_logs(&self, container_id: &str, tail: u32) -> Result<Box<dyn LogFollow>, ContainersError>;

    /// One sample of the container's memory and CPU.
    async fn stats(&self, container_id: &str) -> Result<crate::domain::container_stats::DockerStats, ContainersError>;
}

/// How the `docker` command of a sibling container ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiblingResult {
    /// Its exit code; none when a signal ended it, and -1 when it could not be started.
    pub code: Option<i32>,
    /// Both streams as text, with invalid bytes replaced; whole, never cut.
    pub stdout: String,
    pub stderr: String,
}

impl SiblingResult {
    pub fn exited(code: i32, stdout: &str, stderr: &str) -> Self {
        Self { code: Some(code), stdout: stdout.to_string(), stderr: stderr.to_string() }
    }

    /// The command never ran: code -1 and, as stderr, what `String(error)` is for a JavaScript `Error` with this message.
    pub fn not_started(message: &str) -> Self {
        Self { code: Some(-1), stdout: String::new(), stderr: format!("Error: {message}") }
    }
}

/// A detached sibling could not be started. The message is Node's `Error#message`, as `spawn docker ENOENT`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiblingError(pub String);

impl std::fmt::Display for SiblingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SiblingError {}

/// Sibling containers: short-lived `docker` commands on the host's daemon that do what this container cannot do to itself.
#[async_trait]
pub trait Siblings: Send + Sync {
    /// Runs `docker <args>` to the end, with no time limit. It never fails: a command that could not be started is a
    /// [`SiblingResult::not_started`].
    async fn run(&self, args: &[String]) -> SiblingResult;

    /// Starts `docker <args>` in a session of its own with no input or output, and returns without waiting: the command
    /// outlives this process. Node crashes on a command it cannot start, where this reports it.
    fn spawn_detached(&self, args: &[String]) -> Result<(), SiblingError>;
}

/// A body read a chunk at a time: an upload's bytes in, a stored entry's bytes out. Dropping it ends it.
#[async_trait]
pub trait Chunks: Send {
    /// The next chunk, none at the end. An error is a source that broke: the connection closed, or the file could not be read.
    async fn next_chunk(&mut self) -> std::io::Result<Option<Vec<u8>>>;
}
