//! A thread's CLI runs at the workspace host, with its own tools and the credentials installed there: the executor's `session_open` starts its live process or
//! server and streams its output, `session_send` writes to its input, and this adapter hands both to the driver as a started process.
use async_trait::async_trait;
use rca_cli_driver::application::ports::{Link, Linker, Running};
use rca_cli_driver::domain::Driver;
use rca_exec_channel::remote::{ExecutorChannel, Streamed};
use rca_exec_channel::{SESSION_OPEN, SESSION_SEND};
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

const STDERR_TAIL: usize = 2000;

/// How the live process or server of a CLI gets started for a workspace's executor.
pub trait CliLauncher: Send + Sync {
    /// A launcher that starts nothing has the default: every start is refused.
    fn linker(&self, _: &Arc<dyn ExecutorChannel>) -> Arc<dyn Linker> {
        Arc::new(NoLinker)
    }
}

/// The linker of a launcher that starts no servers.
pub struct NoLinker;

#[async_trait]
impl Linker for NoLinker {
    async fn open(&self, driver: Driver, _: &Path, _: &[String]) -> Result<Link, String> {
        Err(format!("this launcher starts no live process for {}", driver.id()))
    }
}

/// The launcher of the service: the CLI runs where the workspace is.
pub struct AtWorkspace;

impl CliLauncher for AtWorkspace {
    fn linker(&self, channel: &Arc<dyn ExecutorChannel>) -> Arc<dyn Linker> {
        Arc::new(HostLinker { channel: Arc::clone(channel) })
    }
}

/// A launcher with one linker for every workspace, for the flows that script the CLIs' servers.
pub struct FixedLinker(pub Arc<dyn Linker>);

impl CliLauncher for FixedLinker {
    fn linker(&self, _: &Arc<dyn ExecutorChannel>) -> Arc<dyn Linker> {
        Arc::clone(&self.0)
    }
}

/// How long the executor may take to say a session has started.
const SESSION_START_WAIT: std::time::Duration = std::time::Duration::from_secs(60);

/// The live servers of CLIs at the workspace host: `session_open` starts the CLI and streams its output until it ends, `session_send` writes a line to
/// its input, and ending the open action (a drop of it) stops the process and everything it started.
pub struct HostLinker {
    channel: Arc<dyn ExecutorChannel>,
}

#[async_trait]
impl Linker for HostLinker {
    async fn open(&self, driver: Driver, cwd: &Path, flags: &[String]) -> Result<Link, String> {
        if self.channel.is_closed() {
            return Err("the workspace's executor is gone".into());
        }
        let session = format!("s-{}", uuid::Uuid::new_v4().simple());
        let mut request = json!({"session": session, "program": driver.program()});
        if !flags.is_empty() {
            request["args"] = json!(flags);
        }
        if cwd != Path::new(".") {
            request["cwd"] = json!(cwd.to_string_lossy());
        }
        let (stream, mut chunks) = mpsc::unbounded_channel();
        let channel = Arc::clone(&self.channel);
        let call = tokio::spawn(async move { channel.call_stream(SESSION_OPEN, request, stream).await });
        let abort = call.abort_handle();
        let (lines_tx, lines_rx) = mpsc::channel::<String>(64);
        let (done_tx, done_rx) = oneshot::channel();
        let (started_tx, started_rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            let (mut partial, mut tail, mut started) = (String::new(), String::new(), Some(started_tx));
            while let Some(item) = chunks.recv().await {
                match item {
                    Streamed::Event(event) if event["kind"] == "session.started" => {
                        if let Some(started) = started.take() {
                            let _ = started.send(());
                        }
                    }
                    Streamed::Output { stream, data } if stream == "stdout" => {
                        partial.push_str(&data);
                        while let Some(end) = partial.find('\n') {
                            let line: String = partial.drain(..=end).collect();
                            let _ = lines_tx.send(line.trim_end_matches(['\n', '\r']).to_string()).await;
                        }
                    }
                    Streamed::Output { data, .. } => {
                        tail.push_str(&data);
                        if tail.len() > STDERR_TAIL {
                            let mut cut = tail.len() - STDERR_TAIL;
                            while !tail.is_char_boundary(cut) {
                                cut += 1;
                            }
                            tail.drain(..cut);
                        }
                    }
                    _ => {}
                }
            }
            if !partial.is_empty() {
                let _ = lines_tx.send(partial).await;
            }
            drop(lines_tx);
            let outcome = match call.await {
                Ok(Ok(done)) => match done.result["exit_code"].as_i64() {
                    Some(0) | None => Ok(()),
                    Some(code) => Err(if tail.trim().is_empty() { format!("exited with code {code}") } else { tail.trim().to_string() }),
                },
                Ok(Err(e)) => Err(if tail.trim().is_empty() { e.message() } else { format!("{}: {}", e.message(), tail.trim()) }),
                // aborted: the session was closed here
                Err(_) => Ok(()),
            };
            drop(started);
            let _ = done_tx.send(outcome);
        });
        // Nothing may be sent before the executor has the session's input: it says so with its first event
        match tokio::time::timeout(SESSION_START_WAIT, started_rx).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => {
                let why = done_rx.await.unwrap_or(Ok(())).err().unwrap_or_else(|| "the session ended before it started".into());
                return Err(why);
            }
            Err(_) => {
                abort.abort();
                return Err("the executor did not start the session in time".into());
            }
        }
        let (input_tx, mut input_rx) = mpsc::unbounded_channel::<String>();
        let (channel, writer_abort) = (Arc::clone(&self.channel), abort.clone());
        tokio::spawn(async move {
            while let Some(line) = input_rx.recv().await {
                if channel.call(SESSION_SEND, json!({"session": session, "line": line})).await.is_err() {
                    // a line that cannot be written leaves the session out of step: it ends
                    writer_abort.abort();
                    break;
                }
            }
        });
        let send: Arc<dyn Fn(String) -> bool + Send + Sync> = Arc::new(move |line| input_tx.send(line).is_ok());
        let kill: Arc<dyn Fn() + Send + Sync> = Arc::new(move || abort.abort());
        let lines = futures_util::stream::unfold(lines_rx, |mut rx| async move { rx.recv().await.map(|line| (line, rx)) });
        Ok(Link { running: Running { lines: Box::pin(lines), finish: Box::pin(async move { done_rx.await.unwrap_or(Ok(())) }), kill }, send })
    }
}
