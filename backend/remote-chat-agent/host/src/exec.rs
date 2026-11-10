//! The real adapter of the CLI driver's `Spawner`: provider CLIs as tokio child processes. A CLI of a provider starts with that provider's
//! environment alone (`provider_env`): the shared variables, its own prefixed credentials and its directory under the providers' root, so it
//! cannot reach another provider's login nor the service's own secret.
use async_trait::async_trait;
use rca_cli_driver::application::ports::{Running, Spawner};
use rca_cli_driver::domain::{driver_of_command, provider_env};
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

/// The environment of this process as pairs, for `provider_env` to choose from.
pub fn inherited() -> Vec<(String, String)> {
    std::env::vars().collect()
}

pub struct Processes {
    root: String,
}

impl Processes {
    /// Provider CLIs keep their logins under `root`.
    pub fn at(root: impl Into<String>) -> Processes {
        Processes { root: root.into() }
    }
}

/// A child process as the callers hold it: its lines, how it ended, a kill, and a way to write to its input when it was started with one.
pub struct Supervised {
    pub running: Running,
    pub input: Option<tokio::sync::mpsc::UnboundedSender<String>>,
}

/// Takes over `child`: stdout lines go to `lines` (and stderr's too when `merge_stderr`, for a login that prints its prompt there), the end of
/// the process is reported with the tail of stderr on a failure, and `kill` ends it.
pub fn supervise(mut child: Child, merge_stderr: bool) -> Result<Supervised, String> {
    let mut stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?).lines();
    let mut stderr = child.stderr.take().ok_or("no stderr")?;
    let stdin = child.stdin.take();
    let (lines_tx, lines_rx) = tokio::sync::mpsc::channel::<String>(64);
    let (kill_tx, mut kill_rx) = tokio::sync::mpsc::channel::<()>(1);
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
    let input = stdin.map(|mut pipe| {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        tokio::spawn(async move {
            while let Some(line) = rx.recv().await {
                if pipe.write_all(format!("{line}\n").as_bytes()).await.is_err() || pipe.flush().await.is_err() {
                    break;
                }
            }
        });
        tx
    });
    let stderr_lines = merge_stderr.then(|| lines_tx.clone());
    tokio::spawn(async move {
        let mut killed = false;
        let mut tail = String::new();
        let mut err_pipe = tokio::spawn(async move {
            let mut text = String::new();
            match stderr_lines {
                Some(lines) => {
                    let mut reader = BufReader::new(stderr).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        text.push_str(&line);
                        text.push('\n');
                        let _ = lines.send(line).await;
                    }
                }
                None => {
                    let _ = stderr.read_to_string(&mut text).await;
                }
            }
            text
        });
        loop {
            tokio::select! {
                line = stdout.next_line() => match line {
                    Ok(Some(line)) => { if lines_tx.send(line).await.is_err() { killed = true; let _ = child.kill().await; break; } }
                    _ => break,
                },
                _ = kill_rx.recv() => { killed = true; let _ = child.kill().await; break; }
            }
        }
        drop(lines_tx);
        let status = child.wait().await;
        if let Ok(text) = (&mut err_pipe).await {
            tail = text.chars().rev().take(400).collect::<Vec<_>>().into_iter().rev().collect();
        }
        let _ = done_tx.send(match status {
            _ if killed => Ok(()),
            Ok(s) if s.success() => Ok(()),
            Ok(s) => Err(format!("exited with {s}: {}", tail.trim())),
            Err(e) => Err(e.to_string()),
        });
    });
    let kill = Arc::new(move || {
        let _ = kill_tx.try_send(());
    });
    let running = Running {
        lines: Box::pin(tokio_stream::wrappers::ReceiverStream::new(lines_rx)),
        finish: Box::pin(async move { done_rx.await.unwrap_or_else(|_| Err("the supervisor ended".into())) }),
        kill,
    };
    Ok(Supervised { running, input })
}

/// `command` with the environment of the provider it starts (none of the service's own, when it starts no provider's CLI beyond the shared
/// variables of `provider_env`'s rule) and the working directory `cwd`.
pub fn provider_command(root: &str, program: &str, args: &[String], cwd: &Path) -> Command {
    let mut command = Command::new(program);
    command.args(args).current_dir(cwd);
    if let Some(driver) = driver_of_command(program, args) {
        command.env_clear().envs(provider_env(driver, root, &inherited()));
    }
    command
}

#[async_trait]
impl Spawner for Processes {
    async fn spawn(&self, program: &str, args: &[String], cwd: &Path) -> Result<Running, String> {
        let mut command = provider_command(&self.root, program, args, cwd);
        let child = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true).spawn().map_err(|e| e.kind().to_string())?;
        Ok(supervise(child, false)?.running)
    }
}
