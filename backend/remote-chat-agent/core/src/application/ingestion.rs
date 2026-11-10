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
            dispatch(
                engine,
                Command {
                    command_id: format!("provider:stream-lost:{}", engine.new_id()),
                    thread_id: thread_id.to_string(),
                    body: Body::SessionSet { session: Session { cli: provider.clone(), provider_session_id: None, status: "error".into(), active_turn_id: None, last_error: Some(reason) } },
                },
            );
        }
    }
    tracing::info!(thread_id, cli = %provider, "ingestion.ended");
}
