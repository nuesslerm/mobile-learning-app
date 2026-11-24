//! The double of the crate's port for tests: provider CLIs' live processes as scripted ones.
use crate::application::ports::{Link, Linker, Running};
use crate::domain::Driver;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};

enum Emit {
    Line(String),
    End(Result<(), String>),
}

#[derive(Default)]
struct Starts {
    count: usize,
    kills: usize,
    received: Vec<(usize, serde_json::Value)>,
    opened: Vec<PathBuf>,
    flags: Vec<Vec<String>>,
    emitters: Vec<mpsc::UnboundedSender<Emit>>,
    refuse: Option<String>,
}

type React = Arc<dyn Fn(&[String], usize, &serde_json::Value) -> Vec<String> + Send + Sync>;

/// A CLI's live server as a scripted process: what it answers to each line it is sent is decided by a function of the start it is and the
/// message. A test pushes lines of its own (`emit`) and ends the process as a crash does (`die`).
pub struct ScriptedLinker {
    state: Arc<Mutex<Starts>>,
    react: React,
    /// What every process prints when it starts, before it is sent anything.
    greeting: Vec<String>,
}

impl ScriptedLinker {
    /// A server that answers each message with the lines `react(start, message)` returns, `start` counting the processes from 1.
    pub fn reacting(react: impl Fn(usize, &serde_json::Value) -> Vec<String> + Send + Sync + 'static) -> Arc<ScriptedLinker> {
        Arc::new(ScriptedLinker { state: Arc::default(), react: Arc::new(move |_, start, message| react(start, message)), greeting: Vec::new() })
    }

    /// As `reacting`, for a process that says `greeting` when it starts.
    pub fn reacting_greeted(greeting: Vec<String>, react: impl Fn(usize, &serde_json::Value) -> Vec<String> + Send + Sync + 'static) -> Arc<ScriptedLinker> {
        Arc::new(ScriptedLinker { state: Arc::default(), react: Arc::new(move |_, start, message| react(start, message)), greeting })
    }

    /// As `reacting`, for a process that is told its flags when it starts (a conversation to run in).
    pub fn reacting_with_flags(react: impl Fn(&[String], usize, &serde_json::Value) -> Vec<String> + Send + Sync + 'static) -> Arc<ScriptedLinker> {
        Arc::new(ScriptedLinker { state: Arc::default(), react: Arc::new(react), greeting: Vec::new() })
    }

    fn state(&self) -> std::sync::MutexGuard<'_, Starts> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Prints `line` on the newest process.
    pub fn emit(&self, line: impl Into<String>) {
        if let Some(tx) = self.state().emitters.last() {
            let _ = tx.send(Emit::Line(line.into()));
        }
    }

    /// Ends the newest process as one that crashed, with `stderr` as its last words.
    pub fn die(&self, stderr: &str) {
        if let Some(tx) = self.state().emitters.last() {
            let _ = tx.send(Emit::End(Err(stderr.to_string())));
        }
    }

    /// The next start is refused with `why`.
    pub fn refuse_next(&self, why: &str) {
        self.state().refuse = Some(why.to_string());
    }

    /// Every message the processes were sent, with the number of the process (from 1).
    pub fn received(&self) -> Vec<(usize, serde_json::Value)> {
        self.state().received.clone()
    }

    /// The messages with this `method` that were sent, with the number of the process.
    pub fn received_of(&self, method: &str) -> Vec<(usize, serde_json::Value)> {
        self.received().into_iter().filter(|(_, m)| m["method"] == method).collect()
    }

    pub fn starts(&self) -> usize {
        self.state().count
    }

    pub fn kills(&self) -> usize {
        self.state().kills
    }

    pub fn opened(&self) -> Vec<PathBuf> {
        self.state().opened.clone()
    }

    /// The flags each start was given, in order.
    pub fn flags(&self) -> Vec<Vec<String>> {
        self.state().flags.clone()
    }
}

#[async_trait]
impl Linker for ScriptedLinker {
    async fn open(&self, _: Driver, cwd: &Path, args: &[String]) -> Result<Link, String> {
        let (start, tx, rx) = {
            let mut state = self.state();
            if let Some(why) = state.refuse.take() {
                return Err(why);
            }
            state.count += 1;
            state.opened.push(cwd.to_path_buf());
            state.flags.push(args.to_vec());
            let (tx, rx) = mpsc::unbounded_channel::<Emit>();
            state.emitters.push(tx.clone());
            for line in &self.greeting {
                let _ = tx.send(Emit::Line(line.clone()));
            }
            (state.count, tx, rx)
        };
        let (done_tx, done_rx) = oneshot::channel();
        let lines = futures_util::stream::unfold((rx, Some(done_tx)), |(mut rx, mut done)| async move {
            match rx.recv().await {
                Some(Emit::Line(line)) => Some((line, (rx, done))),
                Some(Emit::End(result)) => {
                    if let Some(done) = done.take() {
                        let _ = done.send(result);
                    }
                    None
                }
                None => {
                    if let Some(done) = done.take() {
                        let _ = done.send(Ok(()));
                    }
                    None
                }
            }
        });
        let (state, react, answer, flags) = (Arc::clone(&self.state), Arc::clone(&self.react), tx.clone(), args.to_vec());
        let send: Arc<dyn Fn(String) -> bool + Send + Sync> = Arc::new(move |line| {
            let Ok(message) = serde_json::from_str::<serde_json::Value>(&line) else { return false };
            state.lock().unwrap_or_else(|p| p.into_inner()).received.push((start, message.clone()));
            for out in react(&flags, start, &message) {
                if answer.send(Emit::Line(out)).is_err() {
                    return false;
                }
            }
            true
        });
        let (state, stop) = (Arc::clone(&self.state), tx);
        let kill: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            state.lock().unwrap_or_else(|p| p.into_inner()).kills += 1;
            let _ = stop.send(Emit::End(Ok(())));
        });
        Ok(Link { running: Running { lines: Box::pin(lines), finish: Box::pin(async move { done_rx.await.unwrap_or(Ok(())) }), kill }, send })
    }
}

/// What a scripted server says, as the lines it prints.
pub fn say(v: serde_json::Value) -> String {
    v.to_string()
}

pub fn reply(id: &serde_json::Value, result: serde_json::Value) -> String {
    say(serde_json::json!({"id": id, "result": result}))
}

pub fn note(method: &str, params: serde_json::Value) -> String {
    say(serde_json::json!({"method": method, "params": params}))
}

pub fn started(thread: &str, turn: &str) -> String {
    note("turn/started", serde_json::json!({"threadId": thread, "turn": {"id": turn}}))
}

pub fn delta(thread: &str, turn: &str, text: &str) -> String {
    note("item/agentMessage/delta", serde_json::json!({"threadId": thread, "turnId": turn, "itemId": "m", "delta": text}))
}

pub fn finish(thread: &str, turn: &str, status: &str) -> String {
    note("turn/completed", serde_json::json!({"threadId": thread, "turn": {"id": turn, "status": status, "error": null}}))
}

/// A Codex app-server that mints `th-<n>` for each thread it starts and `u-<n>` for each turn: a prompt `hello` is answered at once, `hold` runs until
/// it is steered or interrupted, `approve` asks to run a command and goes on when the person has answered.
pub fn app_server() -> Arc<ScriptedLinker> {
    use serde_json::json;
    let (threads, turns) = (AtomicUsize::new(0), AtomicUsize::new(0));
    ScriptedLinker::reacting(move |_, m| {
        let (id, params) = (&m["id"], &m["params"]);
        let thread = params["threadId"].as_str().unwrap_or_default().to_string();
        match m["method"].as_str() {
            Some("initialize") => vec![reply(id, json!({"userAgent": "scripted"}))],
            Some("thread/start") => vec![reply(id, json!({"thread": {"id": format!("th-{}", threads.fetch_add(1, Ordering::SeqCst) + 1)}}))],
            Some("thread/resume") => vec![reply(id, json!({"thread": {"id": thread}}))],
            Some("turn/start") => {
                let turn = format!("u-{}", turns.fetch_add(1, Ordering::SeqCst) + 1);
                let mut out = vec![reply(id, json!({"turn": {"id": turn}})), started(&thread, &turn)];
                match params["input"][0]["text"].as_str().unwrap_or_default() {
                    "hold" => out.push(delta(&thread, &turn, "working")),
                    "approve" => out.extend([
                        note("item/started", json!({"threadId": thread, "turnId": turn, "item": {"type": "commandExecution", "id": "c1", "command": "touch a", "cwd": "/w"}})),
                        say(json!({"id": 0, "method": "item/commandExecution/requestApproval", "params": {"threadId": thread, "turnId": turn, "itemId": "c1", "command": "touch a", "reason": "May I?"}})),
                    ]),
                    _ => out.extend([delta(&thread, &turn, "Hi"), finish(&thread, &turn, "completed")]),
                }
                out
            }
            Some("turn/steer") => {
                let turn = params["expectedTurnId"].as_str().unwrap_or_default();
                vec![reply(id, json!({"turnId": turn})), delta(&thread, turn, " steered"), finish(&thread, turn, "completed")]
            }
            Some("turn/interrupt") => vec![reply(id, json!({})), finish(&thread, params["turnId"].as_str().unwrap_or_default(), "interrupted")],
            None if m["id"] == 0 => vec![
                note("item/completed", json!({"threadId": "th-1", "turnId": "u-1", "item": {"type": "commandExecution", "id": "c1", "command": "touch a", "status": "completed", "aggregatedOutput": "", "exitCode": 0}})),
                delta("th-1", "u-1", "done"),
                finish("th-1", "u-1", "completed"),
            ],
            _ => vec![],
        }
    })
}

/// A Claude Code process on stream-json: a prompt `hello` is answered at once (text, a tool call and its result, then the result frame), `hold` runs until it is
/// steered or interrupted, `approve` asks for the permission to write a file and goes on when the person has answered, and `ask` asks two questions through
/// `AskUserQuestion`. Every result names the messages it consumed.
pub fn claude_session() -> Arc<ScriptedLinker> {
    use serde_json::{json, Value};
    let current = Mutex::new(Vec::<String>::new());
    // The conversation a process runs in is the id its flags name, as the real CLI's frames carry it
    let conversation = |flags: &[String]| flags.iter().position(|f| f == "--session-id" || f == "--resume").and_then(|at| flags.get(at + 1)).cloned().unwrap_or_default();
    let control = |id: &Value, body: Value| say(json!({"type": "control_response", "response": {"subtype": "success", "request_id": id, "response": body}}));
    let init = |id: &str| say(json!({"type": "system", "subtype": "init", "session_id": id}));
    let delta = |message: &str, text: &str| {
        [
            say(json!({"type": "stream_event", "parent_tool_use_id": null, "event": {"type": "message_start", "message": {"id": message}}})),
            say(json!({"type": "stream_event", "parent_tool_use_id": null, "event": {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": text}}})),
        ]
    };
    let result = |messages: &[String], aborted: bool, error: Option<&str>| {
        say(json!({"type": "result", "subtype": if aborted { "error_during_execution" } else { "success" }, "is_error": aborted || error.is_some(), "terminal_reason": if aborted { "aborted_streaming" } else { "completed" }, "result": error, "user_message_uuids": messages}))
    };
    let tool = |id: &str, name: &str, input: Value| say(json!({"type": "assistant", "message": {"id": format!("m-{id}"), "content": [{"type": "tool_use", "id": id, "name": name, "input": input}]}}));
    let finished = |id: &str, text: &str, is_error: bool| say(json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": id, "content": text, "is_error": is_error}]}}));
    let asks = |request: &str, tool: &str, id: &str, input: Value| say(json!({"type": "control_request", "request_id": request, "request": {"subtype": "can_use_tool", "tool_name": tool, "input": input, "tool_use_id": id}}));
    ScriptedLinker::reacting_with_flags(move |flags, _, m| {
        let id = &m["request_id"];
        let session = conversation(flags);
        match (m["type"].as_str(), m["request"]["subtype"].as_str()) {
            (Some("control_request"), Some("initialize")) => vec![control(id, json!({"models": [], "pid": 1}))],
            (Some("control_request"), Some("set_permission_mode")) => {
                // The real CLI only enters full access when it was launched in it
                let launched = flags.windows(2).any(|w| w[0] == "--permission-mode" && w[1] == "bypassPermissions");
                if m["request"]["mode"] == "bypassPermissions" && !launched {
                    return vec![say(json!({"type": "control_response", "response": {"subtype": "error", "request_id": id, "error": "Cannot set permission mode to bypassPermissions because the session was not launched with --dangerously-skip-permissions"}}))];
                }
                vec![control(id, json!({"mode": m["request"]["mode"]}))]
            }
            (Some("control_request"), Some("set_model")) => vec![control(id, json!({"model": m["request"]["model"]}))],
            (Some("control_request"), Some("interrupt")) => {
                let messages = std::mem::take(&mut *current.lock().unwrap_or_else(|p| p.into_inner()));
                vec![control(id, json!({"still_queued": [], "cancelled": []})), result(&messages, true, None)]
            }
            (Some("user"), _) => {
                let uuid = m["uuid"].as_str().unwrap_or_default().to_string();
                let mut messages = current.lock().unwrap_or_else(|p| p.into_inner());
                if m["priority"] == "now" {
                    let aborted = result(&messages, true, None);
                    messages.push(uuid.clone());
                    let mut out = vec![aborted, init(&session)];
                    out.extend(delta("m-steer", " steered"));
                    out.push(result(&[uuid], false, None));
                    messages.clear();
                    return out;
                }
                *messages = vec![uuid.clone()];
                let mut out = vec![init(&session)];
                match m["message"]["content"][0]["text"].as_str().unwrap_or_default() {
                    "hold" => out.extend(delta("m1", "working")),
                    "approve" => out.extend([tool("toolu_9", "Write", json!({"file_path": "a.txt"})), asks("c9", "Write", "toolu_9", json!({"file_path": "a.txt"}))]),
                    "ask" => out.push(asks("q1", "AskUserQuestion", "toolu_q", json!({"questions": [
                        {"question": "Which library?", "header": "Library", "multiSelect": false, "options": [{"label": "date-fns", "description": "a"}, {"label": "dayjs", "description": "b"}]},
                        {"question": "Which style?", "header": "Style", "multiSelect": false, "options": [{"label": "tabs", "description": "a"}, {"label": "spaces", "description": "b"}]}]}))),
                    _ => {
                        out.extend(delta("m1", "Hi"));
                        out.extend([say(json!({"type": "assistant", "message": {"id": "m1", "content": [{"type": "text", "text": "Hi"}]}})), tool("k1", "Bash", json!({"command": "ls"})), finished("k1", "a.txt", false), result(&[uuid], false, None)]);
                        messages.clear();
                    }
                }
                out
            }
            (Some("control_response"), _) => {
                let (request, body) = (m["response"]["request_id"].as_str().unwrap_or_default(), &m["response"]["response"]);
                let messages = std::mem::take(&mut *current.lock().unwrap_or_else(|p| p.into_inner()));
                let mut out = Vec::new();
                match (request, body["behavior"].as_str()) {
                    ("c9", Some("allow")) => {
                        out.push(finished("toolu_9", "wrote a.txt", false));
                        out.extend(delta("m-after", "written"));
                    }
                    ("c9", Some("deny")) => {
                        out.push(finished("toolu_9", "User declined tool execution.", true));
                        out.extend(delta("m-after", "declined"));
                    }
                    ("q1", _) => out.extend(delta("m-after", "chose")),
                    _ => return vec![],
                }
                out.push(result(&messages, false, None));
                out
            }
            _ => vec![],
        }
    })
}

/// A line of the executor's bridge: the answer to request `id`.
pub fn response(id: &serde_json::Value, status: u16, body: serde_json::Value) -> String {
    say(serde_json::json!({"id": id, "status": status, "body": if body.is_null() { String::new() } else { body.to_string() }}))
}

/// A line of the executor's bridge: one frame of the server's event stream.
pub fn frame(kind: &str, properties: serde_json::Value) -> String {
    say(serde_json::json!({"event": {"directory": "/w", "payload": {"id": "evt", "type": kind, "properties": properties}}}))
}

fn user_message(session: &str, message: &str, text: &str) -> Vec<String> {
    vec![
        frame("message.updated", serde_json::json!({"sessionID": session, "info": {"id": message, "role": "user"}})),
        frame("message.part.updated", serde_json::json!({"sessionID": session, "part": {"id": format!("{message}-p"), "messageID": message, "type": "text", "text": text}})),
    ]
}

fn assistant_says(session: &str, message: &str, text: &str) -> Vec<String> {
    use serde_json::json;
    let part = format!("{message}-t");
    vec![
        frame("message.updated", json!({"sessionID": session, "info": {"id": message, "role": "assistant"}})),
        frame("message.part.updated", json!({"sessionID": session, "part": {"id": part, "messageID": message, "type": "text", "text": ""}})),
        frame("message.part.delta", json!({"sessionID": session, "messageID": message, "partID": part, "field": "text", "delta": text})),
        frame("message.part.updated", json!({"sessionID": session, "part": {"id": part, "messageID": message, "type": "text", "text": text}})),
    ]
}

fn idle(session: &str) -> Vec<String> {
    vec![frame("session.status", serde_json::json!({"sessionID": session, "status": {"type": "idle"}})), frame("session.idle", serde_json::json!({"sessionID": session}))]
}

/// An opencode server behind the executor's bridge: it says it is ready when it starts, mints `ses-<n>` for each session created (and knows no session
/// named `ses-missing`), and answers a prompt by its text: `hello` is answered at once (text, a tool call and its result, then idle), `hold` runs until it is
/// steered or aborted, `silent` is taken and says nothing, `approve` asks for the permission to run a command and goes on when the person has answered, `ask` asks a question, and `fail` fails
/// the session. A prompt on a session that is busy is a steer.
pub fn opencode_server() -> Arc<ScriptedLinker> {
    use serde_json::json;
    let (sessions, stored, busy) = (AtomicUsize::new(0), AtomicUsize::new(0), Mutex::new(std::collections::HashSet::<String>::new()));
    ScriptedLinker::reacting_greeted(vec![say(json!({"ready": true}))], move |start, m| {
        let (id, path) = (&m["id"], m["path"].as_str().unwrap_or_default());
        let route = path.split('?').next().unwrap_or_default();
        let parts: Vec<&str> = route.trim_matches('/').split('/').collect();
        let mut busy = busy.lock().unwrap_or_else(|p| p.into_inner());
        match (m["method"].as_str().unwrap_or_default(), parts.as_slice()) {
            ("POST", ["session"]) => vec![response(id, 200, json!({"id": format!("ses-{}", sessions.fetch_add(1, Ordering::SeqCst) + 1)}))],
            ("GET", ["session", "ses-missing"]) => vec![response(id, 404, json!({"name": "NotFoundError", "data": {"message": "Session not found: ses-missing"}}))],
            ("GET" | "PATCH", ["session", session]) => vec![response(id, 200, json!({"id": session}))],
            ("POST", ["session", session, "prompt_async"]) => {
                let (session, key) = (session.to_string(), format!("{start}/{session}"));
                let text = m["body"]["parts"][0]["text"].as_str().unwrap_or_default().to_string();
                let message = format!("u-{start}-{}-{}", stored.fetch_add(1, Ordering::SeqCst), text.replace(' ', "-"));
                let mut out = vec![response(id, 204, json!(null))];
                if text == "silent" {
                    return out;
                }
                out.extend(user_message(&session, &message, &text));
                if busy.contains(&key) {
                    out.extend(assistant_says(&session, &format!("a-{message}"), " steered"));
                    out.extend(idle(&session));
                    busy.remove(&key);
                    return out;
                }
                out.push(frame("session.status", json!({"sessionID": session, "status": {"type": "busy"}})));
                let reply = format!("a-{message}");
                match text.as_str() {
                    "hold" => {
                        busy.insert(key.clone());
                        out.extend(assistant_says(&session, &reply, "working"));
                    }
                    "approve" => {
                        busy.insert(key.clone());
                        out.extend(assistant_says(&session, &reply, "need to run it"));
                        out.push(frame("permission.asked", json!({"id": "per_1", "sessionID": session, "permission": "bash", "patterns": ["touch a"], "metadata": {}, "always": ["bash"], "tool": {"messageID": reply, "callID": "c1"}})));
                    }
                    "ask" => {
                        busy.insert(key.clone());
                        out.extend(assistant_says(&session, &reply, "a question"));
                        out.push(frame("question.asked", json!({"id": "que_1", "sessionID": session, "questions": [{"question": "Which library?", "header": "Library", "options": [{"label": "date-fns", "description": "a"}, {"label": "dayjs", "description": "b"}]}]})));
                    }
                    "fail" => {
                        out.push(frame("session.error", json!({"sessionID": session, "error": {"name": "APIError", "data": {"message": "rate limited"}}})));
                        out.extend(idle(&session));
                    }
                    _ => {
                        out.extend(assistant_says(&session, &reply, "Hi"));
                        out.push(frame("message.part.updated", json!({"sessionID": session, "part": {"id": format!("{reply}-k"), "messageID": reply, "type": "tool", "callID": "k1", "tool": "bash", "state": {"status": "running", "input": {"command": "ls"}}}})));
                        out.push(frame("message.part.updated", json!({"sessionID": session, "part": {"id": format!("{reply}-k"), "messageID": reply, "type": "tool", "callID": "k1", "tool": "bash", "state": {"status": "completed", "input": {"command": "ls"}, "output": "a.txt"}}})));
                        out.extend(idle(&session));
                    }
                }
                out
            }
            ("POST", ["session", session, "abort"]) => {
                let mut out = vec![response(id, 200, json!(true))];
                if busy.remove(&format!("{start}/{session}")) {
                    out.push(frame("session.error", json!({"sessionID": session, "error": {"name": "MessageAbortedError", "data": {"message": "aborted"}}})));
                }
                out.extend(idle(session));
                out
            }
            ("POST", ["permission", request, "reply"]) => {
                let session = "ses-1";
                let reply = m["body"]["reply"].as_str().unwrap_or_default();
                busy.remove(&format!("{start}/{session}"));
                let mut out = vec![response(id, 200, json!(true)), frame("permission.replied", json!({"sessionID": session, "requestID": request, "reply": reply}))];
                out.extend(assistant_says(session, "a-after", if reply == "reject" { "declined" } else { "done" }));
                out.extend(idle(session));
                out
            }
            ("POST", ["question", request, "reply" | "reject"]) => {
                let session = "ses-1";
                busy.remove(&format!("{start}/{session}"));
                let mut out = vec![response(id, 200, json!(true)), frame("question.replied", json!({"sessionID": session, "requestID": request, "answers": m["body"]["answers"]}))];
                out.extend(assistant_says(session, "a-after", "chose"));
                out.extend(idle(session));
                out
            }
            _ => vec![response(id, 404, json!({"name": "NotFoundError", "data": {"message": "no route"}}))],
        }
    })
}

/// What one prompt of a scripted opencode does: the lines it prints, in the dialect of `opencode run --format json` (a `text` line, a `tool_use` line with its
/// state, an `error` line), and how the turn ends.
#[derive(Clone, Default)]
pub struct CliScript {
    pub lines: Vec<String>,
    /// The turn fails with this message after its lines.
    pub exit: Option<String>,
    /// The turn stays open after its lines until it is steered or aborted.
    pub hold: bool,
}

/// An opencode server for the flows that need no more than "a CLI that says these lines": every prompt plays the next script (the last again after
/// the end), as the events a real server sends for those lines. A steer of a held turn ends it; an abort ends it as an abort does.
pub struct ScriptedOpencode {
    linker: Arc<ScriptedLinker>,
    scripts: Arc<Mutex<Vec<CliScript>>>,
    prompts: Arc<Mutex<Vec<String>>>,
    aborts: Arc<AtomicUsize>,
}

impl ScriptedOpencode {
    pub fn new() -> Arc<ScriptedOpencode> {
        use serde_json::{json, Value};
        let scripts: Arc<Mutex<Vec<CliScript>>> = Arc::default();
        let prompts: Arc<Mutex<Vec<String>>> = Arc::default();
        let aborts = Arc::new(AtomicUsize::new(0));
        let (played, sessions, held) = (Arc::new(AtomicUsize::new(0)), AtomicUsize::new(0), Mutex::new(std::collections::HashSet::<String>::new()));
        let (read, seen, stopped) = (Arc::clone(&scripts), Arc::clone(&prompts), Arc::clone(&aborts));
        let linker = ScriptedLinker::reacting_greeted(vec![say(json!({"ready": true}))], move |start, m| {
            let (id, path) = (&m["id"], m["path"].as_str().unwrap_or_default());
            let parts: Vec<&str> = path.split('?').next().unwrap_or_default().trim_matches('/').split('/').collect();
            let mut held = held.lock().unwrap_or_else(|p| p.into_inner());
            match (m["method"].as_str().unwrap_or_default(), parts.as_slice()) {
                ("POST", ["session"]) => vec![response(id, 200, json!({"id": format!("ses-{}", sessions.fetch_add(1, Ordering::SeqCst) + 1)}))],
                ("GET" | "PATCH", ["session", session]) => vec![response(id, 200, json!({"id": session}))],
                ("POST", ["session", session, "abort"]) => {
                    stopped.fetch_add(1, Ordering::SeqCst);
                    let mut out = vec![response(id, 200, json!(true))];
                    if held.remove(*session) {
                        out.push(frame("session.error", json!({"sessionID": session, "error": {"name": "MessageAbortedError", "data": {"message": "aborted"}}})));
                    }
                    out.extend(idle(session));
                    out
                }
                ("POST", ["session", session, "prompt_async"]) => {
                    let text = m["body"]["parts"][0]["text"].as_str().unwrap_or_default().to_string();
                    let n = played.fetch_add(1, Ordering::SeqCst);
                    seen.lock().unwrap_or_else(|p| p.into_inner()).push(text.clone());
                    let message = format!("u-{n}");
                    let mut out = vec![response(id, 204, json!(null))];
                    out.extend(user_message(session, &message, &text));
                    if held.contains(*session) {
                        out.extend(assistant_says(session, &format!("a-{message}"), " steered"));
                        out.extend(idle(session));
                        held.remove(*session);
                        return out;
                    }
                    let script = {
                        let all = read.lock().unwrap_or_else(|p| p.into_inner());
                        all.get(n).or(all.last()).cloned().unwrap_or_default()
                    };
                    let reply = format!("a-{message}");
                    out.push(frame("session.status", json!({"sessionID": session, "status": {"type": "busy"}})));
                    out.push(frame("message.updated", json!({"sessionID": session, "info": {"id": reply, "role": "assistant"}})));
                    for (i, line) in script.lines.iter().enumerate() {
                        let Ok(line) = serde_json::from_str::<Value>(line) else { continue };
                        let part = format!("{reply}-{i}");
                        match line["type"].as_str() {
                            Some("text") => {
                                let text = line["part"]["text"].as_str().unwrap_or_default();
                                out.push(frame("message.part.updated", json!({"sessionID": session, "part": {"id": part, "messageID": reply, "type": "text", "text": text}})));
                            }
                            Some("tool_use") => {
                                let (tool, state) = (&line["part"], &line["part"]["state"]);
                                for state in [json!({"status": "running", "input": state["input"]}), state.clone()] {
                                    out.push(frame("message.part.updated", json!({"sessionID": session, "part": {"id": part, "messageID": reply, "type": "tool", "callID": tool["callID"], "tool": tool["tool"], "state": state}})));
                                }
                            }
                            Some("error") => out.push(frame("session.error", json!({"sessionID": session, "error": {"name": "UnknownError", "data": {"message": line["error"]["data"]["message"]}}}))),
                            _ => {}
                        }
                    }
                    if let Some(why) = &script.exit {
                        out.push(frame("session.error", json!({"sessionID": session, "error": {"name": "UnknownError", "data": {"message": why}}})));
                    }
                    if script.hold {
                        held.insert(session.to_string());
                    } else {
                        out.extend(idle(session));
                    }
                    let _ = start;
                    out
                }
                _ => vec![response(id, 404, json!({"name": "NotFoundError", "data": {"message": "no route"}}))],
            }
        });
        Arc::new(ScriptedOpencode { linker, scripts, prompts, aborts })
    }

    /// What every prompt does, whichever it is.
    pub fn script(&self, script: CliScript) {
        self.script_each(vec![script]);
    }

    /// What the prompts do, in order: the first prompt plays the first script, and a prompt after the last plays the last again.
    pub fn script_each(&self, scripts: Vec<CliScript>) {
        *self.scripts.lock().unwrap_or_else(|p| p.into_inner()) = scripts;
    }

    pub fn linker(&self) -> Arc<dyn Linker> {
        self.linker.clone()
    }

    pub fn server(&self) -> &Arc<ScriptedLinker> {
        &self.linker
    }

    /// The prompts the server was sent, oldest first.
    pub fn prompts(&self) -> Vec<String> {
        self.prompts.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// How many sessions were asked to abort.
    pub fn aborts(&self) -> usize {
        self.aborts.load(Ordering::SeqCst)
    }
}
