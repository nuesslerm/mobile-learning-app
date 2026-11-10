use async_trait::async_trait;
use std::ffi::OsString;
use std::io;
use std::process::Stdio;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use vm_ops_api_core::application::ports::{SiblingError, SiblingResult, Siblings};

/// The names Node gives the errors it reports for a command that cannot be started; the rest read as the OS says them.
fn errno_name(error: &io::Error) -> Option<&'static str> {
    match error.raw_os_error()? {
        libc::ENOENT => Some("ENOENT"),
        libc::EACCES => Some("EACCES"),
        libc::EAGAIN => Some("EAGAIN"),
        libc::EMFILE => Some("EMFILE"),
        libc::ENFILE => Some("ENFILE"),
        _ => None,
    }
}

fn spawn_message(error: &io::Error) -> String {
    format!("spawn docker {}", errno_name(error).map_or_else(|| error.to_string(), str::to_string))
}

/// What Node reads from a pipe at once: each of its `data` chunks is decoded alone.
const CHUNK: usize = 64 * 1024;

struct Captured {
    text: String,
    failure: Option<io::Error>,
}

/// Each read is decoded alone, as Node decodes each chunk: a character cut across two reads is two replacement characters.
async fn read_all(stream: Option<impl AsyncRead + Unpin>) -> Captured {
    let (mut text, mut failure) = (String::new(), None);
    if let Some(mut stream) = stream {
        let mut chunk = vec![0u8; CHUNK];
        loop {
            match stream.read(&mut chunk).await {
                Ok(0) => break,
                Ok(read) => text.push_str(&String::from_utf8_lossy(&chunk[..read])),
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
    }
    Captured { text, failure }
}

/// The sibling containers: the host's `docker` CLI, found on PATH, which talks to the host's daemon through the mounted socket.
pub struct DockerSiblings {
    path: Option<OsString>,
}

impl Default for DockerSiblings {
    fn default() -> Self {
        Self::new()
    }
}

impl DockerSiblings {
    /// Looks `docker` up on this process's PATH, and hands the command this process's environment as it is.
    pub fn new() -> Self {
        Self { path: None }
    }

    /// As [`DockerSiblings::new`], but with this PATH for the lookup and in the command's environment.
    pub fn with_path(path: impl Into<OsString>) -> Self {
        Self { path: Some(path.into()) }
    }

    fn command(&self, args: &[String]) -> Command {
        let mut command = Command::new("docker");
        command.args(args);
        if let Some(path) = &self.path {
            command.env("PATH", path);
        }
        command
    }
}

#[async_trait]
impl Siblings for DockerSiblings {
    async fn run(&self, args: &[String]) -> SiblingResult {
        let mut command = self.command(args);
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => return SiblingResult::not_started(&spawn_message(&error)),
        };
        // Node leaves its stdin pipe open until the command is gone; `wait` would close it first.
        let _stdin = child.stdin.take();
        let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
        let (out, err, status) = tokio::join!(read_all(stdout), read_all(stderr), child.wait());
        let (mut code, mut failure) = match status {
            Ok(status) => (status.code(), None),
            Err(error) => (Some(-1), Some(error)),
        };
        failure = failure.or(out.failure).or(err.failure);
        let (stdout, mut stderr) = (out.text, err.text);
        if let Some(error) = failure {
            code = Some(-1);
            stderr.push_str(&format!("Error: {error}"));
        }
        SiblingResult { code, stdout, stderr }
    }

    /// Needs the tokio runtime, which reaps the command if it ends while this process still runs.
    fn spawn_detached(&self, args: &[String]) -> Result<(), SiblingError> {
        let mut command = self.command(args);
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        // SAFETY: between fork and exec the closure calls `setsid` only, which is async-signal-safe.
        unsafe {
            command.pre_exec(|| if libc::setsid() == -1 { Err(io::Error::last_os_error()) } else { Ok(()) });
        }
        command.spawn().map(drop).map_err(|error| SiblingError(spawn_message(&error)))
    }
}
