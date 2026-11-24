//! The `browser` group of the agent API: each method of the contract asked of the executor that owns the workspace (`BrowserGroup`: the service keeps no
//! browser, no page and no address policy; the executor validates every address). Every method answers `BROWSER_DISABLED` unless the group was built
//! with the browser on, which the service does only when `RCA_BROWSER` says so: Chromium needs memory the VM does not yet have to spare.
use agent_api_browser::{self as api, code as browser_code, message};
use agent_api_proto::{code, GroupName, Response};
use async_trait::async_trait;
use exec_proto::Code;
use rca_agent_core::{Call, Group, Served};
use rca_exec_channel::remote::RemoteError;
use rca_exec_channel::Executors;
use serde_json::Value;
use std::sync::Arc;

#[cfg(test)]
mod tests;

pub const SERVED: [Served; 8] = [
    Served::service(api::OPEN, &[exec_browser::BROWSER_OPEN]),
    Served::service(api::GOTO, &[exec_browser::BROWSER_GOTO]),
    Served::service(api::HISTORY, &[exec_browser::BROWSER_HISTORY]),
    Served::service(api::EVAL, &[exec_browser::BROWSER_EVAL]),
    Served::service(api::SCREENSHOT, &[exec_browser::BROWSER_SCREENSHOT]),
    Served::service(api::INPUT, &[exec_browser::BROWSER_INPUT]),
    Served::service(api::CLOSE, &[exec_browser::BROWSER_CLOSE]),
    Served::service(api::PAGES, &[exec_browser::BROWSER_LIST]),
];

pub struct BrowserGroup {
    executors: Arc<dyn Executors>,
    enabled: bool,
}

impl BrowserGroup {
    pub fn new(executors: Arc<dyn Executors>, enabled: bool) -> BrowserGroup {
        BrowserGroup { executors, enabled }
    }

    async fn relay(&self, spec: &api::Spec, params: &Value) -> Result<Value, (String, String)> {
        if !self.enabled {
            return Err((browser_code::BROWSER_DISABLED.to_string(), message::DISABLED.to_string()));
        }
        let workspace = api::workspace_param(params).map_err(|e| (code::INVALID_PARAMS.to_string(), e.0))?;
        let channel = self.executors.get(&workspace).ok_or_else(|| (Code::NoExecutor.as_str().to_string(), format!("no executor is attached for workspace {workspace}")))?;
        channel.call(spec.action, api::executor_args(spec, params, &workspace)).await.map(|out| out.result).map_err(|e: RemoteError| (e.code().to_string(), e.message().to_string()))
    }
}

#[async_trait]
impl Group for BrowserGroup {
    fn name(&self) -> GroupName {
        api::GROUP
    }

    fn methods(&self) -> &[Served] {
        &SERVED
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let spec = api::spec_of(call.method).unwrap_or_else(|| unreachable!("{} is not a method of browser", call.method.name()));
        match self.relay(spec, call.params).await {
            Ok(result) => Response::result(call.id, result),
            Err((code, message)) => Response::error(call.id, &code, message),
        }
    }
}
