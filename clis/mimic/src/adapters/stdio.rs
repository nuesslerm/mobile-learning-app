//! `mimic-driver/1` over a child process: one JSON object per line on stdin and stdout. `{"hello":true}` answers
//! `{"hello":{driver,target_version,coverage}}`; `{"id","op","input"}` answers `{"id","steps","effects","outcome"}` or `{"id","error"}`.
use mimic_core::ports::{Driver, DriverError, Hello, Observed, Request};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

const STDERR_KEPT: usize = 20;
const GRACE_POLLS: u32 = 100;

#[derive(Clone, Debug)]
pub struct StdioSpec {
    pub command: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub timeout: Duration,
}

struct Live {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    stderr: Arc<Mutex<VecDeque<String>>>,
    stderr_reader: JoinHandle<()>,
}

pub struct StdioDriver {
    spec: StdioSpec,
    live: Option<Live>,
}

fn err(message: impl Into<String>) -> DriverError {
    DriverError(message.into())
}

impl StdioDriver {
    pub fn new(spec: StdioSpec) -> Self {
        Self { spec, live: None }
    }

    fn spawn(&self) -> Result<Live, DriverError> {
        let (program, args) = self.spec.command.split_first().ok_or_else(|| err("the driver command is empty"))?;
        let mut command = Command::new(program);
        command.args(args).envs(self.spec.env.iter().map(|(k, v)| (k, v))).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(cwd) = &self.spec.cwd {
            command.current_dir(cwd);
        }
        wf_kit::logfmt::mcp("info", "mimic", "driver.spawn", &[("program", program.clone())]);
        let mut child = command.spawn().map_err(|e| err(format!("cannot start the driver {program}: {e}")))?;
        let (stdin, stdout, stderr) = (child.stdin.take().expect("piped"), child.stdout.take().expect("piped"), child.stderr.take().expect("piped"));
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let kept = Arc::new(Mutex::new(VecDeque::new()));
        let sink = Arc::clone(&kept);
        let stderr_reader = std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let mut q = sink.lock().unwrap_or_else(|e| e.into_inner());
                if q.len() == STDERR_KEPT {
                    q.pop_front();
                }
                q.push_back(line);
            }
        });
        Ok(Live { child, stdin, lines, stderr: kept, stderr_reader })
    }

    /// A driver that holds a real server needs to see its input end to stop it, so the child gets a moment to go before it is killed.
    fn stop(&mut self) {
        if let Some(Live { mut child, stdin, .. }) = self.live.take() {
            drop(stdin);
            for _ in 0..GRACE_POLLS {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// What the driver said on stderr before it went: the child is stopped first so the reader has seen all of it.
    fn stderr_tail(&mut self) -> String {
        let Some(mut live) = self.live.take() else { return String::new() };
        let _ = live.child.kill();
        let _ = live.child.wait();
        // a grandchild can hold the pipe open, so the wait is bounded
        for _ in 0..40 {
            if live.stderr_reader.is_finished() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let q = live.stderr.lock().unwrap_or_else(|e| e.into_inner());
        if q.is_empty() {
            String::new()
        } else {
            format!(" (driver stderr: {})", q.iter().cloned().collect::<Vec<_>>().join(" | "))
        }
    }

    fn call(&mut self, request: &Value) -> Result<Value, DriverError> {
        if self.live.is_none() {
            self.live = Some(self.spawn()?);
        }
        let timeout = self.spec.timeout;
        let live = self.live.as_mut().expect("spawned");
        let written = writeln!(live.stdin, "{request}").and_then(|()| live.stdin.flush());
        let outcome = match written {
            Err(e) => Err(format!("cannot write to the driver: {e}")),
            Ok(()) => loop {
                match live.lines.recv_timeout(timeout) {
                    Ok(line) if line.trim().is_empty() => continue,
                    Ok(line) => break serde_json::from_str::<Value>(&line).map_err(|e| format!("the driver wrote a line that is not JSON ({e}): {}", line.chars().take(200).collect::<String>())),
                    Err(RecvTimeoutError::Timeout) => break Err(format!("the driver did not answer within {} ms", timeout.as_millis())),
                    Err(RecvTimeoutError::Disconnected) => {
                        let status = live.child.wait().map_or_else(|e| e.to_string(), |s| s.to_string());
                        break Err(format!("the driver exited ({status}) before answering"));
                    }
                }
            },
        };
        outcome.map_err(|m| {
            let tail = self.stderr_tail();
            err(format!("{m}{tail}"))
        })
    }

    fn write_line(&mut self, request: &Value) -> Result<(), DriverError> {
        if self.live.is_none() {
            self.live = Some(self.spawn()?);
        }
        let live = self.live.as_mut().expect("spawned");
        writeln!(live.stdin, "{request}").and_then(|()| live.stdin.flush()).map_err(|e| err(format!("cannot write to the driver: {e}")))
    }

    fn read_line(&mut self) -> Result<Value, DriverError> {
        let timeout = self.spec.timeout;
        let live = self.live.as_mut().ok_or_else(|| err("the driver is not running"))?;
        let outcome = loop {
            match live.lines.recv_timeout(timeout) {
                Ok(line) if line.trim().is_empty() => continue,
                Ok(line) => break serde_json::from_str::<Value>(&line).map_err(|e| format!("the driver wrote a line that is not JSON ({e}): {}", line.chars().take(200).collect::<String>())),
                Err(RecvTimeoutError::Timeout) => break Err(format!("the driver did not answer within {} ms", timeout.as_millis())),
                Err(RecvTimeoutError::Disconnected) => break Err("the driver exited before answering".to_string()),
            }
        };
        outcome.map_err(|m| {
            let tail = self.stderr_tail();
            err(format!("{m}{tail}"))
        })
    }
}

impl Drop for StdioDriver {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Driver for StdioDriver {
    fn hello(&mut self) -> Result<Hello, DriverError> {
        let reply = self.call(&json!({"hello": true}))?;
        let hello = reply.get("hello").cloned().ok_or_else(|| err(format!("the driver answered hello with {reply}")))?;
        serde_json::from_value(hello).map_err(|e| err(format!("the driver's hello is not mimic-driver/1: {e}")))
    }

    fn run(&mut self, request: &Request) -> Result<Observed, DriverError> {
        let reply = self.call(&serde_json::to_value(request).expect("a request serialises"))?;
        if reply.get("id").and_then(Value::as_str) != Some(request.id.as_str()) {
            return Err(err(format!("the driver answered another request than {}: {reply}", request.id)));
        }
        observed(&request.id, reply)
    }

    /// `{"reset":true}`: a driver that does not know it answers an error, and has no state of its own to bring back.
    fn reset(&mut self) -> Result<(), DriverError> {
        if self.live.is_none() {
            return Ok(());
        }
        let reply = self.call(&json!({"reset": true}))?;
        match reply.get("reset") {
            Some(_) => Ok(()),
            None if reply.get("error").and_then(Value::as_str).is_some_and(|m| m.starts_with("not a request") || m.starts_with("unknown")) => Ok(()),
            None => Err(err(format!("the driver could not reset: {reply}"))),
        }
    }

    /// Every request is written before any answer is read: a driver that answers concurrently (`MIMIC_DRIVER_CONCURRENCY`) overlaps them.
    fn run_many(&mut self, requests: &[Request]) -> Vec<Result<Observed, DriverError>> {
        if requests.len() < 2 {
            return requests.iter().map(|r| self.run(r)).collect();
        }
        let sent = requests.iter().try_for_each(|r| self.write_line(&serde_json::to_value(r).expect("a request serialises")));
        if let Err(e) = sent {
            return requests.iter().map(|_| Err(err(e.0.clone()))).collect();
        }
        let mut answers: HashMap<String, Value> = HashMap::new();
        let mut out = Vec::with_capacity(requests.len());
        for r in requests {
            while !answers.contains_key(&r.id) {
                match self.read_line() {
                    Ok(reply) => match reply.get("id").and_then(Value::as_str) {
                        Some(id) => {
                            answers.insert(id.to_string(), reply);
                        }
                        None => {
                            let e = err(format!("the driver answered a batch with {reply}"));
                            return requests.iter().map(|_| Err(err(e.0.clone()))).collect();
                        }
                    },
                    Err(e) => return requests.iter().map(|_| Err(err(e.0.clone()))).collect(),
                }
            }
            out.push(observed(&r.id, answers.remove(&r.id).expect("present")));
        }
        out
    }
}

fn observed(id: &str, reply: Value) -> Result<Observed, DriverError> {
    if let Some(message) = reply.get("error") {
        return Err(err(message.as_str().map_or_else(|| message.to_string(), String::from)));
    }
    serde_json::from_value(reply).map_err(|e| err(format!("the reply to {id} is not mimic-driver/1: {e}")))
}

/// The driver's side of the protocol: answers each line of `input` with the `driver` until it ends (`{"reset":true}` is `Driver::reset`). A line that is not a request is
/// answered `{"id":null,"error":..}` and the loop goes on.
pub fn serve(driver: &mut dyn Driver, input: impl BufRead, mut out: impl Write) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Err(e) => json!({"id": null, "error": format!("not JSON: {e}")}),
            Ok(v) if v.get("hello").is_some() => match driver.hello() {
                Ok(h) => json!({"hello": h}),
                Err(e) => json!({"id": null, "error": e.0}),
            },
            Ok(v) if v.get("reset").is_some() => match driver.reset() {
                Ok(()) => json!({"reset": "ok"}),
                Err(e) => json!({"id": null, "error": e.0}),
            },
            Ok(v) => match serde_json::from_value::<Request>(v.clone()) {
                Err(e) => json!({"id": v.get("id"), "error": format!("not a request: {e}")}),
                Ok(request) => match driver.run(&request) {
                    Ok(observed) => {
                        let mut reply = serde_json::to_value(&observed).expect("observed serialises");
                        reply["id"] = json!(request.id);
                        reply
                    }
                    Err(e) => json!({"id": request.id, "error": e.0}),
                },
            },
        };
        writeln!(out, "{reply}")?;
        out.flush()?;
    }
    Ok(())
}
