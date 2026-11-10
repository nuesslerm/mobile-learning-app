use clap::{Parser, ValueEnum};
use secrecy::SecretString;

/// Everything the service reads from its environment (or the same-named flags). A variable that is set but empty counts as missing.
#[derive(Parser)]
#[command(name = "leetcode-auth-cookie-capture-rs", about = "LeetCode login capture listener")]
pub struct Config {
    /// Ask the running listener on this host for `/` and exit 0 when it answers at all (the container healthcheck).
    #[arg(long)]
    pub healthcheck: bool,

    /// Run one capture attempt now and exit (0 when the cookie was pushed), instead of listening for triggers.
    #[arg(long)]
    pub once: bool,

    #[arg(long, env = "CAPTURE_LISTENER_PORT", default_value_t = 8792)]
    pub port: u16,

    /// The `wf` binary that drives the browser; unset or empty means the one on `PATH`.
    #[arg(long, env = "WF_BIN")]
    pub wf_bin: Option<String>,

    #[arg(long, env = "LEETCODE_BACKEND_BASE_URL", value_name = "LEETCODE_BACKEND_BASE_URL", value_parser = non_empty, required_unless_present = "healthcheck")]
    pub backend_base_url: Option<String>,

    #[arg(long, env = "LEETCODE_BACKEND_ADMIN_SECRET", value_name = "LEETCODE_BACKEND_ADMIN_SECRET", hide_env_values = true, value_parser = non_empty_secret, required_unless_present = "healthcheck")]
    pub admin_secret: Option<SecretString>,

    #[arg(long, env = "TELEGRAM_NOTIFY_BASE_URL", value_name = "TELEGRAM_NOTIFY_BASE_URL", value_parser = non_empty, required_unless_present = "healthcheck")]
    pub notify_base_url: Option<String>,

    #[arg(long, env = "TELEGRAM_NOTIFY_SECRET", value_name = "TELEGRAM_NOTIFY_SECRET", hide_env_values = true, value_parser = non_empty_secret, required_unless_present = "healthcheck")]
    pub notify_secret: Option<SecretString>,

    /// The shared secret a trigger must carry; not needed to run once.
    #[arg(long, env = "LEETCODE_CAPTURE_TRIGGER_SECRET", value_name = "LEETCODE_CAPTURE_TRIGGER_SECRET", hide_env_values = true, value_parser = non_empty_secret, required_unless_present_any = ["healthcheck", "once"])]
    pub trigger_secret: Option<SecretString>,

    #[arg(long, env = "CAPTURE_MODE", value_name = "CAPTURE_MODE", value_enum, default_value = "remote")]
    pub mode: ModeArg,

    #[arg(long, env = "CAPTURE_NOVNC_PORT", default_value_t = 6080)]
    pub novnc_port: u16,

    #[arg(long, env = "CAPTURE_TIMEOUT_MS", default_value_t = 300_000)]
    pub timeout_ms: u64,

    #[arg(long, env = "CAPTURE_POLL_INTERVAL_MS", default_value_t = 2_000)]
    pub poll_interval_ms: u64,

    #[arg(long, env = "CAPTURE_LOGIN_URL", default_value = "https://leetcode.com/accounts/login")]
    pub login_url: String,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum ModeArg {
    Local,
    Remote,
}

fn non_empty(value: &str) -> Result<String, String> {
    if value.is_empty() {
        Err("must not be empty".into())
    } else {
        Ok(value.to_string())
    }
}

fn non_empty_secret(value: &str) -> Result<SecretString, String> {
    non_empty(value).map(SecretString::from)
}
