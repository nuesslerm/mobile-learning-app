//! The ports the application calls out through: an agent CLI (`contracts/providerAdapter.ts` and the event half of
//! `contracts/agentCliGateway.ts`: sessions, turns, interrupts, answers, and the stream of what the agent reports) and the working
//! tree of a thread. The event log's port is `store`. Adapters live in the service crate.
//! The service ports (opencode's HTTP API, programs, other HTTP services, the cooldown ledger) sit below the agent ones.

use crate::domain::agent::AgentEvent;
use crate::domain::types::ModelSelection;
use async_trait::async_trait;
use futures_util::Stream;
use serde_json::{Map, Value};
use std::path::Path;
use std::pin::Pin;
use std::time::Duration;

/// What the agent reports for a directory until the stream is dropped: an item ends the stream when it is `None`, and breaks it
/// when it is an `Err`.
pub type EventStream = Pin<Box<dyn Stream<Item = Result<AgentEvent, String>> + Send>>;

pub struct SendTurn<'a> {
    pub thread_id: &'a str,
    pub text: &'a str,
    pub model: &'a ModelSelection,
    pub runtime_mode: &'a str,
    pub interaction_mode: &'a str,
    /// File parts (a picture the model is to see) sent beside the text.
    pub files: &'a [Value],
}

#[async_trait]
pub trait Backend: Send + Sync {
    fn provider(&self) -> &str;
    /// Creates the agent session for a thread and remembers it; returns the agent's id for it.
    async fn start_session(&self, thread_id: &str, runtime_mode: &str) -> Result<String, String>;
    async fn send_turn(&self, input: &SendTurn<'_>) -> Result<(), String>;
    async fn interrupt_turn(&self, thread_id: &str) -> Result<(), String>;
    async fn respond_to_request(&self, thread_id: &str, request_id: &str, decision: &str) -> Result<(), String>;
    async fn respond_to_user_input(&self, thread_id: &str, request_id: &str, answers: &Map<String, Value>) -> Result<(), String>;
    async fn stop_session(&self, thread_id: &str) -> Result<(), String>;
    /// The agent's own id for a thread's session, once one exists.
    fn session_for(&self, thread_id: &str) -> Option<String>;
    /// Takes the session the engine recorded for a thread as the thread's own, after a restart that lost the agent's map.
    fn adopt_session(&self, thread_id: &str, session_id: &str);
    /// A stream of everything the agent reports for a directory, until dropped.
    async fn open_events(&self, directory: &str) -> Result<EventStream, String>;
}

/// One opencode reply, whatever its status.
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn json(&self) -> Result<Value, String> {
        serde_json::from_slice(&self.body).map_err(|e| e.to_string())
    }
}

/// The `data:` payload of each server-sent event, until the server ends the stream or it breaks.
pub type DataStream = Pin<Box<dyn Stream<Item = Result<String, String>> + Send>>;

/// `opencode serve`, addressed by path and query (`/session?directory=...`).
#[async_trait]
pub trait Opencode: Send + Sync {
    /// One exchange, the whole reply. An error is a connection or protocol failure, worded as the transport words it; an HTTP error
    /// status is a `Reply`.
    async fn request(&self, method: &str, path: &str, body: Option<&Value>, timeout: Duration) -> Result<Reply, String>;

    /// An event stream whose head has been read, so the server holds the subscription when this returns. Dropping it unsubscribes.
    async fn open_stream(&self, path: &str) -> Result<DataStream, String>;
}

#[derive(Debug)]
pub struct Output {
    pub stdout: String,
}

#[derive(Debug)]
pub struct ExecError {
    /// `Command failed: <command line>\n<stderr>`, or why the command could not start.
    pub message: String,
    pub stdout: String,
    pub stderr: String,
}

impl ExecError {
    /// A program that ran and exited with a failure.
    pub fn failed(program: &str, args: &[&str], stdout: String, stderr: String) -> Self {
        Self { message: format!("Command failed: {program} {}\n{stderr}", args.join(" ")), stdout, stderr }
    }

    /// A program that was killed for running past its time.
    pub fn timed_out(program: &str, args: &[&str], stdout: String, stderr: String) -> Self {
        Self { message: format!("Command failed: {program} {}\ntimed out", args.join(" ")), stdout, stderr }
    }

    /// A program that could not be started; `reason` is the OS error kind, such as `not found`.
    pub fn not_started(program: &str, reason: &str) -> Self {
        Self { message: format!("spawn {program} {}", reason.to_uppercase().replace(' ', "_")), stdout: String::new(), stderr: String::new() }
    }
}

/// A program run to completion with its output captured: `git` (only the diff stat of a one-shot run; a repository is read and changed by the
/// executor's `git` group), `gh`, `pnpm`, `sqlite3`, `pdftotext`, `tesseract`, `ffmpeg`,
/// whisper.cpp.
#[async_trait]
pub trait Commands: Send + Sync {
    async fn run(&self, program: &str, args: &[&str], cwd: Option<&Path>, timeout: Option<Duration>) -> Result<Output, ExecError>;
}

pub struct FormPart {
    pub name: String,
    /// A part with a file name is a file; one without is a text field.
    pub file_name: Option<String>,
    pub mime: Option<String>,
    pub bytes: Vec<u8>,
}

pub enum Payload {
    Json(Value),
    Form(Vec<FormPart>),
}

pub struct Post {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub basic_auth: Option<(String, String)>,
    pub payload: Payload,
}

pub struct Answer {
    pub status: u16,
    pub body: Vec<u8>,
}

#[derive(Debug)]
pub struct PostError {
    pub message: String,
    /// The same without the URL, for an error that is shown to a caller and whose URL carries a credential.
    pub without_url: String,
}

/// A POST to an HTTP service other than opencode: the speech-to-text engines, the edit-stats table, Telegram.
#[async_trait]
pub trait Outbound: Send + Sync {
    async fn post(&self, post: Post) -> Result<Answer, PostError>;
}

/// Where the model ladder keeps its pool cooldowns: the one store every caller of the ladder shares. An error says what is missing or
/// down, and a run that gets one does not start.
pub trait Ledgers: Send + Sync {
    fn open(&self, runtime: &tokio::runtime::Handle, cwd: &str) -> Result<Box<dyn rca_ladder::store::CooldownStore>, String>;
}

/// Text out of documents and pictures: a PDF, a Word document, the words in a picture.
#[async_trait]
pub trait DocumentText: Send + Sync {
    /// The text of every page, pages separated by a blank line. An error is the reason, worded for the caller; it can be empty.
    async fn pdf_text(&self, bytes: &[u8]) -> Result<String, String>;

    fn docx_text(&self, bytes: &[u8]) -> Result<String, String>;

    /// The words in a picture; nothing when it is too large or cannot be read, never an error.
    async fn ocr_text(&self, bytes: &[u8]) -> String;
}

/// What one recording said and how long each leg took.
#[derive(Debug, PartialEq)]
pub struct Transcription {
    pub transcript: String,
    pub ffmpeg_ms: u64,
    pub whisper_ms: u64,
    pub model_name: String,
    pub threads: u64,
}

/// A failed transcription with the class name the Node service threw it as (`WhisperError`, `Error`, ...): the edit-latency row
/// stores it as the error code.
#[derive(Debug, PartialEq)]
pub struct SttError {
    pub name: &'static str,
    pub message: String,
}

/// Speech to text: the words in a recording on disk.
#[async_trait]
pub trait SpeechToText: Send + Sync {
    async fn transcribe(&self, audio: &Path) -> Result<Transcription, SttError>;
}

/// The service's own data on the host where it runs: preview pages under `FORK_DIR`, the staging directories of one-shot runs and the state it
/// keeps. It is the service's volume and no workspace: a workspace's files are its executor's (`file.*` of the agent API), and no path of an
/// executor's root is ever resolved here (flow `agent_workspace`).
pub trait WorkspaceFiles: Send + Sync {
    /// The path with every symlink resolved, or none when it does not exist.
    fn canonical(&self, path: &Path) -> Option<std::path::PathBuf>;
    fn exists(&self, path: &Path) -> bool;
    fn read(&self, path: &Path) -> Option<Vec<u8>>;
    /// Creates the directory and its parents.
    fn create_dir(&self, path: &Path) -> Result<(), String>;
    fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), String>;
    /// Appends to the file, creating it and its directory first.
    fn append(&self, path: &Path, bytes: &[u8]) -> Result<(), String>;
    /// Removes a file or a directory tree; a missing one is not an error.
    fn remove(&self, path: &Path);
    /// The entries directly in the directory; none when it cannot be read.
    fn list(&self, dir: &Path) -> Vec<std::path::PathBuf>;
    /// Every regular file under the directory with its bytes, at any depth.
    fn files_under(&self, dir: &Path) -> Vec<(std::path::PathBuf, Vec<u8>)>;
    /// How long ago the file was last modified.
    fn age(&self, path: &Path) -> Option<Duration>;
    /// A fresh directory `<base>/<prefix><random>` readable by its owner only (`base` is created when missing); the caller removes it.
    fn temp_dir(&self, base: &Path, prefix: &str) -> Result<std::path::PathBuf, String>;
}
