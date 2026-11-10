//! The M8 rows: the terminals of a thread, each a shell on a pty the executor runs, asked of the agent API's `terminal` group. T3 names a terminal
//! by its thread and its own id; the agent API by one name, the two joined with a dot. A terminal's output is followed by asking again after the
//! byte offset reached, so a dropped connection, a restarted executor or a reconnecting phone resumes where the stream stood.
use super::queries::{field, QueryFailure};
use super::service::Service;
use crate::ports::{AgentError, Op, TerminalBatch, TerminalEvent, TerminalSummary};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;
use t3_contracts::time::iso;

/// How long one look at a terminal waits for it to say something before it looks again.
const WAIT: Duration = Duration::from_secs(1);
/// A rest between two looks that found nothing, for an agent API that does not wait itself.
const IDLE: Duration = Duration::from_millis(20);
/// The longest name of a terminal in the agent API.
const NAME_MAX: usize = 48;
const LABEL_MAX: usize = 128;

fn plain(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The agent API's name for a terminal of a thread, none when the ids cannot be joined into one.
pub fn terminal_name(thread: &str, terminal: &str) -> Option<String> {
    (plain(thread) && plain(terminal) && thread.len() + terminal.len() < NAME_MAX).then(|| format!("{thread}.{terminal}"))
}

/// The thread and terminal ids of an agent API name `terminal_name` made.
pub fn split_name(name: &str) -> Option<(&str, &str)> {
    name.split_once('.').filter(|(thread, terminal)| plain(thread) && plain(terminal))
}

/// Where a terminal request is aimed.
struct Target {
    thread: String,
    terminal: String,
    name: String,
    root: String,
}

fn lookup(thread: &str, terminal: &str) -> QueryFailure {
    QueryFailure::Error(json!({"_tag": "TerminalSessionLookupError", "threadId": thread, "terminalId": terminal}))
}

/// The state of a terminal as the app's snapshot shows it.
struct Shown<'a> {
    thread: &'a str,
    terminal: &'a str,
    summary: Option<&'a TerminalSummary>,
    history: String,
    running: bool,
    exit_code: Option<i32>,
    sequence: u64,
    size: Option<(u16, u16)>,
}

impl Shown<'_> {
    fn snapshot(&self, fallback_cwd: &str) -> Value {
        let s = self.summary;
        let mut snapshot = Map::new();
        snapshot.insert("threadId".into(), json!(self.thread));
        snapshot.insert("terminalId".into(), json!(self.terminal));
        snapshot.insert("cwd".into(), json!(s.map_or(fallback_cwd, |s| s.cwd.as_str())));
        snapshot.insert("worktreePath".into(), s.and_then(|s| s.worktree_path.clone()).map_or(Value::Null, Value::from));
        snapshot.insert("status".into(), json!(if self.running { "running" } else { "exited" }));
        snapshot.insert("pid".into(), s.and_then(|s| s.pid).map_or(Value::Null, Value::from));
        snapshot.insert("history".into(), json!(self.history));
        snapshot.insert("exitCode".into(), self.exit_code.map_or(Value::Null, Value::from));
        snapshot.insert("exitSignal".into(), Value::Null);
        snapshot.insert("label".into(), json!(label(s)));
        snapshot.insert("updatedAt".into(), json!(iso(s.map_or(0, |s| s.updated_at_ms as i64))));
        snapshot.insert("sequence".into(), json!(self.sequence));
        if let Some((cols, rows)) = self.size {
            snapshot.insert("cols".into(), json!(cols));
            snapshot.insert("rows".into(), json!(rows));
        }
        Value::Object(snapshot)
    }
}

fn label(summary: Option<&TerminalSummary>) -> String {
    summary.map_or_else(String::new, |s| s.title.chars().take(LABEL_MAX).collect())
}

/// What the metadata stream has said: the summaries by name, as the app was last told them (without the time, which every output moves).
#[derive(Default)]
pub struct MetaFold {
    told: BTreeMap<String, Value>,
}

fn summary_value(thread: &str, terminal: &str, s: &TerminalSummary) -> Value {
    json!({
        "threadId": thread, "terminalId": terminal, "cwd": s.cwd, "worktreePath": s.worktree_path, "status": if s.running { "running" } else { "exited" },
        "pid": s.pid, "exitCode": s.exit_code, "exitSignal": null, "hasRunningSubprocess": false, "label": label(Some(s)), "updatedAt": iso(s.updated_at_ms as i64),
    })
}

fn without_time(mut value: Value) -> Value {
    if let Some(map) = value.as_object_mut() {
        map.remove("updatedAt");
    }
    value
}

impl MetaFold {
    /// The frames that bring the app from what it was told to `now`: all of them as a snapshot when it was told nothing yet, else an upsert for
    /// each terminal that is new or changed and a remove for each that is gone.
    pub fn frames(&mut self, now: &[TerminalSummary], first: bool) -> Vec<Value> {
        let current: BTreeMap<String, Value> = now.iter().filter_map(|s| split_name(&s.name).map(|(t, i)| (s.name.clone(), summary_value(t, i, s)))).collect();
        let mut frames = Vec::new();
        if first {
            frames.push(json!({"type": "snapshot", "terminals": current.values().collect::<Vec<_>>()}));
        } else {
            for (name, value) in &current {
                if self.told.get(name).map(|v| without_time(v.clone())) != Some(without_time(value.clone())) {
                    frames.push(json!({"type": "upsert", "terminal": value}));
                }
            }
            for name in self.told.keys().filter(|n| !current.contains_key(*n)) {
                if let Some((thread, terminal)) = split_name(name) {
                    frames.push(json!({"type": "remove", "threadId": thread, "terminalId": terminal}));
                }
            }
        }
        self.told = current;
        frames
    }
}

impl Service {
    fn terminal_target(&self, payload: &Value, root: Option<&str>) -> Result<Target, QueryFailure> {
        let (thread, terminal) = (field(payload, "threadId")?, field(payload, "terminalId")?);
        let name = terminal_name(thread, terminal).ok_or_else(|| QueryFailure::Defect(format!("A terminal id is letters, digits, `_` and `-`, and together with its thread at most {NAME_MAX} long\n  at [\"terminalId\"]")))?;
        let root = root.map(str::to_string).unwrap_or_else(|| self.thread_dir(thread));
        Ok(Target { thread: thread.into(), terminal: terminal.into(), name, root })
    }

    fn terminal_failure(&self, e: &AgentError, target: &Target, extra: Value) -> QueryFailure {
        let code = match e {
            AgentError::Refused { code, .. } => code.as_str(),
            _ => "",
        };
        let mut error = match code {
            "UNKNOWN_TERMINAL" => return lookup(&target.thread, &target.terminal),
            "TERMINAL_EXITED" => json!({"_tag": "TerminalNotRunningError", "threadId": target.thread, "terminalId": target.terminal}),
            _ => json!({"_tag": "TerminalWriteError", "threadId": target.thread, "terminalId": target.terminal, "terminalPid": 0, "cause": e.message()}),
        };
        if let (Some(into), Some(from)) = (error.as_object_mut(), extra.as_object()) {
            into.extend(from.clone());
        }
        QueryFailure::Error(error)
    }

    /// `terminal.open`: the shell started in `cwd` (a terminal of that name that runs is returned as it is).
    pub fn terminal_open(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let cwd = field(payload, "cwd")?;
        let worktree = payload.get("worktreePath").and_then(Value::as_str).filter(|p| !p.is_empty());
        let target = self.terminal_target(payload, Some(worktree.unwrap_or(cwd)))?;
        let size = |key: &str| payload.get(key).and_then(Value::as_u64).map(|n| n as u16);
        let env = payload.get("env").and_then(Value::as_object).map(|env| env.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect()).unwrap_or_default();
        let op = Op::TerminalOpen { root: target.root.clone(), terminal: target.name.clone(), cwd: cwd.into(), cols: size("cols"), rows: size("rows"), env };
        let opened = self.agent().run(&op).map_err(|e| match &e {
            AgentError::Refused { code, .. } if code == "NOT_FOUND" => QueryFailure::Error(json!({"_tag": "TerminalCwdNotFoundError", "cwd": cwd})),
            AgentError::Refused { code, .. } if code == "NOT_A_DIRECTORY" => QueryFailure::Error(json!({"_tag": "TerminalCwdNotDirectoryError", "cwd": cwd})),
            _ => QueryFailure::Error(json!({"_tag": "TerminalCwdStatError", "cwd": cwd, "cause": e.message()})),
        })?;
        let summary = TerminalSummary {
            name: target.name.clone(),
            cwd: opened["cwd"].as_str().unwrap_or(cwd).to_string(),
            worktree_path: worktree.map(str::to_string),
            title: opened["title"].as_str().unwrap_or_default().to_string(),
            running: opened["status"] == "running",
            exit_code: opened["exit_code"].as_i64().map(|c| c as i32),
            pid: opened["pid"].as_u64().map(|p| p as u32),
            updated_at_ms: opened["updated_at"].as_u64().unwrap_or(0),
        };
        let size = opened["cols"].as_u64().zip(opened["rows"].as_u64()).map(|(c, r)| (c as u16, r as u16));
        let shown = Shown { thread: &target.thread, terminal: &target.terminal, summary: Some(&summary), history: String::new(), running: summary.running, exit_code: summary.exit_code, sequence: opened["offset"].as_u64().unwrap_or(0), size };
        Ok(shown.snapshot(cwd))
    }

    /// `terminal.write`, `terminal.resize` and `terminal.clear`: nothing comes back but the refusal.
    pub fn terminal_act(&self, tag: &str, payload: &Value) -> Result<(), QueryFailure> {
        let target = self.terminal_target(payload, None)?;
        let (root, terminal) = (target.root.clone(), target.name.clone());
        let size = |key: &str| payload.get(key).and_then(Value::as_u64).filter(|n| *n > 0).map(|n| n as u16).ok_or_else(|| QueryFailure::Defect(format!("Expected a positive integer\n  at [\"{key}\"]")));
        let (op, extra) = match tag {
            "terminal.write" => {
                let data = payload.get("data").and_then(Value::as_str).filter(|d| !d.is_empty() && d.len() <= 65_536).ok_or_else(|| QueryFailure::Defect("Expected between 1 and 65536 characters\n  at [\"data\"]".into()))?;
                (Op::TerminalWrite { root, terminal, data: data.into() }, json!({}))
            }
            "terminal.resize" => {
                let (cols, rows) = (size("cols")?, size("rows")?);
                (Op::TerminalResize { root, terminal, cols, rows }, json!({"_tag": "TerminalResizeError", "cols": cols, "rows": rows}))
            }
            _ => (Op::TerminalClear { root, terminal }, json!({})),
        };
        self.agent().run(&op).map(drop).map_err(|e| {
            let mut failure = self.terminal_failure(&e, &target, extra.clone());
            if let (QueryFailure::Error(error), true) = (&mut failure, tag == "terminal.resize") {
                if let Some(map) = error.as_object_mut() {
                    map.insert("terminalPid".into(), json!(0));
                }
            }
            failure
        })
    }

    /// `terminal.close`: the terminal of the thread named, or every terminal of the thread when none is.
    pub fn terminal_close(&self, payload: &Value) -> Result<(), QueryFailure> {
        let thread = field(payload, "threadId")?;
        let named = payload.get("terminalId").and_then(Value::as_str).filter(|t| !t.is_empty());
        let root = self.thread_dir(thread);
        let names: Vec<String> = match named {
            Some(terminal) => vec![terminal_name(thread, terminal).ok_or_else(|| lookup(thread, terminal))?],
            None => {
                let listed = self.agent().terminals().map_err(|e| QueryFailure::Error(json!({"_tag": "TerminalHistoryError", "operation": "read", "threadId": thread, "terminalId": "", "cause": e.message()})))?;
                listed.into_iter().filter(|s| split_name(&s.name).is_some_and(|(t, _)| t == thread)).map(|s| s.name).collect()
            }
        };
        for name in names {
            let terminal = split_name(&name).map_or("", |(_, t)| t).to_string();
            let target = Target { thread: thread.into(), terminal, name: name.clone(), root: root.clone() };
            self.agent().run(&Op::TerminalClose { root: root.clone(), terminal: name }).map_err(|e| self.terminal_failure(&e, &target, json!({})))?;
        }
        Ok(())
    }

    /// `terminal.attach` and `terminal.observe`: what the stream says first (the terminal as a snapshot), and what follows it. An attach with a
    /// `cwd` opens a terminal the agent API has not got; an observe never does.
    pub fn open_terminal_stream(&self, payload: &Value, attach: bool) -> Result<(TerminalFollow, Vec<Value>), QueryFailure> {
        let worktree = payload.get("worktreePath").and_then(Value::as_str).filter(|p| !p.is_empty());
        let cwd = payload.get("cwd").and_then(Value::as_str).filter(|c| !c.is_empty());
        let target = self.terminal_target(payload, worktree.or(cwd))?;
        let mut first = self.agent().terminal_events(&target.root, &target.name, None, WAIT).map_err(|e| self.terminal_failure(&e, &target, json!({})))?;
        if !first.found {
            let Some(cwd) = cwd.filter(|_| attach) else { return Err(lookup(&target.thread, &target.terminal)) };
            let mut open = payload.clone();
            open["cwd"] = json!(cwd);
            self.terminal_open(&open)?;
            first = self.agent().terminal_events(&target.root, &target.name, None, WAIT).map_err(|e| self.terminal_failure(&e, &target, json!({})))?;
        }
        let mut follow = TerminalFollow { thread: target.thread, terminal: target.terminal, name: target.name, root: target.root, cwd: cwd.unwrap_or_default().to_string(), cursor: None, ended: false, exit_told: false, tail: Vec::new() };
        let frames = self.terminal_frames(&mut follow, &first);
        Ok((follow, frames))
    }

    /// The frames the events of a batch are, in order; the follow's cursor moves past them.
    fn terminal_frames(&self, follow: &mut TerminalFollow, batch: &TerminalBatch) -> Vec<Value> {
        let (thread, terminal) = (follow.thread.clone(), follow.terminal.clone());
        let base = |kind: &str, extra: Value| {
            let mut frame = json!({"type": kind, "threadId": thread, "terminalId": terminal});
            if let (Some(into), Some(from)) = (frame.as_object_mut(), extra.as_object()) {
                into.extend(from.clone());
            }
            frame
        };
        let mut frames = Vec::new();
        for event in &batch.events {
            if let Some(end) = event.end() {
                follow.cursor = Some(end);
            }
            if matches!(event, TerminalEvent::Exited { .. }) && std::mem::replace(&mut follow.exit_told, true) {
                continue;
            }
            frames.push(match event {
                TerminalEvent::Snapshot { offset, data, cols, rows, running, exit_code, .. } => {
                    let summary = self.agent().terminals().ok().and_then(|all| all.into_iter().find(|s| s.name == follow.name));
                    follow.tail.clear();
                    let history = decode_chunk(&mut follow.tail, data);
                    let shown = Shown { thread: &thread, terminal: &terminal, summary: summary.as_ref(), history, running: *running, exit_code: *exit_code, sequence: *offset, size: Some((*cols, *rows)) };
                    json!({"type": "snapshot", "snapshot": shown.snapshot(&follow.cwd)})
                }
                TerminalEvent::Output { end, data, .. } => {
                    let text = decode_chunk(&mut follow.tail, data);
                    if text.is_empty() && !data.is_empty() {
                        continue;
                    }
                    base("output", json!({"sequence": end, "data": text}))
                }
                TerminalEvent::Exited { exit_code } => base("exited", json!({"exitCode": exit_code, "exitSignal": null})),
                TerminalEvent::Closed => {
                    follow.ended = true;
                    base("closed", json!({}))
                }
                TerminalEvent::Cleared => base("cleared", json!({})),
            });
        }
        frames
    }

    /// Follows the terminal on a thread of its own, sending the frames to `to` (with the id of the stream) until it is closed, the agent API
    /// stops answering or `stop` is set.
    pub fn follow_terminal(&self, mut follow: TerminalFollow, id: Value, to: Sender<(Value, Value)>, stop: Arc<AtomicBool>) {
        let svc = self.clone();
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) && !follow.ended {
                match svc.agent().terminal_events(&follow.root, &follow.name, follow.cursor, WAIT) {
                    Ok(batch) if !batch.found => {
                        let _ = to.send((id.clone(), json!({"type": "closed", "threadId": follow.thread, "terminalId": follow.terminal})));
                        return;
                    }
                    Ok(batch) => {
                        let frames = svc.terminal_frames(&mut follow, &batch);
                        if frames.is_empty() {
                            std::thread::sleep(IDLE);
                        }
                        if frames.into_iter().any(|frame| to.send((id.clone(), frame)).is_err()) {
                            return;
                        }
                    }
                    Err(e) => {
                        svc.log("terminal.follow.stopped", &[("thread", follow.thread.clone()), ("terminal", follow.terminal.clone()), ("message", e.message())]);
                        return;
                    }
                }
            }
        });
    }

    /// `subscribeTerminalMetadata`: the terminals now as a snapshot, and then an upsert or a remove for each change, on a thread of its own.
    pub fn open_terminal_metadata(&self) -> Result<(MetaFold, Value), QueryFailure> {
        let listed = self.agent().terminals().map_err(|e| QueryFailure::Error(crate::usecases::terminal::unreadable(&e)))?;
        let mut fold = MetaFold::default();
        let first = fold.frames(&listed, true).remove(0);
        Ok((fold, first))
    }

    pub fn follow_terminal_metadata(&self, mut fold: MetaFold, id: Value, to: Sender<(Value, Value)>, stop: Arc<AtomicBool>) {
        let svc = self.clone();
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(WAIT);
                match svc.agent().terminals() {
                    Ok(listed) => {
                        if fold.frames(&listed, false).into_iter().any(|frame| to.send((id.clone(), frame)).is_err()) {
                            return;
                        }
                    }
                    Err(e) => {
                        svc.log("terminal.metadata.stopped", &[("message", e.message())]);
                        return;
                    }
                }
            }
        });
    }
}

pub(crate) fn unreadable(e: &AgentError) -> Value {
    json!({"_tag": "TerminalHistoryError", "operation": "read", "threadId": "", "terminalId": "", "cause": e.message()})
}

/// A terminal stream being followed: where it is and how far it was told.
pub struct TerminalFollow {
    thread: String,
    terminal: String,
    name: String,
    root: String,
    cwd: String,
    cursor: Option<u64>,
    ended: bool,
    /// The exit was told: every look after it hears it again, so the app is told once.
    exit_told: bool,
    /// Bytes of a multibyte character cut by the end of a chunk: the next chunk completes it.
    tail: Vec<u8>,
}

fn decode_chunk(tail: &mut Vec<u8>, data: &[u8]) -> String {
    let mut buf = std::mem::take(tail);
    buf.extend_from_slice(data);
    let (mut out, mut rest) = (String::new(), &buf[..]);
    while let Err(e) = std::str::from_utf8(rest) {
        let (ok, bad) = rest.split_at(e.valid_up_to());
        out.push_str(&String::from_utf8_lossy(ok));
        match e.error_len() {
            None => {
                *tail = bad.to_vec();
                return out;
            }
            Some(n) => {
                out.push('\u{FFFD}');
                rest = &bad[n..];
            }
        }
    }
    out.push_str(&String::from_utf8_lossy(rest));
    out
}

impl TerminalFollow {
    pub fn ended(&self) -> bool {
        self.ended
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_multibyte_character_split_across_two_chunks_when_decoded_then_it_arrives_whole() {
        // Given a 4-byte emoji and a 3-byte box character cut in the middle
        let bytes = "a\u{1F600}\u{2500}b".as_bytes();
        for cut in 1..bytes.len() {
            let mut tail = Vec::new();
            // When the two halves are decoded in order
            let joined = decode_chunk(&mut tail, &bytes[..cut]) + &decode_chunk(&mut tail, &bytes[cut..]);
            // Then no replacement character appears
            assert_eq!(joined, "a\u{1F600}\u{2500}b", "cut {cut}");
        }
        // And a byte that cannot start a character is replaced, not held
        assert_eq!(decode_chunk(&mut Vec::new(), &[b'x', 0xff, b'y']), "x\u{FFFD}y");
    }

    fn summary(name: &str, running: bool, at: u64) -> TerminalSummary {
        TerminalSummary { name: name.into(), cwd: "/p".into(), worktree_path: None, title: "t".into(), running, exit_code: None, pid: Some(9), updated_at_ms: at }
    }

    #[test]
    fn given_ids_of_a_thread_and_a_terminal_when_joined_then_the_name_splits_back_and_ids_that_cannot_be_joined_are_none() {
        // Given a uuid thread and the default terminal id
        let name = terminal_name("0b3a1c2e-1111-4222-8333-444455556666", "term-1").unwrap();
        // Then the name splits back to the two ids, and ids with a dot, a slash, nothing or too much are none
        assert_eq!(split_name(&name), Some(("0b3a1c2e-1111-4222-8333-444455556666", "term-1")));
        for (thread, terminal) in [("a.b", "t"), ("a", "t/x"), ("", "t"), ("a", ""), (&"x".repeat(40), "term-1234")] {
            assert_eq!(terminal_name(thread, terminal), None, "{thread} {terminal}");
        }
        assert_eq!(split_name("nodot"), None);
    }

    #[test]
    fn given_the_terminals_listed_over_time_when_folded_then_a_snapshot_comes_first_and_then_an_upsert_for_what_changed_and_a_remove_for_what_went_without_the_clock_counting_as_a_change() {
        // Given a fold told two terminals, one of which is not a T3 terminal
        let mut fold = MetaFold::default();
        let first = fold.frames(&[summary("t1.a", true, 1), summary("t1.b", true, 1), summary("other", true, 1)], true);
        assert_eq!((first.len(), first[0]["type"].clone(), first[0]["terminals"].as_array().unwrap().len()), (1, json!("snapshot"), 2));
        // When only the time moved, one exited, and one was closed
        assert!(fold.frames(&[summary("t1.a", true, 99), summary("t1.b", true, 99)], false).is_empty());
        let changes = fold.frames(&[summary("t1.a", false, 100)], false);
        // Then the exit is an upsert with the new status, and the closed one is a remove naming its thread and terminal
        assert_eq!(changes[0]["type"], json!("upsert"));
        assert_eq!(changes[0]["terminal"]["status"], json!("exited"));
        assert_eq!(changes[1], json!({"type": "remove", "threadId": "t1", "terminalId": "b"}));
    }
}
