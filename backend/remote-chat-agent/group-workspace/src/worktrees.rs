//! The `workspace.worktrees` group: `worktree.create`, `remove` and `list` over the executor's worktree actions, and the progress of a
//! worktree's setup commands (`worktree.setup.subscribe`, `worktree.setup.cancel`). A worktree is a workspace of its own once created, owned
//! by the executor that made it and lying at the path that executor named. Every method of the group is served, so `service.status` lists it.
use crate::relay::{respond, target, Answer, Refusal};
use crate::setup::{self, Run};
use agent_api_proto::{GroupName, Response};
use agent_api_workspace::worktrees::{self as wt, CancelResult, CreateParams, CreateResult, RemoveParams, SubscribeParams, SubscribeResult};
use agent_api_workspace::{code, workspace_param, RemoveResult};
use async_trait::async_trait;
use exec_git::WorktreeAdded;
use rca_agent_core::{Call, Connection, Group, Served};
use rca_exec_channel::{Executors, Place, WorkspaceError};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub const WORKTREES_SERVED: [Served; 5] = [
    Served::service(wt::WORKTREE_CREATE, &[exec_git::GIT_WORKTREE_ADD, exec_git::WORKTREE_SETUP]),
    Served::service(wt::WORKTREE_REMOVE, &[exec_git::GIT_WORKTREE_REMOVE]),
    Served::service(wt::WORKTREE_LIST, &[exec_git::GIT_WORKTREE_LIST]),
    Served::service(wt::SETUP_SUBSCRIBE, &[]),
    Served::service(wt::SETUP_CANCEL, &[]),
];

/// The group serves every method it names: `scratch.ensure`, `repo.refs` and `repo.sparse.*` moved to later sub-groups (plan, section 2).
pub const WORKTREES_PENDING: [&str; 0] = [];

pub struct WorktreesGroup {
    executors: Arc<dyn Executors>,
    runs: Mutex<HashMap<String, Arc<Run>>>,
}

fn value(result: &impl Serialize) -> Answer {
    serde_json::to_value(result).map_err(|e| Refusal::new("INTERNAL", e.to_string()))
}

fn taken(workspace: &str) -> Refusal {
    Refusal::new(code::WORKSPACE_EXISTS, format!("workspace {workspace} exists"))
}

impl WorktreesGroup {
    pub fn new(executors: Arc<dyn Executors>) -> WorktreesGroup {
        WorktreesGroup { executors, runs: Mutex::default() }
    }

    fn runs(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<Run>>> {
        self.runs.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The worktree's name when `workspace` is one, else the refusal that says it is not (or not known).
    fn worktree_of(&self, workspace: &str) -> Result<String, Refusal> {
        if self.executors.owner(workspace).is_none() {
            return Err(Refusal::new(code::UNKNOWN_WORKSPACE, format!("no workspace {workspace}")));
        }
        self.executors.place(workspace).worktree.ok_or_else(|| Refusal::new(code::NOT_A_WORKTREE, format!("{workspace} is not a worktree")))
    }

    async fn create(&self, params: &Value) -> Answer {
        let p = CreateParams::parse(params)?;
        if self.executors.place(&p.workspace).worktree.is_some() {
            return Err(Refusal::invalid(format!("{} is a worktree: name the repository's workspace", p.workspace)));
        }
        let route = self.executors.route_worktree(&p.workspace)?;
        if self.executors.owner(&p.worktree).is_some() {
            return Err(taken(&p.worktree));
        }
        let mut args = json!({"name": p.worktree, "branch": p.branch});
        if let Some(base) = &p.base {
            args["base"] = json!(base);
        }
        let done = route.channel.call(exec_git::GIT_WORKTREE_ADD, args).await?;
        let added: WorktreeAdded = serde_json::from_value(done.result).map_err(|e| Refusal::new("BAD_RESULT", format!("the executor's worktree result is not the contract's: {e}")))?;
        let place = Place { prefix: Some(added.path.clone()), worktree: Some(p.worktree.clone()) };
        self.executors.add_workspace_at(&route.executor, &p.worktree, place).map_err(|e| match e {
            WorkspaceError::Taken { workspace, .. } => taken(&workspace),
            WorkspaceError::UnknownExecutor(id) => Refusal::new(exec_proto::Code::NoExecutor.as_str(), format!("no executor {id} is attached")),
        })?;
        let mut started = false;
        let mut setup_error = None;
        if !p.setup.is_empty() {
            let run = setup::start(route.channel.clone(), &p.worktree, &p.worktree, &p.setup).await;
            match run.first() {
                Some(first) if first.kind == "setup.started" => started = true,
                Some(first) => setup_error = first.data.get("error").map(|e| e["message"].as_str().unwrap_or("the setup did not start").to_string()),
                None => setup_error = Some("the executor did not start the setup in time".into()),
            }
            self.runs().insert(p.worktree.clone(), run);
        }
        let mut result = value(&CreateResult { workspace: p.worktree, repo: p.workspace, executor: route.executor, path: added.path, branch: added.branch, head: added.head, setup: started })?;
        if let Some(error) = setup_error {
            result["setup_error"] = json!(error);
        }
        Ok(result)
    }

    async fn remove(&self, params: &Value) -> Answer {
        let p = RemoveParams::parse(params)?;
        let name = self.worktree_of(&p.workspace)?;
        let route = self.executors.route_worktree(&p.workspace)?;
        let mut args = json!({"name": name});
        if p.force {
            args["force"] = json!(true);
        }
        route.channel.call(exec_git::GIT_WORKTREE_REMOVE, args).await?;
        if let Some(run) = self.runs().remove(&p.workspace) {
            run.cancel();
        }
        let removed = self.executors.remove_workspace(&p.workspace);
        value(&RemoveResult { workspace: p.workspace, removed })
    }

    async fn list(&self, params: &Value) -> Answer {
        let workspace = workspace_param(params)?;
        let target = target(self.executors.as_ref(), &workspace)?;
        Ok(target.call(exec_git::GIT_WORKTREE_LIST, json!({})).await?.result)
    }

    fn run_of(&self, workspace: &str) -> Result<Arc<Run>, Refusal> {
        self.worktree_of(workspace)?;
        self.runs().get(workspace).cloned().ok_or_else(|| Refusal::new(code::NO_SETUP, format!("no setup ran in worktree {workspace}")))
    }

    /// Answers with where the replay ends and leaves the stream on the connection: the recorded events after `after_seq` and then each new one
    /// are pushed once the answer is queued, until the setup finished.
    fn subscribe(&self, params: &Value, connection: &Connection) -> Answer {
        let p = SubscribeParams::parse(params)?;
        let run = self.run_of(&p.workspace)?;
        let mut changes = run.watch();
        let result = SubscribeResult { workspace: p.workspace.clone(), after_seq: p.after_seq, last_seq: run.last_seq(), running: run.running() };
        let sink = connection.sink();
        connection.follow(format!("worktree/{}/setup", p.workspace), async move {
            let mut after = p.after_seq;
            loop {
                let slice = run.since(after);
                for event in &slice.events {
                    after = event.seq;
                    if !sink.send_replay(event).await {
                        return;
                    }
                }
                if slice.finished && slice.events.is_empty() {
                    return;
                }
                if slice.events.is_empty() && changes.changed().await.is_err() {
                    return;
                }
            }
        });
        value(&result)
    }

    fn cancel(&self, params: &Value) -> Answer {
        let workspace = workspace_param(params)?;
        let run = self.run_of(&workspace)?;
        value(&CancelResult { workspace, cancelled: run.cancel() })
    }
}

#[async_trait]
impl Group for WorktreesGroup {
    fn name(&self) -> GroupName {
        wt::GROUP
    }

    fn methods(&self) -> &[Served] {
        &WORKTREES_SERVED
    }

    fn pending(&self) -> &[&'static str] {
        &WORKTREES_PENDING
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let answer = match call.method {
            wt::WORKTREE_CREATE => self.create(call.params).await,
            wt::WORKTREE_REMOVE => self.remove(call.params).await,
            wt::WORKTREE_LIST => self.list(call.params).await,
            wt::SETUP_SUBSCRIBE => self.subscribe(call.params, call.connection),
            wt::SETUP_CANCEL => self.cancel(call.params),
            other => unreachable!("{} is not a method of workspace.worktrees", other.name()),
        };
        respond(call.id, answer)
    }
}
