//! A double of rca's terminal group: terminals held in memory, answered in the shapes of the contract (`agent-api-terminal`). The chain runs
//! the real service and executor against the same calls, so a shape that drifts fails there.
use orca_core::testing::ScriptedAgent;
use agent_api_terminal::{AttachParams, Status, TerminalInfo};
use orca_core::{AgentError, EventStream, StreamEvent};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;
use std::sync::{Arc, Mutex};

type Key = (String, String);
type Queue = Arc<Mutex<VecDeque<StreamEvent>>>;

/// The retained tail of one terminal: `base` is the offset of its first byte.
#[derive(Default)]
struct Tail {
    base: u64,
    bytes: Vec<u8>,
}

impl Tail {
    fn end(&self) -> u64 {
        self.base + self.bytes.len() as u64
    }

    fn trim(&mut self, retain: Option<usize>) {
        if let Some(cap) = retain.filter(|c| self.bytes.len() > *c) {
            let drop = self.bytes.len() - cap;
            self.bytes.drain(..drop);
            self.base += drop as u64;
        }
    }
}

struct Attached(Queue);

impl EventStream for Attached {
    fn next(&mut self, _wait: Duration) -> Result<Option<StreamEvent>, AgentError> {
        Ok(self.0.lock().unwrap().pop_front())
    }
}

fn stream_event(e: agent_api_proto::Event) -> StreamEvent {
    StreamEvent { seq: e.seq, kind: e.kind, data: Value::Object(e.data) }
}

#[derive(Default)]
struct State {
    exit_codes: BTreeMap<Key, i32>,
    retain: Option<usize>,
    subscribers: BTreeMap<Key, Vec<Queue>>,
    /// (workspace, terminal) -> (title, running).
    terminals: BTreeMap<(String, String), (String, bool)>,
    written: Vec<(String, String, String)>,
    cleared: Vec<String>,
    resized: Vec<(String, u64, u64)>,
    opened: Vec<Value>,
    output: BTreeMap<Key, Tail>,
}

#[derive(Clone, Default)]
pub struct Terminals(Arc<Mutex<State>>);

fn refused(code: &str, message: impl Into<String>) -> AgentError {
    AgentError::Refused { code: code.into(), message: message.into() }
}

impl Terminals {
    pub fn terminal(&self, workspace: &str, name: &str, running: bool) -> &Self {
        self.0.lock().unwrap().terminals.insert((workspace.into(), name.into()), (name.into(), running));
        self
    }

    /// What `terminal.write` was sent, as (workspace, terminal, data).
    pub fn written(&self) -> Vec<(String, String, String)> {
        self.0.lock().unwrap().written.clone()
    }

    pub fn cleared(&self) -> Vec<String> {
        self.0.lock().unwrap().cleared.clone()
    }

    /// What `terminal.resize` was sent, as (terminal, cols, rows).
    pub fn resized(&self) -> Vec<(String, u64, u64)> {
        self.0.lock().unwrap().resized.clone()
    }

    /// What the terminal has printed, kept from offset 0 (or from the last bytes the cap of `with_retain` keeps).
    pub fn output(&self, workspace: &str, name: &str, bytes: &[u8]) -> &Self {
        let mut s = self.0.lock().unwrap();
        let mut tail = Tail { base: 0, bytes: bytes.to_vec() };
        tail.trim(s.retain);
        s.output.insert((workspace.into(), name.into()), tail);
        self
    }

    /// Keeps only the last `n` bytes of each terminal's output, as rca keeps a tail: an offset before it answers `reset`.
    pub fn with_retain(&self, n: usize) -> &Self {
        self.0.lock().unwrap().retain = Some(n);
        self
    }

    /// The terminal prints `bytes`: every open attach gets an output event whose `seq` is the offset after them.
    pub fn push_output(&self, workspace: &str, name: &str, bytes: &[u8]) -> &Self {
        let key: Key = (workspace.into(), name.into());
        let mut s = self.0.lock().unwrap();
        let retain = s.retain;
        let tail = s.output.entry(key.clone()).or_default();
        let from = tail.end();
        tail.bytes.extend_from_slice(bytes);
        tail.trim(retain);
        Self::tell(&s, &key, agent_api_terminal::output_event(workspace, name, from, bytes));
        self
    }

    /// The terminal's process ends with `code`: every open attach gets one exited event, and a second exit says nothing more.
    pub fn exit(&self, workspace: &str, name: &str, code: i32) -> &Self {
        let key: Key = (workspace.into(), name.into());
        let mut s = self.0.lock().unwrap();
        let end = s.output.get(&key).map_or(0, Tail::end);
        let Some(entry) = s.terminals.get_mut(&key) else { return self };
        if !entry.1 {
            return self;
        }
        entry.1 = false;
        s.exit_codes.insert(key.clone(), code);
        Self::tell(&s, &key, agent_api_terminal::exited_event(workspace, name, end, Some(code)));
        self
    }

    /// The terminal's buffer is cleared: the kept bytes go, offsets go on, and every open attach gets a cleared event.
    pub fn clear(&self, workspace: &str, name: &str) -> &Self {
        let key: Key = (workspace.into(), name.into());
        let mut s = self.0.lock().unwrap();
        let tail = s.output.entry(key.clone()).or_default();
        tail.base = tail.end();
        tail.bytes.clear();
        let end = tail.base;
        Self::tell(&s, &key, agent_api_terminal::cleared_event(workspace, name, end));
        self
    }

    fn tell(s: &State, key: &Key, event: agent_api_proto::Event) {
        for queue in s.subscribers.get(key).into_iter().flatten() {
            queue.lock().unwrap().push_back(stream_event(event.clone()));
        }
    }

    /// What `terminal.open` was sent.
    pub fn opened(&self) -> Vec<Value> {
        self.0.lock().unwrap().opened.clone()
    }

    pub fn title(&self, workspace: &str, name: &str) -> Option<String> {
        self.0.lock().unwrap().terminals.get(&(workspace.into(), name.into())).map(|(t, _)| t.clone())
    }

    pub fn exists(&self, workspace: &str, name: &str) -> bool {
        self.title(workspace, name).is_some()
    }

    /// Makes `agent` answer the methods of `USES` from this terminal set.
    pub fn install(&self, agent: &ScriptedAgent) {
        let me = self.clone();
        agent.reply("terminal.list", move |p| {
            let w = p["workspace"].as_str().unwrap_or_default();
            let s = me.0.lock().unwrap();
            let rows: Vec<Value> = s.terminals.iter().filter(|((fw, _), _)| fw == w).map(|((fw, name), (title, running))| json!({"workspace": fw, "terminal": name, "title": title, "cwd": "/r", "cols": 80, "rows": 24, "status": if *running { "running" } else { "exited" }, "exit_code": s.exit_codes.get(&(fw.clone(), name.clone())), "pid": 7, "offset": s.output.get(&(fw.clone(), name.clone())).map_or(0, Tail::end), "base_offset": s.output.get(&(fw.clone(), name.clone())).map_or(0, |t| t.base), "updated_at": 1_700_000_000_000u64})).collect();
            Ok(json!({"workspace": w, "terminals": rows}))
        });
        let me = self.clone();
        agent.reply("terminal.write", move |p| {
            let (w, t, data) = (p["workspace"].as_str().unwrap_or_default(), p["terminal"].as_str().unwrap_or_default(), p["data"].as_str().unwrap_or_default());
            let mut s = me.0.lock().unwrap();
            match s.terminals.get(&(w.into(), t.into())) {
                None => Err(refused("UNKNOWN_TERMINAL", format!("no terminal {t}"))),
                Some((_, false)) => Err(refused("TERMINAL_EXITED", format!("{t} has ended"))),
                Some(_) => {
                    s.written.push((w.into(), t.into(), data.into()));
                    Ok(json!({"bytes": data.len()}))
                }
            }
        });
        let me = self.clone();
        agent.reply("terminal.close", move |p| {
            let key = (p["workspace"].as_str().unwrap_or_default().to_string(), p["terminal"].as_str().unwrap_or_default().to_string());
            me.0.lock().unwrap().terminals.remove(&key).map(|_| json!({"workspace": key.0, "terminal": key.1})).ok_or_else(|| refused("UNKNOWN_TERMINAL", format!("no terminal {}", key.1)))
        });
        let me = self.clone();
        agent.reply("terminal.clear", move |p| {
            let key = (p["workspace"].as_str().unwrap_or_default().to_string(), p["terminal"].as_str().unwrap_or_default().to_string());
            let mut s = me.0.lock().unwrap();
            if !s.terminals.contains_key(&key) {
                return Err(refused("UNKNOWN_TERMINAL", format!("no terminal {}", key.1)));
            }
            s.cleared.push(key.1.clone());
            Ok(json!({"workspace": key.0, "terminal": key.1}))
        });
        let me = self.clone();
        agent.reply("terminal.read", move |p| {
            let key = (p["workspace"].as_str().unwrap_or_default().to_string(), p["terminal"].as_str().unwrap_or_default().to_string());
            let s = me.0.lock().unwrap();
            let Some((_, running)) = s.terminals.get(&key) else { return Err(refused("UNKNOWN_TERMINAL", format!("no terminal {}", key.1))) };
            let tail = s.output.get(&key);
            let (base, end) = tail.map_or((0, 0), |t| (t.base, t.end()));
            let after = p["after_offset"].as_u64();
            if after.is_some_and(|a| a > end) {
                return Err(refused("OFFSET_AHEAD", "after_offset is past the end of the stream"));
            }
            let reset = after.is_some_and(|a| a < base);
            let start = after.filter(|a| *a >= base).unwrap_or(base);
            let data = tail.map_or(&[][..], |t| &t.bytes[(start - base) as usize..]);
            Ok(json!({"workspace": key.0, "terminal": key.1, "after_offset": after, "start_offset": start, "offset": end, "reset": reset, "data": agent_api_terminal::b64::encode(data), "cols": 80, "rows": 24, "status": if *running { "running" } else { "exited" }, "exit_code": s.exit_codes.get(&key)}))
        });
        let me = self.clone();
        agent.follower("terminal.attach", move |p| me.attach(p));
        let me = self.clone();
        agent.reply("terminal.open", move |p| {
            let key = (p["workspace"].as_str().unwrap_or_default().to_string(), p["terminal"].as_str().unwrap_or_default().to_string());
            let mut s = me.0.lock().unwrap();
            if s.terminals.contains_key(&key) {
                return Err(refused("TERMINAL_EXISTS", format!("{} already exists", key.1)));
            }
            let title = p["title"].as_str().unwrap_or(&key.1).to_string();
            s.terminals.insert(key.clone(), (title.clone(), true));
            s.opened.push(p.clone());
            Ok(json!({"workspace": key.0, "terminal": key.1, "title": title}))
        });
        let me = self.clone();
        agent.reply("terminal.resize", move |p| {
            let key = (p["workspace"].as_str().unwrap_or_default().to_string(), p["terminal"].as_str().unwrap_or_default().to_string());
            let (cols, rows) = (p["cols"].as_u64().unwrap_or_default(), p["rows"].as_u64().unwrap_or_default());
            let mut s = me.0.lock().unwrap();
            match s.terminals.get(&key) {
                None => Err(refused("UNKNOWN_TERMINAL", format!("no terminal {}", key.1))),
                Some((_, false)) => Err(refused("TERMINAL_EXITED", format!("{} has ended", key.1))),
                Some(_) => {
                    s.resized.push((key.1.clone(), cols, rows));
                    Ok(json!({"workspace": key.0, "terminal": key.1, "cols": cols, "rows": rows}))
                }
            }
        });
        let me = self.clone();
        agent.reply("terminal.rename", move |p| {
            let key = (p["workspace"].as_str().unwrap_or_default().to_string(), p["terminal"].as_str().unwrap_or_default().to_string());
            let mut s = me.0.lock().unwrap();
            let entry = s.terminals.get_mut(&key).ok_or_else(|| refused("UNKNOWN_TERMINAL", format!("no terminal {}", key.1)))?;
            entry.0 = p["title"].as_str().unwrap_or_default().to_string();
            Ok(json!({"workspace": key.0, "terminal": key.1, "title": entry.0}))
        });
    }

    /// `terminal.attach` as the service answers it: the replay from the caller's offset as one snapshot event, then the live events.
    fn attach(&self, params: &Value) -> Result<(Value, Box<dyn EventStream>), AgentError> {
        let parsed = AttachParams::parse(params).map_err(|e| refused("INVALID_PARAMS", e.0))?;
        let key: Key = (parsed.session.workspace.clone(), parsed.session.terminal.clone());
        let mut s = self.0.lock().unwrap();
        let Some((title, running)) = s.terminals.get(&key).cloned() else { return Err(refused("UNKNOWN_TERMINAL", format!("no terminal {}", key.1))) };
        let (base, end, bytes) = s.output.get(&key).map_or((0, 0, Vec::new()), |t| (t.base, t.end(), t.bytes.clone()));
        let after = parsed.after_offset;
        if after.is_some_and(|a| a > end) {
            return Err(refused("OFFSET_AHEAD", "after_offset is past the end of the stream"));
        }
        let reset = after.is_some_and(|a| a < base);
        let start = after.filter(|a| *a >= base).unwrap_or(base);
        let replay = &bytes[(start - base) as usize..];
        let status = if running { Status::Running } else { Status::Exited };
        let exit_code = s.exit_codes.get(&key).copied();
        let info = TerminalInfo { workspace: key.0.clone(), terminal: key.1.clone(), title, cwd: "/r".into(), cols: 80, rows: 24, status, exit_code, pid: Some(7), offset: end, base_offset: base, updated_at: 1_700_000_000_000 };
        let queue: Queue = Arc::default();
        if reset || !replay.is_empty() {
            queue.lock().unwrap().push_back(stream_event(agent_api_terminal::snapshot_event(&info, start, replay, reset)));
        }
        s.subscribers.entry(key.clone()).or_default().push(queue.clone());
        let answer = json!({"workspace": key.0, "terminal": key.1, "after_offset": after, "start_offset": start, "offset": end, "reset": reset, "cols": 80, "rows": 24, "status": status, "exit_code": exit_code});
        Ok((answer, Box::new(Attached(queue))))
    }
}
