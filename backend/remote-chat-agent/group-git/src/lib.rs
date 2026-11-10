//! The `git` group of the agent API (`libraries/agent-api/agent-api-git`): each method is the executor action its contract row names, asked of
//! the executor that owns the workspace, with a worktree workspace aimed at its worktree. The service keeps no repository, no path and no
//! status: results and refusals are the executor's own, and the status stream is the executor's `git_watch` relayed event by event.
mod group;

pub use group::{GitGroup, SERVED};

#[cfg(test)]
mod tests;
