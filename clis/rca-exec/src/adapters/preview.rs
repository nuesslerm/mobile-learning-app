//! The preview table: the dev servers this executor started. A server runs through `sh -c` in a process group of its own, in a directory the
//! caller already confined, with an environment built from `INHERITED_ENV`, `PORT`, `HOST` and what the request names (never the executor's
//! other variables), its output kept as a short tail. `start` returns once the port accepts connections.
use exec_preview::{url_of, Listed, PreviewInfo, PreviewStart, State, Stopped, INHERITED_ENV, MAX_EXITED, MAX_RUNNING, TAIL_MAX};
use exec_proto::{Code, ExecError, ExecResult};
use std::io::Read;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct Entry {
    info: PreviewInfo,
    child: Child,
    tail: Arc<Mutex<Vec<u8>>>,
}

#[derive(Default)]
pub struct PreviewTable {
    entries: Mutex<Vec<Entry>>,
    next: AtomicU64,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn io(e: impl std::fmt::Display) -> ExecError {
    ExecError::new(Code::Io, e.to_string())
}

fn kill_group(pid: u32) {
    // SAFETY: a negative pid addresses the process group the child leads; the call only sends a signal.
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
}

fn listening(port: u16) -> bool {
    TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, port)), Duration::from_millis(100)).is_ok()
}

fn pump(mut from: impl Read + Send + 'static, tail: Arc<Mutex<Vec<u8>>>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = from.read(&mut buf) {
            if n == 0 {
                break;
            }
            let mut tail = tail.lock().unwrap_or_else(|p| p.into_inner());
            tail.extend_from_slice(&buf[..n]);
            let over = tail.len().saturating_sub(TAIL_MAX);
            tail.drain(..over);
        }
    })
}

fn text(tail: &Mutex<Vec<u8>>) -> String {
    String::from_utf8_lossy(&tail.lock().unwrap_or_else(|p| p.into_inner())).into_owned()
}

impl PreviewTable {
    fn lock(&self) -> MutexGuard<'_, Vec<Entry>> {
        self.entries.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Notes the servers that have exited, and forgets the oldest exited ones beyond `MAX_EXITED`.
    fn reap(entries: &mut Vec<Entry>) {
        for e in entries.iter_mut().filter(|e| e.info.state == State::Running) {
            if let Ok(Some(status)) = e.child.try_wait() {
                kill_group(e.info.pid);
                (e.info.state, e.info.exit_code, e.info.exited_ms) = (State::Exited, Some(status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0))), Some(now_ms()));
            }
        }
        let exited = entries.iter().filter(|e| e.info.state == State::Exited).count();
        let mut drop_exited = exited.saturating_sub(MAX_EXITED);
        entries.retain(|e| e.info.state == State::Running || drop_exited == 0 || { drop_exited -= 1; false });
    }

    fn snapshot(entries: &[Entry]) -> Vec<PreviewInfo> {
        entries.iter().map(|e| PreviewInfo { tail: text(&e.tail), ..e.info.clone() }).collect()
    }

    /// Starts the server in `dir` (already confined by the caller; `shown` is how the request named it) and waits for its port.
    pub fn start(&self, dir: &Path, shown: &str, args: &PreviewStart) -> ExecResult<PreviewInfo> {
        let (id, tail) = {
            let mut entries = self.lock();
            Self::reap(&mut entries);
            let running: Vec<&Entry> = entries.iter().filter(|e| e.info.state == State::Running).collect();
            if running.len() >= MAX_RUNNING {
                return Err(ExecError::new(Code::AlreadyExists, format!("{MAX_RUNNING} preview servers are already running; stop one first")));
            }
            if running.iter().any(|e| e.info.port == args.port) || listening(args.port) {
                return Err(ExecError::new(Code::AlreadyExists, format!("port {} is already in use", args.port)));
            }
            (format!("preview-{}", self.next.fetch_add(1, Ordering::SeqCst) + 1), Arc::new(Mutex::new(Vec::new())))
        };
        let mut command = Command::new("sh");
        command.arg("-c").arg(&args.command).current_dir(dir).env_clear().stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).process_group(0);
        for name in INHERITED_ENV {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command.envs(args.env.iter().map(|(k, v)| (k, v))).env("PORT", args.port.to_string()).env("HOST", "127.0.0.1").env("BROWSER", "none");
        let mut child = command.spawn().map_err(io)?;
        let pumps = [pump(child.stdout.take().expect("piped"), tail.clone()), pump(child.stderr.take().expect("piped"), tail.clone())];
        let pid = child.id();
        let deadline = Instant::now() + Duration::from_millis(args.ready_timeout_ms);
        loop {
            if let Ok(Some(status)) = child.try_wait() {
                kill_group(pid);
                // The group is dead, so the pipes end and the tail holds everything it printed.
                pumps.into_iter().for_each(|p| drop(p.join()));
                return Err(ExecError::new(Code::Io, format!("the server exited ({}) before it listened on port {}: {}", status.code().map_or_else(|| format!("signal {}", status.signal().unwrap_or(0)), |c| format!("code {c}")), args.port, text(&tail).trim())));
            }
            if listening(args.port) {
                break;
            }
            if Instant::now() >= deadline {
                kill_group(pid);
                let _ = child.wait();
                return Err(ExecError::new(Code::Io, format!("the server was not listening on port {} after {} ms: {}", args.port, args.ready_timeout_ms, text(&tail).trim())));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let name = args.name.clone().unwrap_or_else(|| id.clone());
        let info = PreviewInfo { id, name, command: args.command.clone(), cwd: shown.to_string(), port: args.port, url: url_of(args.port), pid, state: State::Running, exit_code: None, started_ms: now_ms(), exited_ms: None, tail: String::new() };
        self.lock().push(Entry { info: info.clone(), child, tail });
        Ok(info)
    }

    pub fn stop(&self, id: &str) -> Stopped {
        let mut entries = self.lock();
        let Some(e) = entries.iter_mut().find(|e| e.info.id == id && e.info.state == State::Running) else { return Stopped { stopped: false } };
        kill_group(e.info.pid);
        let status = e.child.wait().ok();
        (e.info.state, e.info.exit_code, e.info.exited_ms) = (State::Exited, status.map(|s| s.code().unwrap_or_else(|| 128 + s.signal().unwrap_or(0))), Some(now_ms()));
        Stopped { stopped: true }
    }

    pub fn list(&self) -> Listed {
        let mut entries = self.lock();
        Self::reap(&mut entries);
        Listed { previews: Self::snapshot(&entries) }
    }

    /// Stops every running server (the executor is going away).
    pub fn stop_all(&self) {
        for e in self.lock().iter_mut().filter(|e| e.info.state == State::Running) {
            kill_group(e.info.pid);
            let _ = e.child.wait();
        }
    }
}

impl Drop for PreviewTable {
    fn drop(&mut self) {
        self.stop_all();
    }
}
