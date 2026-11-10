//! The `files` group: `file.read`, `file.list` and `file.diff`, each performed by the executor of the workspace the params name. The result
//! is the executor's, relayed unchanged, and so is its failure, with its own code.
use crate::relay::{executor_args, respond, target, Answer};
use agent_api_proto::{GroupName, Response};
use agent_api_workspace::{self as workspace, FileParams};
use async_trait::async_trait;
use rca_agent_core::{Call, Group, Served};
use rca_exec_channel::Executors;
use std::sync::Arc;

pub const FILES_SERVED: [Served; 3] = [
    Served::service(workspace::FILE_READ, &[exec_fs::READ_FILE]),
    Served::service(workspace::FILE_LIST, &[exec_fs::LIST_DIR]),
    Served::service(workspace::FILE_DIFF, &[exec_git::GIT_DIFF]),
];

pub struct FilesGroup {
    executors: Arc<dyn Executors>,
}

impl FilesGroup {
    pub fn new(executors: Arc<dyn Executors>) -> FilesGroup {
        FilesGroup { executors }
    }
}

/// Relays one file method to the executor of its workspace; shared by the two groups of file methods.
pub(crate) async fn relay(executors: &dyn Executors, served: &[Served], call: &Call<'_>) -> Answer {
    let [action] = served.iter().find(|s| s.method == call.method).map(|s| s.actions).unwrap_or_default() else {
        unreachable!("a file method calls exactly one executor action (the group's rows)")
    };
    let params = FileParams::parse(call.method, call.params)?;
    let target = target(executors, &params.workspace)?;
    let args = executor_args(call.method, params, &target.place)?;
    Ok(target.call(action, args).await?.result)
}

#[async_trait]
impl Group for FilesGroup {
    fn name(&self) -> GroupName {
        workspace::FILES
    }

    fn methods(&self) -> &[Served] {
        &FILES_SERVED
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let answer = relay(self.executors.as_ref(), &FILES_SERVED, &call).await;
        respond(call.id, answer)
    }
}
