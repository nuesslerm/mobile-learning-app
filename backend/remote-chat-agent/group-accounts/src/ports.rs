//! What the accounts need from outside, owned here and implemented by the service's adapters (doubles behind the `testing` feature): the
//! providers' directories, the start of a CLI's login, and the model the service itself asks.
use agent_api_accounts::{Limits, ResetOutcome, Updated};
use async_trait::async_trait;
use futures_util::Stream;
use rca_cli_driver::domain::{Driver, LoginMethod};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// The directory each provider keeps its login in, on the service's volume. The service reads whether a login is there and removes it on a
/// logout; the CLI writes it. It never reads a token, so the port has no method that returns one.
#[async_trait]
pub trait ProviderDirs: Send + Sync {
    /// Reads again where the logins are kept, for a store that is somewhere else (an executor's home); the local directories have nothing to read.
    async fn refresh(&self) {}
    /// Ends the provider's login where it is kept; the local directories empty it with `clear`.
    async fn logout(&self, driver: Driver) -> Result<(), String> {
        self.clear(driver)
    }
    /// Makes the provider's directory if it is not there.
    fn ensure(&self, driver: Driver) -> Result<(), String>;
    /// Whether the provider's directory holds a login.
    fn signed_in(&self, driver: Driver) -> bool;
    /// Removes the provider's login and nothing else in its directory.
    fn clear(&self, driver: Driver) -> Result<(), String>;
    /// The provider a new turn uses by default, when one was chosen.
    fn selected(&self) -> Option<Driver>;
    fn select(&self, driver: Driver) -> Result<(), String>;
    fn deselect(&self) -> Result<(), String>;
}

/// A started login command: what it prints, how it ends, where a pasted code goes, and a way to kill it.
pub struct LoginRun {
    pub lines: Pin<Box<dyn Stream<Item = String> + Send>>,
    /// `Err` carries the tail of the command's error output after a failure.
    pub finish: Pin<Box<dyn Future<Output = Result<(), String>> + Send>>,
    /// Sends a line to the command's input; false once it cannot take one.
    pub input: Arc<dyn Fn(String) -> bool + Send + Sync>,
    pub kill: Arc<dyn Fn() + Send + Sync>,
}

/// Starts a provider CLI's login with the environment of that provider alone, so the CLI keeps what it gets in the provider's directory.
#[async_trait]
pub trait ProviderLogin: Send + Sync {
    /// `Err` is why the login could not start (such as `not found`).
    async fn start(&self, driver: Driver, method: LoginMethod) -> Result<LoginRun, String>;
}

/// One request to the model the service asks itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Completion {
    pub system: String,
    pub prompt: String,
    pub max_tokens: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Completed {
    pub text: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModelError {
    /// No model is configured, or the one that is could not answer.
    Unavailable(String),
    /// The model API has no such operation for the provider.
    NotSupported(String),
}

/// The model API the service asks for itself (a commit message), with a key from the secrets registry, never one a mapper sent.
#[async_trait]
pub trait ModelApi: Send + Sync {
    async fn complete(&self, request: Completion) -> Result<Completed, ModelError>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum AccountError {
    /// The provider has no such thing (no account to ask, no update command of its own).
    NotSupported(String),
    /// The workspace host or the provider's account could not answer.
    Unavailable(String),
}

/// What the providers' own accounts say and do, asked at the workspace host under the login each CLI keeps in its home (decision 9): the
/// service holds no token and reads none.
#[async_trait]
pub trait ProviderAccounts: Send + Sync {
    /// The limits of the provider's account, or none for a provider with no account to ask.
    async fn limits(&self, driver: Driver) -> Result<Option<Limits>, AccountError>;
    /// Spends a reset credit of the provider's account; `key` is shared by the retries of one spend.
    async fn reset_credit(&self, driver: Driver, key: &str) -> Result<ResetOutcome, AccountError>;
    /// Runs the provider CLI's own update.
    async fn update(&self, driver: Driver) -> Result<Updated, AccountError>;
}
