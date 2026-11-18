//! `target = "chain"`: the candidate is a set of processes a `launch` command starts (t3-serve over rca-service and rca-exec). The command prints
//! one JSON line when the chain listens and serves until its stdin closes; the replay then plays it as a `url` target and closes the stdin after.
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const STOP_WAIT: Duration = Duration::from_secs(20);

pub struct Launched {
    child: Child,
    pub addr: String,
    pub workspace: String,
    pub mint: String,
    /// The link an `orca` sidecar pairs with, when the launcher printed one.
    pub pairing: Option<String>,
    pub cli_dir: Option<PathBuf>,
}

/// Starts `argv` (its program resolved from `dir` when it has a slash) and waits up to `wait` for its listening line.
pub fn launch(argv: &[String], dir: &std::path::Path, wait: Duration) -> Result<Launched, String> {
    let program = if argv[0].contains('/') { dir.join(&argv[0]) } else { PathBuf::from(&argv[0]) };
    let mut child = Command::new(&program).args(&argv[1..]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().map_err(|e| format!("the chain launcher {}: {e}", program.display()))?;
    let stdout = child.stdout.take().ok_or("the chain launcher has no stdout")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(stdout).read_line(&mut line);
        let _ = tx.send(line);
    });
    let mut launched = Launched { child, addr: String::new(), workspace: String::new(), mint: String::new(), pairing: None, cli_dir: None };
    let line = rx.recv_timeout(wait).map_err(|_| format!("the chain launcher {} did not say it listens within {} s", program.display(), wait.as_secs()))?;
    let said: Value = serde_json::from_str(line.trim()).map_err(|e| format!("the chain launcher printed {line:?}, not a JSON line ({e})"))?;
    if said["ok"] != true || said["event"] != "listening" {
        return Err(format!("the chain launcher did not listen: {said}"));
    }
    let text = |k: &str| said[k].as_str().map(String::from).ok_or_else(|| format!("the chain launcher's line has no {k}: {said}"));
    launched.pairing = said["pairing"].as_str().map(String::from);
    if launched.pairing.is_none() {
        launched.addr = text("addr")?;
        launched.workspace = text("workspace")?;
    }
    launched.mint = said["mint"].as_str().unwrap_or_default().to_string();
    launched.cli_dir = said["cli_dir"].as_str().map(PathBuf::from);
    Ok(launched)
}

impl Drop for Launched {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        let until = Instant::now() + STOP_WAIT;
        while Instant::now() < until {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
