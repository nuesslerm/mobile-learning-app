//! The `workspace` group: `workspace.list`, `workspace.create`, `workspace.remove` and `repo.list`. A workspace is the root of an executor or a directory
//! below it (a new one is always a directory: the root is a workspace already); creating one only records it for the executor that will serve it (the directory is not looked at here: a file method that names
//! a directory that is not there is answered by the executor with `NOT_FOUND`). `workspace.activate` and `repo.hooks` of the
//! owner set are not served, so `service.status` does not list the group.
use crate::relay::{respond, Answer, Refusal};
use agent_api_proto::{GroupName, Response};
use agent_api_workspace::{self as workspace, code, CreateParams, CreateResult, Kind, ListResult, RemoveResult, Repo, RepoListResult};
use async_trait::async_trait;
use rca_agent_core::{Call, Group, Served};
use rca_exec_channel::{Executors, Place, WorkspaceError};
use serde_json::{json, Value};
use std::sync::Arc;

pub const WORKSPACE_SERVED: [Served; 4] = [Served::service(workspace::LIST, &[]), Served::service(workspace::WORKSPACE_CREATE, &[]), Served::service(workspace::WORKSPACE_REMOVE, &[]), Served::service(workspace::REPO_LIST, &[])];

/// The methods of the owner set this service does not answer yet (`docs/plans/rca-parity/plan.md`, section 2).
pub const WORKSPACE_PENDING: [&str; 2] = ["workspace.activate", "repo.hooks"];

pub struct WorkspaceGroup {
    executors: Arc<dyn Executors>,
}

impl WorkspaceGroup {
    pub fn new(executors: Arc<dyn Executors>) -> WorkspaceGroup {
        WorkspaceGroup { executors }
    }

    fn create(&self, params: &Value) -> Answer {
        let p = CreateParams::parse(params)?;
        if self.executors.owner(&p.workspace).is_some() {
            return Err(Refusal::new(code::WORKSPACE_EXISTS, format!("workspace {} exists", p.workspace)));
        }
        let route = self.executors.route_new(p.executor.as_deref())?;
        let place = Place::directory(&p.path).map_err(Refusal::invalid)?;
        let path = place.prefix.clone().unwrap_or_default();
        self.executors.add_workspace_at(&route.executor, &p.workspace, place).map_err(|e| match e {
            WorkspaceError::Taken { workspace, .. } => Refusal::new(code::WORKSPACE_EXISTS, format!("workspace {workspace} exists")),
            WorkspaceError::UnknownExecutor(id) => Refusal::new(exec_proto::Code::NoExecutor.as_str(), format!("no executor {id} is attached")),
        })?;
        Ok(serde_json::to_value(CreateResult { workspace: p.workspace, executor: route.executor, path }).unwrap_or_else(|_| json!({})))
    }

    fn repos(&self) -> Answer {
        let attached = self.executors.executors();
        let repos = self.executors.known_workspaces().into_iter().filter_map(|workspace| {
            let executor = self.executors.owner(&workspace)?;
            let place = self.executors.place(&workspace);
            let kind = match (&place.worktree, &place.prefix) {
                (Some(_), _) => Kind::Worktree,
                (None, Some(_)) => Kind::Directory,
                (None, None) => Kind::Root,
            };
            Some(Repo { attached: attached.contains(&executor), workspace, executor, kind, path: place.prefix })
        });
        Ok(serde_json::to_value(RepoListResult { repos: repos.collect() }).unwrap_or_else(|_| json!({})))
    }

    fn remove(&self, params: &Value) -> Answer {
        let workspace = workspace::workspace_param(params)?;
        if self.executors.owner(&workspace).is_none() {
            return Err(Refusal::new(code::UNKNOWN_WORKSPACE, format!("no workspace {workspace}")));
        }
        let place = self.executors.place(&workspace);
        if place.worktree.is_some() {
            return Err(Refusal::new(code::NOT_REMOVABLE, format!("{workspace} is a worktree: remove it with worktree.remove")));
        }
        let attached = self.executors.owner(&workspace).is_some_and(|owner| self.executors.executors().contains(&owner));
        if place == Place::default() && attached {
            return Err(Refusal::new(code::NOT_REMOVABLE, format!("{workspace} is an executor's root workspace")));
        }
        let removed = self.executors.remove_workspace(&workspace);
        Ok(serde_json::to_value(RemoveResult { workspace, removed }).unwrap_or_else(|_| json!({})))
    }
}

#[async_trait]
impl Group for WorkspaceGroup {
    fn name(&self) -> GroupName {
        workspace::WORKSPACE
    }

    fn methods(&self) -> &[Served] {
        &WORKSPACE_SERVED
    }

    fn pending(&self) -> &[&'static str] {
        &WORKSPACE_PENDING
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let answer = match call.method {
            workspace::LIST => Ok(ListResult { workspaces: self.executors.workspaces() }.to_value()),
            workspace::WORKSPACE_CREATE => self.create(call.params),
            workspace::WORKSPACE_REMOVE => self.remove(call.params),
            workspace::REPO_LIST => self.repos(),
            other => unreachable!("{} is not a method of the workspace group", other.name()),
        };
        respond(call.id, answer)
    }
}
