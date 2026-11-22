//! The double of the crate's port for tests: provider CLIs as scripted processes.
use crate::application::ports::{Link, Linker, Running, Spawner};
use crate::domain::Driver;
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot, Notify};

/// What a faked provider CLI prints and how it ends.
#[derive(Clone, Default)]
pub struct CliScript {
    pub lines: Vec<String>,
    /// `Err` is the stderr tail of a CLI that exited with a failure.
    pub exit: Option<String>,
    /// The process stays up after its lines until it is killed.
    pub hold: bool,
    /// The program cannot be started (such as `not found`).
    pub not_started: Option<String>,
}

/// One start the spawner was asked for.
#[derive(Clone, Debug)]
pub struct Spawned {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

/// The provider CLIs as scripted processes, keyed by program name; a program with no script prints nothing and exits cleanly.
#[derive(Default)]
pub struct ScriptedSpawner {
    scripts: Mutex<HashMap<String, Vec<CliScript>>>,
    spawned: Mutex<Vec<Spawned>>,
    killed: Arc<AtomicUsize>,
}

impl ScriptedSpawner {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// What every start of `program` does.
    pub fn script(&self, program: &str, script: CliScript) {
        self.script_each(program, vec![script]);
    }

    /// What the starts of `program` do, in order: the first start plays the first script, and a start after the last plays the last again.
    pub fn script_each(&self, program: &str, scripts: Vec<CliScript>) {
        self.scripts.lock().unwrap_or_else(|p| p.into_inner()).insert(program.to_string(), scripts);
    }

    pub fn spawned(&self) -> Vec<Spawned> {
        self.spawned.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub fn kills(&self) -> usize {
        self.killed.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Spawner for ScriptedSpawner {
    async fn spawn(&self, program: &str, args: &[String], cwd: &Path) -> Result<Running, String> {
        let start = {
            let mut spawned = self.spawned.lock().unwrap_or_else(|p| p.into_inner());
            spawned.push(Spawned { program: program.into(), args: args.to_vec(), cwd: cwd.to_path_buf() });
            spawned.iter().filter(|s| s.program == program).count() - 1
        };
        let script = self.scripts.lock().unwrap_or_else(|p| p.into_inner()).get(program).and_then(|all| all.get(start).or(all.last())).cloned().unwrap_or_default();
        if let Some(why) = script.not_started.clone() {
            return Err(why);
        }
        let (tx, rx) = mpsc::channel::<String>(64);
        let (done_tx, done_rx) = oneshot::channel();
        let stop = Arc::new(Notify::new());
        let waiting = Arc::clone(&stop);
        let counter = Arc::clone(&self.killed);
        tokio::spawn(async move {
            for line in script.lines {
                if tx.send(line).await.is_err() {
                    break;
                }
            }
            let killed = if script.hold {
                waiting.notified().await;
                true
            } else {
                false
            };
            drop(tx);
            let _ = done_tx.send(match (&script.exit, killed) {
                (Some(stderr), false) => Err(stderr.clone()),
                _ => Ok(()),
            });
        });
        let kill: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            stop.notify_one();
        });
        Ok(Running {
            lines: Box::pin(futures_util::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|line| (line, rx)) })),
            finish: Box::pin(async move { done_rx.await.unwrap_or(Ok(())) }),
            kill,
        })
    }
}

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
    emitters: Vec<mpsc::UnboundedSender<Emit>>,
    refuse: Option<String>,
}

type React = Arc<dyn Fn(usize, &serde_json::Value) -> Vec<String> + Send + Sync>;

/// A CLI's live server as a scripted process: what it answers to each line it is sent is decided by a function of the start it is and the
/// message. A test pushes lines of its own (`emit`) and ends the process as a crash does (`die`).
pub struct ScriptedLinker {
    state: Arc<Mutex<Starts>>,
    react: React,
}

impl ScriptedLinker {
    /// A server that answers each message with the lines `react(start, message)` returns, `start` counting the processes from 1.
    pub fn reacting(react: impl Fn(usize, &serde_json::Value) -> Vec<String> + Send + Sync + 'static) -> Arc<ScriptedLinker> {
        Arc::new(ScriptedLinker { state: Arc::default(), react: Arc::new(react) })
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
}

#[async_trait]
impl Linker for ScriptedLinker {
    async fn open(&self, _: Driver, cwd: &Path) -> Result<Link, String> {
        let (start, tx, rx) = {
            let mut state = self.state();
            if let Some(why) = state.refuse.take() {
                return Err(why);
            }
            state.count += 1;
            state.opened.push(cwd.to_path_buf());
            let (tx, rx) = mpsc::unbounded_channel::<Emit>();
            state.emitters.push(tx.clone());
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
        let (state, react, answer) = (Arc::clone(&self.state), Arc::clone(&self.react), tx.clone());
        let send: Arc<dyn Fn(String) -> bool + Send + Sync> = Arc::new(move |line| {
            let Ok(message) = serde_json::from_str::<serde_json::Value>(&line) else { return false };
            state.lock().unwrap_or_else(|p| p.into_inner()).received.push((start, message.clone()));
            for out in react(start, &message) {
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
