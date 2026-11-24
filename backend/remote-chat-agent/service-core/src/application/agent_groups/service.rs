//! The `service` group: `service.status`, which every mapper asks first (its answer is the build and the groups the service lists), and `service.logs`,
//! the last events the service's own code wrote (`application::log`).
use agent_api_proto::{Response, PROTOCOL};
use agent_api_service::logs::{logs_result, LogsParams};
use agent_api_service::{self as service, StatusResult};
use async_trait::async_trait;
use rca_agent_core::{Call, Group, Served};

const SERVED: [Served; 2] = [Served::service(service::STATUS, &[]), Served::service(service::LOGS, &[])];

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
        if call.method == service::LOGS {
            return match LogsParams::parse(call.params) {
                Ok(p) => {
                    let (lines, kept) = crate::application::log::tail().recent(p.limit, p.min_level);
                    Response::result(call.id, logs_result(&lines, kept))
                }
                Err(e) => Response::error(call.id, agent_api_proto::code::INVALID_PARAMS, e.0),
            };
        }
        let groups = call.listed.iter().map(|g| g.as_str().to_string()).collect();
        Response::result(call.id, StatusResult { protocol: PROTOCOL, version: call.build.version.into(), commit: call.build.commit.into(), groups }.to_value())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::log;
    use rca_agent_core::{AgentApi, Build, Group};
    use serde_json::{json, Value};
    use std::sync::Arc;

    async fn ask(api: &AgentApi, params: Value) -> Value {
        api.dispatch_text(Build { version: "1", commit: "c" }, &json!({"id": 1, "method": "service.logs", "params": params}).to_string()).await
    }

    #[tokio::test]
    async fn given_events_the_service_wrote_when_its_logs_are_read_then_they_come_back_newest_last_by_level_and_a_credential_field_is_not_in_them() {
        // Given a service that wrote an info event, a warning with a token field and an error, under names of this test
        let api = AgentApi::new(vec![Arc::new(ServiceGroup) as Arc<dyn Group>]).unwrap();
        log::info("logs.test.first", &[("n", json!(1))]);
        log::warn("logs.test.second", &[("auth_token", json!("secret-value-1")), ("why", json!("slow"))]);
        log::error("logs.test.third", &[]);
        // When the logs are read at warn and above, and a bad limit is sent
        let loud = ask(&api, json!({"limit": 1000, "min_level": "warn"})).await;
        let bad = ask(&api, json!({"limit": 0})).await;
        // Then the warning and the error are there in order with the token redacted, the info event is not, and the bad limit is refused
        let ours: Vec<(&str, &str)> = loud["result"]["lines"].as_array().unwrap().iter().filter(|l| l["event"].as_str().unwrap().starts_with("logs.test.")).map(|l| (l["event"].as_str().unwrap(), l["level"].as_str().unwrap())).collect();
        assert_eq!(ours, [("logs.test.second", "warn"), ("logs.test.third", "error")]);
        assert!(!loud.to_string().contains("secret-value-1") && loud.to_string().contains("<redacted>"));
        assert_eq!(bad["error"]["code"], "INVALID_PARAMS");
    }
}
