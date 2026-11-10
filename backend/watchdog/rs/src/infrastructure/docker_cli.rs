use async_trait::async_trait;
use serde_json::Value;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use watchdog_core::application::{Docker, PortError, Recreated};
use watchdog_core::domain::snapshot::{parse_snapshots, Snapshot};

const TIMEOUT: Duration = Duration::from_secs(300);
const ERROR_TAIL: usize = 500;

/// What one run of the `docker` binary gave back.
pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// The `docker` binary: argv in, exit code and output out.
#[async_trait]
pub trait DockerCommand: Send + Sync {
    async fn run(&self, args: &[&str]) -> Output;
}

/// The `docker` CLI on the PATH (the image carries it and the compose plugin; the socket is mounted).
pub struct DockerOnPath;

#[async_trait]
impl DockerCommand for DockerOnPath {
    async fn run(&self, args: &[&str]) -> Output {
        let mut command = Command::new("docker");
        command.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        match tokio::time::timeout(TIMEOUT, command.output()).await {
            Ok(Ok(output)) => Output {
                code: output.status.code().unwrap_or(-1),
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            },
            Ok(Err(error)) => Output { code: -1, stdout: String::new(), stderr: error.to_string() },
            Err(_) => Output { code: -1, stdout: String::new(), stderr: "timed out".into() },
        }
    }
}

/// The last 500 characters of what the command wrote to stderr, trimmed.
fn stderr_tail(output: &Output) -> String {
    let trimmed = output.stderr.trim();
    trimmed.chars().rev().take(ERROR_TAIL).collect::<Vec<_>>().into_iter().rev().collect()
}

/// The Docker port spoken through the `docker` CLI: `ps`, `inspect` and `compose`.
pub struct DockerCli {
    command: Arc<dyn DockerCommand>,
}

impl DockerCli {
    pub fn new(command: Arc<dyn DockerCommand>) -> Self {
        Self { command }
    }
}

#[async_trait]
impl Docker for DockerCli {
    async fn container_ids(&self) -> Result<Vec<String>, PortError> {
        let ps = self.command.run(&["ps", "-a", "--format", "{{json .}}"]).await;
        if ps.code != 0 {
            return Err(PortError(format!("docker ps failed: {}", ps.stderr.trim())));
        }
        ps.stdout
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_str::<Value>(line).map(|row| row["ID"].as_str().unwrap_or_default().to_string()).map_err(|e| PortError(e.to_string())))
            .collect()
    }

    async fn inspect(&self, ids: &[String]) -> Result<Vec<Snapshot>, PortError> {
        let mut args = vec!["inspect"];
        args.extend(ids.iter().map(String::as_str));
        let inspect = self.command.run(&args).await;
        if inspect.code != 0 && inspect.stdout.trim().is_empty() {
            return Err(PortError(format!("docker inspect failed: {}", inspect.stderr.trim())));
        }
        let text = if inspect.stdout.trim().is_empty() { "[]" } else { inspect.stdout.as_str() };
        serde_json::from_str::<Value>(text).map(|value| parse_snapshots(&value)).map_err(|e| PortError(e.to_string()))
    }

    async fn compose(&self, args: &[String]) -> Recreated {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let output = self.command.run(&args).await;
        if output.code == 0 {
            return Recreated { ok: true, error: None };
        }
        Recreated { ok: false, error: Some(stderr_tail(&output)) }
    }

    async fn services(&self, args: &[String]) -> Result<Vec<String>, PortError> {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let output = self.command.run(&args).await;
        if output.code != 0 {
            return Err(PortError(format!("docker compose config failed: {}", stderr_tail(&output))));
        }
        Ok(output.stdout.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_string).collect())
    }
}
