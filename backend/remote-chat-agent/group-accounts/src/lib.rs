//! The `accounts` group (`libraries/agent-api/agent-api-accounts`) and the `git.ai` sub-group (`git.generateCommitMessage`, in
//! `agent-api-git`). The accounts hold no token: each provider's CLI keeps its login in a directory of its own (`ProviderDirs`), a login is a
//! session over the CLI's login command (`ProviderLogin`, read for the address and code it prints), and what the service asks a model itself
//! goes through `ModelApi`. The ports are this crate's; the service implements them and `testing` doubles them.
mod accounts;
mod git_ai;
mod ports;
mod sessions;

#[cfg(any(test, feature = "testing"))]
pub mod testing;
#[cfg(test)]
mod tests;

pub use accounts::{AccountsGroup, SERVED};
pub use git_ai::{commit_message_prompt, tidy_message, GitAiGroup, GIT_AI_SERVED};
pub use ports::{Completed, Completion, LoginRun, ModelApi, ModelError, ProviderDirs, ProviderLogin};
pub use sessions::Sessions;
