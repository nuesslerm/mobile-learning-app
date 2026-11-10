//! The `agent.lifecycle` group: what happens to a thread beyond its turns. The engine decides each command and reads across the log (T3's rules);
//! this group does the part that is a process: ending the turn a forced stop, a workflow stop or a delete names, and starting the turn of a launch or a retry.
use crate::checkpoint::{live_turns, restore};
use crate::port::{Command, NewThread};
use crate::refusal::{respond, to_value, Answer, Refusal};
use crate::runner::{Known, Runner, TurnInput};
use agent_api_agent::lifecycle::{
    self as api, ArchiveParams, ArchiveResult, DeleteParams, DeleteResult, DetachParams, DetachResult, FamilyParams, FamilyResult, ForceStopOutcome, ForceStopParams, ForceStopResult, ForkParams, ForkResult, ItemGetParams, ItemGetResult, LaunchParams, LaunchResult, MergeBackParams,
    MergeBackResult, RetryParams, RetryResult, RollbackParams, RollbackResult, SearchParams, SearchResult, SettleParams, SettleResult, ShelfParams, ThreadState, WorkflowStopParams, WorkflowStopResult,
};
use agent_api_agent::{queue, Status, TranscriptItem, TranscriptResult};
use agent_api_proto::{GroupName, Response};
use async_trait::async_trait;
use rca_agent_core::{Call, Group, Served};
use serde_json::Value;
use std::sync::Arc;

pub const SERVED: [Served; 15] = [
    Served::cli(api::THREAD_LAUNCH, &[]),
    Served::service(api::THREAD_ARCHIVE, &[]),
    Served::service(api::THREAD_DELETE, &[]),
    Served::service(api::THREAD_FORK, &[]),
    Served::service(api::THREAD_SETTLE, &[]),
    Served::service(api::THREAD_SEARCH, &[]),
    Served::service(api::THREAD_FAMILY, &[]),
    Served::cli(api::THREAD_FORCE_STOP, &[]),
    Served::service(api::ITEM_GET, &[]),
    Served::cli(api::WORKFLOW_STOP, &[]),
    Served::service(api::THREAD_MERGE_BACK, &[]),
    Served::service(api::THREAD_ROLLBACK, &[exec_git::GIT_DISCARD]),
    Served::service(api::THREAD_DETACH, &[]),
    Served::cli(api::TURN_RETRY, &[]),
    Served::service(api::THREAD_SHELF, &[]),
];

pub struct AgentLifecycleGroup {
    runner: Arc<Runner>,
}

impl AgentLifecycleGroup {
    pub fn new(runner: Arc<Runner>) -> AgentLifecycleGroup {
        AgentLifecycleGroup { runner }
    }

    fn state(&self, thread: &str) -> Result<ThreadState, Refusal> {
        self.runner.engine().state(thread)?.ok_or_else(|| Refusal::not_found(thread))
    }

    /// A thread and its first turn in one call. The turn is checked before the thread exists, so one that cannot start leaves no thread behind.
    async fn launch(&self, params: &Value) -> Answer {
        let p = LaunchParams::parse(params)?;
        if p.attachments.as_ref().is_some_and(|ids| !ids.is_empty()) {
            return Err(Refusal::invalid("attachments are placed by turn.start only: send this message as a turn, or without them"));
        }
        let thread = p.thread.clone().unwrap_or_else(|| self.runner.engine().new_id());
        if self.runner.engine().get(&thread)?.is_some() {
            return Err(Refusal::new(api::code::THREAD_EXISTS, api::message::thread_exists(&thread)));
        }
        let input = TurnInput { workspace: p.workspace.clone(), driver: p.driver.clone(), model: p.model.clone(), prompt: p.prompt.clone(), runtime_mode: p.runtime_mode.clone(), interaction_mode: p.interaction_mode.clone() };
        let prepared = self.runner.prepare(&thread, &Known::default(), None, input)?;
        let title = p.title.clone().unwrap_or_else(|| "New thread".into());
        let view = self.runner.engine().create(NewThread { id: thread.clone(), title, workspace: Some(p.workspace), driver: Some(p.driver), model: p.model })?;
        let turn = prepared.launch().await?;
        to_value(&LaunchResult { thread: self.runner.thread(&view.id).unwrap_or(view), turn })
    }

    fn archive(&self, params: &Value) -> Answer {
        let p = ArchiveParams::parse(params)?;
        let events = self.runner.decide(&p.thread, Command::Archive { archived: p.archive() })?;
        to_value(&ArchiveResult { seq: self.runner.seq_of(&p.thread, &events)?, thread: self.state(&p.thread)? })
    }

    fn delete(&self, params: &Value) -> Answer {
        let p = DeleteParams::parse(params)?;
        let events = self.runner.decide(&p.thread, Command::Delete { only_if_unused: p.only_if_unused.unwrap_or(false) })?;
        let deleted = self.state(&p.thread)?.deleted_at_ms.is_some();
        if deleted && !events.is_empty() {
            self.runner.stop_for_delete(&p.thread);
        }
        to_value(&DeleteResult { thread: p.thread, deleted, seq: events.last().map(|e| e.seq) })
    }

    fn fork(&self, params: &Value) -> Answer {
        let p = ForkParams::parse(params)?;
        let fork = p.fork.clone().unwrap_or_else(|| self.runner.engine().new_id());
        let events = self.runner.decide(&fork, Command::Fork { source: p.thread.clone(), turn: p.turn, title: p.title })?;
        let turns = events.iter().find(|e| e.kind == "thread.forked").and_then(|e| e.data.get("turns")).and_then(Value::as_array).map(|all| all.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
        to_value(&ForkResult { seq: self.runner.seq_of(&fork, &events)?, thread: self.state(&fork)?, source: p.thread, turns })
    }

    fn settle(&self, params: &Value) -> Answer {
        let p = SettleParams::parse(params)?;
        let events = self.runner.decide(&p.thread, Command::Settle { state: p.state, until_ms: p.until_ms, at_ms: p.at_ms })?;
        to_value(&SettleResult { seq: self.runner.seq_of(&p.thread, &events)?, thread: self.state(&p.thread)? })
    }

    fn search(&self, params: &Value) -> Answer {
        let p = SearchParams::parse(params)?;
        to_value(&SearchResult { matches: self.runner.engine().search(&p.query, p.page() as usize)? })
    }

    fn family(&self, params: &Value) -> Answer {
        let p = FamilyParams::parse(params)?;
        let (root, threads) = self.runner.engine().family(&p.thread)?;
        to_value(&FamilyResult { root, threads })
    }

    fn force_stop(&self, params: &Value) -> Answer {
        let p = ForceStopParams::parse(params)?;
        let events = self.runner.force_stop(&p.thread, p.turn)?;
        let turn = events.first().and_then(|e| e.turn.clone()).unwrap_or_default();
        to_value(&ForceStopResult { seq: self.runner.seq_of(&p.thread, &events)?, thread: p.thread, turn, outcome: ForceStopOutcome::Killed, shared_with: Vec::new() })
    }

    fn item(&self, params: &Value) -> Answer {
        let p = ItemGetParams::parse(params)?;
        let page = self.runner.engine().history(&p.thread, 0, usize::MAX)?.ok_or_else(|| Refusal::not_found(&p.thread))?;
        let item = TranscriptResult::fold(&p.thread, &page.events).items.into_iter().find(|item| seq_of_item(item) == p.seq);
        match item {
            Some(item) => to_value(&ItemGetResult { thread: p.thread, item }),
            None => Err(Refusal::new(api::code::ITEM_NOT_FOUND, api::message::item_not_found(&p.thread, p.seq))),
        }
    }

    /// Ends the turn the workflow runs in: the CLIs are one process per turn, so the process is the workflow.
    fn workflow_stop(&self, params: &Value) -> Answer {
        let p = WorkflowStopParams::parse(params)?;
        let events = self.runner.decide(&p.thread, Command::WorkflowStop { turn: p.turn.clone(), workflow: p.workflow })?;
        self.runner.interrupt(&p.thread, Some(&p.turn));
        to_value(&WorkflowStopResult { seq: self.runner.seq_of(&p.thread, &events)?, thread: p.thread, turn: p.turn })
    }

    fn merge_back(&self, params: &Value) -> Answer {
        let p = MergeBackParams::parse(params)?;
        let events = self.runner.decide(&p.into, Command::MergeBack { source: p.thread.clone(), turn: p.turn })?;
        to_value(&MergeBackResult { seq: self.runner.seq_of(&p.into, &events)?, thread: self.state(&p.into)?, source: p.thread })
    }

    /// Takes the later turns out of the thread, and (unless asked not to) puts the workspace's files back to what the first of them started from.
    /// The files go first and only for a rollback the engine would accept; a first dropped turn with no checkpoint refuses the whole rollback.
    async fn rollback(&self, params: &Value) -> Answer {
        let p = RollbackParams::parse(params)?;
        let state = self.state(&p.thread)?;
        let mut restored = false;
        if p.restores_files() && state.thread.status != Status::Running {
            let events = self.runner.engine().history(&p.thread, 0, usize::MAX)?.map(|page| page.events).unwrap_or_default();
            let live = live_turns(&events);
            let first_dropped = live.iter().position(|t| *t == p.turn).and_then(|at| live.get(at + 1));
            if let Some(first) = first_dropped {
                let workspace = state.thread.workspace.clone().ok_or_else(|| Refusal::new(api::code::NO_CHECKPOINT, api::message::no_checkpoint(&p.thread, first)))?;
                restore(self.runner.executors().as_ref(), &workspace, &p.thread, first).await?;
                restored = true;
            }
        }
        let events = self.runner.decide(&p.thread, Command::Rollback { turn: p.turn.clone(), restore_files: restored })?;
        let dropped = events.first().and_then(|e| e.data.get("dropped")).and_then(Value::as_array).map(|all| all.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
        to_value(&RollbackResult { seq: self.runner.seq_of(&p.thread, &events)?, thread: p.thread, turn: p.turn, dropped, restored_files: restored })
    }

    /// Forgets the CLI conversation `session`: the next turn starts a fresh one.
    fn detach(&self, params: &Value) -> Answer {
        let p = DetachParams::parse(params)?;
        let events = self.runner.decide(&p.thread, Command::Detach { session: p.session.clone(), reason: p.reason })?;
        self.runner.forget_session(&p.thread);
        to_value(&DetachResult { seq: self.runner.seq_of(&p.thread, &events)?, thread: p.thread, session: p.session })
    }

    /// Runs the prompt of a failed turn again as a new turn of the thread's own defaults. The workspace and driver are checked before the engine
    /// decides, so a retry that cannot run records nothing.
    async fn retry(&self, params: &Value) -> Answer {
        let p = RetryParams::parse(params)?;
        let view = self.runner.thread(&p.thread)?;
        let (Some(workspace), Some(driver)) = (view.workspace.clone(), view.driver.clone()) else {
            return Err(Refusal::new(queue::code::NO_DEFAULTS, queue::message::no_defaults(&p.thread)));
        };
        let turn = format!("turn-{}", self.runner.engine().new_id());
        let input = TurnInput { workspace, driver, model: None, prompt: String::new(), runtime_mode: None, interaction_mode: None };
        let mut prepared = self.runner.prepare(&p.thread, &Known::of(&view), Some(turn.clone()), input)?;
        let events = self.runner.decide(&p.thread, Command::Retry { turn: p.turn.clone(), new_turn: turn.clone() })?;
        let prompt = events.first().and_then(|e| e.data.get("prompt")).and_then(Value::as_str).unwrap_or_default().to_string();
        prepared.say(&prompt);
        let seq = self.runner.seq_of(&p.thread, &events)?;
        prepared.launch().await?;
        to_value(&RetryResult { thread: p.thread, turn, retry_of: p.turn, seq })
    }

    fn shelf(&self, params: &Value) -> Answer {
        let p = ShelfParams::parse(params)?;
        to_value(&self.runner.engine().shelf(p.shelf, p.before.clone(), p.page() as usize)?)
    }
}

/// The `seq` of the event a transcript row opened.
fn seq_of_item(item: &TranscriptItem) -> u64 {
    match item {
        TranscriptItem::Message { seq, .. } | TranscriptItem::Reasoning { seq, .. } | TranscriptItem::Tool { seq, .. } | TranscriptItem::File { seq, .. } | TranscriptItem::Plan { seq, .. } | TranscriptItem::Error { seq, .. } => *seq,
    }
}

#[async_trait]
impl Group for AgentLifecycleGroup {
    fn name(&self) -> GroupName {
        api::GROUP
    }

    fn methods(&self) -> &[Served] {
        &SERVED
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let Call { id, method, params, .. } = call;
        let answer = match method.name() {
            "thread.launch" => self.launch(params).await,
            "thread.archive" => self.archive(params),
            "thread.delete" => self.delete(params),
            "thread.fork" => self.fork(params),
            "thread.settle" => self.settle(params),
            "thread.search" => self.search(params),
            "thread.family" => self.family(params),
            "thread.forceStop" => self.force_stop(params),
            "item.get" => self.item(params),
            "workflow.stop" => self.workflow_stop(params),
            "thread.mergeBack" => self.merge_back(params),
            "thread.rollback" => self.rollback(params).await,
            "thread.detach" => self.detach(params),
            "turn.retry" => self.retry(params).await,
            "thread.shelf" => self.shelf(params),
            other => unreachable!("{other} is no method of agent.lifecycle (SERVED)"),
        };
        respond(id, answer)
    }
}
