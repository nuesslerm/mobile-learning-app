//! Codex's live sessions: one `codex app-server` process per workspace that every thread of the workspace runs in, spoken to over its stdio
//! (`domain::codex`). The process is started when a turn first needs it, closed when no turn has run in it for the idle time, and replaced when it
//! dies; a thread whose process is gone is resumed by its conversation id (`thread/resume`) on its next turn. A steer is `turn/steer` and an
//! interrupt `turn/interrupt`, both on the live process; an approval the CLI asks for is handed out as an event and answered on the same process.
use super::ports::{Linker, SessionDriver};
use crate::domain::codex::{self, AskKind, Inbound, Step, Translator};
use crate::domain::{Driver, ExecEvent, TurnRequest, Verdict};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

/// How long a request of ours may wait for its answer (a thread's start waits for the CLI's tool servers), how long the process may take to end
/// once its output is closed, and how long an interrupted turn may take to end before the process is taken down.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const END_GRACE: Duration = Duration::from_secs(5);
const DEFAULT_IDLE: Duration = Duration::from_secs(10 * 60);
const DEFAULT_INTERRUPT_GRACE: Duration = Duration::from_secs(15);

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

struct Thread {
    rca: String,
    tx: Option<mpsc::Sender<ExecEvent>>,
    turn: Option<String>,
    translator: Translator,
    /// An interrupt was asked for before the server named the turn.
    stop_asked: bool,
}

struct Waiting {
    thread: String,
    rpc: Value,
    kind: AskKind,
}

#[derive(Default)]
struct Inner {
    pending: HashMap<u64, oneshot::Sender<Result<Value, String>>>,
    threads: HashMap<String, Thread>,
    asks: HashMap<String, Waiting>,
    last_active: Option<Instant>,
    ended: Option<String>,
}

struct Server {
    workspace: String,
    send: Arc<dyn Fn(String) -> bool + Send + Sync>,
    kill: Arc<dyn Fn() + Send + Sync>,
    next: AtomicU64,
    inner: Mutex<Inner>,
    shared: Arc<Shared>,
}

struct Live {
    server: Arc<Server>,
    codex: String,
}

#[derive(Default)]
struct Slot(tokio::sync::Mutex<Option<Arc<Server>>>);

struct Shared {
    slots: Mutex<HashMap<String, Arc<Slot>>>,
    live: Mutex<HashMap<String, Live>>,
    idle_ms: AtomicU64,
    interrupt_grace_ms: AtomicU64,
}

pub struct CodexSessions {
    shared: Arc<Shared>,
}

impl Default for CodexSessions {
    fn default() -> Self {
        CodexSessions { shared: Arc::new(Shared { slots: Mutex::default(), live: Mutex::default(), idle_ms: AtomicU64::new(DEFAULT_IDLE.as_millis() as u64), interrupt_grace_ms: AtomicU64::new(DEFAULT_INTERRUPT_GRACE.as_millis() as u64) }) }
    }
}

impl CodexSessions {
    pub fn new(idle: Duration) -> CodexSessions {
        let sessions = CodexSessions::default();
        sessions.set_idle(idle);
        sessions
    }

    /// How long a server may stay without a turn before it is closed; zero keeps it for ever.
    pub fn set_idle(&self, idle: Duration) {
        self.shared.idle_ms.store(idle.as_millis() as u64, Ordering::SeqCst);
    }

    /// How long a turn that was asked to stop may take to end before the server is taken down.
    pub fn set_interrupt_grace(&self, grace: Duration) {
        self.shared.interrupt_grace_ms.store(grace.as_millis() as u64, Ordering::SeqCst);
    }

    fn live(&self, thread: &str) -> Option<(Arc<Server>, String)> {
        locked(&self.shared.live).get(thread).map(|l| (Arc::clone(&l.server), l.codex.clone()))
    }

    fn slot(&self, workspace: &str) -> Arc<Slot> {
        Arc::clone(locked(&self.shared.slots).entry(workspace.to_string()).or_default())
    }

    async fn server(&self, workspace: &str, linker: &dyn Linker, cwd: &Path) -> Result<Arc<Server>, String> {
        let slot = self.slot(workspace);
        let mut held = slot.0.lock().await;
        if let Some(server) = held.as_ref().filter(|s| s.claim()) {
            return Ok(Arc::clone(server));
        }
        let link = linker.open(Driver::Codex, cwd, &[]).await.map_err(|e| format!("codex could not start: {e}"))?;
        let server = Arc::new(Server { workspace: workspace.to_string(), send: link.send, kill: link.running.kill, next: AtomicU64::new(0), inner: Mutex::new(Inner { last_active: Some(Instant::now()), ..Inner::default() }), shared: Arc::clone(&self.shared) });
        tokio::spawn(Arc::clone(&server).read(link.running.lines, link.running.finish));
        if let Err(why) = server.request(codex::initialize).await {
            (server.kill)();
            return Err(format!("codex app-server did not start: {why}"));
        }
        (server.send)(codex::initialized());
        tokio::spawn(Arc::clone(&server).watch_idle());
        *held = Some(Arc::clone(&server));
        Ok(server)
    }

    /// The thread of the session the request's thread runs in, and whether it is a new conversation (its id is to be announced).
    async fn thread_of(&self, server: &Arc<Server>, req: &TurnRequest) -> Result<(String, bool), String> {
        if let Some((on, codex)) = self.live(&req.thread_id) {
            if Arc::ptr_eq(&on, server) {
                return Ok((codex, false));
            }
        }
        let (codex, fresh) = match &req.resume {
            Some(id) => {
                let resumed = match server.request(|n| codex::thread_resume(n, id)).await {
                    Err(why) if why.contains("archived") || why.contains("unarchive") => {
                        server.request(|n| codex::thread_unarchive(n, id)).await?;
                        server.request(|n| codex::thread_resume(n, id)).await
                    }
                    other => other,
                };
                resumed.map_err(|why| format!("codex could not resume thread {id}: {why}"))?;
                (id.clone(), false)
            }
            None => {
                let started = server.request(|n| codex::thread_start(n, req.model.as_deref())).await.map_err(|why| format!("codex could not start a thread: {why}"))?;
                let id = started["thread"]["id"].as_str().filter(|id| !id.is_empty()).ok_or("codex started a thread and named none")?.to_string();
                (id, true)
            }
        };
        locked(&server.inner).threads.entry(codex.clone()).or_insert_with(|| Thread { rca: req.thread_id.clone(), tx: None, turn: None, translator: Translator::default(), stop_asked: false });
        locked(&self.shared.live).insert(req.thread_id.clone(), Live { server: Arc::clone(server), codex: codex.clone() });
        Ok((codex, fresh))
    }
}

impl Server {
    fn inner(&self) -> MutexGuard<'_, Inner> {
        locked(&self.inner)
    }

    /// Whether the server is still to be used; it is counted as active from now, so the idle close cannot take it between this and the turn's start.
    fn claim(&self) -> bool {
        let mut inner = self.inner();
        let alive = inner.ended.is_none();
        if alive {
            inner.last_active = Some(Instant::now());
        }
        alive
    }

    /// Ends the process for `why`; from here no turn is given to this server, though what it was running is settled only when its output closes.
    fn shut(&self, why: &str) {
        self.inner().ended.get_or_insert_with(|| why.to_string());
        (self.kill)();
    }

    /// Sends the request `line` builds for a fresh id and waits for the answer: its `result`, or why there is none.
    async fn request(&self, line: impl FnOnce(u64) -> String) -> Result<Value, String> {
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, rx) = oneshot::channel();
        {
            let mut inner = self.inner();
            if let Some(why) = &inner.ended {
                return Err(why.clone());
            }
            inner.pending.insert(id, tx);
        }
        if !(self.send)(line(id)) {
            self.inner().pending.remove(&id);
            return Err("the Codex app-server is gone".into());
        }
        match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(answer)) => answer,
            Ok(Err(_)) => Err(self.inner().ended.clone().unwrap_or_else(|| "the Codex app-server is gone".into())),
            Err(_) => {
                self.inner().pending.remove(&id);
                Err("the Codex app-server did not answer in time".into())
            }
        }
    }

    /// Reads what the process prints until its output closes, then settles every request and turn that was waiting on it.
    async fn read(self: Arc<Self>, mut lines: std::pin::Pin<Box<dyn futures_util::Stream<Item = String> + Send>>, finish: std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>) {
        while let Some(line) = lines.next().await {
            self.on_line(&line).await;
        }
        let ended = match tokio::time::timeout(END_GRACE, finish).await {
            Ok(done) => done,
            Err(_) => {
                (self.kill)();
                Err("it did not end after its output closed".into())
            }
        };
        let why = match ended {
            Ok(()) => "the Codex app-server ended".to_string(),
            Err(why) => format!("the Codex app-server ended: {why}"),
        };
        self.end(why).await;
    }

    async fn end(&self, why: String) {
        let streams: Vec<mpsc::Sender<ExecEvent>> = {
            let mut inner = self.inner();
            inner.ended = Some(why.clone());
            for (_, waiting) in inner.pending.drain() {
                let _ = waiting.send(Err(why.clone()));
            }
            inner.asks.clear();
            inner.threads.drain().filter_map(|(_, t)| t.tx).collect()
        };
        locked(&self.shared.live).retain(|_, l| !std::ptr::eq(Arc::as_ptr(&l.server), self));
        if let Some(slot) = locked(&self.shared.slots).get(&self.workspace).cloned() {
            if let Ok(mut held) = slot.0.try_lock() {
                if held.as_ref().is_some_and(|s| std::ptr::eq(Arc::as_ptr(s), self)) {
                    *held = None;
                }
            }
        }
        for tx in streams {
            let _ = tx.send(ExecEvent::Error(why.clone())).await;
        }
    }

    async fn watch_idle(self: Arc<Self>) {
        loop {
            let idle = Duration::from_millis(self.shared.idle_ms.load(Ordering::SeqCst));
            let wait = {
                let mut inner = self.inner();
                if inner.ended.is_some() {
                    return;
                }
                let busy = inner.threads.values().any(|t| t.tx.is_some()) || !inner.asks.is_empty();
                let wait = if idle.is_zero() {
                    Duration::from_secs(1)
                } else if busy {
                    idle.clamp(Duration::from_millis(50), Duration::from_secs(1))
                } else {
                    idle.saturating_sub(inner.last_active.map_or(idle, |at| at.elapsed()))
                };
                if wait.is_zero() {
                    inner.ended = Some("the Codex app-server was closed after it sat idle".into());
                }
                wait
            };
            if wait.is_zero() {
                (self.kill)();
                return;
            }
            tokio::time::sleep(wait.max(Duration::from_millis(5))).await;
        }
    }

    async fn on_line(&self, text: &str) {
        match codex::parse(text) {
            Inbound::Response { id, outcome } => {
                if let Some(waiting) = self.inner().pending.remove(&id) {
                    let _ = waiting.send(outcome);
                }
            }
            Inbound::Notification { method, params } => self.on_notification(&method, &params).await,
            Inbound::Request { id, method, params } => self.on_request(id, &method, &params).await,
            Inbound::Other => {}
        }
    }

    async fn on_notification(&self, method: &str, params: &Value) {
        if method == "serverRequest/resolved" {
            let resolved = &params["requestId"];
            self.inner().asks.retain(|_, w| &w.rpc != resolved);
            return;
        }
        let Some(thread) = params["threadId"].as_str() else { return };
        let (tx, events, stop) = {
            let mut inner = self.inner();
            let Some(t) = inner.threads.get_mut(thread) else { return };
            // What a turn that is over says late belongs to no turn
            let turn = params["turnId"].as_str().or_else(|| params["turn"]["id"].as_str());
            if let (Some(current), Some(named)) = (&t.turn, turn) {
                if current != named {
                    return;
                }
            }
            let Some(tx) = t.tx.clone() else { return };
            let mut events = Vec::new();
            let mut stop = None;
            for step in t.translator.notification(method, params) {
                match step {
                    Step::Event(e) => events.push(e),
                    Step::Started(id) => {
                        t.turn.get_or_insert(id.clone());
                        if t.stop_asked {
                            stop = Some(id);
                        }
                    }
                    Step::Ended(_) => {
                        events.push(ExecEvent::Done);
                        t.tx = None;
                        t.turn = None;
                        t.stop_asked = false;
                    }
                }
            }
            if t.tx.is_none() {
                inner.asks.retain(|_, w| w.thread != thread);
                inner.last_active = Some(Instant::now());
            }
            (tx, events, stop)
        };
        if let Some(turn) = stop {
            self.send_interrupt(thread, &turn);
        }
        for event in events {
            if tx.send(event).await.is_err() {
                // nobody follows the turn any more
                let turn = self.inner().threads.get(thread).and_then(|t| t.turn.clone());
                if let Some(turn) = turn {
                    self.send_interrupt(thread, &turn);
                }
                break;
            }
        }
    }

    async fn on_request(&self, id: Value, method: &str, params: &Value) {
        let Some(ask) = codex::ask_of(method, params) else {
            (self.send)(codex::refuse(&id, method));
            return;
        };
        let (tx, key) = {
            let mut inner = self.inner();
            match inner.threads.get(&ask.thread).and_then(|t| t.tx.clone()) {
                None => (None, ask.key.clone()),
                Some(tx) => {
                    let (mut key, mut n) = (ask.key.clone(), 1);
                    while inner.asks.contains_key(&key) {
                        n += 1;
                        key = format!("{}.{n}", ask.key);
                    }
                    inner.asks.insert(key.clone(), Waiting { thread: ask.thread.clone(), rpc: id.clone(), kind: ask.kind.clone() });
                    (Some(tx), key)
                }
            }
        };
        let Some(tx) = tx else {
            (self.send)(codex::answer(&id, &ask.kind, Verdict::Decline));
            return;
        };
        if tx.send(ExecEvent::Approval { id: key, tool: ask.tool, args: ask.args }).await.is_err() {
            (self.send)(codex::answer(&id, &ask.kind, Verdict::Decline));
        }
    }

    fn send_interrupt(&self, thread: &str, turn: &str) {
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        (self.send)(codex::turn_interrupt(id, thread, turn));
    }
}

#[async_trait]
impl SessionDriver for CodexSessions {
    async fn start(&self, workspace: &str, linker: &dyn Linker, cwd: &Path) -> Result<(), String> {
        self.server(workspace, linker, cwd).await.map(drop)
    }

    async fn turn(&self, workspace: &str, linker: &dyn Linker, req: &TurnRequest) -> Result<mpsc::Receiver<ExecEvent>, String> {
        let server = self.server(workspace, linker, Path::new(&req.cwd)).await?;
        let (codex, fresh) = self.thread_of(&server, req).await?;
        let (tx, rx) = mpsc::channel(256);
        {
            let mut inner = server.inner();
            let thread = inner.threads.get_mut(&codex).ok_or("the Codex app-server ended")?;
            if thread.tx.is_some() {
                return Err(format!("thread {} already has a running turn", req.thread_id));
            }
            thread.tx = Some(tx.clone());
            thread.turn = None;
            thread.stop_asked = false;
            thread.translator.reset();
            inner.last_active = Some(Instant::now());
        }
        if fresh {
            let _ = tx.send(ExecEvent::Session(codex.clone())).await;
        }
        let started = server.request(|n| codex::turn_start(n, &codex, req)).await;
        // A server that ended in the meantime has told the stream so (`end`), and the turn it did start is followed to that end
        let stop = {
            let mut inner = server.inner();
            match (inner.threads.get_mut(&codex), &started) {
                (Some(thread), Ok(result)) => {
                    if let Some(id) = result["turn"]["id"].as_str() {
                        thread.turn.get_or_insert_with(|| id.to_string());
                    }
                    thread.stop_asked.then(|| thread.turn.clone()).flatten()
                }
                (Some(thread), Err(_)) => {
                    thread.tx = None;
                    None
                }
                (None, _) => None,
            }
        };
        started.map_err(|why| format!("codex could not start the turn: {why}"))?;
        if let Some(turn) = stop {
            server.send_interrupt(&codex, &turn);
        }
        Ok(rx)
    }

    async fn steer(&self, thread: &str, prompt: &str) -> Result<(), String> {
        let (server, codex) = self.live(thread).ok_or_else(|| format!("thread {thread} has no live session"))?;
        let turn = server.inner().threads.get(&codex).and_then(|t| t.tx.as_ref().and(t.turn.clone())).ok_or_else(|| format!("thread {thread} has no running turn to steer"))?;
        server.request(|n| codex::turn_steer(n, &codex, &turn, prompt)).await.map(drop).map_err(|why| format!("codex did not take the steer: {why}"))
    }

    fn interrupt(&self, thread: &str) -> bool {
        let Some((server, codex)) = self.live(thread) else { return false };
        let turn = {
            let mut inner = server.inner();
            let Some(t) = inner.threads.get_mut(&codex).filter(|t| t.tx.is_some()) else { return false };
            if t.turn.is_none() {
                t.stop_asked = true;
            }
            t.turn.clone()
        };
        let Some(turn) = turn else { return true };
        server.send_interrupt(&codex, &turn);
        // A server that does not end the turn is taken down, so the thread is never left running for ever
        let grace = Duration::from_millis(self.shared.interrupt_grace_ms.load(Ordering::SeqCst));
        tokio::spawn(async move {
            tokio::time::sleep(grace).await;
            let still = server.inner().threads.get(&codex).is_some_and(|t| t.tx.is_some() && t.turn.as_deref() == Some(turn.as_str()));
            if still {
                server.shut("the Codex app-server did not end the interrupted turn");
            }
        });
        true
    }

    fn answer(&self, thread: &str, approval: &str, verdict: Verdict) -> bool {
        let Some((server, _)) = self.live(thread) else { return false };
        let waiting = {
            let mut inner = server.inner();
            match inner.asks.get(approval) {
                Some(w) if inner.threads.get(&w.thread).is_some_and(|t| t.rca == thread) => inner.asks.remove(approval),
                _ => None,
            }
        };
        let Some(waiting) = waiting else { return false };
        (server.send)(codex::answer(&waiting.rpc, &waiting.kind, verdict))
    }

    fn answer_input(&self, _: &str, _: &str, _: Option<String>) -> bool {
        false
    }

    fn alive(&self, thread: &str) -> bool {
        self.live(thread).is_some()
    }

    fn forget(&self, thread: &str) {
        if let Some(live) = locked(&self.shared.live).remove(thread) {
            let mut inner = live.server.inner();
            if inner.threads.get(&live.codex).is_some_and(|t| t.tx.is_none()) {
                inner.threads.remove(&live.codex);
            }
        }
    }

    fn close(&self, workspace: &str) -> bool {
        let Some(slot) = locked(&self.shared.slots).get(workspace).cloned() else { return false };
        let Ok(held) = slot.0.try_lock() else { return false };
        match held.as_ref() {
            Some(server) => {
                server.shut("the Codex app-server was closed");
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::ScriptedLinker;
    use serde_json::json;

    use crate::testing::{app_server, reply, say, started};

    fn request(thread: &str, prompt: &str, resume: Option<&str>) -> TurnRequest {
        TurnRequest { thread_id: thread.into(), run_id: "r".into(), instance_id: "codex".into(), model: None, cwd: "work".into(), prompt: prompt.into(), runtime_mode: "full-access".into(), interaction_mode: "default".into(), resume: resume.map(str::to_string), tools: None }
    }

    async fn drain(mut events: mpsc::Receiver<ExecEvent>) -> Vec<ExecEvent> {
        let mut all = Vec::new();
        while let Some(event) = events.recv().await {
            let last = matches!(event, ExecEvent::Done);
            all.push(event);
            if last {
                break;
            }
        }
        all
    }

    async fn until(what: &str, mut ok: impl FnMut() -> bool) {
        for _ in 0..400 {
            if ok() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("timed out waiting for {what}");
    }

    fn methods(linker: &ScriptedLinker) -> Vec<(usize, String)> {
        linker.received().into_iter().filter_map(|(start, m)| m["method"].as_str().map(|method| (start, method.to_string()))).collect()
    }

    #[tokio::test]
    async fn given_a_workspace_without_a_server_when_a_turn_runs_then_the_server_starts_once_in_its_directory_the_thread_is_announced_and_the_text_streams_to_its_end() {
        // Given a scripted app-server and no session yet
        let (sessions, linker) = (CodexSessions::default(), app_server());
        // When a thread runs its first turn
        let events = sessions.turn("w1", linker.as_ref(), &request("t1", "hello", None)).await.unwrap();
        // Then the thread's conversation is announced first, the text streams, and the turn ends with done
        assert_eq!(drain(events).await, vec![ExecEvent::Session("th-1".into()), ExecEvent::Text("Hi".into()), ExecEvent::Done]);
        // And the server was started once, in the request's directory, with the handshake before the thread and the turn, and the turn named its thread
        assert_eq!((linker.starts(), linker.opened()), (1, vec![std::path::PathBuf::from("work")]));
        assert_eq!(methods(&linker).into_iter().map(|(_, m)| m).collect::<Vec<_>>(), ["initialize", "initialized", "thread/start", "turn/start"]);
        assert_eq!(linker.received_of("turn/start")[0].1["params"]["threadId"], "th-1");
    }

    #[tokio::test]
    async fn given_a_running_server_when_the_same_thread_and_another_thread_of_the_workspace_run_turns_then_no_process_starts_and_a_known_thread_is_not_started_again() {
        // Given a session that has run one turn of t1
        let (sessions, linker) = (CodexSessions::default(), app_server());
        drain(sessions.turn("w1", linker.as_ref(), &request("t1", "hello", None)).await.unwrap()).await;
        // When t1 runs another turn (its stored conversation is given) and t2 runs its first turn
        let again = drain(sessions.turn("w1", linker.as_ref(), &request("t1", "hello", Some("th-1"))).await.unwrap()).await;
        let other = drain(sessions.turn("w1", linker.as_ref(), &request("t2", "hello", None)).await.unwrap()).await;
        // Then both ran in the one process, t1 went on in its loaded thread with no thread request and no announcement, and t2 got a thread of its own
        assert_eq!(linker.starts(), 1);
        assert_eq!(again, vec![ExecEvent::Text("Hi".into()), ExecEvent::Done]);
        assert_eq!(other[0], ExecEvent::Session("th-2".into()));
        assert_eq!(methods(&linker).iter().filter(|(_, m)| m == "thread/start").count(), 2);
        assert!(linker.received_of("thread/resume").is_empty());
        // And a second workspace has a process of its own
        drain(sessions.turn("w2", linker.as_ref(), &request("t3", "hello", None)).await.unwrap()).await;
        assert_eq!(linker.starts(), 2);
    }

    #[tokio::test]
    async fn given_a_turn_in_progress_when_it_is_steered_then_the_prompt_goes_to_the_live_process_and_the_same_turn_goes_on_with_nothing_restarted() {
        // Given a turn that is working
        let (sessions, linker) = (CodexSessions::default(), app_server());
        let mut events = sessions.turn("w1", linker.as_ref(), &request("t1", "hold", None)).await.unwrap();
        assert_eq!((events.recv().await, events.recv().await), (Some(ExecEvent::Session("th-1".into())), Some(ExecEvent::Text("working".into()))));
        // When it is steered
        sessions.steer("t1", "go left").await.unwrap();
        // Then the same turn takes the new prompt on the same process, goes on, and ends once
        assert_eq!(drain(events).await, vec![ExecEvent::Text(" steered".into()), ExecEvent::Done]);
        let steer = &linker.received_of("turn/steer")[0];
        assert_eq!((steer.0, steer.1["params"]["expectedTurnId"].clone(), steer.1["params"]["input"][0]["text"].clone()), (1, json!("u-1"), json!("go left")));
        assert_eq!((linker.starts(), linker.kills()), (1, 0));
        // And a thread with no running turn cannot be steered
        assert!(sessions.steer("t1", "late").await.unwrap_err().contains("no running turn"));
        assert!(sessions.steer("nobody", "x").await.unwrap_err().contains("no live session"));
    }

    #[tokio::test]
    async fn given_a_turn_that_asks_to_run_a_command_when_the_person_answers_then_the_ask_is_an_event_the_answer_goes_to_the_live_process_and_the_turn_goes_on() {
        // Given a turn that has asked for the person's approval of a command
        let (sessions, linker) = (CodexSessions::default(), app_server());
        let mut events = sessions.turn("w1", linker.as_ref(), &request("t1", "approve", None)).await.unwrap();
        let mut seen = Vec::new();
        let approval = loop {
            match events.recv().await.unwrap() {
                ExecEvent::Approval { id, tool, args } => break (id, tool, args),
                other => seen.push(other),
            }
        };
        // Then the ask names the command and its reason, and the tool call it is about was shown before it
        assert_eq!((approval.0.as_str(), approval.1.as_str(), approval.2["command"].clone(), approval.2["reason"].clone()), ("c1", "command", json!("touch a"), json!("May I?")));
        assert!(seen.iter().any(|e| matches!(e, ExecEvent::ToolStarted { id, .. } if id == "c1")));
        // When the person accepts, and answers again after it was taken
        assert!(sessions.answer("t1", "c1", Verdict::Accept));
        assert!(!sessions.answer("t1", "c1", Verdict::Accept));
        // Then the answer was written to the live process under the id of its request, and the turn went on to its end
        let rest = drain(events).await;
        assert!(rest.contains(&ExecEvent::ToolFinished { id: "c1".into(), output: String::new(), ok: true }) && rest.last() == Some(&ExecEvent::Done), "{rest:?}");
        let answered: Vec<Value> = linker.received().into_iter().map(|(_, m)| m).filter(|m| m.get("method").is_none() && m["id"] == 0).collect();
        assert_eq!(answered, vec![json!({"id": 0, "result": {"decision": "accept"}})]);
        assert_eq!(linker.starts(), 1);
    }

    #[tokio::test]
    async fn given_a_running_turn_when_it_is_interrupted_then_the_server_is_asked_to_interrupt_that_turn_and_the_turn_ends_without_an_error_or_a_restart() {
        // Given a turn that is working
        let (sessions, linker) = (CodexSessions::default(), app_server());
        let mut events = sessions.turn("w1", linker.as_ref(), &request("t1", "hold", None)).await.unwrap();
        events.recv().await;
        events.recv().await;
        // When it is interrupted, and a thread with no turn is
        assert!(sessions.interrupt("t1"));
        assert!(!sessions.interrupt("nobody"));
        // Then the stream ends with done, the interrupt named the thread and the turn, and the process lives on
        assert_eq!(drain(events).await, vec![ExecEvent::Done]);
        assert_eq!(linker.received_of("turn/interrupt")[0].1["params"], json!({"threadId": "th-1", "turnId": "u-1"}));
        assert_eq!((linker.starts(), linker.kills()), (1, 0));
        // And the thread can run again on it
        assert_eq!(drain(sessions.turn("w1", linker.as_ref(), &request("t1", "hello", Some("th-1"))).await.unwrap()).await.last(), Some(&ExecEvent::Done));
    }

    #[tokio::test]
    async fn given_a_server_that_does_not_end_an_interrupted_turn_when_the_grace_runs_out_then_it_is_taken_down_and_the_turn_ends_in_an_error() {
        // Given a server that ignores interrupts, and a short grace
        let linker = ScriptedLinker::reacting(|_, m| match m["method"].as_str() {
            Some("initialize") => vec![reply(&m["id"], json!({}))],
            Some("thread/start") => vec![reply(&m["id"], json!({"thread": {"id": "th-1"}}))],
            Some("turn/start") => vec![reply(&m["id"], json!({"turn": {"id": "u-1"}})), started("th-1", "u-1")],
            _ => vec![],
        });
        let sessions = CodexSessions::default();
        sessions.set_interrupt_grace(Duration::from_millis(100));
        let events = sessions.turn("w1", linker.as_ref(), &request("t1", "hold", None)).await.unwrap();
        // When the turn is interrupted
        assert!(sessions.interrupt("t1"));
        // Then the process is killed and the turn's stream ends with the reason
        let all: Vec<ExecEvent> = tokio_stream_collect(events).await;
        assert!(matches!(all.last(), Some(ExecEvent::Error(why)) if why.contains("ended")), "{all:?}");
        assert_eq!(linker.kills(), 1);
    }

    async fn tokio_stream_collect(mut events: mpsc::Receiver<ExecEvent>) -> Vec<ExecEvent> {
        let mut all = Vec::new();
        while let Some(event) = events.recv().await {
            all.push(event);
        }
        all
    }

    #[tokio::test]
    async fn given_a_server_that_stays_without_a_turn_when_the_idle_time_passes_then_it_is_closed_and_the_next_turn_starts_a_new_one_and_resumes_the_thread_by_its_id() {
        // Given a session that ran a turn, and an idle time of 150 ms
        let (sessions, linker) = (CodexSessions::new(Duration::from_millis(150)), app_server());
        let first = drain(sessions.turn("w1", linker.as_ref(), &request("t1", "hello", None)).await.unwrap()).await;
        let stored = match &first[0] {
            ExecEvent::Session(id) => id.clone(),
            other => panic!("{other:?}"),
        };
        // When nothing runs for longer than that
        until("the idle close", || linker.kills() == 1).await;
        // Then the process was ended, and the thread's next turn starts a second one, resumes the thread by the id the first announced, and starts no new thread
        let events = sessions.turn("w1", linker.as_ref(), &request("t1", "hello", Some(&stored))).await.unwrap();
        assert_eq!(drain(events).await, vec![ExecEvent::Text("Hi".into()), ExecEvent::Done]);
        assert_eq!(linker.starts(), 2);
        let resumes = linker.received_of("thread/resume");
        assert_eq!((resumes.len(), resumes[0].0, resumes[0].1["params"]["threadId"].clone()), (1, 2, json!("th-1")));
        assert_eq!(linker.received_of("thread/start").len(), 1);
    }

    #[tokio::test]
    async fn given_a_turn_in_progress_when_the_server_process_dies_then_the_turn_ends_in_an_error_naming_why_and_the_next_turn_restarts_the_server_and_resumes_the_thread() {
        // Given a turn that is working
        let (sessions, linker) = (CodexSessions::default(), app_server());
        let mut events = sessions.turn("w1", linker.as_ref(), &request("t1", "hold", None)).await.unwrap();
        events.recv().await;
        events.recv().await;
        // When the process is killed from outside
        linker.die("killed by a signal");
        // Then the stream says why and ends
        let rest = tokio_stream_collect(events).await;
        assert!(matches!(rest.as_slice(), [ExecEvent::Error(why)] if why.contains("killed by a signal")), "{rest:?}");
        // And the thread's next turn restarts the server and resumes the conversation by its id
        let again = sessions.turn("w1", linker.as_ref(), &request("t1", "hello", Some("th-1"))).await.unwrap();
        assert_eq!(drain(again).await, vec![ExecEvent::Text("Hi".into()), ExecEvent::Done]);
        assert_eq!(linker.starts(), 2);
        assert_eq!(linker.received_of("thread/resume")[0].1["params"]["threadId"], "th-1");
    }

    #[tokio::test]
    async fn given_a_server_that_cannot_start_or_a_thread_that_cannot_be_resumed_when_a_turn_runs_then_it_is_refused_with_the_reason_and_a_busy_thread_is_refused() {
        // Given a program that cannot be started
        let (sessions, linker) = (CodexSessions::default(), app_server());
        linker.refuse_next("not found");
        // When a turn runs, Then it is refused naming the program and why
        assert_eq!(sessions.turn("w1", linker.as_ref(), &request("t1", "hello", None)).await.err().as_deref(), Some("codex could not start: not found"));
        // And a conversation the server does not know is refused with the server's own words, and starts no other thread
        let broken = ScriptedLinker::reacting(|_, m| match m["method"].as_str() {
            Some("initialize") => vec![reply(&m["id"], json!({}))],
            Some("thread/resume") => vec![say(json!({"id": m["id"], "error": {"code": -32600, "message": "no rollout found for thread gone"}}))],
            _ => vec![],
        });
        let refused = sessions.turn("w2", broken.as_ref(), &request("t2", "hello", Some("gone"))).await.err().unwrap();
        assert!(refused.contains("could not resume thread gone") && refused.contains("no rollout found"), "{refused}");
        assert!(broken.received_of("thread/start").is_empty());
        // And a thread that has a turn running is refused a second one
        let mut first = sessions.turn("w3", linker.as_ref(), &request("t3", "hold", None)).await.unwrap();
        first.recv().await;
        let busy = sessions.turn("w3", linker.as_ref(), &request("t3", "hello", Some("th-1"))).await.err();
        assert_eq!(busy.as_deref(), Some("thread t3 already has a running turn"));
    }

    #[tokio::test]
    async fn given_a_thread_whose_conversation_is_let_go_when_it_runs_again_then_a_new_thread_starts_and_a_request_the_cli_makes_that_is_no_approval_is_refused() {
        // Given a thread that ran a turn and then had its conversation detached
        let (sessions, linker) = (CodexSessions::default(), app_server());
        drain(sessions.turn("w1", linker.as_ref(), &request("t1", "hello", None)).await.unwrap()).await;
        sessions.forget("t1");
        // When it runs again with no stored conversation
        let events = sessions.turn("w1", linker.as_ref(), &request("t1", "hello", None)).await.unwrap();
        // Then a new thread was started for it and announced
        assert_eq!(drain(events).await[0], ExecEvent::Session("th-2".into()));
        // And a request of the server's that is no approval is answered with a method that is not found
        linker.emit(say(json!({"id": 907, "method": "item/tool/call", "params": {"threadId": "th-2"}})));
        until("the refusal", || linker.received().iter().any(|(_, m)| m["id"] == 907)).await;
        assert_eq!(linker.received().iter().find(|(_, m)| m["id"] == 907).unwrap().1["error"]["code"], -32601);
    }

    #[tokio::test]
    async fn given_an_approval_for_a_thread_that_runs_no_turn_when_the_server_asks_then_it_is_declined_so_the_server_never_waits_for_nobody() {
        // Given a session whose thread has ended its turn
        let (sessions, linker) = (CodexSessions::default(), app_server());
        drain(sessions.turn("w1", linker.as_ref(), &request("t1", "hello", None)).await.unwrap()).await;
        // When the server asks for an approval of that thread
        linker.emit(say(json!({"id": 903, "method": "item/fileChange/requestApproval", "params": {"threadId": "th-1", "turnId": "u-1", "itemId": "f1"}})));
        // Then it is declined at once
        until("the decline", || linker.received().iter().any(|(_, m)| m["id"] == 903)).await;
        assert_eq!(linker.received().iter().find(|(_, m)| m["id"] == 903).unwrap().1["result"], json!({"decision": "decline"}));
    }

    #[tokio::test]
    async fn given_a_workspace_when_it_is_closed_then_its_server_is_killed_and_nothing_else_is() {
        // Given two workspaces with a server each
        let (sessions, linker) = (CodexSessions::default(), app_server());
        sessions.start("w1", linker.as_ref(), Path::new(".")).await.unwrap();
        sessions.start("w2", linker.as_ref(), Path::new(".")).await.unwrap();
        // When the first is closed, and one that has no server
        let (closed, none) = (sessions.close("w1"), sessions.close("w9"));
        // Then one process was killed
        until("the kill", || linker.kills() == 1).await;
        assert_eq!((closed, none, linker.starts()), (true, false, 2));
    }

    #[tokio::test]
    async fn given_a_server_that_was_just_closed_when_a_turn_asks_for_one_before_its_output_has_closed_then_a_new_process_runs_it_instead_of_the_dying_one() {
        // Given a workspace whose server has run a turn
        let (sessions, linker) = (CodexSessions::default(), app_server());
        drain(sessions.turn("w1", linker.as_ref(), &request("t1", "hello", None)).await.unwrap()).await;
        // When it is closed and the thread asks for a turn in the same breath, with its stored conversation
        assert!(sessions.close("w1"));
        let events = sessions.turn("w1", linker.as_ref(), &request("t1", "hello", Some("th-1"))).await.unwrap();
        // Then the turn ran to its end on a second process that resumed the conversation
        assert_eq!(drain(events).await, vec![ExecEvent::Text("Hi".into()), ExecEvent::Done]);
        assert_eq!((linker.starts(), linker.received_of("thread/resume").len()), (2, 1));
    }
}
