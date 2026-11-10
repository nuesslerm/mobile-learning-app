//! The `files.edit` group: reading bytes, writing, creating, stat, entry search and browse of a workspace's files, making a directory, and the two diffs against a
//! base revision, each one executor action of the workspace's executor.
use crate::files::relay;
use crate::relay::respond;
use agent_api_proto::{GroupName, Response};
use agent_api_workspace::edit;
use async_trait::async_trait;
use rca_agent_core::{Call, Group, Served};
use rca_exec_channel::Executors;
use std::sync::Arc;

pub const EDIT_SERVED: [Served; 9] = [
    Served::service(edit::FILE_READ_CHUNK, &[exec_fs::READ_CHUNK]),
    Served::service(edit::FILE_WRITE, &[exec_fs::WRITE_FILE]),
    Served::service(edit::FILE_CREATE, &[exec_fs::CREATE_FILE]),
    Served::service(edit::FILE_STAT, &[exec_fs::STAT]),
    Served::service(edit::FILE_SEARCH, &[exec_fs::SEARCH_ENTRIES]),
    Served::service(edit::FILE_BROWSE, &[exec_fs::BROWSE]),
    Served::service(edit::FILE_MKDIR, &[exec_fs::MAKE_DIR]),
    Served::service(edit::TURN_DIFF, &[exec_git::GIT_DIFF]),
    Served::service(edit::THREAD_DIFF, &[exec_git::GIT_DIFF]),
];

pub struct FilesEditGroup {
    executors: Arc<dyn Executors>,
}

impl FilesEditGroup {
    pub fn new(executors: Arc<dyn Executors>) -> FilesEditGroup {
        FilesEditGroup { executors }
    }
}

#[async_trait]
impl Group for FilesEditGroup {
    fn name(&self) -> GroupName {
        edit::GROUP
    }

    fn methods(&self) -> &[Served] {
        &EDIT_SERVED
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let answer = relay(self.executors.as_ref(), &EDIT_SERVED, &call).await;
        respond(call.id, answer)
    }
}
