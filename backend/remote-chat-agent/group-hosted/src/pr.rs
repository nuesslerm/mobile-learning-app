use crate::refusal::{channel_of, Refused};
use agent_api_hosted::{self as api, Spec};
use agent_api_proto::{GroupName, Response};
use async_trait::async_trait;
use rca_agent_core::{Call, Group, Served};
use rca_exec_channel::Executors;
use serde_json::Value;
use std::sync::Arc;

/// The executor's answer to the one action of `spec`, asked with the params it forwards.
pub(crate) async fn relay(executors: &Arc<dyn Executors>, spec: &Spec, params: &Value) -> Result<Value, Refused> {
    let workspace = api::workspace_param(params)?;
    let channel = channel_of(executors, &workspace)?;
    let action = spec.actions[0];
    Ok(channel.call(action, api::executor_args(spec, action, params)).await?.result)
}

pub(crate) fn rows(specs: &[Spec]) -> Vec<Served> {
    specs.iter().map(|s| Served::service(s.method, s.actions)).collect()
}

/// `hosted.pr`: every method is one executor action; the result and the refusal are the executor's.
pub struct HostedPrGroup {
    executors: Arc<dyn Executors>,
    served: Vec<Served>,
}

impl HostedPrGroup {
    pub fn new(executors: Arc<dyn Executors>) -> HostedPrGroup {
        HostedPrGroup { executors, served: rows(api::pr::SPECS) }
    }
}

#[async_trait]
impl Group for HostedPrGroup {
    fn name(&self) -> GroupName {
        api::GROUP_PR
    }

    fn methods(&self) -> &[Served] {
        &self.served
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let Some(spec) = api::pr::SPECS.iter().find(|s| s.method == call.method) else { unreachable!("{} is not a method of hosted.pr", call.method.name()) };
        match relay(&self.executors, spec, call.params).await {
            Ok(result) => Response::result(call.id, result),
            Err(r) => Response::error(call.id, &r.code, r.message),
        }
    }
}
