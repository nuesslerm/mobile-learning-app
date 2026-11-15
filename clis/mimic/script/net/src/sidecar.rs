//! The phone behind a child process: one JSON object per line on its stdin, one answer per line on its stdout. The child is the codec
//! sidecar that wraps the real client's sealed session (Orca's `DirectRpcClient`), so the keys and the handshake stay in the client's own
//! code. Ops: `connect {timeout_ms}` answers `{state}`; `request {method, params, timeout_ms}` answers `{ok: true, payload}` or
//! `{ok: false, error}`; `disconnect` ends the session and `close` ends the child. The sidecar masks the paths its host makes (user data dir).
use mimic_script_core::Phone;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::rc::Rc;

pub struct Sidecar {
    child: Child,
    stdin: ChildStdin,
    lines: BufReader<ChildStdout>,
}

impl Sidecar {
    /// Starts `program` with exactly `env` (nothing of the caller's environment reaches it).
    pub fn spawn(program: &str, args: &[String], env: &[(String, String)], cwd: Option<&std::path::Path>) -> Result<Sidecar, String> {
        let mut command = Command::new(program);
        command.args(args).env_clear().envs(env.iter().cloned()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit());
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        let mut child = command.spawn().map_err(|e| format!("{program}: {e}"))?;
        let stdin = child.stdin.take().ok_or("the sidecar has no stdin")?;
        let lines = BufReader::new(child.stdout.take().ok_or("the sidecar has no stdout")?);
        Ok(Sidecar { child, stdin, lines })
    }

    fn call(&mut self, message: &Value) -> Result<Value, String> {
        writeln!(self.stdin, "{message}").and_then(|()| self.stdin.flush()).map_err(|e| format!("the sidecar is gone: {e}"))?;
        let mut line = String::new();
        if self.lines.read_line(&mut line).map_err(|e| format!("the sidecar is gone: {e}"))? == 0 {
            return Err("the sidecar exited".into());
        }
        serde_json::from_str(line.trim()).map_err(|e| format!("the sidecar answered {line:?}: {e}"))
    }

    pub fn stop(&mut self) {
        let _ = self.call(&json!({"op": "close"}));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A handle on one sidecar: the interpreter holds one, the server that started it holds another to stop it.
#[derive(Clone)]
pub struct SidecarPhone(pub Rc<RefCell<Sidecar>>);

impl SidecarPhone {
    pub fn new(sidecar: Sidecar) -> SidecarPhone {
        SidecarPhone(Rc::new(RefCell::new(sidecar)))
    }
}

impl Phone for SidecarPhone {
    fn connect(&mut self, timeout_ms: u64) -> String {
        match self.0.borrow_mut().call(&json!({"op": "connect", "timeout_ms": timeout_ms})) {
            Ok(answer) => answer["state"].as_str().unwrap_or("disconnected").to_string(),
            Err(_) => "disconnected".into(),
        }
    }

    fn request(&mut self, method: &str, params: &Value, timeout_ms: u64) -> Result<Value, String> {
        let answer = self.0.borrow_mut().call(&json!({"op": "request", "method": method, "params": params, "timeout_ms": timeout_ms}))?;
        if answer["ok"] == Value::Bool(true) {
            Ok(answer["payload"].clone())
        } else {
            Err(answer["error"].as_str().unwrap_or("the sidecar failed").to_string())
        }
    }

    /// The session ends with the connection; the sidecar itself stays for the next case until it is stopped.
    fn close(&mut self) {
        let _ = self.0.borrow_mut().call(&json!({"op": "disconnect"}));
    }
}
