use crate::pr::{relay, rows};
use agent_api_hosted::{self as api, Spec};
use agent_api_proto::{GroupName, Response};
use async_trait::async_trait;
use rca_agent_core::{Call, Group, Served};
use rca_exec_channel::Executors;
use std::sync::Arc;

/// `hosted.project`: every method is one executor action, asked of the workspace's executor or, with no workspace, of one whose gh is signed in.
pub struct HostedProjectGroup {
    executors: Arc<dyn Executors>,
    served: Vec<Served>,
}

impl HostedProjectGroup {
    pub fn new(executors: Arc<dyn Executors>) -> HostedProjectGroup {
        HostedProjectGroup { executors, served: rows(api::project::SPECS) }
    }
}

#[async_trait]
impl Group for HostedProjectGroup {
    fn name(&self) -> GroupName {
        api::GROUP_PROJECT
    }

    fn methods(&self) -> &[Served] {
        &self.served
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let Some(spec) = api::project::SPECS.iter().find(|s| s.method == call.method) else { unreachable!("{} is not a method of hosted.project", call.method.name()) };
        let spec: &Spec = spec;
        match relay(&self.executors, spec, call.params).await {
            Ok(result) => Response::result(call.id, result),
            Err(r) => Response::error(call.id, &r.code, r.message),
        }
    }
}
