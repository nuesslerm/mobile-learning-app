//! The per-thread loop that reads an agent's event stream while a turn runs and feeds what it says into the engine. One loop per
//! thread serves every turn queued on it; it reads only the events of the thread's own session, ends when the agent goes idle with
//! no further turn running on the thread (or is cancelled), and always closes its subscription.

use super::engine::Engine;
use super::ports::{Backend, EventStream};
use crate::domain::decider::has_open_blocking_request;
use crate::domain::ingest::ingest;
use crate::domain::translate::Translator;
use crate::domain::types::*;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// What differs between the engine behind the chat stream and the one behind the thread view.
#[derive(Clone, Copy)]
pub struct Options {
    /// Tool activities carry their input and output.
    pub tool_data: bool,
    /// A loop that has not ended after this long is cut: the session ends in error and the agent is told to abort.
    pub turn_deadline: Option<Duration>,
}

struct Active {
    closed: AtomicBool,
    cancel: CancellationToken,
}

#[derive(Default)]
pub struct Ingestions {
    active: Mutex<HashMap<String, Arc<Active>>>,
}

pub struct Wiring {
    pub engine: Arc<Engine>,
    pub backend: Arc<dyn Backend>,
    pub options: Options,
}

impl Ingestions {
    /// Opens the subscription for a turn about to be sent (before the prompt, so no early event is missed), or lets the turn
    /// join the loop that is already running for the thread: the agent folds queued prompts into one run that ends in one idle.
    pub async fn begin(self: &Arc<Self>, wiring: &Arc<Wiring>, thread_id: &str, directory: &str) -> Result<(), String> {
        {
            let active = self.active.lock().unwrap_or_else(|e| e.into_inner());
            if active.get(thread_id).is_some_and(|a| !a.closed.load(Ordering::SeqCst)) {
                return Ok(());
            }
        }
        // One reactor worker serves a thread at a time, so no other `begin` for it can run while the stream is being opened.
        let stream = wiring.backend.open_events(directory).await?;
        let entry = Arc::new(Active { closed: AtomicBool::new(false), cancel: CancellationToken::new() });
        self.active.lock().unwrap_or_else(|e| e.into_inner()).insert(thread_id.to_string(), Arc::clone(&entry));
        let (me, wiring, key) = (Arc::clone(self), Arc::clone(wiring), thread_id.to_string());
        tokio::spawn(async move { run(&me, &wiring, &entry, stream, &key).await });
        Ok(())
    }

    /// Ends the loop of a thread now, whatever is pending (the request gave up, or the turn could not be sent).
    pub fn cancel(&self, thread_id: &str) {
        let entry = self.active.lock().unwrap_or_else(|e| e.into_inner()).remove(thread_id);
        if let Some(entry) = entry {
            entry.closed.store(true, Ordering::SeqCst);
            entry.cancel.cancel();
        }
    }

    /// A turn completed; true when the thread has no turn running besides it and the loop should end. Decided under the lock `begin`
    /// takes, so a turn that arrives after the loop ended opens a loop of its own.
    fn settle(&self, thread_id: &str, entry: &Arc<Active>, engine: &Engine) -> bool {
        let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        let running = engine.with_thread(thread_id, |t| t.and_then(|t| t.session.as_ref()).is_some_and(|s| s.status == "running")).unwrap_or(false);
        if running {
            return false;
        }
        if active.get(thread_id).is_some_and(|a| Arc::ptr_eq(a, entry)) {
            active.remove(thread_id);
        }
        true
    }

    fn forget(&self, thread_id: &str, entry: &Arc<Active>) {
        let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        if active.get(thread_id).is_some_and(|a| Arc::ptr_eq(a, entry)) {
            active.remove(thread_id);
        }
    }
}

fn dispatch(engine: &Engine, command: Command) {
    if let Err(e) = engine.dispatch(command, "provider") {
        tracing::warn!(error = %e, "ingestion.dispatch_failed");
    }
}

async fn run(ingestions: &Arc<Ingestions>, wiring: &Arc<Wiring>, entry: &Arc<Active>, mut stream: EventStream, thread_id: &str) {
    let engine = &wiring.engine;
    let provider = wiring.backend.provider().to_string();
    let session = wiring.backend.session_for(thread_id);
    let mut translator = Translator::new(thread_id, &provider, wiring.options.tool_data, engine.clock(), engine.ids());
    let mut ended_early: Option<String> = None;
    let mut timed_out = false;
    // Per turn: restarted when a queued turn begins, and held still while an approval or user-input answer is awaited.
    let budget = wiring.options.turn_deadline;
    let mut deadline_at = budget.map(|d| tokio::time::Instant::now() + d);
    let mut parked_since: Option<tokio::time::Instant> = None;
    tracing::info!(thread_id, cli = %provider, "ingestion.started");
    loop {
        let expiry = async {
            match deadline_at.filter(|_| parked_since.is_none()) {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        let next = tokio::select! {
            () = entry.cancel.cancelled() => break,
            () = expiry => {
                ended_early = Some(format!("hard_timeout: turn exceeded {}s", budget.map_or(0, |d| d.as_secs())));
                timed_out = true;
                break;
            }
            next = stream.next() => next,
        };
        let event = match next {
            Some(Ok(event)) => event,
            None => {
                ended_early = Some("opencode ended its event stream".into());
                break;
            }
            Some(Err(e)) => {
                if !entry.closed.load(Ordering::SeqCst) {
                    ended_early = Some(format!("opencode event stream failed: {e}"));
                }
                break;
            }
        };
        // Everything the directory's stream carries, from every session; only this thread's own is its turn.
        if let (Some(theirs), Some(ours)) = (event.session_id(), session.as_deref()) {
            if theirs != ours {
                continue;
            }
        }
        let active_turn = engine.with_thread(thread_id, |t| t.and_then(|t| t.session.as_ref()).filter(|s| s.status == "running").and_then(|s| s.active_turn_id.clone())).unwrap_or(None);
        let runtimes = translator.translate(&event, active_turn.as_deref());
        let commands = engine.with_thread(thread_id, |thread| runtimes.iter().flat_map(|rt| ingest(thread, rt, &|| engine.new_id())).collect::<Vec<Command>>()).unwrap_or_default();
        for command in commands {
            dispatch(engine, command);
        }
        match event.kind.as_str() {
            "session.idle" | "session.error" => {
                if ingestions.settle(thread_id, entry, engine) {
                    break;
                }
                deadline_at = budget.map(|d| tokio::time::Instant::now() + d);
                parked_since = None;
            }
            _ => {}
        }
        let blocked = engine.with_thread(thread_id, |t| t.is_some_and(has_open_blocking_request)).unwrap_or(false);
        match (blocked, parked_since) {
            (true, None) => parked_since = Some(tokio::time::Instant::now()),
            (false, Some(since)) => {
                deadline_at = deadline_at.map(|at| at + since.elapsed());
                parked_since = None;
            }
            _ => {}
        }
    }
    entry.closed.store(true, Ordering::SeqCst);
    entry.cancel.cancel();
    drop(stream);
    ingestions.forget(thread_id, entry);
    if timed_out {
        if let Err(e) = wiring.backend.interrupt_turn(thread_id).await {
            tracing::warn!(thread_id, error = %e, "ingestion.abort_failed");
        }
    }
    if let Some(reason) = ended_early {
        let settled = engine.with_thread(thread_id, |t| t.and_then(|t| t.session.as_ref()).is_none_or(|s| s.status != "running")).unwrap_or(true);
        if !settled {
            tracing::warn!(thread_id, reason = %reason, "ingestion.stream_lost");
            // the conversation is still there (the backend's, or the one the thread already names): losing the stream must not make the next turn start a new one
            // the lost stream keeps the session id: opencode still holds the conversation, and the next turn must go on in it
            let kept = wiring.backend.session_for(thread_id).or_else(|| engine.with_thread(thread_id, |t| t.and_then(|t| t.session.as_ref()).and_then(|s| s.provider_session_id.clone())).unwrap_or(None));
            dispatch(
                engine,
                Command {
                    command_id: format!("provider:stream-lost:{}", engine.new_id()),
                    thread_id: thread_id.to_string(),
                    body: Body::SessionSet { session: Session { cli: provider.clone(), provider_session_id: kept, status: "error".into(), active_turn_id: None, last_error: Some(reason) } },
                },
            );
        }
    }
    tracing::info!(thread_id, cli = %provider, "ingestion.ended");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::SendTurn;
    use crate::application::reactor::Orchestrator;
    use crate::application::store::MemoryStore;
    use crate::domain::agent::AgentEvent;
    use crate::testing::{CountingIds, FixedClock};
    use async_trait::async_trait;
    use serde_json::{Map, Value};
    use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

    /// An opencode that holds one session per thread, whose event stream the test cuts on demand.
    #[derive(Default)]
    struct Opencode {
        sessions: Mutex<HashMap<String, String>>,
        started: Mutex<Vec<String>>,
        adopted: Mutex<Vec<(String, String)>>,
        sent: Mutex<Vec<Option<String>>>,
        feed: Mutex<Option<UnboundedSender<AgentEvent>>>,
    }

    #[async_trait]
    impl Backend for Opencode {
        fn provider(&self) -> &str {
            "opencode"
        }
        async fn start_session(&self, thread_id: &str, _runtime_mode: &str) -> Result<String, String> {
            self.started.lock().unwrap().push(thread_id.into());
            self.sessions.lock().unwrap().insert(thread_id.into(), "ses_1".into());
            Ok("ses_1".into())
        }
        async fn send_turn(&self, input: &SendTurn<'_>) -> Result<(), String> {
            self.sent.lock().unwrap().push(self.sessions.lock().unwrap().get(input.thread_id).cloned());
            Ok(())
        }
        async fn interrupt_turn(&self, _thread_id: &str) -> Result<(), String> {
            Ok(())
        }
        async fn respond_to_request(&self, _thread_id: &str, _request_id: &str, _decision: &str) -> Result<(), String> {
            Ok(())
        }
        async fn respond_to_user_input(&self, _thread_id: &str, _request_id: &str, _answers: &Map<String, Value>) -> Result<(), String> {
            Ok(())
        }
        async fn stop_session(&self, _thread_id: &str) -> Result<(), String> {
            Ok(())
        }
        fn session_for(&self, thread_id: &str) -> Option<String> {
            self.sessions.lock().unwrap().get(thread_id).cloned()
        }
        fn adopt_session(&self, thread_id: &str, session_id: &str) {
            self.adopted.lock().unwrap().push((thread_id.into(), session_id.into()));
            self.sessions.lock().unwrap().insert(thread_id.into(), session_id.into());
        }
        async fn open_events(&self, _directory: &str) -> Result<EventStream, String> {
            let (tx, rx) = unbounded_channel();
            *self.feed.lock().unwrap() = Some(tx);
            Ok(Box::pin(futures_util::stream::unfold(rx, |mut rx| async { rx.recv().await.map(|event| (Ok(event), rx)) })))
        }
    }

    fn orchestrator_over(store: Arc<MemoryStore>, opencode: &Arc<Opencode>) -> (Arc<Engine>, Arc<Orchestrator>) {
        let engine = Arc::new(Engine::new(Box::new(store), "/work".into(), Arc::new(FixedClock(1_700_000_000_000)), Arc::new(CountingIds::default())));
        let options = Options { tool_data: false, turn_deadline: None };
        let backend: Arc<dyn Backend> = opencode.clone();
        (engine.clone(), Orchestrator::new(engine, vec![backend], options))
    }

    fn start_turn(engine: &Engine, message: &str) {
        let body = Body::TurnStart { message_id: message.into(), text: "go".into(), attachments: None, model_selection: None, runtime_mode: "full-access".into(), interaction_mode: "default".into(), source_proposed_plan: None, bootstrap: None };
        let receipt = engine.dispatch(Command { command_id: format!("c-{message}"), thread_id: "t1".into(), body }, "client").unwrap();
        assert!(receipt.accepted, "{:?}", receipt.error);
    }

    fn session_of(engine: &Engine) -> Option<Session> {
        engine.with_thread("t1", |t| t.and_then(|t| t.session.clone())).unwrap()
    }

    async fn until(what: &str, mut done: impl FnMut() -> bool) {
        for _ in 0..500 {
            if done() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("{what}");
    }

    #[tokio::test]
    async fn given_an_opencode_thread_with_a_session_when_its_event_stream_is_lost_then_the_thread_keeps_the_session_in_error_and_the_next_turn_goes_on_in_it() {
        // Given an opencode thread whose turn runs in session ses_1 and whose event stream is open
        let store: Arc<MemoryStore> = Arc::default();
        let opencode = Arc::new(Opencode::default());
        let (engine, _orchestrator) = orchestrator_over(store.clone(), &opencode);
        let selection = ModelSelection { cli: "opencode".into(), provider_id: None, model_id: None };
        let create = Body::Create { title: "t".into(), cwd: Some("/work".into()), model_selection: selection, runtime_mode: "full-access".into(), interaction_mode: "default".into(), branch: None, worktree_path: None };
        assert!(engine.dispatch(Command { command_id: "c-create".into(), thread_id: "t1".into(), body: create }, "client").unwrap().accepted);
        start_turn(&engine, "m1");
        until("the turn runs in its session", || opencode.feed.lock().unwrap().is_some() && session_of(&engine).is_some_and(|s| s.status == "running" && s.provider_session_id.as_deref() == Some("ses_1"))).await;
        // When the stream is lost while the turn still runs
        opencode.feed.lock().unwrap().take();
        until("the thread's session errors", || session_of(&engine).is_some_and(|s| s.status == "error")).await;
        // Then the thread keeps its provider session id
        assert_eq!(session_of(&engine).and_then(|s| s.provider_session_id).as_deref(), Some("ses_1"));
        // And after a service restart that loses the backend's own map, the next turn is sent in that session without starting another
        let restarted = Arc::new(Opencode::default());
        let (engine, _orchestrator) = orchestrator_over(store, &restarted);
        start_turn(&engine, "m2");
        until("the next turn is sent", || !restarted.sent.lock().unwrap().is_empty()).await;
        assert_eq!(*restarted.sent.lock().unwrap(), [Some("ses_1".to_string())]);
        assert_eq!((restarted.started.lock().unwrap().len(), restarted.adopted.lock().unwrap().clone()), (0, vec![("t1".to_string(), "ses_1".to_string())]));
    }
}
