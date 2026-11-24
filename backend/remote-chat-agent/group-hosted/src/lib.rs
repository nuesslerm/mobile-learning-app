//! The `hosted.pr`, `hosted.issue`, `hosted.project`, `hosted.clone` and `hosted.auth` groups of the agent API (`libraries/agent-api/agent-api-hosted`): pull and merge requests,
//! clones, and the provider sign-in, each method the executor action its contract row names, asked of the executor that owns the workspace.
//! The service holds no GitHub or GitLab token: the executor's machine is signed in, and the only credential data the service ever sees is the
//! `HostCredentials` port's: which account a provider is signed in as, and the one-time code and address of a sign-in in progress.
mod account;
mod auth;
mod clones;
mod credentials;
mod pr;
mod project;
mod refusal;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use auth::{HostedAuthGroup, SERVED_AUTH};
pub use clones::HostedCloneGroup;
pub use credentials::{ExecutorCredentials, HostCredentials, ProviderStatus, SignInPrompt, SignInState};
pub use pr::{HostedIssueGroup, HostedPrGroup};
pub use project::HostedProjectGroup;
pub use refusal::Refused;

#[cfg(test)]
mod tests;
