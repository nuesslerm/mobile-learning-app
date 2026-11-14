//! The turns in flight: which thread runs which turn, whether it was asked to stop or to go on with another prompt, and the task that follows a CLI into the log.
//! A steer ends the CLI process and starts another in the same turn, so a turn is one or more processes.
use crate::neutral::{Calls, CLI_START_FAILED, EXECUTOR_GONE};
use crate::runner::Runner;
use agent_api_agent::{FinishReason, ThreadEvent};
use rca_cli_driver::domain::{command_for_remote, Driver, ExecEvent, TurnRequest};
use rca_exec_channel::remote::{ExecutorChannel, RemoteError};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

struct Running {
    turn: String,
    interrupted: bool,
    /// The prompt the turn goes on with once the CLI process it runs has ended.
    steer: Option<String>,
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
        held.insert(thread.to_string(), Running { turn: turn.to_string(), interrupted: false, steer: None, detached: false });
        Some(Claim { in_flight: Arc::clone(self), thread: thread.to_string(), turn: turn.to_string(), ended: false })
    }

    /// Marks the thread's running turn as asked to stop, and names it; none when the thread runs no turn, or runs one other than `named`.
    pub fn interrupt(&self, thread: &str, named: Option<&str>) -> Option<String> {
        let mut held = self.held();
        let running = held.get_mut(thread).filter(|r| named.is_none_or(|n| n == r.turn))?;
        running.interrupted = true;
        Some(running.turn.clone())
    }

    /// Marks the thread's running turn to go on with `prompt` when its CLI process has ended, and names it. None when the thread runs no turn, runs
    /// one other than `named`, or the turn is being stopped.
    pub fn steer(&self, thread: &str, named: Option<&str>, prompt: &str) -> Option<String> {
        let mut held = self.held();
        let running = held.get_mut(thread).filter(|r| !r.interrupted && named.is_none_or(|n| n == r.turn))?;
        running.steer = Some(prompt.to_string());
        Some(running.turn.clone())
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

/// What a turn's claim found when its process ended.
pub struct End {
    pub interrupted: bool,
    /// The prompt of a steer that arrived: the turn is not over, and its claim stays.
    pub steer: Option<String>,
}

/// A thread's right to run a turn, held until the turn's task ends (or the turn fails to start).
pub struct Claim {
    in_flight: Arc<InFlight>,
    thread: String,
    turn: String,
    ended: bool,
}

impl Claim {
    /// Ends the turn unless a steer waits, and says which. Deciding both under one lock is what lets a steer that was accepted always run: it is
    /// either found here or refused for want of a turn.
    pub fn end(&mut self) -> End {
        let mut held = self.in_flight.held();
        if let Some(running) = held.get_mut(&self.thread).filter(|r| r.turn == self.turn) {
            if !running.interrupted {
                if let Some(steer) = running.steer.take() {
                    return End { interrupted: false, steer: Some(steer) };
                }
            }
        }
        self.ended = true;
        let gone = held.remove(&self.thread).filter(|r| r.turn == self.turn);
        End { interrupted: gone.as_ref().is_some_and(|r| r.interrupted), steer: None }
    }

    /// Ends the turn whatever waits: a steer that came too late for a turn that cannot go on is dropped.
    pub fn finish(&mut self) -> End {
        let mut held = self.in_flight.held();
        self.ended = true;
        let gone = held.remove(&self.thread).filter(|r| r.turn == self.turn);
        End { interrupted: gone.as_ref().is_some_and(|r| r.interrupted), steer: None }
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
    /// What the turn runs with: a steer runs it again with another prompt.
    pub request: TurnRequest,
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
                    Some(event) => {
                        *failed |= matches!(event, ExecEvent::Error(_));
                        self.record(calls.events(event));
                    }
                },
                _ = self.channel.closed() => {
                    self.runner.turns().interrupt(&self.thread);
                    self.record([ThreadEvent::Error { code: EXECUTOR_GONE.into(), message: RemoteError::ExecutorGone.message() }]);
                    *failed = true;
                    return false;
                }
            }
        }
    }

    /// Starts the turn's CLI again with `prompt`, in the conversation the turn's own CLI announced when it did.
    async fn relaunch(&mut self, prompt: String, session: &Option<String>) -> Result<mpsc::Receiver<ExecEvent>, String> {
        self.request.prompt = prompt;
        if session.is_some() {
            self.request.resume = session.clone();
        }
        let command = command_for_remote(self.driver, &self.request, &self.runner.mcp_url(&self.thread)).map_err(|why| why.0)?;
        self.runner.turns().launch(self.driver, &self.thread, command, self.runner.cli_cwd()).await
    }

    /// Follows the CLI into the log until the turn ends (a steer starts another CLI in the same turn), records `interrupted`, `failed` or
    /// `completed`, and lets the thread's queue run its next prompt.
    pub async fn follow(mut self, mut events: mpsc::Receiver<ExecEvent>) {
        let mut calls = Calls::default();
        let (mut failed, mut session) = (false, None);
        let end = loop {
            let alive = self.pump(&mut events, &mut calls, &mut failed, &mut session).await;
            self.keep_session(session.as_deref());
            if !alive {
                break self.claim.finish();
            }
            let end = self.claim.end();
            let Some(prompt) = end.steer.clone() else { break end };
            failed = false;
            match self.relaunch(prompt, &session).await {
                Ok(next) => events = next,
                Err(why) => {
                    self.record([ThreadEvent::Error { code: CLI_START_FAILED.into(), message: why }]);
                    failed = true;
                    break self.claim.finish();
                }
            }
        };
        let reason = if end.interrupted { FinishReason::Interrupted } else if failed { FinishReason::Failed } else { FinishReason::Completed };
        // The executor is let go before the finish is recorded: whoever sees the thread idle may start a turn that binds it again
        self.runner.executors().unbind(&self.thread);
        self.record([ThreadEvent::TurnFinished { reason }]);
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
