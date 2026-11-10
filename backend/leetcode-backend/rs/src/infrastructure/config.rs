use clap::Parser;
use secrecy::SecretString;
use std::path::PathBuf;

/// Everything the service reads from its environment (or the same-named flags).
#[derive(Parser)]
#[command(name = "leetcode-backend-rs", about = "LeetCode practice backend")]
pub struct Config {
    /// Ask the running service on this host for /health and exit 0 when it answers (the container healthcheck).
    #[arg(long)]
    pub healthcheck: bool,

    #[arg(long, env = "PORT", default_value_t = 8789)]
    pub port: u16,

    #[arg(long, env = "GIT_COMMIT", default_value = "unknown")]
    pub commit: String,

    #[arg(long, env = "LEETCODE_BACKEND_SESSION_FILE", default_value = "./.data/session.json")]
    pub session_file: PathBuf,

    #[arg(long, env = "LEETCODE_ORIGIN", default_value = "https://leetcode.com")]
    pub leetcode_origin: String,

    #[arg(long, env = "LEETCODE_CAPTURE_TOOL_BASE_URL", required_unless_present = "healthcheck")]
    pub capture_base_url: Option<String>,

    #[arg(long, env = "LEETCODE_CAPTURE_TRIGGER_SECRET", hide_env_values = true, required_unless_present = "healthcheck")]
    pub capture_secret: Option<SecretString>,

    #[arg(long, env = "LEETCODE_BACKEND_ADMIN_SECRET", hide_env_values = true, required_unless_present = "healthcheck")]
    pub admin_secret: Option<SecretString>,

    #[arg(long, env = "VM_GATEWAY_AUTH_SEED", hide_env_values = true, required_unless_present = "healthcheck")]
    pub bearer_token: Option<SecretString>,
}
