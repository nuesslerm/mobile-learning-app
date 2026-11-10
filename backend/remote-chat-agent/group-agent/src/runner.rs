//! What the groups share to run a thread's turns: the CLI of a turn and its executor, the turns in flight and the requests a turn waits on.
//! Starting a turn, steering, stopping one and starting the queue's next prompt are all here.
use crate::neutral::CLI_START_FAILED;
use crate::port::{Command, ThreadEngine};
use crate::refusal::Refusal;
use crate::requests::Waiting;
use crate::turns::{Claim, InFlight, TurnTask};
use agent_api_agent::lifecycle::LifecycleEvent;
use agent_api_agent::queue::QueueItem;
use agent_api_agent::{code, message, FinishReason, Role, Status, ThreadEvent, ThreadView};
use agent_api_proto::Event;
use rca_cli_driver::application::ports::Spawner;
use rca_cli_driver::application::turns::Turns;
use rca_cli_driver::domain::{command_for_remote, Driver, TurnRequest};
use rca_exec_channel::remote::ExecutorChannel;
use rca_exec_channel::Executors;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

/// Where a CLI of this host reaches the tool server of a thread: the service's own `/mcp/<thread>` on its loopback port, which is known only
/// once the service listens.
pub trait McpEndpoint: Send + Sync {
    fn url(&self, thread: &str) -> String;
}

/// What a turn runs with.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnInput {
    pub workspace: String,
    pub driver: String,
    pub model: Option<String>,
    pub prompt: String,
    pub runtime_mode: Option<String>,
    pub interaction_mode: Option<String>,
}

/// What a turn needs to know of its thread before it exists as far as the engine goes: the model it defaults to, and whether it already runs one.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Known {
    pub model: Option<String>,
    pub running: bool,
}

impl Known {
    pub fn of(thread: &ThreadView) -> Known {
        Known { model: thread.model.clone(), running: thread.status == Status::Running }
    }
}

/// A turn that can start: the driver is one this service runs, its workspace has an executor, the thread is free and holds the claim. Nothing is
/// recorded and nothing runs until `launch`; a `Prepared` that is dropped gives the thread back.
pub struct Prepared {
    runner: Arc<Runner>,
    thread: String,
    turn: String,
    driver: Driver,
    channel: Arc<dyn ExecutorChannel>,
    workspace: String,
    claim: Claim,
    request: TurnRequest,
    /// What the person said, as the log records it.
    said: String,
    merge: Option<(String, String)>,
}

impl Prepared {
    pub fn turn(&self) -> &str {
        &self.turn
    }

    /// Gives the turn another prompt than the one it was prepared with (a retry learns its prompt from the engine's decision).
    pub fn say(&mut self, prompt: &str) {
        self.said = prompt.to_string();
        self.request.prompt = with_context(self.merge.as_ref(), prompt);
    }

    /// Tells the CLI, after the prompt, which files of the workspace the person attached; the log still records only what the person said.
    pub fn note_files(&mut self, paths: &[String]) {
        let list: String = paths.iter().map(|p| format!("\n- {p}")).collect();
        self.request.prompt = format!("{}\n\nFiles attached to this message, in the workspace:{list}", self.request.prompt);
    }

    /// Records the turn's start and its prompt, and runs its CLI; the turn is followed on a task of its own. A CLI that cannot start ends the turn
    /// failed, in the log, as one that fails after it began does.
    pub async fn launch(self) -> Result<String, Refusal> {
        let Prepared { runner, thread, turn, driver, channel, workspace, claim, request, said, merge } = self;
        let command = command_for_remote(driver, &request, &runner.mcp.url(&thread)).map_err(|why| Refusal::new(code::UNKNOWN_DRIVER, format!("{}: {}", message::unknown_driver(driver.id()), why.0)))?;
        for event in [ThreadEvent::TurnStarted {}, ThreadEvent::MessageDelta { role: Role::User, text: said }] {
            runner.engine.record(&thread, Some(&turn), event)?;
        }
        if let Some((source, _)) = &merge {
            runner.engine.record_value(&thread, Some(&turn), serde_json::to_value(LifecycleEvent::MergeConsumed { source: source.clone() }).unwrap_or_default())?;
        }
        runner.executors.bind(&thread, &workspace);
        let task = TurnTask { runner: Arc::clone(&runner), channel, claim, thread: thread.clone(), turn: turn.clone(), driver, request };
        crate::checkpoint::take(runner.executors.as_ref(), &task.channel, &workspace, &thread, &turn).await;
        match runner.turns.launch(driver, &thread, command, &runner.cli_cwd).await {
            Ok(events) => {
                tokio::spawn(task.follow(events));
            }
            Err(why) => {
                // Started and ended: whoever follows the thread hears why, as for a turn whose CLI fails after it began
                let mut task = task;
                task.record([ThreadEvent::Error { code: CLI_START_FAILED.into(), message: why }, ThreadEvent::TurnFinished { reason: FinishReason::Failed }]);
                task.claim.finish();
                runner.executors.unbind(&thread);
                runner.start_next_later(&thread);
            }
        }
        Ok(turn)
    }
}

/// The prompt a CLI is given: what the person said, after the context a fork's merge back left for it.
fn with_context(merge: Option<&(String, String)>, prompt: &str) -> String {
    match merge {
        Some((source, context)) => format!("Context carried over from the side thread {source}:\n\n{context}\n\n---\n\n{prompt}"),
        None => prompt.to_string(),
    }
}

pub struct Runner {
    engine: Arc<dyn ThreadEngine>,
    executors: Arc<dyn Executors>,
    turns: Arc<Turns>,
    mcp: Arc<dyn McpEndpoint>,
    /// The directory the CLI of every turn starts in: the service's own, not the workspace's (the CLI has no file tool of its own).
    cli_cwd: PathBuf,
    in_flight: Arc<InFlight>,
    waiting: Waiting,
    me: Weak<Runner>,
}

impl Runner {
    pub fn new(engine: Arc<dyn ThreadEngine>, executors: Arc<dyn Executors>, spawner: Arc<dyn Spawner>, mcp: Arc<dyn McpEndpoint>, cli_cwd: PathBuf) -> Arc<Runner> {
        Arc::new_cyclic(|me| Runner { engine, executors, turns: Arc::new(Turns::new(spawner)), mcp, cli_cwd, in_flight: Arc::new(InFlight::default()), waiting: Waiting::default(), me: me.clone() })
    }

    pub fn engine(&self) -> &Arc<dyn ThreadEngine> {
        &self.engine
    }

    pub fn executors(&self) -> &Arc<dyn Executors> {
        &self.executors
    }

    pub fn turns(&self) -> &Arc<Turns> {
        &self.turns
    }

    pub fn cli_cwd(&self) -> &Path {
        &self.cli_cwd
    }

    pub fn mcp_url(&self, thread: &str) -> String {
        self.mcp.url(thread)
    }

    pub fn waiting(&self) -> &Waiting {
        &self.waiting
    }

    pub fn in_flight(&self) -> &Arc<InFlight> {
        &self.in_flight
    }

    fn arc(&self) -> Arc<Runner> {
        self.me.upgrade().expect("a runner lives as long as the groups that hold it")
    }

    pub fn thread(&self, thread: &str) -> Result<ThreadView, Refusal> {
        self.engine.get(thread)?.ok_or_else(|| Refusal::not_found(thread))
    }

    /// The engine's decision on `command`, its events or its refusal.
    pub fn decide(&self, thread: &str, command: Command) -> Result<Vec<Event>, Refusal> {
        Ok(self.engine.decide(thread, command)??)
    }

    /// The `seq` a call that recorded `events` answers with: the last of them, or the thread's newest when it recorded none.
    pub fn seq_of(&self, thread: &str, events: &[Event]) -> Result<u64, Refusal> {
        match events.last() {
            Some(last) => Ok(last.seq),
            None => Ok(self.thread(thread)?.last_seq),
        }
    }

    /// Checks that a turn can run and takes the thread for it. The order of the refusals is the contract's: the driver, the workspace's executor,
    /// a turn already running.
    pub fn prepare(self: &Arc<Self>, thread: &str, known: &Known, turn: Option<String>, input: TurnInput) -> Result<Prepared, Refusal> {
        let Some(driver) = Driver::parse(&input.driver) else { return Err(Refusal::new(code::UNKNOWN_DRIVER, message::unknown_driver(&input.driver))) };
        let Some(channel) = self.executors.get(&input.workspace) else {
            return Err(Refusal::new(exec_proto::Code::NoExecutor.as_str(), format!("no executor is attached for workspace {}", input.workspace)));
        };
        let running = || Refusal::new(code::TURN_RUNNING, message::turn_running(thread));
        if known.running {
            return Err(running());
        }
        let turn = turn.unwrap_or_else(|| format!("turn-{}", self.engine.new_id()));
        let merge = self.engine.pending_merge(thread).unwrap_or(None);
        let request = TurnRequest {
            thread_id: thread.to_string(),
            run_id: turn.clone(),
            instance_id: driver.id().into(),
            model: input.model.clone().or_else(|| known.model.clone()),
            cwd: self.cli_cwd.display().to_string(),
            prompt: with_context(merge.as_ref(), &input.prompt),
            runtime_mode: input.runtime_mode.clone().unwrap_or_else(|| "full-access".into()),
            interaction_mode: input.interaction_mode.clone().unwrap_or_else(|| "default".into()),
            resume: if driver == Driver::ClaudeAgent { self.engine.provider_session(thread).unwrap_or(None) } else { None },
        };
        command_for_remote(driver, &request, &self.mcp.url(thread)).map_err(|why| Refusal::new(code::UNKNOWN_DRIVER, format!("{}: {}", message::unknown_driver(&input.driver), why.0)))?;
        let claim = self.in_flight.claim(thread, &turn).ok_or_else(running)?;
        Ok(Prepared { runner: Arc::clone(self), thread: thread.to_string(), turn, driver, channel, workspace: input.workspace, claim, request, said: input.prompt, merge })
    }

    /// Asks the running turn of `thread` (the one `named`, when one is) to stop, and stops its CLI; the turn ends `interrupted`. None when there is none.
    pub fn interrupt(&self, thread: &str, named: Option<&str>) -> Option<String> {
        let turn = self.in_flight.interrupt(thread, named)?;
        self.turns.interrupt(thread);
        Some(turn)
    }

    /// The running turn goes on with `prompt`: its CLI ends and the turn's task starts another in the same turn. The engine decides first, so a
    /// refusal changes nothing; a turn the group does not run is refused for want of one.
    pub fn steer(&self, thread: &str, turn: Option<String>, prompt: Option<String>, item: Option<String>) -> Result<(String, Vec<Event>), Refusal> {
        let events = self.decide(thread, Command::Steer { turn: turn.clone(), prompt, item })?;
        let said = events.first().and_then(|e| e.data.get("prompt")).and_then(|p| p.as_str()).unwrap_or_default().to_string();
        let running = self.in_flight.steer(thread, turn.as_deref(), &said).ok_or_else(|| Refusal::new(code::NO_TURN, message::no_turn(thread, turn.as_deref())))?;
        self.turns.interrupt(thread);
        Ok((running, events))
    }

    /// Ends the thread's turn and its process now. A turn the group holds no process of cannot end by itself, so its finish is recorded here.
    pub fn force_stop(&self, thread: &str, turn: Option<String>) -> Result<Vec<Event>, Refusal> {
        let events = self.decide(thread, Command::ForceStop { turn })?;
        let stopped = events.first().and_then(|e| e.turn.clone());
        if self.interrupt(thread, stopped.as_deref()).is_none() {
            self.executors.unbind(thread);
            self.engine.record(thread, stopped.as_deref(), ThreadEvent::TurnFinished { reason: FinishReason::Interrupted })?;
            self.release_requests(thread);
        }
        Ok(events)
    }

    /// The CLI's conversation of the thread is detached: the running turn does not keep the one it announced, and the permissions given for the
    /// session are no more.
    pub fn forget_session(&self, thread: &str) {
        self.in_flight.detach(thread);
        self.waiting.forget_grants(thread);
    }

    /// The thread's requests that wait are answered as unanswerable: its turn is over.
    pub fn release_requests(&self, thread: &str) {
        self.waiting.release(thread);
    }

    /// Runs the thread's queue again on a task of its own, for a turn that has just ended.
    pub fn start_next_later(&self, thread: &str) {
        let (runner, thread) = (self.arc(), thread.to_string());
        tokio::spawn(async move {
            runner.start_next(&thread).await;
        });
    }

    /// The first prompt of the thread's queue, when the thread may start it by itself: it is active, its queue is not held and no turn runs.
    fn next_item(&self, thread: &str) -> Option<QueueItem> {
        let (held, items) = self.engine.queue(thread).ok().flatten()?;
        let state = self.engine.state(thread).ok().flatten()?;
        let free = state.archived_at_ms.is_none() && state.deleted_at_ms.is_none() && state.thread.status == Status::Idle && self.in_flight.running(thread).is_none();
        if held || !free {
            return None;
        }
        items.into_iter().next()
    }

    /// Starts the prompts of the thread's queue, one turn, whose id is the item's: the first that can run. A prompt that cannot run (its driver is
    /// unknown, its workspace has no executor) leaves the queue as a turn that failed, and the next is tried. The id of the first turn started.
    pub async fn start_next(self: &Arc<Self>, thread: &str) -> Option<String> {
        let mut first = None;
        while let Some(item) = self.next_item(thread) {
            match self.start_item(thread, item).await {
                Started::Running(turn) => return first.or(Some(turn)),
                Started::Failed(turn) => {
                    first.get_or_insert(turn);
                }
                Started::Nothing => return first,
            }
        }
        first
    }

    async fn start_item(self: &Arc<Self>, thread: &str, item: QueueItem) -> Started {
        let Ok(view) = self.thread(thread) else { return Started::Nothing };
        let (workspace, driver) = (item.workspace.clone().or_else(|| view.workspace.clone()), item.driver.clone().or_else(|| view.driver.clone()));
        let input = TurnInput { workspace: workspace.unwrap_or_default(), driver: driver.unwrap_or_default(), model: item.model.clone(), prompt: item.prompt.clone(), runtime_mode: item.runtime_mode.clone(), interaction_mode: item.interaction_mode.clone() };
        let prepared = self.prepare(thread, &Known::of(&view), Some(item.item.clone()), input);
        let log_refused = |why: &str| tracing::warn!(event = "agent_queue.start_refused", thread = %thread, item = %item.item, reason = %why);
        match prepared {
            Ok(prepared) => match self.decide(thread, Command::QueueStart { item: item.item.clone() }) {
                Ok(_) => match prepared.launch().await {
                    Ok(turn) => Started::Running(turn),
                    Err(refusal) => {
                        log_refused(&refusal.message);
                        Started::Nothing
                    }
                },
                Err(refusal) => {
                    log_refused(&refusal.message);
                    Started::Nothing
                }
            },
            Err(refusal) if refusal.code == code::TURN_RUNNING => Started::Nothing,
            Err(refusal) => {
                log_refused(&refusal.message);
                match self.fail_item(thread, &item, &refusal) {
                    Ok(()) => Started::Failed(item.item),
                    Err(_) => Started::Nothing,
                }
            }
        }
    }

    /// The prompt of `item` cannot run: it leaves the queue as a turn of its own id that began and failed with the reason.
    fn fail_item(&self, thread: &str, item: &QueueItem, why: &Refusal) -> Result<(), Refusal> {
        self.decide(thread, Command::QueueStart { item: item.item.clone() })?;
        let turn = Some(item.item.as_str());
        for event in [
            ThreadEvent::TurnStarted {},
            ThreadEvent::MessageDelta { role: Role::User, text: item.prompt.clone() },
            ThreadEvent::Error { code: why.code.clone(), message: why.message.clone() },
            ThreadEvent::TurnFinished { reason: FinishReason::Failed },
        ] {
            self.engine.record(thread, turn, event)?;
        }
        Ok(())
    }

    /// Ends what a deleted thread runs.
    pub fn stop_for_delete(&self, thread: &str) {
        if self.interrupt(thread, None).is_some() {
            self.release_requests(thread);
        }
    }
}

enum Started {
    Running(String),
    Failed(String),
    Nothing,
}
