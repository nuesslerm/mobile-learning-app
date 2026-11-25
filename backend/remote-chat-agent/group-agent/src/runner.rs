//! What the groups share to run a thread's turns: the CLI of a turn and its executor, the turns in flight and the requests a turn waits on.
//! Starting a turn, steering, stopping one and starting the queue's next prompt are all here.
use crate::neutral::CLI_START_FAILED;
use crate::port::{Command, ThreadEngine};
use crate::queue::EditHolds;
use crate::refusal::Refusal;
use crate::requests::Waiting;
use crate::turns::{Claim, InFlight, TurnTask};
use agent_api_agent::lifecycle::LifecycleEvent;
use agent_api_agent::queue::QueueItem;
use agent_api_agent::{code, message, FinishReason, MessageAttachment, Role, Status, ThreadEvent, ThreadView};
use agent_api_proto::Event;
use crate::gateway::{NoGateway, TurnGateway};
use crate::host::CliLauncher;
use rca_cli_driver::application::claude_sessions::ClaudeSessions;
use rca_cli_driver::application::sessions::Sessions;
use rca_cli_driver::application::ports::SessionDriver;
use rca_cli_driver::domain::{Driver, ToolServer, TurnRequest, Verdict};
use rca_exec_channel::remote::ExecutorChannel;
use rca_exec_channel::Executors;
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
    attachments: Vec<MessageAttachment>,
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

    /// Tells the CLI where the attachments were staged and keeps their references for message history.
    pub fn note_files(&mut self, ids: &[String], paths: &[String]) {
        self.attachments = ids.iter().zip(paths).filter_map(|(id, path)| {
            let (_, name) = path.rsplit_once('/')?;
            Some(MessageAttachment { id: id.clone(), name: name.to_string() })
        }).collect();
        let list: String = paths.iter().map(|p| format!("\n- {p}")).collect();
        self.request.prompt = format!("{}\n\nFiles attached to this message, in the workspace:{list}", self.request.prompt);
    }

    /// Refuses the turn, naming the CLI, when the workspace's executor reports it missing. An executor that cannot answer the detection is not
    /// taken for one without the CLI: the launch then says why it could not start.
    pub async fn require_cli(&self) -> Result<(), Refusal> {
        let program = self.driver.program();
        let Ok(answer) = self.channel.call(rca_exec_channel::CLI_DETECT, serde_json::json!({"programs": [program]})).await else { return Ok(()) };
        let Ok(found) = serde_json::from_value::<rca_exec_channel::CliDetectResult>(answer.result) else { return Ok(()) };
        if found.clis.iter().any(|c| c.program == program && c.installed) {
            return Ok(());
        }
        Err(Refusal::new(code::CLI_NOT_INSTALLED, message::cli_not_installed(program, &self.workspace)))
    }

    /// Acknowledges a recorded Claude turn after its checkpoint, before Claude answers its startup handshake.
    pub async fn launch(self) -> Result<String, Refusal> {
        let Prepared { runner, thread, turn, driver, channel, workspace, claim, request, said, attachments, merge } = self;
        let recorded = (|| {
            for event in [ThreadEvent::TurnStarted {}, ThreadEvent::MessageDelta { role: Role::User, text: said, attachments }] {
                runner.engine.record(&thread, Some(&turn), event)?;
            }
            if let Some((source, _)) = &merge {
                runner.engine.record_value(&thread, Some(&turn), serde_json::to_value(LifecycleEvent::MergeConsumed { source: source.clone() }).unwrap_or_default())?;
            }
            Ok(())
        })();
        if let Err(refusal) = recorded {
            runner.release_route(&thread);
            return Err(refusal);
        }
        runner.executors.bind(&thread, &workspace);
        let task = TurnTask { runner: Arc::clone(&runner), channel, claim, thread: thread.clone(), turn: turn.clone(), driver };
        crate::checkpoint::take(runner.executors.as_ref(), &task.channel, &workspace, &thread, &turn).await;
        let start = async move {
            if driver == Driver::ClaudeAgent && runner.in_flight().stopping(&task.thread) {
                task.finish(false);
                return;
            }
            let linker = runner.launcher.linker(&task.channel);
            match runner.sessions.turn(&workspace, linker.as_ref(), &request).await {
                Ok(events) => {
                    if runner.in_flight().stopping(&task.thread) {
                        runner.sessions.interrupt(&task.thread);
                    }
                    tokio::spawn(task.follow(events));
                }
                Err(message) => {
                    task.record([ThreadEvent::Error { code: CLI_START_FAILED.into(), message }]);
                    task.finish(true);
                }
            }
        };
        if driver == Driver::ClaudeAgent {
            tokio::spawn(start);
        } else {
            start.await;
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
    /// The live sessions of the CLIs (Claude Code, Codex, opencode).
    sessions: Arc<Sessions>,
    mcp: Arc<dyn McpEndpoint>,
    launcher: Arc<dyn CliLauncher>,
    /// The token each running turn gave its CLI for the service's tool server: the CLI may run on another host than the service.
    mcp_tokens: std::sync::Mutex<std::collections::HashMap<String, String>>,
    gateway: Arc<dyn TurnGateway>,
    in_flight: Arc<InFlight>,
    waiting: Waiting,
    edit_holds: std::sync::Mutex<EditHolds>,
    me: Weak<Runner>,
}

impl Runner {
    pub fn new(engine: Arc<dyn ThreadEngine>, executors: Arc<dyn Executors>, launcher: Arc<dyn CliLauncher>, mcp: Arc<dyn McpEndpoint>) -> Arc<Runner> {
        Self::with_gateway(engine, executors, launcher, mcp, Arc::new(NoGateway))
    }

    /// A runner whose turns go through `gateway` when it routes them.
    pub fn with_gateway(engine: Arc<dyn ThreadEngine>, executors: Arc<dyn Executors>, launcher: Arc<dyn CliLauncher>, mcp: Arc<dyn McpEndpoint>, gateway: Arc<dyn TurnGateway>) -> Arc<Runner> {
        let journal = engine.clone();
        let claude = ClaudeSessions::reporting_tasks(move |thread, task| {
            let call = format!("claude-task:{}:{}:{}", task.turn, task.provider_run.as_deref().unwrap_or_default(), task.id);
            let mut output = task.summary;
            if let Some(stdout) = task.stdout {
                if !stdout.text.is_empty() {
                    output.push_str("\n\n");
                    output.push_str(&stdout.text);
                }
                if stdout.truncated {
                    output.push_str("\n[Background stdout truncated]");
                }
            }
            let events = [
                ThreadEvent::ToolStarted { call: call.clone(), tool: "Background task".into(), args: serde_json::json!({"taskId": task.id}) },
                ThreadEvent::ToolOutput { call: call.clone(), stream: agent_api_agent::OutputStream::Stdout, data: output },
                ThreadEvent::ToolFinished { call, ok: task.status == "completed", result: serde_json::json!({"status": task.status}) },
            ];
            for event in events {
                if let Err(error) = journal.record(thread, Some(&task.turn), event) {
                    tracing::warn!(event = "claude.task_completion_record_failed", thread, turn = %task.turn, error = %error.0);
                    break;
                }
            }
        });
        let sessions = Arc::new(Sessions { claude, ..Sessions::default() });
        let runner = Arc::new_cyclic(|me| Runner { engine, executors, sessions, mcp, launcher, mcp_tokens: Default::default(), gateway, in_flight: Arc::new(InFlight::default()), waiting: Waiting::default(), edit_holds: Default::default(), me: me.clone() });
        let me = Arc::downgrade(&runner);
        runner.engine.on_settled(Arc::new(move |thread| {
            if let Some(runner) = me.upgrade() {
                runner.resume_queue_later(thread);
            }
        }));
        runner
    }

    pub fn engine(&self) -> &Arc<dyn ThreadEngine> {
        &self.engine
    }

    pub fn executors(&self) -> &Arc<dyn Executors> {
        &self.executors
    }

    /// The live sessions of the CLIs that stay up between turns: how long an idle one is kept is set on it.
    pub fn sessions(&self) -> &Arc<Sessions> {
        &self.sessions
    }

    pub fn launcher(&self) -> &Arc<dyn CliLauncher> {
        &self.launcher
    }

    pub fn mcp_url(&self, thread: &str) -> String {
        self.mcp.url(thread)
    }

    /// The token the thread's turn gives its CLI for the tool server, made when the turn first asks and gone when the turn is over (a live CLI keeps the one
    /// its process started with, so the token stays while the process does).
    pub fn mcp_token(&self, thread: &str) -> String {
        let mut tokens = self.mcp_tokens.lock().unwrap_or_else(|p| p.into_inner());
        tokens.entry(thread.to_string()).or_insert_with(|| uuid::Uuid::new_v4().simple().to_string()).clone()
    }

    /// Whether `presented` is the token of the thread's running turn, or of its live process.
    pub fn admits_mcp(&self, thread: &str, presented: &str) -> bool {
        let held = self.mcp_tokens.lock().unwrap_or_else(|p| p.into_inner()).get(thread).is_some_and(|t| t == presented);
        held && (self.in_flight.running(thread).is_some() || self.sessions.alive(thread))
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

    /// A turn asked to stop ends when its CLI says so, a moment later: a turn started on the thread meanwhile waits for that end (a few seconds at most)
    /// instead of finding the thread busy.
    pub async fn await_stopped(&self, thread: &str) {
        if !self.in_flight.stopping(thread) {
            return;
        }
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < until && (self.in_flight.stopping(thread) || self.engine.get(thread).ok().flatten().is_some_and(|view| Known::of(&view).running)) {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
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
            // A workspace that is a directory or a worktree of its executor is where the CLI runs; the root is the executor's own directory
            cwd: self.executors.place(&input.workspace).prefix.filter(|p| !p.is_empty()).unwrap_or_else(|| ".".into()),
            prompt: with_context(merge.as_ref(), &input.prompt),
            runtime_mode: input.runtime_mode.clone().unwrap_or_else(|| "full-access".into()),
            interaction_mode: input.interaction_mode.clone().unwrap_or_else(|| "default".into()),
            resume: self.engine.provider_session(thread, driver.id()).unwrap_or(None),
            // a live CLI is told of the tool server when its process starts
            tools: (driver == Driver::ClaudeAgent).then(|| ToolServer { url: self.mcp_url(thread), token: self.mcp_token(thread) }),
        };
        let claim = self.in_flight.claim(thread, &turn).ok_or_else(running)?;
        Ok(Prepared { runner: Arc::clone(self), thread: thread.to_string(), turn, driver, channel, workspace: input.workspace, claim, request, said: input.prompt, attachments: Vec::new(), merge })
    }

    /// Asks the running turn of `thread` (the one `named`, when one is) to stop, and stops its CLI; the turn ends `interrupted`. None when there is none.
    pub fn interrupt(&self, thread: &str, named: Option<&str>) -> Option<String> {
        let turn = self.in_flight.interrupt(thread, named)?;
        self.stop_process(thread);
        Some(turn)
    }

    /// Stops what runs the thread's turn: the turn on its live session.
    pub fn stop_process(&self, thread: &str) {
        self.sessions.interrupt(thread);
    }

    /// The person's answer to an approval a live session asked for; whether the session was waiting for it.
    pub fn answer_session(&self, thread: &str, approval: &str, verdict: Verdict) -> bool {
        self.sessions.answer(thread, approval, verdict)
    }

    /// The person's answer to a question a live session asked (`None`: they closed it); whether the session was waiting for it.
    pub fn answer_input_session(&self, thread: &str, request: &str, answer: Option<String>) -> bool {
        self.sessions.answer_input(thread, request, answer)
    }

    /// The running turn goes on with `prompt`, which its live session takes as it is. The engine decides first, so a refusal changes nothing; a turn the group
    /// does not run is refused for want of one. A session that does not take the steer leaves an error in the turn's log and refuses the call.
    pub async fn steer(&self, thread: &str, turn: Option<String>, prompt: Option<String>, item: Option<String>) -> Result<(String, Vec<Event>), Refusal> {
        let events = {
            let edits = self.edit_holds();
            if item.as_deref().is_some_and(|item| edits.holds(thread, item)) {
                return Err(Refusal::new(agent_api_agent::queue::code::ITEM_EDITING, "This queued message is being edited."));
            }
            self.decide(thread, Command::Steer { turn: turn.clone(), prompt, item })?
        };
        let said = events.first().and_then(|e| e.data.get("prompt")).and_then(|p| p.as_str()).unwrap_or_default().to_string();
        let running = self.in_flight.steer(thread, turn.as_deref()).ok_or_else(|| Refusal::new(code::NO_TURN, message::no_turn(thread, turn.as_deref())))?;
        if let Err(why) = self.sessions.steer(thread, &said).await {
            self.engine.record(thread, Some(&running), ThreadEvent::Error { code: crate::neutral::CLI_ERROR.into(), message: why.clone() })?;
            return Err(Refusal::new(code::NO_TURN, why));
        }
        Ok((running, events))
    }

    /// Ends the thread's turn and its process now. A turn the group holds no process of cannot end by itself, so its finish is recorded here.
    pub fn force_stop(&self, thread: &str, turn: Option<String>) -> Result<Vec<Event>, Refusal> {
        let events = self.decide(thread, Command::ForceStop { turn })?;
        let stopped = events.first().and_then(|e| e.turn.clone());
        if self.interrupt(thread, stopped.as_deref()).is_none() {
            self.executors.unbind(thread);
            self.engine.record(thread, stopped.as_deref(), ThreadEvent::TurnFinished { reason: FinishReason::Interrupted, cause: None })?;
            self.release_requests(thread);
        }
        Ok(events)
    }

    /// The CLI's conversation of the thread is detached: the running turn does not keep the one it announced, and the permissions given for the
    /// session are no more.
    pub fn forget_session(&self, thread: &str) {
        self.sessions.forget(thread);
        self.in_flight.detach(thread);
        self.waiting.forget_grants(thread);
    }

    /// The thread's gateway token opens nothing any more.
    pub fn release_route(&self, thread: &str) {
        self.gateway.release(thread);
    }

    /// The thread's requests that wait are answered as unanswerable: its turn is over.
    pub fn release_requests(&self, thread: &str) {
        if !self.sessions.alive(thread) {
            self.mcp_tokens.lock().unwrap_or_else(|p| p.into_inner()).remove(thread);
        }
        self.release_route(thread);
        self.waiting.release(thread);
    }

    /// Runs the thread's queue again on a task of its own, for a turn that has just ended.
    pub fn start_next_later(&self, thread: &str) {
        let (runner, thread) = (self.arc(), thread.to_string());
        tokio::spawn(async move {
            runner.start_next(&thread).await;
        });
    }

    pub(crate) fn edit_holds(&self) -> std::sync::MutexGuard<'_, EditHolds> {
        let mut holds = self.edit_holds.lock().unwrap_or_else(|error| error.into_inner());
        holds.prune();
        holds
    }

    pub(crate) fn wake_queue_after(&self, thread: &str, wait: std::time::Duration) {
        let (runner, thread) = (self.me.clone(), thread.to_string());
        tokio::spawn(async move {
            tokio::time::sleep(wait).await;
            if let Some(runner) = runner.upgrade() {
                drop(runner.edit_holds());
                runner.start_next(&thread).await;
            }
        });
    }

    /// After a restart settled the thread's turn as interrupted: starts the next queued prompt once the workspace's executor is back. The interrupted
    /// turn itself is never run again, it may have had side effects; a prompt sent after it resumes the kept session.
    fn resume_queue_later(&self, thread: &str) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            tracing::warn!(event = "agent_queue.resume_no_runtime", thread = %thread);
            return;
        };
        let (runner, thread) = (self.arc(), thread.to_string());
        handle.spawn(async move {
            let Some(item) = runner.engine.queue(&thread).ok().flatten().and_then(|(_, items)| items.into_iter().next()) else { return };
            let workspace = item.workspace.or_else(|| runner.thread(&thread).ok().and_then(|view| view.workspace)).unwrap_or_default();
            for _ in 0..RESTART_WINDOW_POLLS {
                if runner.executors.get(&workspace).is_some() {
                    runner.start_next(&thread).await;
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            tracing::warn!(event = "agent_queue.resume_no_executor", thread = %thread, workspace = %workspace);
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
            match self.start_item(thread, &item.item).await {
                Started::Running(turn) => return first.or(Some(turn)),
                Started::Failed(turn) => {
                    first.get_or_insert(turn);
                }
                Started::Nothing => return first,
            }
        }
        first
    }

    async fn start_item(self: &Arc<Self>, thread: &str, item: &str) -> Started {
        let prepared = match self.prepare_queued(thread, item) {
            Ok(prepared) => prepared,
            Err(outcome) => return outcome,
        };
        match prepared.launch().await {
            Ok(turn) => Started::Running(turn),
            Err(refusal) => {
                tracing::warn!(event = "agent_queue.start_refused", thread = %thread, item = %item, reason = %refusal.message);
                Started::Nothing
            }
        }
    }

    fn prepare_queued(self: &Arc<Self>, thread: &str, wanted: &str) -> Result<Prepared, Started> {
        let edits = self.edit_holds();
        let item = self.next_item(thread).filter(|item| item.item == wanted).ok_or(Started::Nothing)?;
        if edits.holds(thread, wanted) {
            return Err(Started::Nothing);
        }
        let view = self.thread(thread).map_err(|_| Started::Nothing)?;
        let workspace = item.workspace.clone().or_else(|| view.workspace.clone()).unwrap_or_default();
        let driver = item.driver.clone().or_else(|| view.driver.clone()).unwrap_or_default();
        let input = TurnInput { workspace, driver, model: item.model.clone(), prompt: item.prompt.clone(), runtime_mode: item.runtime_mode.clone(), interaction_mode: item.interaction_mode.clone() };
        let prepared = match self.prepare(thread, &Known::of(&view), Some(item.item.clone()), input) {
            Ok(prepared) => prepared,
            Err(refusal) if refusal.code == code::TURN_RUNNING => return Err(Started::Nothing),
            Err(refusal) => {
                tracing::warn!(event = "agent_queue.start_refused", thread = %thread, item = %wanted, reason = %refusal.message);
                return Err(match self.fail_item(thread, &item, &refusal) {
                    Ok(()) => Started::Failed(item.item),
                    Err(_) => Started::Nothing,
                });
            }
        };
        self.decide(thread, Command::QueueStart { item: item.item.clone() }).map_err(|refusal| {
            tracing::warn!(event = "agent_queue.start_refused", thread = %thread, item = %wanted, reason = %refusal.message);
            Started::Nothing
        })?;
        Ok(prepared)
    }

    /// The prompt of `item` cannot run: it leaves the queue as a turn of its own id that began and failed with the reason.
    fn fail_item(&self, thread: &str, item: &QueueItem, why: &Refusal) -> Result<(), Refusal> {
        self.decide(thread, Command::QueueStart { item: item.item.clone() })?;
        let turn = Some(item.item.as_str());
        for event in [
            ThreadEvent::TurnStarted {},
            ThreadEvent::message_delta(Role::User, item.prompt.clone()),
            ThreadEvent::Error { code: why.code.clone(), message: why.message.clone() },
            ThreadEvent::TurnFinished { reason: FinishReason::Failed, cause: None },
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

/// How many 500 ms polls a restarted service waits for the executor of a queued prompt to connect again (15 minutes, rca-exec's default reconnect window).
const RESTART_WINDOW_POLLS: u32 = 1800;

enum Started {
    Running(String),
    Failed(String),
    Nothing,
}
