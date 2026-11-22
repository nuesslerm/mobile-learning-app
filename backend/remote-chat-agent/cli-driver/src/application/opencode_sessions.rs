//! opencode's live sessions: one `opencode serve` per workspace that every thread of the workspace runs a session in, reached over the executor's bridge
//! (`domain::opencode`): an HTTP request goes out as a line and its answer, the server's events and the end of its event stream come back as lines. The
//! server is started when a turn first needs it, closed when no turn has run in it for the idle time, and replaced when it dies; a thread whose server is gone
//! is resumed by its session id, which the server's store keeps. A steer is another prompt on the session, an interrupt an abort, and a permission or question the
//! session asks is handed out as an event and answered on the same server.
use super::ports::{Linker, SessionDriver};
use crate::domain::opencode::{self as oc, Ask, Inbound, Step, Translator};
use crate::domain::{Driver, ExecEvent, TurnRequest, Verdict};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

/// How long a request of ours may wait for its answer (the first call in a directory starts its services), how long the server may take to be ready, how
/// long the process may take to end once its output is closed, and how long an interrupted turn may take to end before the server is taken down.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const READY_TIMEOUT: Duration = Duration::from_secs(90);
const END_GRACE: Duration = Duration::from_secs(5);
const DEFAULT_IDLE: Duration = Duration::from_secs(10 * 60);
const DEFAULT_INTERRUPT_GRACE: Duration = Duration::from_secs(15);
/// A request on a child session names its root through `parentID`; a chain longer than this is given up.
const PARENT_DEPTH: usize = 8;

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

struct Thread {
    rca: String,
    cwd: String,
    tx: Option<mpsc::Sender<ExecEvent>>,
    translator: Translator,
    model: Option<String>,
    plan: bool,
    /// The runtime mode whose rules the session holds; empty for a session resumed, whose rules are not known.
    mode: String,
    /// The turn running now (counted, so a late timer cannot take down the server for the one after).
    turn: u64,
    /// The turn's prompt is on its way to the server: an idle seen before this is the session's old one.
    armed: bool,
    prompts: usize,
    admitted: usize,
    idle_seen: bool,
    stop_asked: bool,
}

struct Waiting {
    thread: String,
    ask: Ask,
}

#[derive(Default)]
struct Inner {
    pending: HashMap<u64, oneshot::Sender<(u16, String)>>,
    threads: HashMap<String, Thread>,
    asks: HashMap<String, Waiting>,
    last_active: Option<Instant>,
    ended: Option<String>,
    ready: Option<oneshot::Sender<()>>,
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
    session: String,
}

#[derive(Default)]
struct Slot(tokio::sync::Mutex<Option<Arc<Server>>>);

struct Shared {
    slots: Mutex<HashMap<String, Arc<Slot>>>,
    live: Mutex<HashMap<String, Live>>,
    idle_ms: AtomicU64,
    interrupt_grace_ms: AtomicU64,
}

pub struct OpencodeSessions {
    shared: Arc<Shared>,
}

impl Default for OpencodeSessions {
    fn default() -> Self {
        OpencodeSessions { shared: Arc::new(Shared { slots: Mutex::default(), live: Mutex::default(), idle_ms: AtomicU64::new(DEFAULT_IDLE.as_millis() as u64), interrupt_grace_ms: AtomicU64::new(DEFAULT_INTERRUPT_GRACE.as_millis() as u64) }) }
    }
}

impl OpencodeSessions {
    /// How long a server may stay without a turn before it is closed; zero keeps it for ever.
    pub fn set_idle(&self, idle: Duration) {
        self.shared.idle_ms.store(idle.as_millis() as u64, Ordering::SeqCst);
    }

    /// How long a turn that was asked to stop may take to end before the server is taken down.
    pub fn set_interrupt_grace(&self, grace: Duration) {
        self.shared.interrupt_grace_ms.store(grace.as_millis() as u64, Ordering::SeqCst);
    }

    fn live(&self, thread: &str) -> Option<(Arc<Server>, String)> {
        locked(&self.shared.live).get(thread).map(|l| (Arc::clone(&l.server), l.session.clone()))
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
        let link = linker.open(Driver::Opencode, cwd, &[]).await.map_err(|e| format!("opencode could not start: {e}"))?;
        let (ready_tx, ready_rx) = oneshot::channel();
        let server = Arc::new(Server { workspace: workspace.to_string(), send: link.send, kill: link.running.kill, next: AtomicU64::new(0), inner: Mutex::new(Inner { last_active: Some(Instant::now()), ready: Some(ready_tx), ..Inner::default() }), shared: Arc::clone(&self.shared) });
        tokio::spawn(Arc::clone(&server).read(link.running.lines, link.running.finish));
        match tokio::time::timeout(READY_TIMEOUT, ready_rx).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => {
                let why = server.inner().ended.clone().unwrap_or_else(|| "it ended before it was ready".into());
                return Err(format!("opencode server did not start: {why}"));
            }
            Err(_) => {
                (server.kill)();
                return Err("opencode server did not start in time".into());
            }
        }
        tokio::spawn(Arc::clone(&server).watch_idle());
        *held = Some(Arc::clone(&server));
        Ok(server)
    }

    /// The server's session the request's thread runs in, and whether it is a new one (its id is to be announced).
    async fn session_of(&self, server: &Arc<Server>, req: &TurnRequest) -> Result<(String, bool), String> {
        if let Some((on, session)) = self.live(&req.thread_id) {
            if Arc::ptr_eq(&on, server) {
                return Ok((session, false));
            }
        }
        let (session, fresh, mode) = match &req.resume {
            Some(id) => {
                server.call("GET", &oc::session_path(id, "", &req.cwd), None).await.map_err(|why| format!("opencode could not resume session {id}: {why}"))?;
                (id.clone(), false, String::new())
            }
            None => {
                let created = server.call("POST", &oc::create_path(&req.cwd), Some(&json!({"permission": oc::rules(&req.runtime_mode)}))).await.map_err(|why| format!("opencode could not start a session: {why}"))?;
                let id = created["id"].as_str().filter(|id| !id.is_empty()).ok_or("opencode started a session and named none")?.to_string();
                (id, true, req.runtime_mode.clone())
            }
        };
        let thread = Thread { rca: req.thread_id.clone(), cwd: req.cwd.clone(), tx: None, translator: Translator::default(), model: None, plan: false, mode, turn: 0, armed: false, prompts: 0, admitted: 0, idle_seen: false, stop_asked: false };
        locked(&server.inner).threads.entry(session.clone()).or_insert(thread);
        locked(&self.shared.live).insert(req.thread_id.clone(), Live { server: Arc::clone(server), session: session.clone() });
        Ok((session, fresh))
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

    /// Sends the request and waits for its answer: the status and the body, or why there is none.
    async fn request(&self, method: &str, path: &str, body: Option<&Value>) -> Result<(u16, String), String> {
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, rx) = oneshot::channel();
        {
            let mut inner = self.inner();
            if let Some(why) = &inner.ended {
                return Err(why.clone());
            }
            inner.pending.insert(id, tx);
        }
        if !(self.send)(oc::request(id, method, path, body)) {
            self.inner().pending.remove(&id);
            return Err("the opencode server is gone".into());
        }
        match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(answer)) => Ok(answer),
            Ok(Err(_)) => Err(self.inner().ended.clone().unwrap_or_else(|| "the opencode server is gone".into())),
            Err(_) => {
                self.inner().pending.remove(&id);
                Err("the opencode server did not answer in time".into())
            }
        }
    }

    /// A request that must succeed: its body as JSON (null when it has none), or why it did not.
    async fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, String> {
        let (status, text) = self.request(method, path, body).await?;
        if !(200..300).contains(&status) {
            return Err(oc::failure(status, &text));
        }
        Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    /// A request nobody waits for (an abort, an answer): its answer is dropped when it comes.
    fn post(&self, path: &str, body: Option<&Value>) -> bool {
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        (self.send)(oc::request(id, "POST", path, body))
    }

    /// Reads what the bridge prints until its output closes, then settles every request and turn that was waiting on it.
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
            Ok(()) => "the opencode server ended".to_string(),
            Err(why) => format!("the opencode server ended: {why}"),
        };
        self.end(why).await;
    }

    async fn end(&self, why: String) {
        let (streams, why): (Vec<mpsc::Sender<ExecEvent>>, String) = {
            let mut inner = self.inner();
            // the reason that took the server down (a stream that ended, an interrupt that was not obeyed) is the one the turns are told
            let why = inner.ended.get_or_insert(why).clone();
            inner.pending.clear();
            inner.ready = None;
            inner.asks.clear();
            (inner.threads.drain().filter_map(|(_, t)| t.tx).collect(), why)
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
                    inner.ended = Some("the opencode server was closed after it sat idle".into());
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

    async fn on_line(self: &Arc<Self>, text: &str) {
        match oc::parse(text) {
            Inbound::Ready => {
                if let Some(ready) = self.inner().ready.take() {
                    let _ = ready.send(());
                }
            }
            Inbound::Response { id, status, body } => {
                if let Some(waiting) = self.inner().pending.remove(&id) {
                    let _ = waiting.send((status, body));
                }
            }
            Inbound::Event { kind, properties } => self.on_event(&kind, properties).await,
            // an event stream that ended leaves the server out of step: it is taken down, and its threads are resumed by their sessions
            Inbound::Closed(why) => self.shut(&why),
            Inbound::Other => {}
        }
    }

    async fn on_event(self: &Arc<Self>, kind: &str, properties: Value) {
        let Some(session) = oc::session_of(&properties).map(str::to_string) else { return };
        let known = self.inner().threads.contains_key(&session);
        if !known {
            // A subagent's session asks in its own name: the person answers it in the thread of its root
            if matches!(kind, "permission.asked" | "question.asked") {
                let (server, kind) = (Arc::clone(self), kind.to_string());
                tokio::spawn(async move { server.deliver_from_child(session, kind, properties).await });
            }
            return;
        }
        self.deliver(&session, kind, &properties).await;
    }

    async fn deliver_from_child(&self, child: String, kind: String, properties: Value) {
        let mut at = child;
        for _ in 0..PARENT_DEPTH {
            let Ok(found) = self.call("GET", &oc::session_path(&at, "", ""), None).await else { return };
            let Some(parent) = found["parentID"].as_str() else { return };
            if self.inner().threads.contains_key(parent) {
                self.deliver(parent, &kind, &properties).await;
                return;
            }
            at = parent.to_string();
        }
    }

    /// Hands the event to the turn of `session`, and ends the turn when it says the session is idle.
    async fn deliver(&self, session: &str, kind: &str, properties: &Value) {
        let (tx, events) = {
            let mut inner = self.inner();
            let Inner { threads, asks, last_active, .. } = &mut *inner;
            let Some(t) = threads.get_mut(session) else { return };
            // What the session says outside a turn, and what its old one says before the new prompt is on its way, belongs to no turn
            let Some(tx) = t.tx.clone().filter(|_| t.armed || !matches!(kind, "session.idle" | "session.status")) else { return };
            let mut events = Vec::new();
            let mut over = false;
            for step in t.translator.event(kind, properties) {
                match step {
                    Step::Event(e) => events.push(e),
                    Step::Asked { id, ask, event } => {
                        asks.insert(id, Waiting { thread: session.to_string(), ask });
                        events.push(event);
                    }
                    Step::Settled(id) => {
                        asks.remove(&id);
                    }
                    Step::Admitted => {
                        t.admitted += 1;
                        over |= t.idle_seen && t.admitted >= t.prompts;
                    }
                    // A steer that the server has not taken into a message yet keeps the turn open past an idle that was before it
                    Step::Idle if t.prompts > 1 && t.admitted < t.prompts => t.idle_seen = true,
                    Step::Idle => over = true,
                }
            }
            if over {
                events.push(ExecEvent::Done);
                t.tx = None;
                t.armed = false;
                t.idle_seen = false;
                t.stop_asked = false;
                asks.retain(|_, w| w.thread != session);
                *last_active = Some(Instant::now());
            }
            (tx, events)
        };
        for event in events {
            if tx.send(event).await.is_err() {
                // nobody follows the turn any more
                self.abort(session);
                break;
            }
        }
    }

    fn abort(&self, session: &str) {
        let cwd = self.inner().threads.get(session).map(|t| t.cwd.clone()).unwrap_or_default();
        self.post(&oc::session_path(session, "/abort", &cwd), None);
    }
}

#[async_trait]
impl SessionDriver for OpencodeSessions {
    async fn start(&self, workspace: &str, linker: &dyn Linker, cwd: &Path) -> Result<(), String> {
        self.server(workspace, linker, cwd).await.map(drop)
    }

    async fn turn(&self, workspace: &str, linker: &dyn Linker, req: &TurnRequest) -> Result<mpsc::Receiver<ExecEvent>, String> {
        let server = self.server(workspace, linker, Path::new(&req.cwd)).await?;
        let (session, fresh) = self.session_of(&server, req).await?;
        let (tx, rx) = mpsc::channel(256);
        let patch = {
            let mut inner = server.inner();
            let thread = inner.threads.get_mut(&session).ok_or("the opencode server ended")?;
            if thread.tx.is_some() {
                return Err(format!("thread {} already has a running turn", req.thread_id));
            }
            thread.tx = Some(tx.clone());
            thread.turn += 1;
            (thread.armed, thread.prompts, thread.admitted, thread.idle_seen, thread.stop_asked) = (false, 1, 0, false, false);
            thread.translator.reset();
            thread.model = req.model.clone();
            thread.plan = req.interaction_mode == "plan";
            let patch = thread.mode != req.runtime_mode;
            inner.last_active = Some(Instant::now());
            patch
        };
        if fresh {
            let _ = tx.send(ExecEvent::Session(session.clone())).await;
        }
        let started = async {
            if patch {
                server.call("PATCH", &oc::session_path(&session, "", &req.cwd), Some(&json!({"permission": oc::rules(&req.runtime_mode)}))).await.map_err(|why| format!("opencode did not take the permission rules: {why}"))?;
                if let Some(t) = server.inner().threads.get_mut(&session) {
                    t.mode = req.runtime_mode.clone();
                }
            }
            if let Some(t) = server.inner().threads.get_mut(&session) {
                t.armed = true;
            }
            server.call("POST", &oc::session_path(&session, "/prompt_async", &req.cwd), Some(&oc::prompt_body(&req.prompt, req.model.as_deref(), req.interaction_mode == "plan"))).await.map(drop).map_err(|why| format!("opencode could not start the turn: {why}"))
        }
        .await;
        let stop = {
            let mut inner = server.inner();
            match (inner.threads.get_mut(&session), &started) {
                (Some(thread), Ok(())) => thread.stop_asked,
                (Some(thread), Err(_)) => {
                    thread.tx = None;
                    thread.armed = false;
                    false
                }
                (None, _) => false,
            }
        };
        started?;
        if stop {
            server.abort(&session);
        }
        Ok(rx)
    }

    async fn steer(&self, thread: &str, prompt: &str) -> Result<(), String> {
        let (server, session) = self.live(thread).ok_or_else(|| format!("thread {thread} has no live session"))?;
        let (path, body) = {
            let mut inner = server.inner();
            let t = inner.threads.get_mut(&session).filter(|t| t.tx.is_some() && t.armed).ok_or_else(|| format!("thread {thread} has no running turn to steer"))?;
            // counted first: an idle that comes before the server has stored the message must not end the turn
            t.prompts += 1;
            (oc::session_path(&session, "/prompt_async", &t.cwd), oc::prompt_body(prompt, t.model.as_deref(), t.plan))
        };
        let sent = server.call("POST", &path, Some(&body)).await;
        if sent.is_err() {
            if let Some(t) = server.inner().threads.get_mut(&session) {
                t.prompts = t.prompts.saturating_sub(1).max(1);
            }
        }
        sent.map(drop).map_err(|why| format!("opencode did not take the steer: {why}"))
    }

    fn interrupt(&self, thread: &str) -> bool {
        let Some((server, session)) = self.live(thread) else { return false };
        let turn = {
            let mut inner = server.inner();
            let Some(t) = inner.threads.get_mut(&session).filter(|t| t.tx.is_some()) else { return false };
            if !t.armed {
                // the prompt is not on its way yet: the turn that is starting aborts itself once it is
                t.stop_asked = true;
                return true;
            }
            t.turn
        };
        server.abort(&session);
        // A server that does not end the turn is taken down, so the thread is never left running for ever
        let grace = Duration::from_millis(self.shared.interrupt_grace_ms.load(Ordering::SeqCst));
        tokio::spawn(async move {
            tokio::time::sleep(grace).await;
            let still = server.inner().threads.get(&session).is_some_and(|t| t.tx.is_some() && t.turn == turn);
            if still {
                server.shut("the opencode server did not end the interrupted turn");
            }
        });
        true
    }

    fn answer(&self, thread: &str, approval: &str, verdict: Verdict) -> bool {
        let Some((server, _)) = self.live(thread) else { return false };
        let cwd = {
            let mut inner = server.inner();
            match inner.asks.get(approval) {
                Some(w) if w.ask == Ask::Permission && inner.threads.get(&w.thread).is_some_and(|t| t.rca == thread) => {
                    let cwd = inner.threads.get(&w.thread).map(|t| t.cwd.clone()).unwrap_or_default();
                    inner.asks.remove(approval);
                    cwd
                }
                _ => return false,
            }
        };
        server.post(&format!("/permission/{approval}/reply{}", oc::directory_query(&cwd)), Some(&oc::reply_body(verdict)))
    }

    fn answer_input(&self, thread: &str, request: &str, answer: Option<String>) -> bool {
        let Some((server, _)) = self.live(thread) else { return false };
        let (cwd, count) = {
            let mut inner = server.inner();
            match inner.asks.get(request) {
                Some(Waiting { thread: owner, ask: Ask::Question(count) }) if inner.threads.get(owner).is_some_and(|t| t.rca == thread) => {
                    let found = (inner.threads.get(owner).map(|t| t.cwd.clone()).unwrap_or_default(), *count);
                    inner.asks.remove(request);
                    found
                }
                _ => return false,
            }
        };
        let query = oc::directory_query(&cwd);
        match answer {
            Some(chosen) => {
                let mut answers = vec![json!([chosen])];
                answers.resize(count, json!([]));
                server.post(&format!("/question/{request}/reply{query}"), Some(&json!({"answers": answers})))
            }
            None => server.post(&format!("/question/{request}/reject{query}"), None),
        }
    }

    fn alive(&self, thread: &str) -> bool {
        self.live(thread).is_some()
    }

    fn forget(&self, thread: &str) {
        if let Some(live) = locked(&self.shared.live).remove(thread) {
            let mut inner = live.server.inner();
            if inner.threads.get(&live.session).is_some_and(|t| t.tx.is_none()) {
                inner.threads.remove(&live.session);
            }
        }
    }

    fn close(&self, workspace: &str) -> bool {
        let Some(slot) = locked(&self.shared.slots).get(workspace).cloned() else { return false };
        let Ok(held) = slot.0.try_lock() else { return false };
        match held.as_ref() {
            Some(server) => {
                server.shut("the opencode server was closed");
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{frame, opencode_server, ScriptedLinker};

    fn request(thread: &str, prompt: &str, resume: Option<&str>) -> TurnRequest {
        TurnRequest { thread_id: thread.into(), run_id: "r".into(), instance_id: "opencode".into(), model: Some("opencode-go/kimi".into()), cwd: "work".into(), prompt: prompt.into(), runtime_mode: "full-access".into(), interaction_mode: "default".into(), resume: resume.map(str::to_string), tools: None }
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

    fn calls(linker: &ScriptedLinker) -> Vec<(usize, String, String)> {
        linker.received().into_iter().map(|(start, m)| (start, m["method"].as_str().unwrap_or_default().to_string(), m["path"].as_str().unwrap_or_default().to_string())).collect()
    }

    fn count(linker: &ScriptedLinker, method: &str, path: &str) -> usize {
        calls(linker).iter().filter(|(_, m, p)| m == method && p.starts_with(path)).count()
    }

    fn creates(linker: &ScriptedLinker) -> usize {
        calls(linker).iter().filter(|(_, m, p)| m == "POST" && p == "/session").count()
    }

    #[tokio::test]
    async fn given_a_workspace_without_a_server_when_a_turn_runs_then_the_server_starts_once_the_session_is_announced_and_the_text_and_the_tool_stream_to_the_end() {
        // Given a scripted opencode server and no session yet
        let (sessions, linker) = (OpencodeSessions::default(), opencode_server());
        // When a thread runs its first turn
        let events = sessions.turn("w1", linker.as_ref(), &request("t1", "hello", None)).await.unwrap();
        // Then the session is announced first, the text streams once, the tool starts and finishes, and the turn ends with done
        assert_eq!(drain(events).await, vec![
            ExecEvent::Session("ses-1".into()),
            ExecEvent::Text("Hi".into()),
            ExecEvent::ToolStarted { id: "k1".into(), name: "bash".into(), input: json!({"command": "ls"}) },
            ExecEvent::ToolFinished { id: "k1".into(), output: "a.txt".into(), ok: true },
            ExecEvent::Done,
        ]);
        // And the server was started once in the request's directory, the session made with full access, and the prompt named its agent and model
        assert_eq!((linker.starts(), linker.opened()), (1, vec![std::path::PathBuf::from("work")]));
        let received = linker.received();
        let create = received.iter().find(|(_, m)| m["method"] == "POST" && m["path"] == "/session").unwrap();
        assert_eq!(create.1["body"]["permission"], json!([{"permission": "*", "pattern": "*", "action": "allow"}]));
        let prompt = received.iter().find(|(_, m)| m["path"].as_str().is_some_and(|p| p.ends_with("/prompt_async"))).unwrap();
        assert_eq!((prompt.1["body"]["agent"].clone(), prompt.1["body"]["model"].clone(), prompt.1["body"]["parts"][0]["text"].clone()), (json!("build"), json!({"providerID": "opencode-go", "modelID": "kimi"}), json!("hello")));
    }

    #[tokio::test]
    async fn given_a_running_server_when_the_same_thread_and_another_thread_run_turns_then_no_process_starts_and_each_thread_has_a_session_of_its_own() {
        // Given a server that has run one turn of t1
        let (sessions, linker) = (OpencodeSessions::default(), opencode_server());
        drain(sessions.turn("w1", linker.as_ref(), &request("t1", "hello", None)).await.unwrap()).await;
        // When t1 runs another turn (its stored session is given) and t2 runs its first turn
        let again = drain(sessions.turn("w1", linker.as_ref(), &request("t1", "hello", Some("ses-1"))).await.unwrap()).await;
        let other = drain(sessions.turn("w1", linker.as_ref(), &request("t2", "hello", None)).await.unwrap()).await;
        // Then both ran in the one process, t1 went on in its session with no create or lookup and no announcement, and t2 got a session of its own
        assert_eq!(linker.starts(), 1);
        assert_eq!(again[0], ExecEvent::Text("Hi".into()));
        assert_eq!(other[0], ExecEvent::Session("ses-2".into()));
        assert_eq!((creates(&linker), count(&linker, "GET", "/session/")), (2, 0));
        // And a second workspace has a server of its own
        drain(sessions.turn("w2", linker.as_ref(), &request("t3", "hello", None)).await.unwrap()).await;
        assert_eq!(linker.starts(), 2);
    }

    #[tokio::test]
    async fn given_a_turn_in_progress_when_it_is_steered_then_the_prompt_goes_to_the_same_session_the_turn_goes_on_and_ends_once() {
        // Given a turn that is working
        let (sessions, linker) = (OpencodeSessions::default(), opencode_server());
        let mut events = sessions.turn("w1", linker.as_ref(), &request("t1", "hold", None)).await.unwrap();
        assert_eq!((events.recv().await, events.recv().await), (Some(ExecEvent::Session("ses-1".into())), Some(ExecEvent::Text("working".into()))));
        // When it is steered
        sessions.steer("t1", "go left").await.unwrap();
        // Then the same turn takes the new prompt on the same session, goes on, and ends once
        assert_eq!(drain(events).await, vec![ExecEvent::Text(" steered".into()), ExecEvent::Done]);
        let prompts: Vec<_> = linker.received().into_iter().filter(|(_, m)| m["path"].as_str().is_some_and(|p| p.starts_with("/session/ses-1/prompt_async"))).map(|(_, m)| m["body"]["parts"][0]["text"].clone()).collect();
        assert_eq!(prompts, vec![json!("hold"), json!("go left")]);
        assert_eq!((linker.starts(), linker.kills()), (1, 0));
        // And a thread with no running turn cannot be steered
        assert!(sessions.steer("t1", "late").await.unwrap_err().contains("no running turn"));
        assert!(sessions.steer("nobody", "x").await.unwrap_err().contains("no live session"));
    }

    #[tokio::test]
    async fn given_a_turn_in_progress_when_it_is_interrupted_then_the_session_is_aborted_the_turn_ends_without_an_error_and_the_server_stays() {
        // Given a turn that is working
        let (sessions, linker) = (OpencodeSessions::default(), opencode_server());
        let mut events = sessions.turn("w1", linker.as_ref(), &request("t1", "hold", None)).await.unwrap();
        events.recv().await;
        events.recv().await;
        // When it is interrupted
        assert!(sessions.interrupt("t1"));
        // Then the session was aborted, the turn ended with done and no error, and the server was not taken down
        assert_eq!(drain(events).await, vec![ExecEvent::Done]);
        assert_eq!(count(&linker, "POST", "/session/ses-1/abort"), 1);
        assert_eq!(linker.kills(), 0);
        assert!(!sessions.interrupt("t1") && !sessions.interrupt("nobody"));
    }

    #[tokio::test]
    async fn given_a_turn_that_asks_for_a_permission_when_the_person_answers_then_the_ask_is_an_event_the_reply_goes_to_the_server_and_the_turn_goes_on() {
        // Given a turn that has asked to run a command
        let (sessions, linker) = (OpencodeSessions::default(), opencode_server());
        let mut events = sessions.turn("w1", linker.as_ref(), &request("t1", "approve", None)).await.unwrap();
        let mut seen = Vec::new();
        while !matches!(seen.last(), Some(ExecEvent::Approval { .. })) {
            seen.push(events.recv().await.unwrap());
        }
        assert_eq!(seen.last(), Some(&ExecEvent::Approval { id: "per_1".into(), tool: "bash".into(), args: json!({"patterns": ["touch a"], "metadata": {}}) }));
        // When another thread answers it, a wrong id is answered, and the thread accepts it for the session
        assert!(!sessions.answer("nobody", "per_1", Verdict::Accept) && !sessions.answer("t1", "per_9", Verdict::Accept));
        assert!(sessions.answer("t1", "per_1", Verdict::AcceptForSession));
        // Then the reply went to the server as always, the turn went on, and a second answer finds nothing waiting
        assert_eq!(drain(events).await, vec![ExecEvent::Text("done".into()), ExecEvent::Done]);
        let reply = linker.received().into_iter().find(|(_, m)| m["path"].as_str().is_some_and(|p| p.starts_with("/permission/per_1/reply"))).unwrap();
        assert_eq!(reply.1["body"], json!({"reply": "always"}));
        assert!(!sessions.answer("t1", "per_1", Verdict::Accept));
    }

    #[tokio::test]
    async fn given_a_turn_that_asks_a_question_when_the_person_answers_or_closes_it_then_the_answer_or_the_rejection_goes_to_the_server() {
        // Given two turns that have asked a question of the same shape (the second after the first)
        let (sessions, linker) = (OpencodeSessions::default(), opencode_server());
        let mut events = sessions.turn("w1", linker.as_ref(), &request("t1", "ask", None)).await.unwrap();
        let mut seen = Vec::new();
        while !matches!(seen.last(), Some(ExecEvent::Input { .. })) {
            seen.push(events.recv().await.unwrap());
        }
        assert_eq!(seen.last(), Some(&ExecEvent::Input { id: "que_1".into(), prompt: "Which library?".into(), options: vec!["date-fns".into(), "dayjs".into()] }));
        // When the person picks an option
        assert!(sessions.answer_input("t1", "que_1", Some("dayjs".into())));
        // Then the answer went to the server as the list of the chosen labels and the turn went on
        assert_eq!(drain(events).await, vec![ExecEvent::Text("chose".into()), ExecEvent::Done]);
        let reply = linker.received().into_iter().find(|(_, m)| m["path"].as_str().is_some_and(|p| p.starts_with("/question/que_1/reply"))).unwrap();
        assert_eq!(reply.1["body"], json!({"answers": [["dayjs"]]}));
        // And a question the person closes is rejected
        let mut again = sessions.turn("w1", linker.as_ref(), &request("t1", "ask", Some("ses-1"))).await.unwrap();
        while !matches!(again.recv().await, Some(ExecEvent::Input { .. })) {}
        assert!(sessions.answer_input("t1", "que_1", None));
        drain(again).await;
        assert_eq!(count(&linker, "POST", "/question/que_1/reject"), 1);
    }

    #[tokio::test]
    async fn given_a_session_that_fails_when_its_turn_runs_then_the_error_is_an_event_before_the_end() {
        // Given a prompt the server fails with a rate limit
        let (sessions, linker) = (OpencodeSessions::default(), opencode_server());
        // When the turn runs
        let events = drain(sessions.turn("w1", linker.as_ref(), &request("t1", "fail", None)).await.unwrap()).await;
        // Then the session is announced, the error follows, and the turn ends
        assert_eq!(events, vec![ExecEvent::Session("ses-1".into()), ExecEvent::Error("rate limited".into()), ExecEvent::Done]);
    }

    #[tokio::test]
    async fn given_a_server_idle_for_its_time_when_the_thread_runs_again_then_a_new_server_starts_and_the_session_is_resumed_by_its_id() {
        // Given a server that closes after 100 ms idle, and a thread that ran a turn on it
        let (sessions, linker) = (OpencodeSessions::default(), opencode_server());
        sessions.set_idle(Duration::from_millis(100));
        drain(sessions.turn("w1", linker.as_ref(), &request("t1", "hello", None)).await.unwrap()).await;
        // When the idle time has passed
        until("the idle close", || linker.kills() == 1).await;
        until("the thread to be let go", || !sessions.alive("t1")).await;
        // Then the thread's next turn starts another server and goes on in the same session by its id, not a new one
        let again = drain(sessions.turn("w1", linker.as_ref(), &request("t1", "hello", Some("ses-1"))).await.unwrap()).await;
        assert_eq!(again[0], ExecEvent::Text("Hi".into()));
        assert_eq!(linker.starts(), 2);
        let second: Vec<_> = calls(&linker).into_iter().filter(|(start, _, _)| *start == 2).map(|(_, m, p)| format!("{m} {p}")).collect();
        assert_eq!(&second[..2], ["GET /session/ses-1", "PATCH /session/ses-1"]);
        assert_eq!(creates(&linker), 1);
    }

    #[tokio::test]
    async fn given_a_server_that_dies_in_a_turn_when_the_thread_runs_again_then_the_turn_ended_with_an_error_and_the_next_one_resumes_its_session_on_a_new_server() {
        // Given a turn that is working
        let (sessions, linker) = (OpencodeSessions::default(), opencode_server());
        let mut events = sessions.turn("w1", linker.as_ref(), &request("t1", "hold", None)).await.unwrap();
        events.recv().await;
        events.recv().await;
        // When the server dies
        linker.die("segfault");
        // Then the turn ends with the reason, and the thread's next turn starts a new server and resumes the session
        let last = events.recv().await;
        assert_eq!(last, Some(ExecEvent::Error("the opencode server ended: segfault".into())));
        until("the thread to be let go", || !sessions.alive("t1")).await;
        let resumed = drain(sessions.turn("w1", linker.as_ref(), &request("t1", "hello", Some("ses-1"))).await.unwrap()).await;
        assert_eq!((resumed[0].clone(), linker.starts()), (ExecEvent::Text("Hi".into()), 2));
        // And a session the server does not know is refused with its reason
        let refused = sessions.turn("w1", linker.as_ref(), &request("t2", "hello", Some("ses-missing"))).await.err();
        assert_eq!(refused.as_deref(), Some("opencode could not resume session ses-missing: 404: Session not found: ses-missing"));
    }

    #[tokio::test]
    async fn given_an_event_stream_that_ends_when_the_server_is_up_then_the_server_is_taken_down_and_the_turn_ends_with_an_error() {
        // Given a turn that is working
        let (sessions, linker) = (OpencodeSessions::default(), opencode_server());
        let mut events = sessions.turn("w1", linker.as_ref(), &request("t1", "hold", None)).await.unwrap();
        events.recv().await;
        events.recv().await;
        // When the bridge says the event stream ended
        linker.emit(serde_json::json!({"closed": "the event stream ended: the server closed it"}).to_string());
        // Then the server is killed and the turn ends with an error
        assert!(matches!(events.recv().await, Some(ExecEvent::Error(why)) if why.contains("the event stream ended")));
        assert_eq!(linker.kills(), 1);
    }

    #[tokio::test]
    async fn given_a_workspace_with_a_server_when_it_is_closed_or_a_thread_is_forgotten_then_the_server_ends_and_the_thread_gets_a_session_of_its_own() {
        // Given a thread that ran a turn
        let (sessions, linker) = (OpencodeSessions::default(), opencode_server());
        drain(sessions.turn("w1", linker.as_ref(), &request("t1", "hello", None)).await.unwrap()).await;
        // When the thread is forgotten and runs without a stored session, then the workspace is closed
        sessions.forget("t1");
        assert!(!sessions.alive("t1"));
        let fresh = drain(sessions.turn("w1", linker.as_ref(), &request("t1", "hello", None)).await.unwrap()).await;
        assert!(sessions.close("w1") && !sessions.close("nobody"));
        // Then the thread was given a new session on the same server, and the close killed it
        assert_eq!(fresh[0], ExecEvent::Session("ses-2".into()));
        until("the kill", || linker.kills() == 1).await;
        let _ = frame("x", json!({}));
    }
}
