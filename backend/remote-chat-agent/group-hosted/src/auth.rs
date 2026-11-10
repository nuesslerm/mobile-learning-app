use crate::credentials::HostCredentials;
use crate::pr::{relay, rows};
use crate::refusal::{channel_of, refuse, Refused};
use agent_api_hosted::{self as api, auth, Spec};
use agent_api_proto::{code, text, GroupName, Response};
use async_trait::async_trait;
use rca_agent_core::{Call, Group, Served};
use rca_exec_channel::Executors;
use serde_json::{json, Value};
use std::sync::Arc;

pub const SERVED_AUTH: &[Spec] = auth::SPECS;

const PROVIDERS: [&str; 2] = ["github", "gitlab"];

/// `hosted.auth`: the sign-in methods go through the `HostCredentials` port, the repository ones to the executor's actions.
pub struct HostedAuthGroup {
    executors: Arc<dyn Executors>,
    credentials: Arc<dyn HostCredentials>,
    served: Vec<Served>,
}

impl HostedAuthGroup {
    pub fn new(executors: Arc<dyn Executors>, credentials: Arc<dyn HostCredentials>) -> HostedAuthGroup {
        HostedAuthGroup { executors, credentials, served: rows(auth::SPECS) }
    }

    fn provider(params: &Value) -> Result<&str, Refused> {
        match text(params, "provider") {
            Some(p) if PROVIDERS.contains(&p) => Ok(p),
            _ => Err(refuse(code::INVALID_PARAMS, "provider must be github or gitlab")),
        }
    }

    fn flow(params: &Value) -> Result<&str, Refused> {
        text(params, "flow").ok_or_else(|| refuse(code::INVALID_PARAMS, "flow is required"))
    }

    /// The status of the named provider, or of both.
    async fn discover(&self, workspace: &str, params: &Value) -> Result<Value, Refused> {
        let host = text(params, "host");
        let named = text(params, "provider");
        let wanted: Vec<&str> = match named {
            Some(_) => vec![Self::provider(params)?],
            None => PROVIDERS.to_vec(),
        };
        let mut providers = Vec::new();
        for provider in wanted {
            providers.push(self.credentials.status(workspace, provider, host).await?.to_json());
        }
        Ok(json!({"providers": providers}))
    }

    /// The signed-in account's permission on the repository; none when the machine is signed out or the repository is not visible to it.
    async fn app_access(&self, workspace: &str, params: &Value) -> Result<Value, Refused> {
        let provider = Self::provider(params)?;
        let status = self.credentials.status(workspace, provider, text(params, "host")).await?;
        let none = |status: Value| json!({"status": status, "repo": null, "permission": null});
        if !status.installed || !status.signed_in {
            return Ok(none(status.to_json()));
        }
        let view = auth::SPECS.iter().find(|s| s.method == auth::LOOKUP_REPOSITORY).expect("lookupRepository has a spec");
        match relay(&self.executors, view, params).await {
            Ok(answer) => Ok(json!({"status": status.to_json(), "permission": answer["repo"]["permission"], "repo": answer["repo"]})),
            Err(r) if r.code == exec_proto::Code::NotFound.as_str() => Ok(none(status.to_json())),
            Err(r) => Err(r),
        }
    }

    async fn answer(&self, spec: &Spec, params: &Value) -> Result<Value, Refused> {
        let workspace = api::workspace_param(params)?;
        // Checked here so a missing executor is the one refusal whichever method asked
        channel_of(&self.executors, &workspace)?;
        match spec.method {
            auth::DISCOVER => self.discover(&workspace, params).await,
            auth::APP_ACCESS => self.app_access(&workspace, params).await,
            auth::SIGN_IN_START => Ok(self.credentials.sign_in_start(&workspace, Self::provider(params)?, text(params, "host")).await?.to_json()),
            auth::SIGN_IN_WAIT => {
                let flow = Self::flow(params)?;
                Ok(self.credentials.sign_in_wait(&workspace, Self::provider(params)?, flow, params.get("timeout_ms").and_then(Value::as_u64)).await?.to_json(flow))
            }
            auth::SIGN_IN_CANCEL => {
                let flow = Self::flow(params)?;
                let cancelled = self.credentials.sign_in_cancel(&workspace, Self::provider(params)?, flow).await?;
                Ok(json!({"flow": flow, "cancelled": cancelled}))
            }
            _ => relay(&self.executors, spec, params).await,
        }
    }
}

#[async_trait]
impl Group for HostedAuthGroup {
    fn name(&self) -> GroupName {
        api::GROUP_AUTH
    }

    fn methods(&self) -> &[Served] {
        &self.served
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let Some(spec) = auth::SPECS.iter().find(|s| s.method == call.method) else { unreachable!("{} is not a method of hosted.auth", call.method.name()) };
        match self.answer(spec, call.params).await {
            Ok(result) => Response::result(call.id, result),
            Err(r) => Response::error(call.id, &r.code, r.message),
        }
    }
}
