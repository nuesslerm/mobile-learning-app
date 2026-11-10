//! Doubles of every port: an in-memory store, a clock that moves when told, ids from a counter, a log that remembers.
use crate::ports::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};

pub struct FixedClock(AtomicI64);

impl FixedClock {
    pub fn at(ms: Millis) -> FixedClock {
        FixedClock(AtomicI64::new(ms))
    }

    pub fn advance_ms(&self, ms: Millis) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for FixedClock {
    fn now_ms(&self) -> Millis {
        self.0.load(Ordering::SeqCst)
    }
}

/// Distinct, reproducible "random" bytes (a xorshift walk from a seed).
pub struct SeqIds(AtomicU64);

impl SeqIds {
    pub fn seeded(seed: u64) -> SeqIds {
        SeqIds(AtomicU64::new(seed | 1))
    }
}

impl Ids for SeqIds {
    fn random_bytes(&self, n: usize) -> Vec<u8> {
        (0..n)
            .map(|_| {
                let mut x = self.0.load(Ordering::SeqCst);
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                self.0.store(x, Ordering::SeqCst);
                (x >> 24) as u8
            })
            .collect()
    }
}

#[derive(Default)]
pub struct MemLog(pub Mutex<Vec<String>>);

impl Log for MemLog {
    fn event(&self, name: &'static str, fields: &[(&'static str, String)]) {
        let line = fields.iter().fold(name.to_string(), |acc, (k, v)| format!("{acc} {k}={v}"));
        self.0.lock().unwrap().push(line);
    }
}

impl MemLog {
    pub fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

/// A host with a reading that can be changed, or none.
pub struct FixedHost(pub Mutex<Result<HostSample, String>>);

impl HostProbe for FixedHost {
    fn sample(&self) -> Result<HostSample, String> {
        self.0.lock().unwrap().clone()
    }
}

pub struct StaticCatalog(pub Vec<(String, String)>);

impl ModelCatalog for StaticCatalog {
    fn models(&self) -> Vec<CatalogModel> {
        self.0.iter().map(|(slug, name)| CatalogModel { slug: slug.clone(), name: name.clone() }).collect()
    }
}

#[derive(Default)]
struct Data {
    events: Vec<StoredEvent>,
    receipts: HashMap<String, Receipt>,
    grants: HashMap<String, (GrantRecord, Millis, bool)>,
    tokens: HashMap<String, TokenRecord>,
    tickets: HashMap<String, (TicketRecord, Millis, bool)>,
    meta: HashMap<String, String>,
}

#[derive(Default)]
pub struct MemStore(Mutex<Data>);

/// The reading of a database the double has, changed by a test.
pub struct FixedStorage(pub Mutex<Result<StorageSample, String>>);

impl StorageProbe for FixedStorage {
    fn sample(&self) -> Result<StorageSample, String> {
        self.0.lock().unwrap().clone()
    }
}

impl MemStore {
    fn data(&self) -> std::sync::MutexGuard<'_, Data> {
        self.0.lock().unwrap()
    }
}

impl EventStore for MemStore {
    fn append(&self, drafts: Vec<EventDraft>, receipt: Option<(&str, &str)>) -> StoreResult<Vec<StoredEvent>> {
        let mut d = self.data();
        let mut out = Vec::new();
        for draft in drafts {
            let event = StoredEvent { sequence: d.events.len() as u64 + 1, event_id: draft.event_id, kind: draft.kind, thread_id: draft.thread_id, project_id: draft.project_id, occurred_at: draft.occurred_at, payload: draft.payload };
            d.events.push(event.clone());
            out.push(event);
        }
        if let (Some((id, kind)), Some(last)) = (receipt, out.last()) {
            d.receipts.insert(id.into(), Receipt { command_type: kind.into(), sequence: last.sequence });
        }
        Ok(out)
    }

    fn read_after(&self, after: u64) -> StoreResult<Vec<StoredEvent>> {
        Ok(self.data().events.iter().filter(|e| e.sequence > after).cloned().collect())
    }

    fn last_sequence(&self) -> StoreResult<u64> {
        Ok(self.data().events.len() as u64)
    }

    fn receipt(&self, command_id: &str) -> StoreResult<Option<Receipt>> {
        Ok(self.data().receipts.get(command_id).cloned())
    }
}

impl AuthStore for MemStore {
    fn put_grant(&self, hash: &str, grant: &GrantRecord, expires_at: Millis) -> StoreResult<()> {
        self.data().grants.insert(hash.into(), (grant.clone(), expires_at, false));
        Ok(())
    }

    fn consume_grant(&self, hash: &str, now: Millis) -> StoreResult<Result<GrantRecord, GrantRefusal>> {
        let mut d = self.data();
        Ok(match d.grants.get_mut(hash) {
            None => Err(GrantRefusal::Unknown),
            Some((_, _, true)) => Err(GrantRefusal::Used),
            Some((_, expires, _)) if now >= *expires => Err(GrantRefusal::Expired),
            Some((grant, _, used)) => {
                *used = true;
                Ok(grant.clone())
            }
        })
    }

    fn put_token(&self, hash: &str, token: &TokenRecord) -> StoreResult<()> {
        self.data().tokens.insert(hash.into(), token.clone());
        Ok(())
    }

    fn find_token(&self, hash: &str) -> StoreResult<Option<TokenRecord>> {
        Ok(self.data().tokens.get(hash).cloned())
    }

    fn put_ticket(&self, hash: &str, ticket: &TicketRecord, expires_at: Millis) -> StoreResult<()> {
        self.data().tickets.insert(hash.into(), (ticket.clone(), expires_at, false));
        Ok(())
    }

    fn consume_ticket(&self, hash: &str, now: Millis) -> StoreResult<Option<TicketRecord>> {
        let mut d = self.data();
        Ok(match d.tickets.get_mut(hash) {
            Some((ticket, expires, used)) if !*used && now < *expires => {
                *used = true;
                Some(ticket.clone())
            }
            _ => None,
        })
    }
}

impl MetaStore for MemStore {
    fn get(&self, key: &str) -> StoreResult<Option<String>> {
        Ok(self.data().meta.get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> StoreResult<()> {
        self.data().meta.insert(key.into(), value.into());
        Ok(())
    }
}

#[derive(Default)]
struct Agents {
    logs: HashMap<String, Vec<AgentEvent>>,
    running: HashMap<String, String>,
    turns_started: u64,
}

/// An agent API whose threads are scripted. A turn appends the events of `script` to its thread's log, as the service would, and `follow` and
/// `history` serve the log: the events after a number, those the thread has first and then the ones that come. Nothing is a turn of a provider.
#[derive(Default)]
pub struct ScriptedAgent {
    agents: Mutex<Agents>,
    news: Condvar,
    /// What the next turns do, after the turn starts and the user's message: the events of a turn, which ends with its own `TurnFinished` or
    /// else is finished here (failed when `fail` says why, completed otherwise).
    pub script: Mutex<Vec<AgentEventKind>>,
    pub fail: Mutex<Option<String>>,
    /// While true a turn runs until it is interrupted, when it finishes `Interrupted`.
    pub hold: std::sync::atomic::AtomicBool,
    pub created: Mutex<Vec<AgentThread>>,
    pub turns: Mutex<Vec<AgentTurn>>,
    pub interrupted: Mutex<Vec<String>>,
    /// While set, `history` answers it instead of the log.
    pub history_error: Mutex<Option<AgentError>>,
    /// While true, `history` sends the log from its first event whatever number it is asked after.
    pub resend: std::sync::atomic::AtomicBool,
    /// The methods asked, in order, by their agent API names.
    pub calls: Mutex<Vec<&'static str>>,
    pub files: Mutex<HashMap<String, String>>,
    /// What `run` answers by agent method name (`{}` when none is set), and every request it was asked, in order.
    pub replies: Mutex<HashMap<&'static str, Result<serde_json::Value, AgentError>>>,
    /// The executor's root, which a browse answers for (`/workspace` when none is set).
    pub root: Mutex<Option<String>>,
    pub ops: Mutex<Vec<Op>>,
    /// Asked before `replies`: an answer that depends on the request (a page of a shelf after a cursor), or none to fall through.
    pub responder: Mutex<Option<Responder>>,
    /// What `setup_events` answers by worktree root: the events the setup has made so far (none when nothing ran there).
    pub setups: Mutex<HashMap<String, SetupBatch>>,
    /// What `terminal_events` answers by terminal name: the events its stream has made so far (a snapshot first), none when it has none.
    pub terminal_streams: Mutex<HashMap<String, Vec<crate::ports::TerminalEvent>>>,
    /// The offsets `terminal_events` was asked after, by terminal name, and what `terminals` answers.
    pub terminal_asks: Mutex<Vec<(String, Option<u64>)>>,
    pub terminal_list: Mutex<Vec<crate::ports::TerminalSummary>>,
    /// The statuses `git_follow` sends, in order, by repository root; it then asks the sink again with `None` until the sink stops.
    pub git_statuses: Mutex<HashMap<String, Vec<serde_json::Value>>>,
    /// The states `auth_follow` sends by session, in order; it stops after one that has ended, else asks the sink again with null.
    pub auth_states: Mutex<HashMap<String, Vec<serde_json::Value>>>,
    /// The clone states `clone_follow` sends, in order, by project root; it then asks the sink again with `None` until the sink stops.
    pub clone_states: Mutex<HashMap<String, Vec<serde_json::Value>>>,
    /// The events `schedule_follow` sends, in order; it then asks the sink again with `None` until the sink stops.
    pub schedule_events: Mutex<Vec<serde_json::Value>>,
    /// The events `preview_follow` sends, in order, and the epoch and number it was asked to follow from; it then asks the sink again with `None`.
    pub preview_events: Mutex<Vec<serde_json::Value>>,
    pub preview_cursor: Mutex<Option<(String, u64)>>,
}

pub type Responder = Box<dyn Fn(&Op) -> Option<Result<serde_json::Value, AgentError>> + Send>;

impl ScriptedAgent {
    fn state(&self) -> std::sync::MutexGuard<'_, Agents> {
        self.agents.lock().unwrap()
    }

    fn called(&self, method: &'static str) {
        self.calls.lock().unwrap().push(method);
    }

    /// Adds an event to a thread's log as the service does, for the turn it names: the number it got.
    pub fn append(&self, thread: &str, turn: Option<&str>, kind: AgentEventKind) -> u64 {
        let mut agents = self.state();
        let log = agents.logs.entry(thread.to_string()).or_default();
        let seq = log.len() as u64 + 1;
        log.push(AgentEvent { seq, turn: turn.map(str::to_string), kind });
        self.news.notify_all();
        seq
    }

    /// The thread's whole log.
    pub fn log(&self, thread: &str) -> Vec<AgentEvent> {
        self.state().logs.get(thread).cloned().unwrap_or_default()
    }
}

impl AgentApi for ScriptedAgent {
    fn run(&self, op: &Op) -> Result<serde_json::Value, AgentError> {
        self.called(op.method());
        self.ops.lock().unwrap().push(op.clone());
        if let Some(answer) = self.responder.lock().unwrap().as_ref().and_then(|r| r(op)) {
            return answer;
        }
        if let Some(reply) = self.replies.lock().unwrap().get(op.method()).cloned() {
            return reply;
        }
        match op {
            // A folder is shown when it is the root or below it, as the executor's browse does
            Op::FileBrowse { path, .. } => {
                let root = self.root.lock().unwrap().clone().unwrap_or_else(|| "/workspace".to_string());
                let shown = if path == "." { root.clone() } else if path.starts_with('/') { path.trim_end_matches('/').to_string() } else { format!("{root}/{path}") };
                if shown == root || shown.starts_with(&format!("{root}/")) {
                    Ok(serde_json::json!({"path": shown, "entries": []}))
                } else {
                    Err(AgentError::Refused { code: crate::ports::agent_codes::PATH_OUTSIDE_ROOT.into(), message: format!("{path} is outside the root") })
                }
            }
            _ => Ok(serde_json::json!({})),
        }
    }

    fn ensure_thread(&self, thread: &AgentThread) -> Result<u64, AgentError> {
        self.called("thread.create");
        self.created.lock().unwrap().push(thread.clone());
        Ok(self.state().logs.entry(thread.thread_id.clone()).or_default().len() as u64)
    }

    fn start_turn(&self, turn: &AgentTurn) -> Result<String, AgentError> {
        self.called("turn.start");
        self.turns.lock().unwrap().push(turn.clone());
        let id = {
            let mut agents = self.state();
            if agents.running.contains_key(&turn.thread_id) {
                return Err(AgentError::Failed(format!("thread {} already has a running turn", turn.thread_id)));
            }
            agents.turns_started += 1;
            let id = format!("u{}", agents.turns_started);
            agents.running.insert(turn.thread_id.clone(), id.clone());
            id
        };
        let thread = turn.thread_id.as_str();
        let script = self.script.lock().unwrap().clone();
        let ends = matches!(script.last(), Some(AgentEventKind::TurnFinished(_)));
        for kind in [AgentEventKind::TurnStarted, AgentEventKind::UserText(turn.prompt.clone())].into_iter().chain(script) {
            self.append(thread, Some(&id), kind);
        }
        if !ends && !self.hold.load(Ordering::SeqCst) {
            match self.fail.lock().unwrap().clone() {
                Some(message) => {
                    self.append(thread, Some(&id), AgentEventKind::Failure(message));
                    self.append(thread, Some(&id), AgentEventKind::TurnFinished(Finish::Failed));
                }
                None => {
                    self.append(thread, Some(&id), AgentEventKind::TurnFinished(Finish::Completed));
                }
            }
        }
        if ends || !self.hold.load(Ordering::SeqCst) {
            self.state().running.remove(thread);
        }
        Ok(id)
    }

    fn interrupt(&self, thread_id: &str) -> Result<(), AgentError> {
        self.called("turn.interrupt");
        self.interrupted.lock().unwrap().push(thread_id.to_string());
        let turn = self.state().running.remove(thread_id);
        if let Some(turn) = turn {
            self.append(thread_id, Some(&turn), AgentEventKind::TurnFinished(Finish::Interrupted));
        }
        Ok(())
    }

    fn follow(&self, thread_id: &str, after_seq: u64, sink: &mut dyn FnMut(AgentEvent) -> Follow) -> Result<(), AgentError> {
        self.called("thread.subscribe");
        let mut seen = after_seq;
        loop {
            let news: Vec<AgentEvent> = {
                let agents = self.state();
                let Some(log) = agents.logs.get(thread_id) else { return Err(AgentError::NoThread) };
                let news: Vec<AgentEvent> = log.iter().filter(|e| e.seq > seen).cloned().collect();
                if news.is_empty() {
                    drop(self.news.wait_timeout(agents, std::time::Duration::from_millis(5)).unwrap());
                }
                news
            };
            for event in news {
                seen = event.seq;
                if sink(event) == Follow::Stop {
                    return Ok(());
                }
            }
        }
    }

    fn history(&self, thread_id: &str, after_seq: u64, limit: u32) -> Result<History, AgentError> {
        self.called("thread.history");
        if let Some(error) = self.history_error.lock().unwrap().clone() {
            return Err(error);
        }
        let agents = self.state();
        let log = agents.logs.get(thread_id).ok_or(AgentError::NoThread)?;
        let after_seq = if self.resend.load(Ordering::SeqCst) { 0 } else { after_seq };
        let after: Vec<AgentEvent> = log.iter().filter(|e| e.seq > after_seq).cloned().collect();
        let more = after.len() > limit as usize;
        Ok(History { events: after.into_iter().take(limit as usize).collect(), last_seq: log.len() as u64, more })
    }

    fn list_dir(&self, root: &str, path: &str) -> Result<Vec<DirEntry>, AgentError> {
        self.called("file.list");
        let prefix = format!("{root}/{}", path.trim_matches('/'));
        let prefix = prefix.trim_end_matches('/').to_string() + "/";
        let files = self.files.lock().unwrap();
        let mut seen: Vec<DirEntry> = Vec::new();
        for (full, body) in files.iter() {
            if let Some(rest) = full.strip_prefix(&prefix) {
                let (name, directory) = rest.split_once('/').map_or((rest, false), |(d, _)| (d, true));
                if !seen.iter().any(|e| e.name == name) {
                    seen.push(DirEntry { name: name.to_string(), directory, size: if directory { 0 } else { body.len() as u64 } });
                }
            }
        }
        seen.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(seen)
    }

    fn read_file(&self, root: &str, path: &str) -> Result<String, AgentError> {
        self.called("file.read");
        self.files.lock().unwrap().get(&format!("{root}/{path}")).cloned().ok_or_else(|| AgentError::Failed(format!("{path} does not exist")))
    }

    fn setup_events(&self, root: &str, after_seq: u64, _: std::time::Duration) -> Result<SetupBatch, AgentError> {
        self.called("worktree.setup.subscribe");
        let batch = self.setups.lock().unwrap().get(root).cloned().unwrap_or_default();
        Ok(SetupBatch { ran: batch.ran, events: batch.events.into_iter().filter(|e| e.seq > after_seq).collect() })
    }

    fn terminal_events(&self, _root: &str, terminal: &str, after_offset: Option<u64>, _: std::time::Duration) -> Result<crate::ports::TerminalBatch, AgentError> {
        use crate::ports::TerminalEvent;
        self.called("terminal.attach");
        self.terminal_asks.lock().unwrap().push((terminal.to_string(), after_offset));
        let streams = self.terminal_streams.lock().unwrap();
        let Some(events) = streams.get(terminal) else { return Ok(crate::ports::TerminalBatch::default()) };
        // A reader with an offset gets what moved the stream past it; one with none gets the snapshot and what came after it
        let events = match after_offset {
            None => events.iter().skip_while(|e| !matches!(e, TerminalEvent::Snapshot { .. })).take_while(|e| !matches!(e, TerminalEvent::Closed)).cloned().chain(events.iter().filter(|e| matches!(e, TerminalEvent::Closed)).cloned()).collect(),
            Some(after) => events.iter().filter(|e| e.end().is_none_or(|end| end > after) && !matches!(e, TerminalEvent::Snapshot { .. })).cloned().collect(),
        };
        Ok(crate::ports::TerminalBatch { found: true, events })
    }

    fn terminals(&self) -> Result<Vec<crate::ports::TerminalSummary>, AgentError> {
        self.called("terminal.list");
        Ok(self.terminal_list.lock().unwrap().clone())
    }

    fn git_follow(&self, root: &str, sink: &mut dyn FnMut(Option<serde_json::Value>) -> Follow) -> Result<(), AgentError> {
        self.called("git.subscribe");
        let statuses = self.git_statuses.lock().unwrap().get(root).cloned().unwrap_or_default();
        for status in statuses {
            if sink(Some(status)) == Follow::Stop {
                return Ok(());
            }
        }
        while sink(None) == Follow::Continue {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Ok(())
    }

    fn clone_follow(&self, root: &str, sink: &mut dyn FnMut(Option<serde_json::Value>) -> Follow) -> Result<(), AgentError> {
        self.called("clone.subscribe");
        let states = self.clone_states.lock().unwrap().get(root).cloned().unwrap_or_default();
        for state in states {
            if sink(Some(state)) == Follow::Stop {
                return Ok(());
            }
        }
        while sink(None) == Follow::Continue {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Ok(())
    }

    fn preview_follow(&self, epoch: &str, after_seq: u64, sink: &mut dyn FnMut(Option<serde_json::Value>) -> Follow) -> Result<(), AgentError> {
        self.called("preview.subscribe");
        *self.preview_cursor.lock().unwrap() = Some((epoch.to_string(), after_seq));
        let events = self.preview_events.lock().unwrap().clone();
        for event in events {
            if sink(Some(event)) == Follow::Stop {
                return Ok(());
            }
        }
        while sink(None) == Follow::Continue {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Ok(())
    }

    fn schedule_follow(&self, sink: &mut dyn FnMut(Option<serde_json::Value>) -> Follow) -> Result<(), AgentError> {
        self.called("schedule.subscribe");
        let events = self.schedule_events.lock().unwrap().clone();
        for event in events {
            if sink(Some(event)) == Follow::Stop {
                return Ok(());
            }
        }
        while sink(None) == Follow::Continue {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Ok(())
    }

    fn auth_follow(&self, auth: &str, sink: &mut dyn FnMut(serde_json::Value) -> Follow) -> Result<(), AgentError> {
        self.called("provider.auth.subscribe");
        let states = self.auth_states.lock().unwrap().get(auth).cloned().unwrap_or_default();
        for state in states {
            let ended = state["status"].as_str().is_some_and(|s| s != "pending");
            if sink(state) == Follow::Stop || ended {
                return Ok(());
            }
        }
        while sink(serde_json::Value::Null) == Follow::Continue {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Ok(())
    }
}
