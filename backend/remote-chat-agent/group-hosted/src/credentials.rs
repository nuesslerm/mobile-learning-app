//! The credentials port: what the service may know of the executor machine's provider sign-in. A status is the account and the scopes, a sign-in
//! is the code and the address the person opens; there is no field a token could travel in, so the adapter drops whatever else an answer holds.
use crate::refusal::{channel_of, refuse, Refused};
use async_trait::async_trait;
use rca_exec_channel::Executors;
use serde_json::{json, Map, Value};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderStatus {
    pub provider: String,
    pub installed: bool,
    pub signed_in: bool,
    pub host: String,
    pub account: Option<String>,
    pub scopes: Vec<String>,
}

impl ProviderStatus {
    pub fn to_json(&self) -> Value {
        json!({"provider": self.provider, "installed": self.installed, "signed_in": self.signed_in, "host": self.host, "account": self.account, "scopes": self.scopes})
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SignInPrompt {
    pub flow: String,
    pub provider: String,
    pub host: String,
    pub user_code: String,
    pub verification_uri: String,
}

impl SignInPrompt {
    pub fn to_json(&self) -> Value {
        json!({"flow": self.flow, "provider": self.provider, "host": self.host, "user_code": self.user_code, "verification_uri": self.verification_uri})
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SignInState {
    Pending,
    SignedIn { account: Option<String> },
    Failed { message: String },
}

impl SignInState {
    pub fn to_json(&self, flow: &str) -> Value {
        match self {
            SignInState::Pending => json!({"flow": flow, "state": "pending"}),
            SignInState::SignedIn { account } => json!({"flow": flow, "state": "signed_in", "account": account}),
            SignInState::Failed { message } => json!({"flow": flow, "state": "failed", "message": message}),
        }
    }
}

/// The provider sign-in of the machine that runs a workspace's executor.
#[async_trait]
pub trait HostCredentials: Send + Sync {
    async fn status(&self, workspace: &str, provider: &str, host: Option<&str>) -> Result<ProviderStatus, Refused>;
    async fn sign_in_start(&self, workspace: &str, provider: &str, host: Option<&str>) -> Result<SignInPrompt, Refused>;
    async fn sign_in_wait(&self, workspace: &str, provider: &str, flow: &str, timeout_ms: Option<u64>) -> Result<SignInState, Refused>;
    /// Whether a sign-in in progress was stopped.
    async fn sign_in_cancel(&self, workspace: &str, provider: &str, flow: &str) -> Result<bool, Refused>;
}

/// The port over the executor channel: the `hosted_auth_*` actions of the workspace's executor.
pub struct ExecutorCredentials {
    executors: Arc<dyn Executors>,
}

impl ExecutorCredentials {
    pub fn new(executors: Arc<dyn Executors>) -> ExecutorCredentials {
        ExecutorCredentials { executors }
    }

    async fn ask(&self, workspace: &str, action: &str, args: Map<String, Value>) -> Result<Value, Refused> {
        Ok(channel_of(&self.executors, workspace)?.call(action, Value::Object(args)).await?.result)
    }
}

fn args(provider: &str, host: Option<&str>) -> Map<String, Value> {
    let mut a = Map::new();
    a.insert("provider".into(), json!(provider));
    if let Some(host) = host {
        a.insert("host".into(), json!(host));
    }
    a
}

fn string(v: &Value, key: &str) -> Result<String, Refused> {
    v.get(key).and_then(Value::as_str).map(str::to_string).ok_or_else(|| refuse("BAD_EXECUTOR_ANSWER", format!("the executor's answer has no `{key}`")))
}

#[async_trait]
impl HostCredentials for ExecutorCredentials {
    async fn status(&self, workspace: &str, provider: &str, host: Option<&str>) -> Result<ProviderStatus, Refused> {
        let v = self.ask(workspace, exec_hosted::AUTH_STATUS, args(provider, host)).await?;
        Ok(ProviderStatus {
            provider: string(&v, "provider")?,
            installed: v["installed"].as_bool().unwrap_or(false),
            signed_in: v["signed_in"].as_bool().unwrap_or(false),
            host: string(&v, "host")?,
            account: v["account"].as_str().map(str::to_string),
            scopes: v["scopes"].as_array().map(|s| s.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
        })
    }

    async fn sign_in_start(&self, workspace: &str, provider: &str, host: Option<&str>) -> Result<SignInPrompt, Refused> {
        let v = self.ask(workspace, exec_hosted::AUTH_LOGIN_START, args(provider, host)).await?;
        Ok(SignInPrompt { flow: string(&v, "flow")?, provider: string(&v, "provider")?, host: string(&v, "host")?, user_code: string(&v, "user_code")?, verification_uri: string(&v, "verification_uri")? })
    }

    async fn sign_in_wait(&self, workspace: &str, provider: &str, flow: &str, timeout_ms: Option<u64>) -> Result<SignInState, Refused> {
        let mut a = args(provider, None);
        a.insert("flow".into(), json!(flow));
        if let Some(t) = timeout_ms {
            a.insert("timeout_ms".into(), json!(t));
        }
        let v = self.ask(workspace, exec_hosted::AUTH_LOGIN_WAIT, a).await?;
        match v["state"].as_str() {
            Some("pending") => Ok(SignInState::Pending),
            Some("signed_in") => Ok(SignInState::SignedIn { account: v["account"].as_str().map(str::to_string) }),
            Some("failed") => Ok(SignInState::Failed { message: v["message"].as_str().unwrap_or("the sign-in failed").to_string() }),
            _ => Err(refuse("BAD_EXECUTOR_ANSWER", "the executor's answer has no known `state`")),
        }
    }

    async fn sign_in_cancel(&self, workspace: &str, provider: &str, flow: &str) -> Result<bool, Refused> {
        let mut a = args(provider, None);
        a.insert("flow".into(), json!(flow));
        Ok(self.ask(workspace, exec_hosted::AUTH_LOGIN_CANCEL, a).await?["cancelled"].as_bool().unwrap_or(false))
    }
}
