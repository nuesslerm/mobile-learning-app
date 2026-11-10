//! The `terminal` group: each method is answered with the executor that owns the workspace (`pty_*` actions of `exec-pty`), and the output of a
//! terminal is served from the tail the service keeps (`terminal.rs`). The service holds no shell and no path: a terminal's cwd is placed in
//! its workspace and then confined by the executor.
use crate::meta::MetaLog;
use crate::terminal::{lock, Clock, Start, Terminal};
use agent_api_proto::{code, GroupName, Response};
use agent_api_terminal::{self as api, AttachParams, AttachResult, Done, ListResult, OpenParams, OpenResult, RenameParams, ResizeParams, Resized, Session, Status, SubscribeParams, SubscribeResult, WriteParams, Written};
use async_trait::async_trait;
use exec_proto::Code;
use rca_agent_core::{Call, Connection, Group, Served};
use rca_exec_channel::remote::ExecutorChannel;
use rca_exec_channel::Executors;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use exec_pty::{PTY_ATTACH, PTY_CLOSE, PTY_LIST, PTY_OPEN, PTY_RESIZE, PTY_WRITE};

pub const SERVED: [Served; 9] = [
    Served::service(api::OPEN, &[PTY_OPEN, PTY_ATTACH, PTY_LIST]),
    Served::service(api::WRITE, &[PTY_WRITE, PTY_LIST]),
    Served::service(api::RESIZE, &[PTY_RESIZE, PTY_LIST]),
    Served::service(api::CLOSE, &[PTY_CLOSE, PTY_LIST]),
    Served::service(api::CLEAR, &[PTY_WRITE, PTY_LIST]),
    Served::service(api::LIST, &[PTY_LIST, PTY_ATTACH]),
    Served::service(api::RENAME, &[PTY_LIST]),
    Served::service(api::ATTACH, &[PTY_ATTACH, PTY_LIST]),
    Served::service(api::SUBSCRIBE, &[PTY_LIST, PTY_ATTACH]),
];

struct Refusal(String, String);

type Answer = Result<Value, Refusal>;

fn refuse(code: &str, message: impl Into<String>) -> Refusal {
    Refusal(code.into(), message.into())
}

fn invalid(message: impl Into<String>) -> Refusal {
    refuse(code::INVALID_PARAMS, message)
}

impl From<agent_api_proto::ParamError> for Refusal {
    fn from(e: agent_api_proto::ParamError) -> Refusal {
        invalid(e.0)
    }
}

impl From<rca_exec_channel::remote::RemoteError> for Refusal {
    fn from(e: rca_exec_channel::remote::RemoteError) -> Refusal {
        Refusal(e.code().to_string(), e.message())
    }
}

fn value(result: &impl Serialize) -> Answer {
    serde_json::to_value(result).map_err(|e| refuse("INTERNAL", e.to_string()))
}

/// The pty's name at the executor: a tag of the workspace, so two workspaces of one executor may use one terminal name.
fn pty_name(workspace: &str, terminal: &str) -> String {
    format!("{}.{terminal}", workspace_tag(workspace))
}

fn workspace_tag(workspace: &str) -> String {
    let hash = workspace.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3));
    format!("{:08x}", hash >> 32 ^ hash & 0xffff_ffff)
}

pub struct TerminalGroup {
    executors: Arc<dyn Executors>,
    clock: Arc<dyn Clock>,
    terminals: Mutex<HashMap<(String, String), Arc<Terminal>>>,
    metas: Mutex<HashMap<String, Arc<MetaLog>>>,
}

impl TerminalGroup {
    pub fn new(executors: Arc<dyn Executors>, clock: Arc<dyn Clock>) -> TerminalGroup {
        TerminalGroup { executors, clock, terminals: Mutex::default(), metas: Mutex::default() }
    }

    fn meta(&self, workspace: &str) -> Arc<MetaLog> {
        lock(&self.metas).entry(workspace.to_string()).or_insert_with(|| Arc::new(MetaLog::new(workspace))).clone()
    }

    fn known(&self, session: &Session) -> Option<Arc<Terminal>> {
        lock(&self.terminals).get(&(session.workspace.clone(), session.terminal.clone())).cloned()
    }

    fn channel(&self, workspace: &str) -> Result<Arc<dyn ExecutorChannel>, Refusal> {
        self.executors.get(workspace).ok_or_else(|| refuse(Code::NoExecutor.as_str(), format!("no executor is attached for workspace {workspace}")))
    }

    fn register(&self, term: Arc<Terminal>) -> Arc<Terminal> {
        lock(&self.terminals).insert((term.workspace.clone(), term.terminal.clone()), term.clone());
        term.spawn_pump(self.executors.clone());
        self.meta(&term.workspace).upsert(&term.info());
        term
    }

    fn new_terminal(&self, session: &Session, start: Start) -> Arc<Terminal> {
        Terminal::new(&session.workspace, &session.terminal, &pty_name(&session.workspace, &session.terminal), start, self.meta(&session.workspace), self.clock.clone())
    }

    /// The ptys of the workspace's executor that belong to the workspace and that the service does not know yet (a service that restarted
    /// while the executor kept its shells): each becomes a terminal again, its tail refilled from the executor's own.
    async fn adopt(&self, workspace: &str) -> Result<(), Refusal> {
        let listed = self.channel(workspace)?.call(PTY_LIST, json!({})).await?;
        let listed: exec_pty::PtyListed = serde_json::from_value(listed.result).map_err(|e| refuse("BAD_RESULT", format!("the executor's pty list is not the contract's: {e}")))?;
        let prefix = format!("{}.", workspace_tag(workspace));
        for pty in listed.ptys {
            let Some(name) = pty.pty.strip_prefix(&prefix) else { continue };
            let session = Session { workspace: workspace.to_string(), terminal: name.to_string() };
            if self.known(&session).is_some() {
                continue;
            }
            let exited = pty.exited.then_some(pty.exit_code);
            let start = Start { title: name.to_string(), cwd: pty.cwd, cols: pty.cols, rows: pty.rows, pid: Some(pty.pid), base: pty.base_offset, exited };
            self.register(self.new_terminal(&session, start));
        }
        Ok(())
    }

    async fn terminal(&self, session: &Session) -> Result<Arc<Terminal>, Refusal> {
        if let Some(term) = self.known(session) {
            return Ok(term);
        }
        self.adopt(&session.workspace).await?;
        self.known(session).ok_or_else(|| refuse(api::code::UNKNOWN_TERMINAL, format!("no terminal {} in workspace {}", session.terminal, session.workspace)))
    }

    async fn open(&self, params: &Value) -> Answer {
        let p = OpenParams::parse(params)?;
        let channel = self.channel(&p.session.workspace)?;
        if let Ok(existing) = self.terminal(&p.session).await {
            if existing.state().status == Status::Running {
                return value(&OpenResult { terminal: existing.info(), opened: false });
            }
            let _ = channel.call(PTY_CLOSE, json!({"pty": existing.pty})).await;
            existing.close();
            lock(&self.terminals).remove(&(p.session.workspace.clone(), p.session.terminal.clone()));
        }
        let place = self.executors.place(&p.session.workspace);
        let cwd = match (&p.cwd, &place.prefix) {
            (Some(cwd), _) => Some(place.path(cwd).map_err(invalid)?),
            (None, prefix) => prefix.clone(),
        };
        let mut args = json!({"pty": pty_name(&p.session.workspace, &p.session.terminal)});
        for (key, v) in [("cwd", cwd.map(Value::from)), ("cols", p.cols.map(Value::from)), ("rows", p.rows.map(Value::from))] {
            if let Some(v) = v {
                args[key] = v;
            }
        }
        if !p.env.is_empty() {
            args["env"] = Value::Object(p.env.clone());
        }
        let opened = channel.call(PTY_OPEN, args).await?;
        let opened: exec_pty::PtyOpened = serde_json::from_value(opened.result).map_err(|e| refuse("BAD_RESULT", format!("the executor's pty is not the contract's: {e}")))?;
        let start = Start { title: p.title.unwrap_or_else(|| p.session.terminal.clone()), cwd: opened.cwd, cols: opened.cols, rows: opened.rows, pid: Some(opened.pid), base: 0, exited: None };
        let term = self.register(self.new_terminal(&p.session, start));
        value(&OpenResult { terminal: term.info(), opened: true })
    }

    async fn write(&self, params: &Value) -> Answer {
        let p = WriteParams::parse(params)?;
        let term = self.terminal(&p.session).await?;
        self.send(&term, &p.data).await
    }

    async fn send(&self, term: &Terminal, data: &str) -> Answer {
        if term.state().status == Status::Exited {
            return Err(refuse(api::code::TERMINAL_EXITED, format!("terminal {} has ended", term.terminal)));
        }
        let done = self.channel(&term.workspace)?.call(PTY_WRITE, json!({"pty": term.pty, "data": data})).await?;
        value(&Written { bytes: done.result["bytes"].as_u64().unwrap_or(data.len() as u64) as usize })
    }

    async fn resize(&self, params: &Value) -> Answer {
        let p = ResizeParams::parse(params)?;
        let term = self.terminal(&p.session).await?;
        self.channel(&term.workspace)?.call(PTY_RESIZE, json!({"pty": term.pty, "cols": p.cols, "rows": p.rows})).await?;
        term.changed(|s| {
            s.cols = p.cols;
            s.rows = p.rows;
        });
        value(&Resized { cols: p.cols, rows: p.rows })
    }

    async fn clear(&self, params: &Value) -> Answer {
        let session = Session::parse(params)?;
        let term = self.terminal(&session).await?;
        term.clear();
        if term.state().status == Status::Running {
            self.send(&term, "\u{c}").await?;
        }
        value(&Done { workspace: session.workspace, terminal: session.terminal })
    }

    async fn close(&self, params: &Value) -> Answer {
        let session = Session::parse(params)?;
        let term = self.terminal(&session).await?;
        match self.channel(&term.workspace)?.call(PTY_CLOSE, json!({"pty": term.pty})).await {
            Ok(_) => {}
            Err(e) if e.code() == Code::NotFound.as_str() => {}
            Err(e) => return Err(e.into()),
        }
        term.close();
        lock(&self.terminals).remove(&(session.workspace.clone(), session.terminal.clone()));
        value(&Done { workspace: session.workspace, terminal: session.terminal })
    }

    async fn rename(&self, params: &Value) -> Answer {
        let p = RenameParams::parse(params)?;
        let term = self.terminal(&p.session).await?;
        term.changed(|s| s.title = p.title);
        value(&term.info())
    }

    async fn list(&self, params: &Value) -> Answer {
        let workspace = api::workspace_param(params)?;
        self.adopt(&workspace).await?;
        value(&ListResult { terminals: self.infos(&workspace), workspace })
    }

    fn infos(&self, workspace: &str) -> Vec<api::TerminalInfo> {
        let mut infos: Vec<_> = lock(&self.terminals).values().filter(|t| t.workspace == workspace).map(|t| t.info()).collect();
        infos.sort_by(|a, b| a.terminal.cmp(&b.terminal));
        infos
    }

    async fn attach(&self, params: &Value, connection: &Connection) -> Answer {
        let p = AttachParams::parse(params)?;
        let term = self.terminal(&p.session).await?;
        let info = term.info();
        if p.after_offset.is_some_and(|after| after > info.offset) {
            return Err(refuse(api::code::OFFSET_AHEAD, format!("after_offset {} is past the end of the stream, {}", p.after_offset.unwrap_or_default(), info.offset)));
        }
        let reset = p.after_offset.is_some_and(|after| after < info.base_offset);
        let snapshot = p.after_offset.is_none() || reset;
        let start = if snapshot { info.base_offset } else { p.after_offset.unwrap_or_default() };
        let result = AttachResult { workspace: info.workspace.clone(), terminal: info.terminal.clone(), after_offset: p.after_offset, start_offset: start, offset: info.offset, reset, cols: info.cols, rows: info.rows, status: info.status, exit_code: info.exit_code };
        let sink = connection.sink();
        let mut changes = term.watch();
        let (first, mut cursor) = if snapshot {
            let s = term.state();
            let (at, bytes) = s.tail_from(0, usize::MAX);
            (Some(api::snapshot_event(&info, at, bytes, reset)), at + bytes.len() as u64)
        } else {
            (None, start)
        };
        let mut seen_cleared = term.state().cleared;
        connection.follow(format!("terminal/{}/{}", info.workspace, info.terminal), async move {
            if let Some(first) = first {
                if !sink.send_replay(&first).await {
                    return;
                }
            }
            let mut announced = false;
            loop {
                match term.next_for(cursor, &mut seen_cleared, &mut announced) {
                    Some((event, after, done)) => {
                        cursor = after;
                        if !sink.send_replay(&event).await || done {
                            return;
                        }
                    }
                    None => {
                        if changes.changed().await.is_err() {
                            return;
                        }
                    }
                }
            }
        });
        value(&result)
    }

    async fn subscribe(&self, params: &Value, connection: &Connection) -> Answer {
        let p = SubscribeParams::parse(params)?;
        if self.executors.get(&p.workspace).is_some() {
            self.adopt(&p.workspace).await?;
        }
        let meta = self.meta(&p.workspace);
        let seq = meta.seq();
        let reset = p.after_seq.is_some_and(|after| meta.since(after).lost);
        let from = match p.after_seq {
            Some(after) if !reset && after <= seq => after,
            _ => seq,
        };
        let result = SubscribeResult { terminals: self.infos(&p.workspace), workspace: p.workspace.clone(), seq, reset };
        let sink = connection.sink();
        let mut changes = meta.watch();
        connection.follow(format!("terminal/{}/metadata", p.workspace), async move {
            let mut after = from;
            loop {
                let slice = meta.since(after);
                if slice.lost {
                    return;
                }
                for event in &slice.events {
                    after = event.seq;
                    if !sink.send_replay(event).await {
                        return;
                    }
                }
                if slice.events.is_empty() && changes.changed().await.is_err() {
                    return;
                }
            }
        });
        value(&result)
    }
}

#[async_trait]
impl Group for TerminalGroup {
    fn name(&self) -> GroupName {
        api::GROUP
    }

    fn methods(&self) -> &[Served] {
        &SERVED
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let answer = match call.method {
            api::OPEN => self.open(call.params).await,
            api::WRITE => self.write(call.params).await,
            api::RESIZE => self.resize(call.params).await,
            api::CLOSE => self.close(call.params).await,
            api::CLEAR => self.clear(call.params).await,
            api::LIST => self.list(call.params).await,
            api::RENAME => self.rename(call.params).await,
            api::ATTACH => self.attach(call.params, call.connection).await,
            api::SUBSCRIBE => self.subscribe(call.params, call.connection).await,
            other => unreachable!("{} is not a method of terminal", other.name()),
        };
        match answer {
            Ok(result) => Response::result(call.id, result),
            Err(Refusal(code, message)) => Response::error(call.id, &code, message),
        }
    }
}
