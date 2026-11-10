//! Child processes and the host: a git query, a command captured under a timeout, the checked-out branch and the host name.
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub struct Out {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

pub fn capture(program: &str, args: &[String], cwd: Option<&Path>, timeout: Option<Duration>) -> Out {
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(c) = cwd {
        cmd.current_dir(c);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Out { code: 1, stdout: String::new(), stderr: e.to_string(), timed_out: false },
    };
    let (mut out_pipe, mut err_pipe) = (child.stdout.take(), child.stderr.take());
    let t_out = std::thread::spawn(move || {
        let mut s = Vec::new();
        if let Some(p) = out_pipe.as_mut() {
            let _ = std::io::Read::read_to_end(p, &mut s);
        }
        s
    });
    let t_err = std::thread::spawn(move || {
        let mut s = Vec::new();
        if let Some(p) = err_pipe.as_mut() {
            let _ = std::io::Read::read_to_end(p, &mut s);
        }
        s
    });
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {
                if timeout.is_some_and(|t| started.elapsed() > t) {
                    let _ = child.kill();
                    timed_out = true;
                    break child.wait().ok();
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => break None,
        }
    };
    let stdout = String::from_utf8_lossy(&t_out.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&t_err.join().unwrap_or_default()).into_owned();
    Out { code: status.and_then(|s| s.code()).unwrap_or(1), stdout, stderr, timed_out }
}

pub fn git_out(root: &Path, args: &[&str]) -> Option<String> {
    let o = Command::new("git").args(args).current_dir(root).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

pub fn current_branch() -> String {
    Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .stdin(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map_or_else(|| "main".to_string(), |o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

pub fn hostname() -> String {
    nix::unistd::gethostname().ok().and_then(|name| name.into_string().ok()).unwrap_or_else(|| "localhost".into())
}
