//! Runs the hosted family's programs: `gh` and `glab` found on the executor's `PATH`, started with `hosted::child_env` (the machine's own
//! configuration directories, no token variable), a process group of their own, bounded output and a deadline. Sign-in flows outlive their
//! request: `Hosted` keeps the running `auth login` and answers `wait` and `cancel` for it.
use crate::hosted::plan::{follow, plan, Cwd, Paths, Program, Step};
use crate::hosted::shape::{brief, parse_login_prompt, shape, Output};
use crate::hosted::{child_env, plan::env_of};
use exec_hosted::{HostedCall, HostedOp, Provider, LOGIN_PROMPT_TIMEOUT_MS, OUTPUT_CAP};
use exec_proto::{Code, ExecError, ExecResult};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const MAX_FLOWS: usize = 4;

#[derive(Debug, Clone, PartialEq)]
enum FlowState {
    Pending,
    SignedIn,
    Failed(String),
}

struct Flow {
    pid: i32,
    state: Arc<(Mutex<FlowState>, Condvar)>,
}

#[derive(Default)]
pub struct Hosted {
    /// A `PATH` for the CLIs in place of the executor's own (tests put their replay doubles on it).
    path: Option<String>,
    running: Mutex<HashMap<String, Arc<AtomicBool>>>,
    flows: Mutex<HashMap<String, Flow>>,
    next_flow: AtomicU64,
}

fn io(message: impl Into<String>) -> ExecError {
    ExecError::new(Code::Io, message)
}

fn kill_group(pid: i32) {
    // SAFETY: a negative pid addresses the process group the child leads; the call only sends a signal.
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
}

fn command(step: &Step, root: &Path, extra: &[(String, String)]) -> Command {
    let mut cmd = Command::new(step.program.name());
    cmd.args(&step.args).env_clear().envs(child_env(std::env::vars(), extra));
    cmd.current_dir(match &step.cwd {
        Cwd::Root => root.to_path_buf(),
        Cwd::Dir(d) => d.clone(),
    });
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).process_group(0);
    cmd
}

fn spawn_error(program: Program, e: std::io::Error) -> ExecError {
    match e.kind() {
        std::io::ErrorKind::NotFound => ExecError::new(Code::NotFound, format!("{}{} (not found on PATH)", program.name(), exec_hosted::NOT_INSTALLED)),
        _ => io(format!("{} did not start: {e}", program.name())),
    }
}

fn pump(mut from: impl Read + Send + 'static, tx: mpsc::Sender<(bool, Vec<u8>)>, is_err: bool) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while let Ok(n) = from.read(&mut buf) {
            if n == 0 || tx.send((is_err, buf[..n].to_vec())).is_err() {
                break;
            }
        }
    });
}

/// How a failed program is told to the caller: not found, not signed in, or what it said.
fn failure(op: &HostedOp, step: &Step, out: &Output) -> ExecError {
    let said = if out.stderr.trim().is_empty() { &out.stdout } else { &out.stderr };
    let lower = said.to_ascii_lowercase();
    let host = op.host.clone().unwrap_or_else(|| op.provider.default_host().to_string());
    let name = step.program.name();
    if lower.contains("auth login") || lower.contains("not logged in") || lower.contains("not logged into") || lower.contains("401") || lower.contains("no token found") || lower.contains("authentication required") {
        return io(format!("{}{host}: {name} says {}", exec_hosted::NOT_SIGNED_IN, brief(said)));
    }
    if lower.contains("could not resolve to a") || lower.contains("404") || lower.contains("could not find") || lower.contains("no pull requests match") || lower.contains("not found") {
        return ExecError::new(Code::NotFound, format!("{name}: {}", brief(said)));
    }
    io(format!("{name} exited {}: {}", out.exit_code, brief(said)))
}

impl Hosted {
    pub fn with_path(mut self, path: &str) -> Hosted {
        self.path = Some(path.to_string());
        self
    }

    fn extra_env(&self, op: &HostedOp) -> Vec<(String, String)> {
        let mut extra = env_of(op);
        extra.extend(self.path.iter().map(|p| ("PATH".to_string(), p.clone())));
        extra
    }

    fn track(&self, id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.running.lock().unwrap().insert(id.to_string(), flag.clone());
        flag
    }

    /// Stops what request `id` runs; false when it runs nothing here.
    pub fn cancel(&self, id: &str) -> bool {
        match self.running.lock().unwrap().get(id) {
            Some(flag) => {
                flag.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    fn run_step(&self, op: &HostedOp, step: &Step, root: &Path, cancelled: &AtomicBool) -> ExecResult<Output> {
        let mut cmd = command(step, root, &self.extra_env(op));
        cmd.stdin(if step.stdin.is_some() { Stdio::piped() } else { Stdio::null() });
        let mut child = cmd.spawn().map_err(|e| spawn_error(step.program, e))?;
        let pid = child.id() as i32;
        if let (Some(text), Some(mut stdin)) = (step.stdin.clone(), child.stdin.take()) {
            std::thread::spawn(move || {
                let _ = stdin.write_all(text.as_bytes());
            });
        }
        let (tx, rx) = mpsc::channel();
        pump(child.stdout.take().expect("piped"), tx.clone(), false);
        pump(child.stderr.take().expect("piped"), tx, true);
        let deadline = Instant::now() + Duration::from_millis(op.timeout_ms);
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let (mut truncated, mut killed, mut timed_out) = (false, false, false);
        loop {
            match rx.recv_timeout(Duration::from_millis(20)) {
                Ok((is_err, bytes)) => {
                    let target = if is_err { &mut stderr } else { &mut stdout };
                    let room = OUTPUT_CAP.saturating_sub(target.len());
                    if bytes.len() > room {
                        truncated = true;
                    }
                    target.extend_from_slice(&bytes[..bytes.len().min(room)]);
                }
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {}
            }
            let stop = if truncated {
                Some("output")
            } else if cancelled.load(Ordering::SeqCst) {
                Some("cancel")
            } else if Instant::now() >= deadline {
                timed_out = true;
                Some("time")
            } else {
                None
            };
            if stop.is_some() && !killed {
                killed = true;
                kill_group(pid);
            }
        }
        let status = child.wait().map_err(|e| io(e.to_string()))?;
        kill_group(pid);
        if timed_out {
            return Err(io(format!("{} did not finish within {} ms and was stopped", step.program.name(), op.timeout_ms)));
        }
        if cancelled.load(Ordering::SeqCst) {
            return Err(io(format!("{} was cancelled", step.program.name())));
        }
        let out = Output { exit_code: status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0)), stdout: String::from_utf8_lossy(&stdout).into_owned(), stderr: String::from_utf8_lossy(&stderr).into_owned(), truncated };
        if truncated || step.ok_exits.contains(&out.exit_code) {
            Ok(out)
        } else {
            Err(failure(op, step, &out))
        }
    }

    /// Runs one validated request. `paths` are the workspace paths the caller confined; `root` is where a step without a directory runs.
    pub fn call(&self, id: &str, op: &HostedOp, paths: &Paths, root: &Path) -> ExecResult<Value> {
        match &op.call {
            HostedCall::LoginStart => self.login_start(op, root),
            HostedCall::LoginWait { flow, wait_ms } => self.login_wait(id, op, flow, *wait_ms, root),
            HostedCall::LoginCancel { flow } => Ok(self.login_cancel(flow)),
            _ => {
                let mut steps = std::collections::VecDeque::from(plan(op, paths).map_err(ExecError::invalid_args)?);
                // A clone that fails or is cancelled leaves a half-populated directory a retry could not clone into; only one this call created goes.
                let created = matches!(op.call, HostedCall::RepoClone { .. }).then(|| paths.dest.clone()).flatten().filter(|d| !d.exists());
                let cancelled = self.track(id);
                let mut outputs = Vec::new();
                let mut failed = None;
                while let Some(step) = steps.pop_front() {
                    match self.run_step(op, &step, root, &cancelled) {
                        Ok(out) => outputs.push(out),
                        Err(e) => {
                            failed = Some(e);
                            break;
                        }
                    }
                    match follow(op, &outputs) {
                        Ok(next) => steps.extend(next),
                        Err(why) => {
                            failed = Some(io(why));
                            break;
                        }
                    }
                }
                self.running.lock().unwrap().remove(id);
                if let (Some(dest), true) = (created, failed.is_some()) {
                    let _ = std::fs::remove_dir_all(dest);
                }
                match failed {
                    // A missing CLI is an answer to a status question, not a failure of it.
                    Some(e) if e.code == Code::NotFound && matches!(op.call, HostedCall::AuthStatus) && e.message.contains(exec_hosted::NOT_INSTALLED) => Ok(json!({"provider": op.provider.as_str(), "installed": false, "signed_in": false, "host": op.host.clone().unwrap_or_else(|| op.provider.default_host().to_string()), "account": Value::Null, "scopes": []})),
                    // A host with no stacks answers 404, which says the request is in none.
                    Some(e) if e.code == Code::NotFound && matches!(op.call, HostedCall::PrStack { .. }) => Ok(json!({"stack": Value::Null})),
                    Some(e) => Err(e),
                    None => shape(op, &outputs),
                }
            }
        }
    }

    fn login_start(&self, op: &HostedOp, root: &Path) -> ExecResult<Value> {
        if self.flows.lock().unwrap().len() >= MAX_FLOWS {
            return Err(io("too many sign-ins are waiting: cancel one"));
        }
        let step = Step { program: if op.provider == Provider::Github { Program::Gh } else { Program::Glab }, args: crate::hosted::plan::login_args(op), stdin: None, ok_exits: vec![0], cwd: Cwd::Root };
        let mut cmd = command(&step, root, &self.extra_env(op));
        cmd.stdin(Stdio::null());
        let mut child = cmd.spawn().map_err(|e| spawn_error(step.program, e))?;
        let pid = child.id() as i32;
        let printed = Arc::new(Mutex::new(String::new()));
        let (tx, rx) = mpsc::channel();
        pump(child.stdout.take().expect("piped"), tx.clone(), false);
        pump(child.stderr.take().expect("piped"), tx, true);
        let state = Arc::new((Mutex::new(FlowState::Pending), Condvar::new()));
        let reader = {
            let printed = printed.clone();
            std::thread::spawn(move || {
                while let Ok((_, bytes)) = rx.recv() {
                    let mut text = printed.lock().unwrap();
                    if text.len() < OUTPUT_CAP {
                        text.push_str(&String::from_utf8_lossy(&bytes));
                    }
                }
            })
        };
        let watcher_state = state.clone();
        let watcher_printed = printed.clone();
        std::thread::spawn(move || {
            let status = child.wait();
            let _ = reader.join();
            let (lock, cv) = &*watcher_state;
            let mut s = lock.lock().unwrap();
            if *s == FlowState::Pending {
                *s = match status {
                    Ok(st) if st.success() => FlowState::SignedIn,
                    Ok(_) | Err(_) => FlowState::Failed(brief(&watcher_printed.lock().unwrap())),
                };
            }
            cv.notify_all();
        });
        // The prompt is ready once the address (and, for GitHub, the code) has been printed.
        let deadline = Instant::now() + Duration::from_millis(LOGIN_PROMPT_TIMEOUT_MS);
        let prompt = loop {
            let text = printed.lock().unwrap().clone();
            if let Some((code, url)) = parse_login_prompt(&text) {
                if code.is_some() || op.provider == Provider::Gitlab {
                    break Ok((code, url));
                }
            }
            if let FlowState::Failed(why) = &*state.0.lock().unwrap() {
                break Err(io(format!("{} sign-in failed before it showed a code: {why}", step.program.name())));
            }
            if *state.0.lock().unwrap() == FlowState::SignedIn {
                break Err(io(format!("{} signed in without showing a code: the machine was already signed in", step.program.name())));
            }
            if Instant::now() >= deadline {
                break Err(io(format!("{} showed no sign-in code within {} ms: {}", step.program.name(), LOGIN_PROMPT_TIMEOUT_MS, brief(&text))));
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        match prompt {
            Ok((code, url)) => {
                let flow = format!("login-{}", self.next_flow.fetch_add(1, Ordering::SeqCst) + 1);
                self.flows.lock().unwrap().insert(flow.clone(), Flow { pid, state });
                Ok(json!({"flow": flow, "provider": op.provider.as_str(), "host": op.host.clone().unwrap_or_else(|| op.provider.default_host().to_string()), "user_code": code, "verification_uri": url}))
            }
            Err(e) => {
                kill_group(pid);
                Err(e)
            }
        }
    }

    fn login_wait(&self, id: &str, op: &HostedOp, flow: &str, wait_ms: u64, root: &Path) -> ExecResult<Value> {
        let state = match self.flows.lock().unwrap().get(flow) {
            Some(f) => f.state.clone(),
            None => return Err(ExecError::new(Code::NotFound, format!("no sign-in flow {flow}"))),
        };
        let cancelled = self.track(id);
        let deadline = Instant::now() + Duration::from_millis(wait_ms);
        let (lock, cv) = &*state;
        let mut current = lock.lock().unwrap();
        while *current == FlowState::Pending && Instant::now() < deadline && !cancelled.load(Ordering::SeqCst) {
            current = cv.wait_timeout(current, Duration::from_millis(50)).unwrap().0;
        }
        let outcome = current.clone();
        drop(current);
        self.running.lock().unwrap().remove(id);
        if cancelled.load(Ordering::SeqCst) && outcome == FlowState::Pending {
            return Err(io("the wait was cancelled; the sign-in goes on"));
        }
        Ok(match outcome {
            FlowState::Pending => json!({"flow": flow, "state": "pending"}),
            FlowState::Failed(why) => {
                self.flows.lock().unwrap().remove(flow);
                json!({"flow": flow, "state": "failed", "message": why})
            }
            FlowState::SignedIn => {
                self.flows.lock().unwrap().remove(flow);
                let status = HostedOp { call: HostedCall::AuthStatus, ..op.clone() };
                let account = self.call(id, &status, &Paths::default(), root).ok().and_then(|v| v.get("account").cloned()).unwrap_or(Value::Null);
                json!({"flow": flow, "state": "signed_in", "account": account})
            }
        })
    }

    fn login_cancel(&self, flow: &str) -> Value {
        match self.flows.lock().unwrap().remove(flow) {
            Some(f) => {
                *f.state.0.lock().unwrap() = FlowState::Failed("cancelled".into());
                f.state.1.notify_all();
                kill_group(f.pid);
                json!({"flow": flow, "cancelled": true})
            }
            None => json!({"flow": flow, "cancelled": false}),
        }
    }
}
