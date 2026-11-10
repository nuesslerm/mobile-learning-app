//! What running a turn needs from outside: a program started in a directory.
use async_trait::async_trait;
use futures_util::Stream;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

/// A started provider process: its stdout lines, how it ended, and a way to kill it.
pub struct Running {
    pub lines: Pin<Box<dyn Stream<Item = String> + Send>>,
    /// Resolves when the process has ended: `Err` carries the tail of stderr (or why it could not finish) after a failure.
    pub finish: Pin<Box<dyn Future<Output = Result<(), String>> + Send>>,
    pub kill: Arc<dyn Fn() + Send + Sync>,
}

#[async_trait]
pub trait Spawner: Send + Sync {
    /// `Err` is why the program could not start (such as `not found`).
    async fn spawn(&self, program: &str, args: &[String], cwd: &Path) -> Result<Running, String>;
}
