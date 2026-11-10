//! The setup commands of a worktree as the service keeps them: one run per worktree, its events in order (numbered from 1), and whether it
//! finished. A run is driven by one task that calls the executor's `worktree_setup`; cancelling drops that call, which sends `cancel` to the
//! executor, and ends the log with a `setup.finished` that says so.
use agent_api_proto::Event;
use agent_api_workspace::worktrees::setup_event;
use rca_exec_channel::remote::{ExecutorChannel, RemoteError};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::task::AbortHandle;

/// How long `worktree.create` waits for the executor to start the setup before it answers.
const START_WAIT: Duration = Duration::from_secs(10);

#[derive(Default)]
struct Log {
    events: Vec<Event>,
    finished: bool,
    abort: Option<AbortHandle>,
}

pub(crate) struct Run {
    workspace: String,
    log: Mutex<Log>,
    changed: watch::Sender<usize>,
}

/// What a subscriber needs after `after_seq`: the events, and whether the run is over.
pub(crate) struct Slice {
    pub events: Vec<Event>,
    pub finished: bool,
}

impl Run {
    fn log(&self) -> std::sync::MutexGuard<'_, Log> {
        self.log.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn push(&self, raw: &Value) {
        let count = {
            let mut log = self.log();
            if log.finished {
                return;
            }
            let event = setup_event(&self.workspace, log.events.len() as u64 + 1, raw);
            log.finished = event.kind == "setup.finished";
            log.events.push(event);
            log.events.len()
        };
        self.changed.send_replace(count);
    }

    /// Ends the log with `finished` unless the executor ended it.
    fn end(&self, finished: Value) {
        self.push(&finished);
    }

    pub fn since(&self, after_seq: u64) -> Slice {
        let log = self.log();
        Slice { events: log.events.iter().filter(|e| e.seq > after_seq).cloned().collect(), finished: log.finished }
    }

    pub fn last_seq(&self) -> u64 {
        self.log().events.len() as u64
    }

    pub fn running(&self) -> bool {
        !self.log().finished
    }

    pub fn watch(&self) -> watch::Receiver<usize> {
        self.changed.subscribe()
    }

    /// Stops the setup; false when it had finished.
    pub fn cancel(&self) -> bool {
        if !self.running() {
            return false;
        }
        if let Some(abort) = self.log().abort.take() {
            abort.abort();
        }
        self.end(json!({"kind": "setup.finished", "ok": false, "cancelled": true}));
        true
    }

    /// The first event, which says whether the executor started the setup or why it did not.
    pub fn first(&self) -> Option<Event> {
        self.log().events.first().cloned()
    }
}

/// Starts `commands` in the worktree `name` at `channel` and returns once the executor has said it started (or why it did not), or after
/// `START_WAIT`. The run goes on in a task of its own.
pub(crate) async fn start(channel: Arc<dyn ExecutorChannel>, workspace: &str, name: &str, commands: &[String]) -> Arc<Run> {
    let run = Arc::new(Run { workspace: workspace.to_string(), log: Mutex::default(), changed: watch::channel(0).0 });
    let mut seen = run.watch();
    let args = json!({"name": name, "commands": commands});
    let driver = run.clone();
    let task = tokio::spawn(async move {
        let (tx, mut rx) = mpsc::unbounded_channel::<Value>();
        let call = channel.call_with_events(exec_git::WORKTREE_SETUP, args, tx);
        tokio::pin!(call);
        let outcome = loop {
            tokio::select! {
                Some(event) = rx.recv() => driver.push(&event),
                done = &mut call => break done,
            }
        };
        while let Ok(event) = rx.try_recv() {
            driver.push(&event);
        }
        match outcome {
            Ok(done) => driver.end(json!({"kind": "setup.finished", "ok": done.result["ok"].as_bool().unwrap_or(false)})),
            Err(e) => driver.end(failed(&e)),
        }
    });
    run.log().abort = Some(task.abort_handle());
    let _ = tokio::time::timeout(START_WAIT, async {
        while *seen.borrow_and_update() == 0 {
            if seen.changed().await.is_err() {
                break;
            }
        }
    })
    .await;
    run
}

fn failed(e: &RemoteError) -> Value {
    json!({"kind": "setup.finished", "ok": false, "error": {"code": e.code(), "message": e.message()}})
}
