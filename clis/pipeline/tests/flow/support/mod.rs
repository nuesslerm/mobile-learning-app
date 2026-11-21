//! The doubles of the flow tests: the shared test kit, which runs `pipeline` (or `WF_BIN`) with stubs first on its PATH.
pub use wf_testkit::*;

/// `git` for a test's throwaway repository: a commit made with explicit paths runs the pre-commit hook with GIT_INDEX_FILE set to the real
/// repository's temporary index, which `wf` (clean environment) never sees, so the test's staging must not use it either.
pub fn git_command() -> std::process::Command {
    let mut command = std::process::Command::new("git");
    for var in ["GIT_INDEX_FILE", "GIT_DIR", "GIT_WORK_TREE", "GIT_PREFIX"] {
        command.env_remove(var);
    }
    command
}
