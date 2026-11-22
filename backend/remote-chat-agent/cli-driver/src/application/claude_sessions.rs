//! Claude Code's live sessions: one `claude -p` process for each thread, speaking stream-json over its stdio (`domain::claude`). The process is started when the
//! thread's turn first needs it, kept between turns, closed when no turn has run in it for the idle time, and replaced when it dies; a thread whose process is gone
//! is resumed by its conversation id (`--resume`) on its next turn. A steer is a message with priority `now`, an interrupt a control request, and a permission the CLI
//! asks for is handed out as an event and answered on the same process. A change of the thread's mode or model is a control request, not a restart.
use super::ports::{Linker, SessionDriver};
use crate::domain::claude::{self, Inbound, Outcome, Question, Step, ToolAsk, Translator};
use crate::domain::{Driver, ExecEvent, TurnRequest, Verdict};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

/// How long a request of ours may wait for its answer, how long the process may take to end once its output is closed, and how long an interrupted turn
/// may take to end before the process is taken down.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const END_GRACE: Duration = Duration::from_secs(5);
const DEFAULT_IDLE: Duration = Duration::from_secs(10 * 60);
const DEFAULT_INTERRUPT_GRACE: Duration = Duration::from_secs(15);

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// The turn a process runs: where its events go, the messages of ours it was given (the person's, and each steer), and what was asked of it.
struct Turn {
    seq: u64,
    tx: mpsc::Sender<ExecEvent>,
    messages: Vec<String>,
    /// The steers no result has covered yet: the result of what a steer aborted does not end the turn.
    steers: Vec<String>,
    stop_asked: bool,
}

/// The questions of an `AskUserQuestion` call and the answers given so far.
type Asked = (Vec<Question>, Vec<(String, String)>);

/// What the CLI waits for the person to say.
struct Waiting {
    ask: ToolAsk,
    /// The questions of an `AskUserQuestion` call and the answers so far; none for a tool call that needs an allow or a deny.
    questions: Option<Asked>,
}

#[derive(Default)]
struct Inner {
    pending: HashMap<String, oneshot::Sender<Result<Value, String>>>,
    interrupts: HashSet<String>,
    turn: Option<Turn>,
    turns: u64,
    asks: HashMap<String, Waiting>,
    translator: Translator,
    last_active: Option<Instant>,
    ended: Option<String>,
    /// What the CLI said went wrong outside a turn (a conversation it cannot resume), for the reason the process ends with.
    failure: Option<String>,
    conversation: String,
    mode: String,
    model: Option<String>,
    /// The thread's conversation was let go: the process closes as soon as no turn runs in it.
    forgotten: bool,
}

struct Proc {
    thread: String,
    workspace: String,
    cwd: String,
    send: Arc<dyn Fn(String) -> bool + Send + Sync>,
    kill: Arc<dyn Fn() + Send + Sync>,
    next: AtomicU64,
    inner: Mutex<Inner>,
    shared: Arc<Shared>,
}

struct Shared {
    procs: Mutex<HashMap<String, Arc<Proc>>>,
    slots: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    idle_ms: AtomicU64,
    interrupt_grace_ms: AtomicU64,
}

pub struct ClaudeSessions {
    shared: Arc<Shared>,
}

impl Default for ClaudeSessions {
    fn default() -> Self {
        ClaudeSessions { shared: Arc::new(Shared { procs: Mutex::default(), slots: Mutex::default(), idle_ms: AtomicU64::new(DEFAULT_IDLE.as_millis() as u64), interrupt_grace_ms: AtomicU64::new(DEFAULT_INTERRUPT_GRACE.as_millis() as u64) }) }
    }
}

impl ClaudeSessions {
    pub fn new(idle: Duration) -> ClaudeSessions {
        let sessions = ClaudeSessions::default();
        sessions.set_idle(idle);
        sessions
    }

    /// How long a process may stay without a turn before it is closed; zero keeps it for ever.
    pub fn set_idle(&self, idle: Duration) {
        self.shared.idle_ms.store(idle.as_millis() as u64, Ordering::SeqCst);
    }

    /// How long a turn that was asked to stop may take to end before its process is taken down.
    pub fn set_interrupt_grace(&self, grace: Duration) {
        self.shared.interrupt_grace_ms.store(grace.as_millis() as u64, Ordering::SeqCst);
    }

    fn live(&self, thread: &str) -> Option<Arc<Proc>> {
        locked(&self.shared.procs).get(thread).cloned()
    }

    fn slot(&self, thread: &str) -> Arc<tokio::sync::Mutex<()>> {
        Arc::clone(locked(&self.shared.slots).entry(thread.to_string()).or_default())
    }

    /// A turn that was asked to stop is over for the service as soon as it asks, but the CLI ends it with its own result a moment later: the thread's next turn
    /// waits for that, as long as the grace a stopped turn has, instead of finding the thread busy.
    async fn await_stopped_turn(&self, thread: &str) {
        let Some(proc) = self.live(thread) else { return };
        let until = Instant::now() + Duration::from_millis(self.shared.interrupt_grace_ms.load(Ordering::SeqCst)) + Duration::from_millis(250);
        while Instant::now() < until && proc.inner().ended.is_none() && proc.inner().turn.as_ref().is_some_and(|t| t.stop_asked) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// Starts the thread's process: a new conversation under an id of ours on its first start, the stored one resumed after, and waits for the CLI to answer its
    /// handshake. A process the CLI refuses to run (a conversation it cannot find) ends here with what it said.
    async fn start(&self, workspace: &str, linker: &dyn Linker, req: &TurnRequest) -> Result<(Arc<Proc>, Option<String>), String> {
        let (resume, conversation) = match &req.resume {
            Some(id) => (true, id.clone()),
            None => (false, uuid::Uuid::new_v4().to_string()),
        };
        let args = claude::session_args(req, resume, &conversation);
        let link = linker.open(Driver::ClaudeAgent, Path::new(&req.cwd), &args).await.map_err(|e| format!("claude could not start: {e}"))?;
        let inner = Inner { last_active: Some(Instant::now()), conversation: conversation.clone(), mode: claude::permission_mode(req).to_string(), model: req.model.clone(), ..Inner::default() };
        let proc = Arc::new(Proc { thread: req.thread_id.clone(), workspace: workspace.to_string(), cwd: req.cwd.clone(), send: link.send, kill: link.running.kill, next: AtomicU64::new(0), inner: Mutex::new(inner), shared: Arc::clone(&self.shared) });
        tokio::spawn(Arc::clone(&proc).read(link.running.lines, link.running.finish));
        if let Err(why) = proc.request(claude::initialize).await {
            (proc.kill)();
            return Err(format!("claude did not start: {why}"));
        }
        tokio::spawn(Arc::clone(&proc).watch_idle());
        locked(&self.shared.procs).insert(req.thread_id.clone(), Arc::clone(&proc));
        Ok((proc, (!resume).then_some(conversation)))
    }
}

impl Proc {
    fn inner(&self) -> MutexGuard<'_, Inner> {
        locked(&self.inner)
    }

    /// Whether the process is still to be used; it is counted as active from now, so the idle close cannot take it between this and the turn's start.
    fn claim(&self) -> bool {
        let mut inner = self.inner();
        let alive = inner.ended.is_none();
        if alive {
            inner.last_active = Some(Instant::now());
        }
        alive
    }

    /// Ends the process for `why`; from here no turn is given to it, though what it was running is settled only when its output closes.
    fn shut(&self, why: &str) {
        self.inner().ended.get_or_insert_with(|| why.to_string());
        (self.kill)();
    }

    fn id(&self) -> String {
        format!("rca-{}", self.next.fetch_add(1, Ordering::SeqCst) + 1)
    }

    /// Sends the control request `line` builds for a fresh id and waits for the answer: its response body, or why there is none.
    async fn request(&self, line: impl FnOnce(&str) -> String) -> Result<Value, String> {
        let id = self.id();
        let (tx, rx) = oneshot::channel();
        {
            let mut inner = self.inner();
            if let Some(why) = &inner.ended {
                return Err(why.clone());
            }
            inner.pending.insert(id.clone(), tx);
        }
        if !(self.send)(line(&id)) {
            self.inner().pending.remove(&id);
            return Err("the Claude Code process is gone".into());
        }
        match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(answer)) => answer,
            Ok(Err(_)) => Err(self.inner().ended.clone().unwrap_or_else(|| "the Claude Code process is gone".into())),
            Err(_) => {
                self.inner().pending.remove(&id);
                Err("the Claude Code process did not answer in time".into())
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
        let said = self.inner().failure.clone();
        let why = match (ended, said) {
            (_, Some(said)) => format!("the Claude Code process ended: {said}"),
            (Ok(()), None) => "the Claude Code process ended".to_string(),
            (Err(why), None) => format!("the Claude Code process ended: {why}"),
        };
        self.end(why).await;
    }

    async fn end(&self, why: String) {
        let stream = {
            let mut inner = self.inner();
            inner.ended = Some(why.clone());
            for (_, waiting) in inner.pending.drain() {
                let _ = waiting.send(Err(why.clone()));
            }
            inner.asks.clear();
            inner.turn.take().map(|t| t.tx)
        };
        {
            let mut procs = locked(&self.shared.procs);
            if procs.get(&self.thread).is_some_and(|p| std::ptr::eq(Arc::as_ptr(p), self)) {
                procs.remove(&self.thread);
            }
        }
        if let Some(tx) = stream {
            let _ = tx.send(ExecEvent::Error(why)).await;
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
                let busy = inner.turn.is_some() || !inner.asks.is_empty();
                let wait = if idle.is_zero() {
                    Duration::from_secs(1)
                } else if busy {
                    idle.clamp(Duration::from_millis(50), Duration::from_secs(1))
                } else {
                    idle.saturating_sub(inner.last_active.map_or(idle, |at| at.elapsed()))
                };
                if wait.is_zero() {
                    inner.ended = Some("the Claude Code process was closed after it sat idle".into());
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
        match claude::parse(text) {
            Inbound::Response { id, outcome } => self.on_response(&id, outcome).await,
            Inbound::Request { id, body } => self.on_request(&id, &body).await,
            Inbound::Withdrawn { id } => self.inner().asks.retain(|_, w| w.ask.request != id),
            Inbound::Frame(frame) => self.on_frame(&frame).await,
            Inbound::Other => {}
        }
    }

    /// The answer to a request of ours. The answer to an interrupt names the messages it cancelled: a turn whose every message was cancelled before it began
    /// gets no result from the CLI, so it ends here.
    async fn on_response(&self, id: &str, outcome: Result<Value, String>) {
        let ended = {
            let mut inner = self.inner();
            if !inner.interrupts.remove(id) {
                if let Some(waiting) = inner.pending.remove(id) {
                    let _ = waiting.send(outcome);
                }
                return;
            }
            let cancelled: Vec<String> = outcome.ok().and_then(|body| body["cancelled"].as_array().map(|all| all.iter().filter_map(Value::as_str).map(str::to_string).collect())).unwrap_or_default();
            let gone = match inner.turn.as_mut() {
                Some(turn) if !cancelled.is_empty() => {
                    turn.messages.retain(|m| !cancelled.contains(m));
                    turn.steers.retain(|m| !cancelled.contains(m));
                    turn.messages.is_empty()
                }
                _ => false,
            };
            gone.then(|| self.finish_turn(&mut inner)).flatten()
        };
        if let Some((tx, events)) = ended {
            for event in events {
                let _ = tx.send(event).await;
            }
        }
    }

    /// Ends the turn that runs: its stream gets `events`, and what it waited for is let go.
    fn finish_turn(&self, inner: &mut Inner) -> Option<(mpsc::Sender<ExecEvent>, Vec<ExecEvent>)> {
        let turn = inner.turn.take()?;
        inner.asks.clear();
        inner.last_active = Some(Instant::now());
        Some((turn.tx, vec![ExecEvent::Done]))
    }

    async fn on_frame(&self, frame: &Value) {
        let (tx, events, close) = {
            let mut inner = self.inner();
            let mut events = Vec::new();
            let mut ended = None;
            for step in inner.translator.frame(frame) {
                match step {
                    Step::Event(event) => events.push(event),
                    Step::Conversation(id) => {
                        if inner.conversation != id {
                            inner.conversation = id.clone();
                            events.push(ExecEvent::Session(id));
                        }
                    }
                    Step::Result(outcome) => match inner.turn.as_mut().map(|turn| settles(turn, &outcome)) {
                        Some(true) => ended = Some(outcome),
                        Some(false) => {}
                        None => {
                            if let Some(why) = outcome.error {
                                inner.failure = Some(why);
                            }
                        }
                    },
                }
            }
            // What a process says between turns belongs to no turn
            let Some(tx) = inner.turn.as_ref().map(|turn| turn.tx.clone()) else { return };
            let mut close = false;
            if let Some(outcome) = ended {
                events.extend(outcome.error.map(ExecEvent::Error));
                events.push(ExecEvent::Done);
                inner.turn = None;
                inner.asks.clear();
                inner.last_active = Some(Instant::now());
                close = inner.forgotten;
            }
            (tx, events, close)
        };
        for event in events {
            if tx.send(event).await.is_err() {
                // nobody follows the turn any more
                self.send_interrupt();
                break;
            }
        }
        if close {
            self.shut("the Claude Code process was closed, its conversation let go");
        }
    }

    async fn on_request(&self, id: &str, body: &Value) {
        let Some(ask) = claude::tool_ask(id, body) else {
            (self.send)(claude::refuse(id, body["subtype"].as_str().unwrap_or_default()));
            return;
        };
        let asked = {
            let mut inner = self.inner();
            match inner.turn.as_ref().map(|t| t.tx.clone()) {
                None => None,
                Some(tx) => {
                    let questions = (ask.tool == "AskUserQuestion").then(|| claude::questions_of(&ask.input)).filter(|q| !q.is_empty());
                    let mut key = ask.tool_use.clone();
                    let mut n = 1;
                    while inner.asks.contains_key(&key) {
                        n += 1;
                        key = format!("{}.{n}", ask.tool_use);
                    }
                    let event = match &questions {
                        Some(all) => ExecEvent::Input { id: key.clone(), prompt: all[0].text.clone(), options: all[0].options.clone() },
                        None => ExecEvent::Approval { id: key.clone(), tool: ask.tool.clone(), args: ask.input.clone() },
                    };
                    inner.asks.insert(key, Waiting { ask: ask.clone(), questions: questions.map(|q| (q, Vec::new())) });
                    Some((tx, event))
                }
            }
        };
        match asked {
            // nobody can answer: the CLI is told no, so it never waits for nobody
            None => {
                (self.send)(if ask.tool == "AskUserQuestion" { claude::dismiss_questions(&ask) } else { claude::answer_tool(&ask, Verdict::Decline) });
            }
            Some((tx, event)) => {
                if tx.send(event).await.is_err() {
                    (self.send)(claude::answer_tool(&ask, Verdict::Decline));
                }
            }
        }
    }

    fn send_interrupt(&self) {
        let id = self.id();
        self.inner().interrupts.insert(id.clone());
        (self.send)(claude::interrupt(&id));
    }
}

/// Whether `outcome` ends `turn`, and what it ends with: a result that is not about the turn's messages is another turn's (a background task waking the model); the
/// result of what a steer aborted, or of a message that ran before the steer, leaves the turn going on with the steer.
fn settles(turn: &mut Turn, outcome: &Outcome) -> bool {
    let ours = outcome.messages.as_ref().is_none_or(|said| said.iter().any(|m| turn.messages.contains(m)));
    if !ours {
        return false;
    }
    if turn.steers.is_empty() || turn.stop_asked || outcome.error.is_some() {
        return true;
    }
    let covers = outcome.messages.as_ref().is_none_or(|said| turn.steers.iter().any(|s| said.contains(s)));
    if outcome.aborted || !covers {
        return false;
    }
    turn.steers.clear();
    true
}

#[async_trait]
impl SessionDriver for ClaudeSessions {
    async fn start(&self, _: &str, _: &dyn Linker, _: &Path) -> Result<(), String> {
        Ok(())
    }

    async fn turn(&self, workspace: &str, linker: &dyn Linker, req: &TurnRequest) -> Result<mpsc::Receiver<ExecEvent>, String> {
        let slot = self.slot(&req.thread_id);
        let _starting = slot.lock().await;
        self.await_stopped_turn(&req.thread_id).await;
        let reusable = self.live(&req.thread_id).filter(|p| p.cwd == req.cwd && p.workspace == workspace);
        if let Some(stale) = self.live(&req.thread_id).filter(|p| reusable.as_ref().is_none_or(|r| !Arc::ptr_eq(p, r))) {
            stale.shut("the Claude Code process was replaced, its workspace changed");
        }
        let (proc, fresh) = match reusable.filter(|p| p.claim()) {
            Some(proc) => (proc, None),
            None => self.start(workspace, linker, req).await?,
        };
        if proc.inner().turn.is_some() {
            return Err(format!("thread {} already has a running turn", req.thread_id));
        }
        // A thread whose mode or model changed goes on in the same process
        let mode = claude::permission_mode(req);
        if proc.inner().mode != mode {
            proc.request(|id| claude::set_permission_mode(id, mode)).await.map_err(|why| format!("claude did not take the mode {mode}: {why}"))?;
            proc.inner().mode = mode.to_string();
        }
        if let Some(model) = req.model.as_ref().filter(|m| proc.inner().model.as_ref() != Some(*m)) {
            proc.request(|id| claude::set_model(id, model)).await.map_err(|why| format!("claude did not take the model {model}: {why}"))?;
            proc.inner().model = Some(model.clone());
        }
        let (tx, rx) = mpsc::channel(256);
        let message = uuid::Uuid::new_v4().to_string();
        {
            let mut inner = proc.inner();
            if inner.ended.is_some() {
                return Err(inner.ended.clone().unwrap_or_default());
            }
            inner.turns += 1;
            let seq = inner.turns;
            inner.turn = Some(Turn { seq, tx: tx.clone(), messages: vec![message.clone()], steers: Vec::new(), stop_asked: false });
            inner.translator.reset();
            inner.last_active = Some(Instant::now());
        }
        if let Some(id) = fresh {
            let _ = tx.send(ExecEvent::Session(id)).await;
        }
        if !(proc.send)(claude::user_message(&message, &req.prompt, None)) {
            proc.inner().turn = None;
            return Err("the Claude Code process is gone".into());
        }
        Ok(rx)
    }

    async fn steer(&self, thread: &str, prompt: &str) -> Result<(), String> {
        let proc = self.live(thread).ok_or_else(|| format!("thread {thread} has no live session"))?;
        let message = uuid::Uuid::new_v4().to_string();
        {
            let mut inner = proc.inner();
            let turn = inner.turn.as_mut().filter(|t| !t.stop_asked).ok_or_else(|| format!("thread {thread} has no running turn to steer"))?;
            turn.messages.push(message.clone());
            turn.steers.push(message.clone());
        }
        if !(proc.send)(claude::user_message(&message, prompt, Some("now"))) {
            return Err("claude did not take the steer: the process is gone".into());
        }
        Ok(())
    }

    fn interrupt(&self, thread: &str) -> bool {
        let Some(proc) = self.live(thread) else { return false };
        let seq = {
            let mut inner = proc.inner();
            let Some(turn) = inner.turn.as_mut() else { return false };
            turn.stop_asked = true;
            turn.seq
        };
        proc.send_interrupt();
        // A process that does not end the turn is taken down, so the thread is never left running for ever
        let grace = Duration::from_millis(self.shared.interrupt_grace_ms.load(Ordering::SeqCst));
        tokio::spawn(async move {
            tokio::time::sleep(grace).await;
            if proc.inner().turn.as_ref().is_some_and(|t| t.seq == seq) {
                proc.shut("the Claude Code process did not end the interrupted turn");
            }
        });
        true
    }

    fn answer(&self, thread: &str, approval: &str, verdict: Verdict) -> bool {
        let Some(proc) = self.live(thread) else { return false };
        let waiting = {
            let mut inner = proc.inner();
            match inner.asks.get(approval) {
                Some(w) if w.questions.is_none() => inner.asks.remove(approval),
                _ => None,
            }
        };
        let Some(waiting) = waiting else { return false };
        (proc.send)(claude::answer_tool(&waiting.ask, verdict))
    }

    fn answer_input(&self, thread: &str, request: &str, answer: Option<String>) -> bool {
        let Some(proc) = self.live(thread) else { return false };
        let (done, next) = {
            let mut inner = proc.inner();
            let Some(mut waiting) = (match inner.asks.get(request) {
                Some(w) if w.questions.is_some() => inner.asks.remove(request),
                _ => None,
            }) else {
                return false;
            };
            let Some(text) = answer else { return (proc.send)(claude::dismiss_questions(&waiting.ask)) };
            let (all, given) = waiting.questions.as_mut().expect("a question is waiting");
            given.push((all[given.len()].text.clone(), text));
            if given.len() < all.len() {
                let (key, question) = (format!("{}.q{}", waiting.ask.tool_use, given.len() + 1), &all[given.len()]);
                let event = ExecEvent::Input { id: key.clone(), prompt: question.text.clone(), options: question.options.clone() };
                let tx = inner.turn.as_ref().map(|t| t.tx.clone());
                inner.asks.insert(key, waiting);
                (None, tx.map(|tx| (tx, event)))
            } else {
                (Some(claude::answer_questions(&waiting.ask, given)), None)
            }
        };
        if let Some(line) = done {
            return (proc.send)(line);
        }
        match next {
            Some((tx, event)) => tx.try_send(event).is_ok(),
            None => false,
        }
    }

    fn alive(&self, thread: &str) -> bool {
        self.live(thread).is_some_and(|p| p.inner().ended.is_none())
    }

    fn forget(&self, thread: &str) {
        let Some(proc) = self.live(thread) else { return };
        let idle = {
            let mut inner = proc.inner();
            inner.forgotten = true;
            inner.turn.is_none()
        };
        if idle {
            proc.shut("the Claude Code process was closed, its conversation let go");
        }
    }

    fn close(&self, workspace: &str) -> bool {
        let all: Vec<Arc<Proc>> = locked(&self.shared.procs).values().filter(|p| p.workspace == workspace).cloned().collect();
        for proc in &all {
            proc.shut("the Claude Code process was closed");
        }
        !all.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{claude_session, ScriptedLinker};
    use serde_json::json;

    fn request(thread: &str, prompt: &str, resume: Option<&str>) -> TurnRequest {
        TurnRequest { thread_id: thread.into(), run_id: "r".into(), instance_id: "claudeAgent".into(), model: None, cwd: "work".into(), prompt: prompt.into(), runtime_mode: "full-access".into(), interaction_mode: "default".into(), resume: resume.map(str::to_string), tools: None }
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

    fn subtypes(linker: &ScriptedLinker) -> Vec<(usize, String)> {
        linker.received().into_iter().map(|(start, m)| (start, if m["type"] == "control_request" { m["request"]["subtype"].as_str().unwrap_or_default().to_string() } else { m["type"].as_str().unwrap_or_default().to_string() })).collect()
    }

    fn first_session(events: &[ExecEvent]) -> String {
        match &events[0] {
            ExecEvent::Session(id) => id.clone(),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn given_a_thread_without_a_process_when_a_turn_runs_then_the_process_starts_with_a_new_conversation_and_the_flags_of_the_thread_and_the_turn_streams_to_its_end() {
        // Given a scripted Claude Code process and a turn in the mode that accepts edits with a model
        let (sessions, linker) = (ClaudeSessions::default(), claude_session());
        let req = TurnRequest { runtime_mode: "auto-accept-edits".into(), model: Some("opus".into()), ..request("t1", "hello", None) };
        // When the thread runs its first turn
        let events = drain(sessions.turn("w1", linker.as_ref(), &req).await.unwrap()).await;
        // Then the conversation of our own id is announced first, the text streamed once, a tool call and its result were shown, and the turn ends with done
        let id = first_session(&events);
        assert_eq!(id.len(), 36);
        assert_eq!(&events[1..], [ExecEvent::Text("Hi".into()), ExecEvent::ToolStarted { id: "k1".into(), name: "Bash".into(), input: json!({"command": "ls"}) }, ExecEvent::ToolFinished { id: "k1".into(), output: "a.txt".into(), ok: true }, ExecEvent::Done]);
        // And the process was started once in the request's directory with the new conversation's id, the mode and the model, and handshook before the message
        assert_eq!((linker.starts(), linker.opened()), (1, vec![std::path::PathBuf::from("work")]));
        assert_eq!(linker.flags()[0], ["--session-id", id.as_str(), "--permission-mode", "acceptEdits", "--model", "opus"]);
        assert_eq!(subtypes(&linker).into_iter().map(|(_, s)| s).collect::<Vec<_>>(), ["initialize", "user"]);
    }

    #[tokio::test]
    async fn given_a_live_process_when_the_thread_runs_another_turn_and_another_thread_runs_one_then_the_first_goes_on_in_its_process_and_the_second_gets_a_process_of_its_own() {
        // Given a thread that ran one turn
        let (sessions, linker) = (ClaudeSessions::default(), claude_session());
        let id = first_session(&drain(sessions.turn("w1", linker.as_ref(), &request("t1", "hello", None)).await.unwrap()).await);
        // When it runs another turn (its stored conversation is given, with a changed mode and model) and another thread runs its first turn
        let req = TurnRequest { runtime_mode: "approval-required".into(), model: Some("opus".into()), ..request("t1", "hello", Some(&id)) };
        let again = drain(sessions.turn("w1", linker.as_ref(), &req).await.unwrap()).await;
        let other = drain(sessions.turn("w1", linker.as_ref(), &request("t2", "hello", None)).await.unwrap()).await;
        // Then the first thread's turn ran in the same process with no announcement, after a mode change and a model change as control requests
        assert_eq!(again.first(), Some(&ExecEvent::Text("Hi".into())));
        let t1: Vec<String> = subtypes(&linker).into_iter().filter(|(start, _)| *start == 1).map(|(_, s)| s).collect();
        assert_eq!(t1, ["initialize", "user", "set_permission_mode", "set_model", "user"]);
        let modes: Vec<Value> = linker.received().into_iter().map(|(_, m)| m).filter(|m| m["request"]["subtype"] == "set_permission_mode").map(|m| m["request"]["mode"].clone()).collect();
        assert_eq!(modes, vec![json!("default")]);
        // And the second thread started a process of its own, with a conversation of its own
        assert_eq!(linker.starts(), 2);
        assert_ne!(first_session(&other), id);
        assert!(sessions.alive("t1") && sessions.alive("t2") && !sessions.alive("t3"));
    }

    #[tokio::test]
    async fn given_a_turn_in_progress_when_it_is_steered_then_the_message_goes_to_the_live_process_with_priority_now_and_the_aborted_result_does_not_end_the_turn() {
        // Given a turn that is working
        let (sessions, linker) = (ClaudeSessions::default(), claude_session());
        let mut events = sessions.turn("w1", linker.as_ref(), &request("t1", "hold", None)).await.unwrap();
        assert!(matches!(events.recv().await, Some(ExecEvent::Session(_))));
        assert_eq!(events.recv().await, Some(ExecEvent::Text("working".into())));
        // When it is steered
        sessions.steer("t1", "go left").await.unwrap();
        // Then the same turn goes on with the steered text and ends once, after the aborted result of the first segment, on the same process
        assert_eq!(drain(events).await, vec![ExecEvent::Text(" steered".into()), ExecEvent::Done]);
        let user: Vec<Value> = linker.received().into_iter().map(|(_, m)| m).filter(|m| m["type"] == "user").collect();
        assert_eq!((user.len(), user[1]["priority"].clone(), user[1]["message"]["content"][0]["text"].clone()), (2, json!("now"), json!("go left")));
        assert_eq!((linker.starts(), linker.kills()), (1, 0));
        // And a thread with no running turn cannot be steered
        assert!(sessions.steer("t1", "late").await.unwrap_err().contains("no running turn"));
        assert!(sessions.steer("nobody", "x").await.unwrap_err().contains("no live session"));
    }

    #[tokio::test]
    async fn given_a_turn_that_asks_to_use_a_tool_when_the_person_answers_then_the_ask_is_an_event_the_answer_goes_to_the_live_process_and_the_turn_goes_on() {
        // Given a turn that asked for the person's permission to write a file
        let (sessions, linker) = (ClaudeSessions::default(), claude_session());
        let mut events = sessions.turn("w1", linker.as_ref(), &request("t1", "approve", None)).await.unwrap();
        let approval = loop {
            if let ExecEvent::Approval { id, tool, args } = events.recv().await.unwrap() {
                break (id, tool, args);
            }
        };
        // Then the ask names the tool and its input, under the id of the tool call
        assert_eq!((approval.0.as_str(), approval.1.as_str(), approval.2["file_path"].clone()), ("toolu_9", "Write", json!("a.txt")));
        // When the person accepts it for the session, and answers again after it was taken
        assert!(sessions.answer("t1", "toolu_9", Verdict::AcceptForSession));
        assert!(!sessions.answer("t1", "toolu_9", Verdict::Accept));
        // Then the answer was written to the live process under the id of its request, and the turn went on to its end
        let rest = drain(events).await;
        assert!(rest.contains(&ExecEvent::Text("written".into())) && rest.last() == Some(&ExecEvent::Done), "{rest:?}");
        let answers: Vec<Value> = linker.received().into_iter().map(|(_, m)| m).filter(|m| m["type"] == "control_response").collect();
        assert_eq!((answers.len(), answers[0]["response"]["request_id"].clone(), answers[0]["response"]["response"]["behavior"].clone()), (1, json!("c9"), json!("allow")));
        assert_eq!(answers[0]["response"]["response"]["updatedPermissions"][0]["destination"], "session");
        assert_eq!(linker.starts(), 1);
    }

    #[tokio::test]
    async fn given_a_permission_that_is_declined_or_withdrawn_when_the_turn_goes_on_then_a_decline_is_a_deny_the_model_reads_and_a_withdrawn_ask_can_no_longer_be_answered() {
        // Given a turn that asks for a permission, and a second one whose ask the CLI withdraws
        let (sessions, linker) = (ClaudeSessions::default(), claude_session());
        let mut events = sessions.turn("w1", linker.as_ref(), &request("t1", "approve", None)).await.unwrap();
        loop {
            if matches!(events.recv().await.unwrap(), ExecEvent::Approval { .. }) {
                break;
            }
        }
        // When the person declines
        assert!(sessions.answer("t1", "toolu_9", Verdict::Decline));
        drain(events).await;
        let answer = linker.received().into_iter().map(|(_, m)| m).find(|m| m["type"] == "control_response").unwrap();
        assert_eq!((answer["response"]["response"]["behavior"].clone(), answer["response"]["response"]["message"].clone()), (json!("deny"), json!("User declined tool execution.")));
        // And when the CLI withdraws an ask, it is gone
        let mut again = sessions.turn("w1", linker.as_ref(), &request("t1", "approve", None)).await.unwrap();
        loop {
            if matches!(again.recv().await.unwrap(), ExecEvent::Approval { .. }) {
                break;
            }
        }
        linker.emit(json!({"type": "control_cancel_request", "request_id": "c9"}).to_string());
        until("the withdrawal", || !sessions.answer("t1", "no-such", Verdict::Accept) && sessions.live("t1").is_some_and(|p| p.inner().asks.is_empty())).await;
        assert!(!sessions.answer("t1", "toolu_9", Verdict::Accept));
    }

    #[tokio::test]
    async fn given_a_turn_that_asks_two_questions_when_the_person_answers_each_then_each_is_an_input_event_in_turn_and_the_answers_go_back_together_keyed_by_the_questions() {
        // Given a turn whose model asks two questions in one call
        let (sessions, linker) = (ClaudeSessions::default(), claude_session());
        let mut events = sessions.turn("w1", linker.as_ref(), &request("t1", "ask", None)).await.unwrap();
        let first = loop {
            if let ExecEvent::Input { id, prompt, options } = events.recv().await.unwrap() {
                break (id, prompt, options);
            }
        };
        assert_eq!((first.1.as_str(), first.2.clone()), ("Which library?", vec!["date-fns".to_string(), "dayjs".to_string()]));
        // When the person answers the first, then the second that follows it
        assert!(sessions.answer_input("t1", &first.0, Some("dayjs".into())));
        let second = loop {
            if let ExecEvent::Input { id, prompt, .. } = events.recv().await.unwrap() {
                break (id, prompt);
            }
        };
        assert_eq!(second.1, "Which style?");
        assert!(sessions.answer_input("t1", &second.0, Some("tabs".into())));
        // Then the CLI got one allow with both answers keyed by the question texts, and the turn ended
        assert_eq!(drain(events).await.last(), Some(&ExecEvent::Done));
        let allow = linker.received().into_iter().map(|(_, m)| m).find(|m| m["type"] == "control_response").unwrap();
        assert_eq!(allow["response"]["response"]["updatedInput"]["answers"], json!({"Which library?": "dayjs", "Which style?": "tabs"}));
        // And a question that is not waiting cannot be answered
        assert!(!sessions.answer_input("t1", &first.0, Some("x".into())));
    }

    #[tokio::test]
    async fn given_a_running_turn_when_it_is_interrupted_then_the_process_is_asked_to_interrupt_and_the_turn_ends_without_an_error_or_a_restart() {
        // Given a turn that is working
        let (sessions, linker) = (ClaudeSessions::default(), claude_session());
        let mut events = sessions.turn("w1", linker.as_ref(), &request("t1", "hold", None)).await.unwrap();
        events.recv().await;
        events.recv().await;
        // When it is interrupted, and a thread with no turn is
        assert!(sessions.interrupt("t1"));
        assert!(!sessions.interrupt("nobody"));
        // Then the stream ends with done, the interrupt cancelled what was queued, and the process lives on
        assert_eq!(drain(events).await, vec![ExecEvent::Done]);
        assert!(subtypes(&linker).iter().any(|(_, s)| s == "interrupt"));
        assert_eq!((linker.starts(), linker.kills()), (1, 0));
        // And the thread runs another turn on it
        assert_eq!(drain(sessions.turn("w1", linker.as_ref(), &request("t1", "hello", Some("c"))).await.unwrap()).await.last(), Some(&ExecEvent::Done));
        assert_eq!(linker.starts(), 1);
    }

    #[tokio::test]
    async fn given_an_interrupted_turn_the_cli_has_not_ended_yet_when_the_thread_runs_again_then_the_new_turn_waits_for_the_old_one_to_end_and_runs_in_the_same_process() {
        // Given a process that takes its time to end an interrupted turn, and answers the next message at once
        let linker = ScriptedLinker::reacting(|_, m| {
            let ok = |body: Value| json!({"type": "control_response", "response": {"subtype": "success", "request_id": m["request_id"], "response": body}}).to_string();
            match (m["type"].as_str(), m["request"]["subtype"].as_str(), m["message"]["content"][0]["text"].as_str()) {
                (Some("control_request"), Some("initialize"), _) => vec![ok(json!({}))],
                (Some("control_request"), Some("interrupt"), _) => vec![ok(json!({"still_queued": [], "cancelled": []}))],
                (Some("user"), _, Some("again")) => vec![json!({"type": "result", "subtype": "success", "is_error": false, "user_message_uuids": [m["uuid"]]}).to_string()],
                _ => vec![],
            }
        });
        let sessions = ClaudeSessions::default();
        let first = sessions.turn("w1", linker.as_ref(), &request("t1", "hold", None)).await.unwrap();
        assert!(sessions.interrupt("t1"));
        let ends = async {
            tokio::time::sleep(Duration::from_millis(150)).await;
            let uuid = linker.received().into_iter().map(|(_, m)| m).find(|m| m["type"] == "user").unwrap()["uuid"].clone();
            linker.emit(json!({"type": "result", "subtype": "error_during_execution", "is_error": true, "terminal_reason": "aborted_streaming", "user_message_uuids": [uuid]}).to_string());
        };
        // When the thread runs another turn while the old one is still ending
        let again = request("t1", "again", Some("c"));
        let (second, ()) = tokio::join!(sessions.turn("w1", linker.as_ref(), &again), ends);
        // Then the new turn was taken once the old one ended, in the same process, and both streams ended
        assert_eq!(drain(second.unwrap()).await.last(), Some(&ExecEvent::Done));
        assert_eq!(drain(first).await.last(), Some(&ExecEvent::Done));
        assert_eq!((linker.starts(), linker.kills()), (1, 0));
    }

    #[tokio::test]
    async fn given_a_process_that_does_not_end_an_interrupted_turn_when_the_grace_runs_out_then_it_is_taken_down_and_the_turn_ends_in_an_error() {
        // Given a process that ignores interrupts, and a short grace
        let linker = ScriptedLinker::reacting(|_, m| match (m["type"].as_str(), m["request"]["subtype"].as_str()) {
            (Some("control_request"), Some("initialize")) => vec![json!({"type": "control_response", "response": {"subtype": "success", "request_id": m["request_id"], "response": {}}}).to_string()],
            _ => vec![],
        });
        let sessions = ClaudeSessions::default();
        sessions.set_interrupt_grace(Duration::from_millis(100));
        let mut events = sessions.turn("w1", linker.as_ref(), &request("t1", "hold", None)).await.unwrap();
        // When the turn is interrupted
        assert!(sessions.interrupt("t1"));
        // Then the process is killed and the turn's stream ends with the reason
        let mut all = Vec::new();
        while let Some(event) = events.recv().await {
            all.push(event);
        }
        assert!(matches!(all.last(), Some(ExecEvent::Error(why)) if why.contains("ended")), "{all:?}");
        assert_eq!(linker.kills(), 1);
    }

    #[tokio::test]
    async fn given_a_process_that_stays_without_a_turn_when_the_idle_time_passes_then_it_is_closed_and_the_next_turn_starts_a_new_one_and_resumes_the_conversation_by_its_id() {
        // Given a thread that ran a turn, and an idle time of 150 ms
        let (sessions, linker) = (ClaudeSessions::new(Duration::from_millis(150)), claude_session());
        let id = first_session(&drain(sessions.turn("w1", linker.as_ref(), &request("t1", "hello", None)).await.unwrap()).await);
        // When nothing runs for longer than that
        until("the idle close", || linker.kills() == 1).await;
        until("the end of the process", || !sessions.alive("t1")).await;
        // Then the thread's next turn starts a second process that resumes the conversation by the id the first announced
        let events = drain(sessions.turn("w1", linker.as_ref(), &request("t1", "hello", Some(&id))).await.unwrap()).await;
        assert_eq!(events.first(), Some(&ExecEvent::Text("Hi".into())));
        assert_eq!(linker.starts(), 2);
        assert_eq!(linker.flags()[1][..2], ["--resume".to_string(), id]);
    }

    #[tokio::test]
    async fn given_a_turn_in_progress_when_the_process_dies_then_the_turn_ends_in_an_error_naming_why_and_the_next_turn_restarts_the_process_and_resumes_the_conversation() {
        // Given a turn that is working
        let (sessions, linker) = (ClaudeSessions::default(), claude_session());
        let mut events = sessions.turn("w1", linker.as_ref(), &request("t1", "hold", None)).await.unwrap();
        let id = first_session(&[events.recv().await.unwrap()]);
        events.recv().await;
        // When the process is killed from outside
        linker.die("killed by a signal");
        // Then the stream says why and ends
        let mut rest = Vec::new();
        while let Some(event) = events.recv().await {
            rest.push(event);
        }
        assert!(matches!(rest.as_slice(), [ExecEvent::Error(why)] if why.contains("killed by a signal")), "{rest:?}");
        // And the thread's next turn restarts the process and resumes the conversation by its id
        until("the process to be gone", || !sessions.alive("t1")).await;
        let again = sessions.turn("w1", linker.as_ref(), &request("t1", "hello", Some(&id))).await.unwrap();
        assert_eq!(drain(again).await.first(), Some(&ExecEvent::Text("Hi".into())));
        assert_eq!(linker.starts(), 2);
        assert_eq!(linker.flags()[1][..2], ["--resume".to_string(), id]);
    }

    #[tokio::test]
    async fn given_a_process_that_cannot_start_or_a_conversation_it_cannot_resume_when_a_turn_runs_then_it_is_refused_with_the_reason_and_a_busy_thread_is_refused() {
        // Given a program that cannot be started
        let (sessions, linker) = (ClaudeSessions::default(), claude_session());
        linker.refuse_next("not found");
        // When a turn runs, Then it is refused naming the program and why
        assert_eq!(sessions.turn("w1", linker.as_ref(), &request("t1", "hello", None)).await.err().as_deref(), Some("claude could not start: not found"));
        // And a conversation the CLI cannot find ends the start with what it said, and nothing runs
        let broken = ScriptedLinker::reacting(|_, m| {
            if m["request"]["subtype"] == "initialize" {
                vec![json!({"type": "result", "subtype": "error_during_execution", "is_error": true, "errors": ["No conversation found with session ID: gone"]}).to_string()]
            } else {
                vec![]
            }
        });
        let dies = async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            broken.die("exited with 1");
        };
        let resumed = request("t2", "hello", Some("gone"));
        let (refused, ()) = tokio::join!(sessions.turn("w2", broken.as_ref(), &resumed), dies);
        let refused = refused.err().unwrap_or_default();
        assert!(refused.contains("did not start") && refused.contains("No conversation found"), "{refused}");
        // And a thread that has a turn running is refused a second one
        let _first = sessions.turn("w3", linker.as_ref(), &request("t3", "hold", None)).await.unwrap();
        let busy = sessions.turn("w3", linker.as_ref(), &request("t3", "hello", Some("c"))).await.err();
        assert_eq!(busy.as_deref(), Some("thread t3 already has a running turn"));
    }

    #[tokio::test]
    async fn given_an_ask_for_a_thread_that_runs_no_turn_when_the_cli_asks_then_it_is_denied_so_the_cli_never_waits_for_nobody_and_a_request_that_is_no_ask_is_refused() {
        // Given a process whose thread has ended its turn
        let (sessions, linker) = (ClaudeSessions::default(), claude_session());
        drain(sessions.turn("w1", linker.as_ref(), &request("t1", "hello", None)).await.unwrap()).await;
        // When the CLI asks for a permission, and sends a request of another kind
        linker.emit(json!({"type": "control_request", "request_id": "x1", "request": {"subtype": "can_use_tool", "tool_name": "Bash", "input": {}, "tool_use_id": "toolu_x"}}).to_string());
        linker.emit(json!({"type": "control_request", "request_id": "x2", "request": {"subtype": "elicitation"}}).to_string());
        // Then the first is denied at once and the second is answered with an error
        until("the answers", || linker.received().iter().filter(|(_, m)| m["type"] == "control_response").count() == 2).await;
        let answers: Vec<Value> = linker.received().into_iter().map(|(_, m)| m).filter(|m| m["type"] == "control_response").collect();
        assert_eq!((answers[0]["response"]["response"]["behavior"].clone(), answers[1]["response"]["subtype"].clone()), (json!("deny"), json!("error")));
    }

    #[tokio::test]
    async fn given_a_result_that_is_another_turns_when_a_turn_runs_then_it_does_not_end_the_turn_and_a_failed_result_ends_it_with_the_cli_s_error() {
        // Given a turn that is working
        let (sessions, linker) = (ClaudeSessions::default(), claude_session());
        let mut events = sessions.turn("w1", linker.as_ref(), &request("t1", "hold", None)).await.unwrap();
        events.recv().await;
        events.recv().await;
        // When a background task wakes the model and its result arrives for no message of ours, and then the turn's own result is a failure
        linker.emit(json!({"type": "result", "subtype": "success", "is_error": false, "user_message_uuids": null}).to_string());
        let uuid = linker.received().into_iter().map(|(_, m)| m).find(|m| m["type"] == "user").unwrap()["uuid"].clone();
        linker.emit(json!({"type": "result", "subtype": "success", "is_error": true, "result": "rate limited", "user_message_uuids": [uuid]}).to_string());
        // Then only the failed result ended the turn, with the error and then done
        assert_eq!(drain(events).await, vec![ExecEvent::Error("rate limited".into()), ExecEvent::Done]);
    }

    #[tokio::test]
    async fn given_a_thread_whose_conversation_is_let_go_when_it_runs_again_then_its_process_was_closed_and_a_new_conversation_starts() {
        // Given a thread that ran a turn
        let (sessions, linker) = (ClaudeSessions::default(), claude_session());
        let first = first_session(&drain(sessions.turn("w1", linker.as_ref(), &request("t1", "hello", None)).await.unwrap()).await);
        // When its conversation is let go
        sessions.forget("t1");
        until("the close", || linker.kills() == 1).await;
        until("the end", || !sessions.alive("t1")).await;
        // Then the next turn, with no stored conversation, starts a new process with a new conversation
        let again = first_session(&drain(sessions.turn("w1", linker.as_ref(), &request("t1", "hello", None)).await.unwrap()).await);
        assert_ne!(again, first);
        assert_eq!(linker.starts(), 2);
    }

    #[tokio::test]
    async fn given_workspaces_with_processes_when_one_is_closed_then_its_processes_are_killed_and_nothing_else_is() {
        // Given two threads in two workspaces
        let (sessions, linker) = (ClaudeSessions::default(), claude_session());
        drain(sessions.turn("w1", linker.as_ref(), &request("t1", "hello", None)).await.unwrap()).await;
        drain(sessions.turn("w2", linker.as_ref(), &request("t2", "hello", None)).await.unwrap()).await;
        // When the first workspace is closed, and one that has none
        let (closed, none) = (sessions.close("w1"), sessions.close("w9"));
        // Then one process was killed
        until("the kill", || linker.kills() == 1).await;
        assert_eq!((closed, none, linker.starts()), (true, false, 2));
    }
}
