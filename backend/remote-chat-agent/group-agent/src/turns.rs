//! The turns in flight: which thread runs which turn, whether it was asked to stop or to go on with another prompt, and the task that follows a CLI into the log.
use crate::neutral::{Calls, EXECUTOR_GONE};
use crate::runner::Runner;
use agent_api_agent::{FinishReason, ThreadEvent};
use rca_cli_driver::domain::{Driver, ExecEvent};
use rca_exec_channel::remote::{ExecutorChannel, RemoteError};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

struct Running {
    turn: String,
    interrupted: bool,
    /// A `thread.detach` ran during the turn, so the conversation the CLI announced is not the thread's to keep.
    detached: bool,
}

/// The turns running now, by thread.
#[derive(Default)]
pub struct InFlight {
    turns: Mutex<HashMap<String, Running>>,
}

impl InFlight {
    fn held(&self) -> std::sync::MutexGuard<'_, HashMap<String, Running>> {
        self.turns.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Takes the thread for `turn`, or none when it already runs one. The claim gives the thread back when it is dropped.
    pub fn claim(self: &Arc<Self>, thread: &str, turn: &str) -> Option<Claim> {
        let mut held = self.held();
        if held.contains_key(thread) {
            return None;
        }
        held.insert(thread.to_string(), Running { turn: turn.to_string(), interrupted: false, detached: false });
        Some(Claim { in_flight: Arc::clone(self), thread: thread.to_string(), turn: turn.to_string(), ended: false })
    }

    /// Marks the thread's running turn as asked to stop, and names it; none when the thread runs no turn, or runs one other than `named`.
    pub fn interrupt(&self, thread: &str, named: Option<&str>) -> Option<String> {
        let mut held = self.held();
        let running = held.get_mut(thread).filter(|r| named.is_none_or(|n| n == r.turn))?;
        running.interrupted = true;
        Some(running.turn.clone())
    }

    /// Whether the thread's running turn was asked to stop.
    pub fn stopping(&self, thread: &str) -> bool {
        self.held().get(thread).is_some_and(|r| r.interrupted)
    }

    /// Names the thread's running turn that a steer goes on in: none when the thread runs no turn, runs one other than `named`, or the turn is being stopped.
    pub fn steer(&self, thread: &str, named: Option<&str>) -> Option<String> {
        let held = self.held();
        held.get(thread).filter(|r| !r.interrupted && named.is_none_or(|n| n == r.turn)).map(|r| r.turn.clone())
    }

    /// The turn the thread runs, as far as the group knows.
    pub fn running(&self, thread: &str) -> Option<String> {
        self.held().get(thread).map(|r| r.turn.clone())
    }

    /// Notes that the thread's CLI conversation was detached while its turn runs; whether there was a turn.
    pub fn detach(&self, thread: &str) -> bool {
        self.held().get_mut(thread).map(|r| r.detached = true).is_some()
    }

    /// Whether the conversation was detached while the thread's turn ran.
    pub fn detached(&self, thread: &str) -> bool {
        self.held().get(thread).is_some_and(|r| r.detached)
    }
}

/// What a turn's claim found when its session ended.
pub struct End {
    pub interrupted: bool,
}

/// A thread's right to run a turn, held until the turn's task ends (or the turn fails to start).
pub struct Claim {
    in_flight: Arc<InFlight>,
    thread: String,
    turn: String,
    ended: bool,
}

impl Claim {
    /// Ends the turn, and says whether it was asked to stop.
    pub fn finish(&mut self) -> End {
        let mut held = self.in_flight.held();
        self.ended = true;
        let gone = held.remove(&self.thread).filter(|r| r.turn == self.turn);
        End { interrupted: gone.as_ref().is_some_and(|r| r.interrupted) }
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        if !self.ended {
            let mut held = self.in_flight.held();
            if held.get(&self.thread).is_some_and(|r| r.turn == self.turn) {
                held.remove(&self.thread);
            }
        }
    }
}

/// Everything the task of one turn holds.
pub struct TurnTask {
    pub runner: Arc<Runner>,
    pub channel: Arc<dyn ExecutorChannel>,
    pub claim: Claim,
    pub thread: String,
    pub turn: String,
    pub driver: Driver,
}

impl TurnTask {
    /// Records events of the turn; a log that cannot be written is logged, because nobody is waiting on this task to tell.
    pub fn record(&self, events: impl IntoIterator<Item = ThreadEvent>) {
        for event in events {
            if let Err(e) = self.runner.engine().record(&self.thread, Some(&self.turn), event) {
                tracing::warn!(event = "agent_threads.record_failed", thread = %self.thread, turn = %self.turn, error = %e.0);
            }
        }
    }

    /// Follows one CLI process's events into the log until it ends; false when the executor went away and the process was stopped for it.
    async fn pump(&self, events: &mut mpsc::Receiver<ExecEvent>, calls: &mut Calls, failed: &mut bool, session: &mut Option<String>) -> bool {
        loop {
            tokio::select! {
                next = events.recv() => match next {
                    None | Some(ExecEvent::Done) => return true,
                    Some(ExecEvent::Session(id)) => {
                        // kept the moment it is announced: a restart in the middle of the turn leaves the thread able to go on with it
                        *session = Some(id);
                        self.keep_session(session.as_deref());
                    }
                    // The CLI ran at the executor, so its process ending in error while the channel is gone is the executor going away, however the two news race
                    Some(ExecEvent::Error(_)) if self.channel.is_closed() => {
                        self.runner.stop_process(&self.thread);
                        self.record([ThreadEvent::Error { code: EXECUTOR_GONE.into(), message: RemoteError::ExecutorGone.message() }]);
                        *failed = true;
                        return false;
                    }
                    Some(event) => {
                        *failed |= matches!(event, ExecEvent::Error(_));
                        self.record(calls.events(event));
                    }
                },
                _ = self.channel.closed() => {
                    self.runner.stop_process(&self.thread);
                    self.record([ThreadEvent::Error { code: EXECUTOR_GONE.into(), message: RemoteError::ExecutorGone.message() }]);
                    *failed = true;
                    return false;
                }
            }
        }
    }

    /// Follows the session's events into the log until the turn ends, records `interrupted`, `failed` or `completed`, and lets the thread's queue run its next
    /// prompt.
    pub async fn follow(mut self, mut events: mpsc::Receiver<ExecEvent>) {
        let mut calls = Calls::default();
        let (mut failed, mut session) = (false, None);
        self.pump(&mut events, &mut calls, &mut failed, &mut session).await;
        self.keep_session(session.as_deref());
        let end = self.claim.finish();
        let reason = if end.interrupted { FinishReason::Interrupted } else if failed { FinishReason::Failed } else { FinishReason::Completed };
        // The executor is let go before the finish is recorded: whoever sees the thread idle may start a turn that binds it again
        self.runner.executors().unbind(&self.thread);
        self.record([ThreadEvent::TurnFinished { reason, cause: None }]);
        self.runner.release_requests(&self.thread);
        self.runner.start_next_later(&self.thread);
    }

    /// The thread keeps the conversation its CLI announced, while the turn still runs so that a detach that follows the turn's end finds it. Not when a
    /// detach already ran during the turn: the conversation is the one the person let go of.
    fn keep_session(&self, session: Option<&str>) {
        let Some(id) = session.filter(|_| !self.runner.in_flight().detached(&self.thread)) else { return };
        if let Err(e) = self.runner.engine().set_session(&self.thread, self.driver.id(), &self.turn, id) {
            tracing::warn!(event = "agent_threads.session_not_kept", thread = %self.thread, error = %e.0);
        }
    }
}
