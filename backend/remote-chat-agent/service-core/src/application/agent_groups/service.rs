//! The `service` group: `service.status`, which every mapper asks first. Its answer is the build and the groups the service lists.
use agent_api_proto::{Response, PROTOCOL};
use agent_api_service::{self as service, StatusResult};
use async_trait::async_trait;
use rca_agent_core::{Call, Group, Served};

const SERVED: [Served; 1] = [Served::service(service::STATUS, &[])];

pub struct ServiceGroup;

#[async_trait]
impl Group for ServiceGroup {
    fn name(&self) -> agent_api_proto::GroupName {
        service::GROUP
    }

    fn methods(&self) -> &[Served] {
        &SERVED
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let groups = call.listed.iter().map(|g| g.as_str().to_string()).collect();
        Response::result(call.id, StatusResult { protocol: PROTOCOL, version: call.build.version.into(), commit: call.build.commit.into(), groups }.to_value())
    }
}
