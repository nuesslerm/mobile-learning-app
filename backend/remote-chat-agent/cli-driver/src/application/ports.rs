//! What running a turn needs from outside: a CLI's live process or server started in a directory, and the live sessions of a driver.
use crate::domain::{Driver, ExecEvent, TurnRequest, Verdict};
use async_trait::async_trait;
use futures_util::Stream;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;

/// A started provider process: its output lines, how it ended, and a way to kill it.
pub struct Running {
    pub lines: Pin<Box<dyn Stream<Item = String> + Send>>,
    /// Resolves when the process has ended: `Err` carries the tail of stderr (or why it could not finish) after a failure.
    pub finish: Pin<Box<dyn Future<Output = Result<(), String>> + Send>>,
    pub kill: Arc<dyn Fn() + Send + Sync>,
}

/// A CLI's server process that stays open: what `Running` has, and a way to write a line to its input (for opencode, a line is a request for its server
/// through the executor's bridge, and its output lines are the bridge's).
pub struct Link {
    pub running: Running,
    /// Queues one line for the process's input; false when the process is gone.
    pub send: Arc<dyn Fn(String) -> bool + Send + Sync>,
}

/// Where a CLI that lives is started: the driver's own live mode with `args` (the flags of one conversation) after it, in a directory, killed with
/// everything it started.
#[async_trait]
pub trait Linker: Send + Sync {
    /// `Err` is why the process could not start (such as `not found`).
    async fn open(&self, driver: Driver, cwd: &Path, args: &[String]) -> Result<Link, String>;
}

/// The live sessions of a CLI that lives: one process per workspace with many conversations in it (Codex), or one process for each conversation (Claude Code).
/// A turn is run on its thread's session, which is started and its conversation resumed as needed; what the CLI does arrives as the neutral events of
/// `ExecEvent`, ending with `Done` (or `Error`, then the end of the stream).
#[async_trait]
pub trait SessionDriver: Send + Sync {
    /// Makes sure the workspace's server runs, started in `cwd` through `linker` (a CLI with a process for each conversation starts it with the turn).
    async fn start(&self, workspace: &str, linker: &dyn Linker, cwd: &Path) -> Result<(), String>;
    /// Runs the turn of `req` on its thread, in the workspace's session: the thread is started, or resumed from `req.resume` when the session has
    /// not got it. `Err` is a refusal before anything ran.
    async fn turn(&self, workspace: &str, linker: &dyn Linker, req: &TurnRequest) -> Result<mpsc::Receiver<ExecEvent>, String>;
    /// Gives the thread's running turn another prompt, on the session, without ending anything.
    async fn steer(&self, thread: &str, prompt: &str) -> Result<(), String>;
    /// Asks the thread's running turn to stop; whether there was one.
    fn interrupt(&self, thread: &str) -> bool;
    /// Answers the approval `approval` the thread's turn asked for; whether it was waiting.
    fn answer(&self, thread: &str, approval: &str, verdict: Verdict) -> bool;
    /// Answers the question `request` the thread's turn asked (`None`: the person closed it); whether it was waiting.
    fn answer_input(&self, thread: &str, request: &str, answer: Option<String>) -> bool;
    /// Whether the thread has a live process now.
    fn alive(&self, thread: &str) -> bool;
    /// The thread's conversation is let go: its next turn starts a new one.
    fn forget(&self, thread: &str);
    /// Ends the workspace's server and everything it started; whether there was one.
    fn close(&self, workspace: &str) -> bool;
}
