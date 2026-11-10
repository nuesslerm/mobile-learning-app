//! The `workspace`, `workspace.worktrees`, `files` and `files.edit` groups of the agent API (`libraries/agent-api/agent-api-workspace`). Each
//! method is answered by the executor that owns the workspace the params name (`rca-exec-channel`), relayed unchanged with its own error codes;
//! the service holds no workspace and touches no path. What the service keeps is which executor owns which workspace and where it lies (the
//! executor's root, a directory below it, a worktree under its base), and the progress of each worktree's setup.
mod edit;
mod files;
mod relay;
mod setup;
mod workspaces;
mod worktrees;

pub use edit::{FilesEditGroup, EDIT_SERVED};
pub use files::{FilesGroup, FILES_SERVED};
pub use workspaces::{WorkspaceGroup, WORKSPACE_PENDING, WORKSPACE_SERVED};
pub use worktrees::{WorktreesGroup, WORKTREES_PENDING, WORKTREES_SERVED};

#[cfg(test)]
mod tests;
