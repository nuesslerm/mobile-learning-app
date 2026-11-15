//! The phone behind a child process: one JSON object per line on its stdin and stdout (`docs/plans/mimic/orca-phone-protocol.md`). The child
//! is the codec sidecar that wraps the real client's sealed session (Orca's `DirectRpcClient`), so the keys and the handshake stay in the
//! client's own code. A control line carries an `id` and is answered `{id, ok}` or `{id, error}`; frames of an open stream, handshake steps
//! and the end of the connection are unsolicited `{event}` lines. A reader thread splits the two, so an event that arrives while a call waits
//! is queued for `poll`, never lost.
use mimic_script_core::{Phone, PhoneEvent};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

type Events = Arc<Mutex<VecDeque<PhoneEvent>>>;

pub struct Sidecar {
    child: Child,
    stdin: ChildStdin,
    replies: Receiver<Value>,
    events: Events,
    next: u64,
}

fn event_of(line: &Value) -> Option<PhoneEvent> {
    let text = |k: &str| line[k].as_str().map(String::from);
    match line["event"].as_str()? {
        "frame" => Some(PhoneEvent::Frame { sub: text("sub")?, binary: line["kind"] == "binary", payload: line["payload"].clone(), len: line["len"].as_u64().map(|n| n as usize) }),
        "handshake" => Some(PhoneEvent::Handshake { stage: text("stage")?, client_to_server: line["dir"] == "c2s", payload: line["payload"].clone() }),
        "close" => Some(PhoneEvent::Close { code: line["code"].as_u64().map(|c| c as u16), reason: text("reason").unwrap_or_default() }),
        _ => None,
    }
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
        let stdout = child.stdout.take().ok_or("the sidecar has no stdout")?;
        let (tx, replies) = channel();
        let events: Events = Arc::default();
        let queue = events.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let Ok(value) = serde_json::from_str::<Value>(line.trim()) else { continue };
                match event_of(&value) {
                    Some(event) => queue.lock().expect("the event queue").push_back(event),
                    None if value.get("event").is_some() => {}
                    None => {
                        if tx.send(value).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        Ok(Sidecar { child, stdin, replies, events, next: 0 })
    }

    /// One control line and its answer: the `ok` object, or the `error` text. `wait_ms` bounds the wait for the answer.
    pub fn call(&mut self, op: &str, input: Value, wait_ms: u64) -> Result<Value, String> {
        self.next += 1;
        let id = self.next.to_string();
        writeln!(self.stdin, "{}", json!({"id": id, "op": op, "input": input})).and_then(|()| self.stdin.flush()).map_err(|e| format!("the sidecar is gone: {e}"))?;
        loop {
            match self.replies.recv_timeout(Duration::from_millis(wait_ms)) {
                Ok(reply) if reply["id"] == Value::String(id.clone()) => return reply.get("error").and_then(Value::as_str).map_or_else(|| Ok(reply["ok"].clone()), |e| Err(e.to_string())),
                Ok(_) => continue,
                Err(RecvTimeoutError::Timeout) => return Err(format!("the sidecar did not answer {op} within {wait_ms} ms")),
                Err(RecvTimeoutError::Disconnected) => return Err("the sidecar exited".into()),
            }
        }
    }

    fn drain(&self) -> Vec<PhoneEvent> {
        self.events.lock().expect("the event queue").drain(..).collect()
    }

    /// Stops the host the sidecar started and the sidecar itself.
    pub fn stop(&mut self) {
        let _ = self.call("quit", Value::Null, 5_000);
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
pub struct SidecarPhone {
    pub sidecar: Rc<RefCell<Sidecar>>,
    tap: Rc<RefCell<(bool, bool)>>,
}

/// The wait for an answer beyond the time the op itself is given, for the sidecar's own work (a build, a handshake).
const MARGIN_MS: u64 = 15_000;

impl SidecarPhone {
    pub fn new(sidecar: Sidecar) -> SidecarPhone {
        SidecarPhone { sidecar: Rc::new(RefCell::new(sidecar)), tap: Rc::default() }
    }
}

impl Phone for SidecarPhone {
    fn connect(&mut self, timeout_ms: u64) -> String {
        let mut sidecar = self.sidecar.borrow_mut();
        let (handshake, close) = *self.tap.borrow();
        if sidecar.call("tap", json!({"handshake": handshake, "close": close}), MARGIN_MS).is_err() {
            return "disconnected".into();
        }
        // the first connect may start the host (a build on a fresh checkout): the wait is the host's, not the phone's
        match sidecar.call("connect", json!({"timeout_ms": timeout_ms}), timeout_ms + 600_000) {
            Ok(answer) => answer["state"].as_str().unwrap_or("disconnected").to_string(),
            Err(_) => "disconnected".into(),
        }
    }

    fn request(&mut self, method: &str, params: &Value, timeout_ms: u64) -> Result<Value, String> {
        let answer = self.sidecar.borrow_mut().call("request", json!({"method": method, "params": params, "timeout_ms": timeout_ms}), timeout_ms + MARGIN_MS)?;
        match answer["client_error"].as_str() {
            Some(message) => Err(message.to_string()),
            None => Ok(answer["response"].clone()),
        }
    }

    fn subscribe(&mut self, method: &str, params: &Value) -> Result<String, String> {
        let answer = self.sidecar.borrow_mut().call("subscribe", json!({"method": method, "params": params}), MARGIN_MS)?;
        match (answer["client_error"].as_str(), answer["sub"].as_str()) {
            (Some(message), _) => Err(message.to_string()),
            (None, Some(sub)) => Ok(sub.to_string()),
            _ => Err("the sidecar opened no stream".into()),
        }
    }

    fn unsubscribe(&mut self, sub: &str) -> Result<(), String> {
        self.sidecar.borrow_mut().call("unsubscribe", json!({"sub": sub}), MARGIN_MS).map(|_| ())
    }

    fn tap(&mut self, handshake: bool, close: bool) {
        *self.tap.borrow_mut() = (handshake, close);
    }

    fn poll(&mut self) -> Vec<PhoneEvent> {
        self.sidecar.borrow().drain()
    }

    fn close(&mut self) {
        let _ = self.sidecar.borrow_mut().call("close", Value::Null, MARGIN_MS);
    }
}
