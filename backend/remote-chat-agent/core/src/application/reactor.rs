//! Engine events into backend calls (`orchestration/reactor.ts`):
//!
//!   turn-start-requested         create the session if the thread has none, open the event stream, mark the turn running, prompt
//!   turn-interrupt-requested     abort the session's turn
//!   approval-response-requested  answer the permission request
//!   user-input-response-requested  answer the question
//!   session-stop-requested       abort and forget the session
//!
//! Events of one thread are handled in order, by one worker that exists only while it has something to do; threads do not wait for
//! each other. A turn that cannot be started ends in an error session, so whoever waits on it hears of it.

use super::engine::{Engine, StaleTurn};
use super::ingestion::{Ingestions, Options, Wiring};
use super::ports::{Backend, SendTurn};
use crate::domain::types::*;
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::runtime::Handle;
use tokio::sync::oneshot;

type Outcome = Result<(), String>;

/// How long a caller waits to hear whether the backend call of a command it dispatched worked.
const OUTCOME_WAIT: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Queue {
    events: VecDeque<Event>,
    running: bool,
}

pub struct Orchestrator {
    pub engine: Arc<Engine>,
    backends: HashMap<String, Arc<dyn Backend>>,
    pub ingestions: Arc<Ingestions>,
    options: Options,
    queues: Mutex<HashMap<String, Queue>>,
    /// Threads whose recorded session was already offered to the backend: a session stopped in this process is not taken back.
    adopted: Mutex<HashSet<String>>,
    /// File parts waiting to go out with the turn whose message id they were filed under.
    files: Mutex<HashMap<String, Vec<Value>>>,
    /// Callers waiting for the backend call of a command, by command id.
    outcomes: Mutex<HashMap<String, oneshot::Sender<Outcome>>>,
    me: Weak<Orchestrator>,
    runtime: Handle,
}

impl Orchestrator {
    pub fn new(engine: Arc<Engine>, backends: Vec<Arc<dyn Backend>>, options: Options) -> Arc<Self> {
        let orchestrator = Arc::new_cyclic(|me| Self {
            engine: Arc::clone(&engine),
            backends: backends.into_iter().map(|b| (b.provider().to_string(), b)).collect(),
            ingestions: Arc::new(Ingestions::default()),
            options,
            queues: Mutex::new(HashMap::new()),
            adopted: Mutex::new(HashSet::new()),
            files: Mutex::new(HashMap::new()),
            outcomes: Mutex::new(HashMap::new()),
            me: me.clone(),
            runtime: Handle::current(),
        });
        let weak = Arc::downgrade(&orchestrator);
        engine.set_on_events(Arc::new(move |events: &[Event]| {
            if let Some(orchestrator) = weak.upgrade() {
                orchestrator.submit(events);
            }
        }));
        orchestrator
    }

    /// Files a picture to be sent with the turn that starts with this message; it must be filed before the turn is dispatched.
    pub fn attach_files(&self, message_id: &str, files: Vec<Value>) {
        self.files.lock().unwrap_or_else(|e| e.into_inner()).insert(message_id.to_string(), files);
    }

    /// Takes back files filed for a turn that was never dispatched.
    pub fn discard_files(&self, message_id: &str) {
        self.files.lock().unwrap_or_else(|e| e.into_inner()).remove(message_id);
    }

    /// Dispatches a command and, when the engine accepts it, waits (briefly) for the backend call it leads to: Err when that call
    /// failed. A command the engine rejects, or a call that is still running after the wait, is the receipt as it is.
    pub async fn dispatch_awaiting(&self, command: Command, actor: &str) -> Result<Receipt, String> {
        let command_id = command.command_id.clone();
        let (tx, rx) = oneshot::channel();
        self.outcomes.lock().unwrap_or_else(|e| e.into_inner()).insert(command_id.clone(), tx);
        let receipt = self.engine.dispatch(command, actor);
        if !receipt.as_ref().is_ok_and(|r| r.accepted) {
            self.outcomes.lock().unwrap_or_else(|e| e.into_inner()).remove(&command_id);
            return receipt;
        }
        match tokio::time::timeout(OUTCOME_WAIT, rx).await {
            Ok(Ok(Err(e))) => Err(e),
            _ => {
                self.outcomes.lock().unwrap_or_else(|e| e.into_inner()).remove(&command_id);
                receipt
            }
        }
    }

    /// After a restart: tells the backend which session each interrupted turn ran in and aborts it there, so the agent agrees with
    /// the engine. Best effort.
    pub fn recover(&self, stale: Vec<StaleTurn>) {
        for turn in stale {
            let Some(me) = self.me.upgrade() else { return };
            self.runtime.spawn(async move {
                let Ok(Some(thread)) = me.engine.get_thread(&turn.thread_id) else { return };
                let Some(backend) = me.backends.get(&thread.model_selection.cli) else { return };
                me.adopt(&thread, backend);
                if let Err(e) = backend.interrupt_turn(&turn.thread_id).await {
                    tracing::warn!(thread_id = %turn.thread_id, error = %e, "reactor.recover.abort_failed");
                }
            });
        }
    }

    /// The first time a thread is looked at in this process: its recorded session becomes the backend's, when the backend lost it.
    fn adopt(&self, thread: &Thread, backend: &Arc<dyn Backend>) {
        if !self.adopted.lock().unwrap_or_else(|e| e.into_inner()).insert(thread.id.clone()) || backend.session_for(&thread.id).is_some() {
            return;
        }
        if let Some(session) = thread.session.as_ref().and_then(|s| s.provider_session_id.as_deref()) {
            backend.adopt_session(&thread.id, session);
        }
    }

    pub fn backend(&self, cli: &str) -> Option<&Arc<dyn Backend>> {
        self.backends.get(cli)
    }

    pub fn clis(&self) -> Vec<String> {
        let mut clis: Vec<String> = self.backends.keys().cloned().collect();
        clis.sort();
        clis
    }

    fn submit(&self, events: &[Event]) {
        for event in events {
            if !matches!(
                event.kind.as_str(),
                "thread.turn-start-requested" | "thread.turn-interrupt-requested" | "thread.approval-response-requested" | "thread.user-input-response-requested" | "thread.session-stop-requested"
            ) {
                continue;
            }
            let start = {
                let mut queues = self.queues.lock().unwrap_or_else(|e| e.into_inner());
                let queue = queues.entry(event.thread_id.clone()).or_default();
                queue.events.push_back(event.clone());
                !std::mem::replace(&mut queue.running, true)
            };
            if start {
                let Some(me) = self.me.upgrade() else { continue };
                let thread_id = event.thread_id.clone();
                self.runtime.spawn(async move { me.work(&thread_id).await });
            }
        }
    }

    async fn work(&self, thread_id: &str) {
        loop {
            let next = {
                let mut queues = self.queues.lock().unwrap_or_else(|e| e.into_inner());
                let queue = queues.entry(thread_id.to_string()).or_default();
                let next = queue.events.pop_front();
                if next.is_none() {
                    queues.remove(thread_id);
                }
                next
            };
            let Some(event) = next else { return };
            self.handle(&event).await;
        }
    }

    async fn handle(&self, event: &Event) {
        let thread_id = event.thread_id.as_str();
        let text = |name: &str| event.payload.get(name).and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let result = match event.kind.as_str() {
            "thread.turn-start-requested" => self.on_turn_start(event).await,
            "thread.turn-interrupt-requested" => match self.backend_of(thread_id) {
                Ok(Some(backend)) => backend.interrupt_turn(thread_id).await,
                other => other.map(|_| ()),
            },
            "thread.approval-response-requested" => match self.backend_of(thread_id) {
                Ok(Some(backend)) => backend.respond_to_request(thread_id, &text("requestId"), &text("decision")).await,
                other => other.map(|_| ()),
            },
            "thread.user-input-response-requested" => match self.backend_of(thread_id) {
                Ok(Some(backend)) => {
                    let answers = event.payload.get("answers").and_then(|v| v.as_object()).cloned().unwrap_or_default();
                    backend.respond_to_user_input(thread_id, &text("requestId"), &answers).await
                }
                other => other.map(|_| ()),
            },
            "thread.session-stop-requested" => {
                let result = match self.backend_of(thread_id) {
                    Ok(Some(backend)) => backend.stop_session(thread_id).await,
                    other => other.map(|_| ()),
                };
                self.ingestions.cancel(thread_id);
                result
            }
            _ => Ok(()),
        };
        if let Err(e) = &result {
            tracing::warn!(event_type = %event.kind, thread_id = %event.thread_id, error = %e, "reactor.handle.failed");
            if event.kind != "thread.turn-start-requested" {
                self.record_failure(event, e);
            }
        }
        let waiting = event.command_id.as_ref().and_then(|id| self.outcomes.lock().unwrap_or_else(|e| e.into_inner()).remove(id));
        if let Some(waiting) = waiting {
            let _ = waiting.send(result);
        }
    }

    /// A command the engine accepted whose backend call failed: an error activity on the thread, so whoever watches it hears.
    fn record_failure(&self, event: &Event, message: &str) {
        let activity = Activity {
            id: self.engine.new_id(),
            tone: "error".into(),
            kind: "command-failed".into(),
            summary: format!("{} failed", event.kind.trim_start_matches("thread.").trim_end_matches("-requested")),
            payload: serde_json::json!({ "commandId": event.command_id, "message": message }),
            turn_id: None,
            sequence: None,
            created_at: self.engine.clock().now_iso(),
        };
        let command = Command { command_id: format!("provider:command-failed:{}", self.engine.new_id()), thread_id: event.thread_id.clone(), body: Body::ActivityAppend { activity } };
        if let Err(e) = self.engine.dispatch(command, "provider") {
            tracing::warn!(thread_id = %event.thread_id, error = %e, "reactor.record_failure.dispatch_failed");
        }
    }

    /// The backend of the thread's chosen CLI; None when the engine has never seen the thread or has no such backend.
    fn backend_of(&self, thread_id: &str) -> Result<Option<Arc<dyn Backend>>, String> {
        let Some(thread) = self.engine.get_thread(thread_id)? else { return Ok(None) };
        let backend = self.backends.get(&thread.model_selection.cli).cloned();
        if let Some(backend) = &backend {
            self.adopt(&thread, backend);
        }
        Ok(backend)
    }

    /// An error session: what a turn that cannot run leaves behind, so the stream waiting on it ends.
    fn fail_turn(&self, thread: &Thread, backend: &Arc<dyn Backend>, message: String) {
        let session = Session {
            cli: thread.model_selection.cli.clone(),
            provider_session_id: backend.session_for(&thread.id),
            status: "error".into(),
            active_turn_id: None,
            last_error: Some(message),
        };
        let command = Command { command_id: format!("provider:turn-failed:{}", self.engine.new_id()), thread_id: thread.id.clone(), body: Body::SessionSet { session } };
        if let Err(e) = self.engine.dispatch(command, "provider") {
            tracing::warn!(thread_id = %thread.id, error = %e, "reactor.fail_turn.dispatch_failed");
        }
    }

    async fn on_turn_start(&self, event: &Event) -> Result<(), String> {
        // Taken first: a turn that cannot start must not leave its files behind.
        let message_id = event.payload.get("messageId").and_then(|v| v.as_str()).unwrap_or_default();
        let files = self.files.lock().unwrap_or_else(|e| e.into_inner()).remove(message_id).unwrap_or_default();
        let Some(thread) = self.engine.get_thread(&event.thread_id)? else { return Ok(()) };
        let cli = thread.model_selection.cli.clone();
        let Some(backend) = self.backends.get(&cli) else {
            if let Some(backend) = self.backends.values().next() {
                self.fail_turn(&thread, backend, format!("this service has no {cli} backend"));
            }
            return Ok(());
        };
        self.adopt(&thread, backend);
        if backend.session_for(&thread.id).is_none() {
            if let Err(e) = backend.start_session(&thread.id, &thread.runtime_mode).await {
                tracing::warn!(thread_id = %thread.id, cli = %cli, error = %e, "reactor.start_session.failed");
                self.fail_turn(&thread, backend, e);
                return Ok(());
            }
        }
        let Some(message) = thread.messages.iter().find(|m| m.id == message_id) else { return Ok(()) };

        let wiring = Arc::new(Wiring { engine: Arc::clone(&self.engine), backend: Arc::clone(backend), options: self.options });
        if let Err(e) = self.ingestions.begin(&wiring, &thread.id, &thread.cwd).await {
            self.fail_turn(&thread, backend, format!("cannot open the {cli} event stream: {e}"));
            return Ok(());
        }
        // Running before the prompt goes out: a turn that settles at once must not be overwritten by a late running.
        let running = Session {
            cli: cli.clone(),
            provider_session_id: backend.session_for(&thread.id),
            status: "running".into(),
            active_turn_id: Some(format!("turn_{}", self.engine.random_hex(5))),
            last_error: None,
        };
        let mark = Command { command_id: format!("provider:turn-running:{}", self.engine.new_id()), thread_id: thread.id.clone(), body: Body::SessionSet { session: running } };
        self.engine.dispatch(mark, "provider")?;

        let send = SendTurn { thread_id: &thread.id, text: &message.text, model: &thread.model_selection, runtime_mode: &thread.runtime_mode, interaction_mode: &thread.interaction_mode, files: &files };
        if let Err(e) = backend.send_turn(&send).await {
            self.ingestions.cancel(&thread.id);
            self.fail_turn(&thread, backend, e);
        }
        Ok(())
    }
}
