//! The shell this service generates for the host and agent-shell, one render function per script. Each is its snapshot
//! byte for byte (`golden`).
pub mod agent_shell_job;
pub mod redeploy;
pub mod redeploy_status;

#[cfg(test)]
mod golden;

/// The repository of the image every sibling runs: the one image that holds this binary, so its helper subcommands.
pub const IMAGE_REPO: &str = "vm-ops-api-rs";
/// The subcommands of the binary that sibling scripts call, in the order a script calls them.
pub const HELPER_SUBCOMMANDS: [&str; 3] = ["provision", "append-env-local", "verify-env"];
pub const HELPER_PROVISION: &str = "vm-ops-api-rs provision";
pub const HELPER_APPEND_ENV_LOCAL: &str = "vm-ops-api-rs append-env-local";
pub const HELPER_VERIFY_ENV: &str = "vm-ops-api-rs verify-env";
