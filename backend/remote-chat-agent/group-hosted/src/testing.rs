//! A `HostCredentials` that answers from a script, for the service crate's tests.
use crate::credentials::{HostCredentials, ProviderStatus, SignInPrompt, SignInState};
use crate::refusal::Refused;
use async_trait::async_trait;
use std::sync::Mutex;

/// Signed in as `account` on every provider; a sign-in shows `user_code` and finishes on the first wait.
pub struct ScriptedCredentials {
    pub account: Option<String>,
    pub user_code: String,
    asked: Mutex<Vec<String>>,
}

impl ScriptedCredentials {
    pub fn new(account: Option<&str>, user_code: &str) -> ScriptedCredentials {
        ScriptedCredentials { account: account.map(str::to_string), user_code: user_code.into(), asked: Mutex::new(Vec::new()) }
    }

    /// What was asked, in order: `status:github`, `start:gitlab`, `wait:f1`, `cancel:f1`.
    pub fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn note(&self, what: String) {
        self.asked.lock().unwrap_or_else(|p| p.into_inner()).push(what);
    }
}

#[async_trait]
impl HostCredentials for ScriptedCredentials {
    async fn status(&self, _workspace: &str, provider: &str, host: Option<&str>) -> Result<ProviderStatus, Refused> {
        self.note(format!("status:{provider}"));
        Ok(ProviderStatus { provider: provider.into(), installed: true, signed_in: self.account.is_some(), host: host.unwrap_or("example.test").into(), account: self.account.clone(), scopes: vec!["repo".into()] })
    }

    async fn sign_in_start(&self, _workspace: &str, provider: &str, host: Option<&str>) -> Result<SignInPrompt, Refused> {
        self.note(format!("start:{provider}"));
        Ok(SignInPrompt { flow: "f1".into(), provider: provider.into(), host: host.unwrap_or("example.test").into(), user_code: self.user_code.clone(), verification_uri: "https://example.test/login/device".into() })
    }

    async fn sign_in_wait(&self, _workspace: &str, _provider: &str, flow: &str, _timeout_ms: Option<u64>) -> Result<SignInState, Refused> {
        self.note(format!("wait:{flow}"));
        Ok(SignInState::SignedIn { account: self.account.clone() })
    }

    async fn sign_in_cancel(&self, _workspace: &str, _provider: &str, flow: &str) -> Result<bool, Refused> {
        self.note(format!("cancel:{flow}"));
        Ok(true)
    }
}
