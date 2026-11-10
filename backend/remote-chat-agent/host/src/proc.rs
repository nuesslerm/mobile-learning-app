//! Running a command to completion with its output captured, the shape of `lib/shell.ts`'s `execFile` wrapper: a failure carries
//! the command line, stdout and stderr, and the output is capped at 4 MB.

pub use remote_chat_agent_core::application::ports::{ExecError, Output};
use remote_chat_agent_core::application::ports::Commands;
use async_trait::async_trait;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

const MAX_BUFFER: usize = 4 * 1024 * 1024;

/// At most `MAX_BUFFER` bytes of the pipe and whether there was more; the rest is read and dropped so the child never blocks on a
/// full pipe.
async fn capped(mut pipe: impl AsyncRead + Unpin) -> (Vec<u8>, bool) {
    let mut bytes = Vec::new();
    let _ = (&mut pipe).take(MAX_BUFFER as u64 + 1).read_to_end(&mut bytes).await;
    let over = bytes.len() > MAX_BUFFER;
    bytes.truncate(MAX_BUFFER);
    let _ = tokio::io::copy(&mut pipe, &mut tokio::io::sink()).await;
    (bytes, over)
}

/// The programs on this machine.
pub struct System;

#[async_trait]
impl Commands for System {
    async fn run(&self, program: &str, args: &[&str], cwd: Option<&Path>, timeout: Option<Duration>) -> Result<Output, ExecError> {
        run(program, args, cwd, timeout).await
    }
}

async fn run(file: &str, args: &[&str], cwd: Option<&Path>, timeout: Option<Duration>) -> Result<Output, ExecError> {
    let mut command = Command::new(file);
    command.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    let mut child = command
        .spawn()
        .map_err(|e| ExecError::not_started(file, &e.kind().to_string()))?;
    let stdout = child.stdout.take().map(|pipe| tokio::spawn(capped(pipe)));
    let stderr = child.stderr.take().map(|pipe| tokio::spawn(capped(pipe)));
    let text = |joined: Option<Result<(Vec<u8>, bool), tokio::task::JoinError>>| joined.and_then(Result::ok).unwrap_or_default();

    let waited = match timeout {
        Some(limit) => tokio::time::timeout(limit, child.wait()).await.ok(),
        None => Some(child.wait().await),
    };
    let Some(status) = waited else {
        let _ = child.kill().await;
        let (out, _) = text(match stdout { Some(task) => Some(task.await), None => None });
        let (err, _) = text(match stderr { Some(task) => Some(task.await), None => None });
        return Err(ExecError::timed_out(file, args, String::from_utf8_lossy(&out).into_owned(), String::from_utf8_lossy(&err).into_owned()));
    };
    let (out, out_over) = text(match stdout { Some(task) => Some(task.await), None => None });
    let (err, err_over) = text(match stderr { Some(task) => Some(task.await), None => None });
    let (out, err) = (String::from_utf8_lossy(&out).into_owned(), String::from_utf8_lossy(&err).into_owned());
    if out_over || err_over {
        return Err(ExecError { message: "stdout maxBuffer length exceeded".into(), stdout: out, stderr: err });
    }
    match status {
        Ok(status) if status.success() => Ok(Output { stdout: out }),
        _ => Err(ExecError::failed(file, args, out, err)),
    }
}
