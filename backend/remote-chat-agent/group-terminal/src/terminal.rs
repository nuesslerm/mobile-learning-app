//! One terminal as the service keeps it: its metadata and the tail of its output, numbered by the pty's byte offset, fed by a pump that
//! attaches to the executor's pty and attaches again, from the offset it reached, whenever the executor's channel is replaced. The tail is
//! bounded and never persisted: a reader older than it is handed a snapshot.
use crate::meta::MetaLog;
use agent_api_proto::Event;
use agent_api_terminal::{self as api, Status, TerminalInfo, CHUNK, RETAIN};
use exec_proto::Code;
use rca_exec_channel::remote::{RemoteError, Streamed};
use rca_exec_channel::Executors;
use serde_json::json;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::task::AbortHandle;

/// How long the pump waits for an executor to attach again before it asks the registry once more.
const RETRY: Duration = Duration::from_millis(100);

pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}

pub(crate) struct State {
    pub base: u64,
    tail: Vec<u8>,
    pub status: Status,
    pub exit_code: Option<i32>,
    pub title: String,
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
    pub pid: Option<u32>,
    pub cleared: u64,
    pub closed: bool,
    updated_at: u64,
}

impl State {
    pub fn end(&self) -> u64 {
        self.base + self.tail.len() as u64
    }

    /// The bytes kept from `offset` (at least the oldest kept byte) on, and where they start.
    pub fn tail_from(&self, offset: u64, cap: usize) -> (u64, &[u8]) {
        let start = offset.max(self.base);
        let at = (start - self.base) as usize;
        (start, &self.tail[at..(at + cap).min(self.tail.len())])
    }

    fn feed(&mut self, offset: u64, data: &[u8]) {
        let end = self.end();
        let data = if offset > end {
            self.tail.clear();
            self.base = offset;
            data
        } else {
            match data.get((end - offset) as usize..) {
                Some(fresh) if !fresh.is_empty() => fresh,
                _ => return,
            }
        };
        self.tail.extend_from_slice(data);
        if self.tail.len() > RETAIN {
            let over = self.tail.len() - RETAIN;
            self.tail.drain(..over);
            self.base += over as u64;
        }
    }
}

pub(crate) struct Terminal {
    pub workspace: String,
    pub terminal: String,
    pub pty: String,
    state: Mutex<State>,
    changed: watch::Sender<u64>,
    abort: Mutex<Option<AbortHandle>>,
    meta: Arc<MetaLog>,
    clock: Arc<dyn Clock>,
}

/// What a terminal starts with: what the executor said, or what an adopted pty reports.
pub(crate) struct Start {
    pub title: String,
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
    pub pid: Option<u32>,
    pub base: u64,
    pub exited: Option<Option<i32>>,
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Terminal {
    pub fn new(workspace: &str, terminal: &str, pty: &str, start: Start, meta: Arc<MetaLog>, clock: Arc<dyn Clock>) -> Arc<Terminal> {
        let (status, exit_code) = match start.exited {
            Some(code) => (Status::Exited, code),
            None => (Status::Running, None),
        };
        let state = State { base: start.base, tail: Vec::new(), status, exit_code, title: start.title, cwd: start.cwd, cols: start.cols, rows: start.rows, pid: start.pid, cleared: 0, closed: false, updated_at: clock.now_ms() };
        Arc::new(Terminal { workspace: workspace.into(), terminal: terminal.into(), pty: pty.into(), state: Mutex::new(state), changed: watch::channel(0).0, abort: Mutex::new(None), meta, clock })
    }

    pub fn state(&self) -> std::sync::MutexGuard<'_, State> {
        lock(&self.state)
    }

    pub fn watch(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }

    fn bump(&self) {
        self.changed.send_modify(|v| *v += 1);
    }

    pub fn info(&self) -> TerminalInfo {
        let s = self.state();
        TerminalInfo { workspace: self.workspace.clone(), terminal: self.terminal.clone(), title: s.title.clone(), cwd: s.cwd.clone(), cols: s.cols, rows: s.rows, status: s.status, exit_code: s.exit_code, pid: s.pid, offset: s.end(), base_offset: s.base, updated_at: s.updated_at }
    }

    /// Records a change of the metadata and tells the metadata stream and the attached readers.
    pub fn changed(&self, edit: impl FnOnce(&mut State)) {
        {
            let mut s = self.state();
            edit(&mut s);
            s.updated_at = self.clock.now_ms();
        }
        self.meta.upsert(&self.info());
        self.bump();
    }

    pub fn clear(&self) {
        {
            let mut s = self.state();
            let end = s.end();
            s.tail.clear();
            s.base = end;
            s.cleared += 1;
        }
        self.bump();
    }

    pub fn close(&self) {
        if let Some(abort) = lock(&self.abort).take() {
            abort.abort();
        }
        self.state().closed = true;
        self.meta.remove(&self.terminal);
        self.bump();
    }

    pub fn is_closed(&self) -> bool {
        self.state().closed
    }

    fn exited(&self, code: Option<i32>) {
        self.changed(|s| {
            s.status = Status::Exited;
            s.exit_code = code;
        });
    }

    fn feed(&self, offset: u64, data: &[u8]) {
        {
            let mut s = self.state();
            s.feed(offset, data);
            s.updated_at = self.clock.now_ms();
        }
        self.bump();
    }

    /// Starts the pump on the runtime: it attaches to the pty at the executor of the workspace until the pty ends or the terminal is closed.
    pub fn spawn_pump(self: &Arc<Self>, executors: Arc<dyn Executors>) {
        let term = self.clone();
        let task = tokio::spawn(async move { term.pump(executors).await });
        *lock(&self.abort) = Some(task.abort_handle());
    }

    async fn pump(&self, executors: Arc<dyn Executors>) {
        while !self.is_closed() && self.state().status == Status::Running {
            let Some(channel) = executors.get(&self.workspace).filter(|c| !c.is_closed()) else {
                tokio::time::sleep(RETRY).await;
                continue;
            };
            let after = self.state().end();
            let (tx, mut rx) = mpsc::unbounded_channel();
            let call = channel.call_stream(exec_pty::PTY_ATTACH, json!({"pty": self.pty, "after_offset": after}), tx);
            tokio::pin!(call);
            let outcome = loop {
                tokio::select! {
                    Some(item) = rx.recv() => self.take(item),
                    done = &mut call => break done,
                }
            };
            while let Ok(item) = rx.try_recv() {
                self.take(item);
            }
            match outcome {
                Ok(done) => {
                    let attached: Option<exec_pty::PtyAttached> = serde_json::from_value(done.result).ok();
                    match attached {
                        Some(a) if a.exited => self.exited(a.exit_code),
                        _ => tokio::time::sleep(RETRY).await,
                    }
                }
                Err(RemoteError::Action { code, .. }) if code == Code::NotFound.as_str() => self.exited(None),
                Err(_) => tokio::time::sleep(RETRY).await,
            }
        }
    }

    fn take(&self, item: Streamed) {
        if let Streamed::Bytes { offset, data } = item {
            self.feed(offset, &data);
        }
    }

    /// What a reader at `cursor` is owed next, and the cursor after it. `seen_cleared` and `announced` are the reader's own marks.
    pub fn next_for(&self, cursor: u64, seen_cleared: &mut u64, announced: &mut bool) -> Option<(Event, u64, bool)> {
        let info = self.info();
        let s = self.state();
        let event = |e: Event, end: u64, done: bool| Some((e, end, done));
        if s.cleared != *seen_cleared {
            *seen_cleared = s.cleared;
            return event(api::cleared_event(&self.workspace, &self.terminal, s.base), s.base, false);
        }
        if cursor < s.base {
            let (start, bytes) = s.tail_from(cursor, usize::MAX);
            return event(api::snapshot_event(&info, start, bytes, true), s.end(), false);
        }
        if cursor < s.end() {
            let (start, bytes) = s.tail_from(cursor, CHUNK);
            return event(api::output_event(&self.workspace, &self.terminal, start, bytes), start + bytes.len() as u64, false);
        }
        if s.closed {
            return event(api::closed_event(&self.workspace, &self.terminal, s.end()), s.end(), true);
        }
        if s.status == Status::Exited && !*announced {
            *announced = true;
            return event(api::exited_event(&self.workspace, &self.terminal, s.end(), s.exit_code), s.end(), false);
        }
        None
    }
}
