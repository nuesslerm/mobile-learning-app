use clap::Parser;
use reporting_core::application::Intervals;
use reporting_core::domain::prune::StoragePolicy;
use std::path::PathBuf;
use std::time::Duration;

/// Everything the service reads from its environment (or the same-named flags).
#[derive(Parser)]
#[command(name = "reporting-service-rs", about = "Metrics sampling and plan reports for the VM")]
pub struct Config {
    /// Ask the running service on this host for /health and exit 0 when it answers (the container healthcheck).
    #[arg(long)]
    pub healthcheck: bool,

    #[arg(long, env = "PORT", default_value_t = 8791)]
    pub port: u16,

    /// The commit the image was built from; `/health` says `unknown` without one.
    #[arg(long, env = "GIT_COMMIT")]
    pub commit: Option<String>,

    /// The ORDS auto-REST url of any table of the `HEALTH_RPT` schema (the schema's SQL endpoint is derived from it).
    #[arg(long, env = "HEALTH_REPORTS_ORDS_BASE_URL")]
    pub ords_base_url: String,

    /// The `HEALTH_RPT` database user's password.
    #[arg(long, env = "HEALTH_REPORTS_ORDS_PASSWORD", hide_env_values = true)]
    pub ords_password: String,

    /// The BetterStack Uptime heartbeat URL pinged after every host sweep; without it no heartbeat is sent.
    #[arg(long, env = "BETTERSTACK_HEARTBEAT_URL", hide_env_values = true)]
    pub heartbeat_url: Option<String>,

    /// The Better Stack metrics source (host and token); without both, no metrics are published.
    #[arg(long, env = "BETTERSTACK_METRICS_HOST")]
    pub metrics_host: Option<String>,

    #[arg(long, env = "BETTERSTACK_METRICS_TOKEN", hide_env_values = true)]
    pub metrics_token: Option<String>,

    /// A read-only GitHub token for the repository's runners and runs; without it, the repository and the runner's name, the CI runner metrics are off.
    #[arg(long, env = "CI_RUNNER_GITHUB_READ_TOKEN", hide_env_values = true)]
    pub runner_github_token: Option<String>,

    /// `owner/repo` whose Actions runner is watched.
    #[arg(long, env = "REPORTING_GITHUB_REPO")]
    pub github_repo: Option<String>,

    /// The runner's name at GitHub (its `RUNNER_NAME`).
    #[arg(long, env = "REPORTING_RUNNER_NAME")]
    pub runner_name: Option<String>,

    /// What the database may hold in all (bytes); the Always Free one holds 20 GiB.
    #[arg(long, env = "REPORTING_STORAGE_BUDGET_BYTES", default_value_t = StoragePolicy::FREE_TIER.budget_bytes)]
    pub storage_budget_bytes: u64,

    /// Percent of the budget at which the oldest samples start to go.
    #[arg(long, env = "REPORTING_PRUNE_HIGH_PCT", default_value_t = StoragePolicy::FREE_TIER.high_pct)]
    pub prune_high_pct: u64,

    /// Percent of the budget the cutting stops under.
    #[arg(long, env = "REPORTING_PRUNE_LOW_PCT", default_value_t = StoragePolicy::FREE_TIER.low_pct)]
    pub prune_low_pct: u64,

    #[arg(long, env = "DOCKER_SOCKET", default_value = "/var/run/docker.sock")]
    pub docker_socket: PathBuf,

    /// Where plans, dispatch runs and pricing are read from; by default the nearest ancestor of the working directory holding `pnpm-workspace.yaml`.
    #[arg(long, env = "REPORTING_REPO_ROOT")]
    pub repo_root: Option<PathBuf>,

    #[arg(long, env = "REPORTING_CONTAINERS_INTERVAL_MS", default_value_t = 15_000, value_parser = positive)]
    pub containers_interval_ms: u64,

    #[arg(long, env = "REPORTING_HOST_INTERVAL_MS", default_value_t = 15_000, value_parser = positive)]
    pub host_interval_ms: u64,

    #[arg(long, env = "REPORTING_RUNNER_INTERVAL_MS", default_value_t = 15_000, value_parser = positive)]
    pub runner_interval_ms: u64,

    #[arg(long, env = "REPORTING_STORAGE_INTERVAL_MS", default_value_t = 900_000, value_parser = positive)]
    pub storage_interval_ms: u64,

    #[arg(long, env = "REPORTING_PRUNE_INTERVAL_MS", default_value_t = 21_600_000, value_parser = positive)]
    pub prune_interval_ms: u64,
}

fn positive(value: &str) -> Result<u64, String> {
    value.parse::<u64>().ok().filter(|ms| *ms > 0).ok_or_else(|| format!("{value} is not a positive number of milliseconds"))
}

impl Config {
    pub fn intervals(&self) -> Intervals {
        Intervals {
            containers: Duration::from_millis(self.containers_interval_ms),
            host: Duration::from_millis(self.host_interval_ms),
            runner: Duration::from_millis(self.runner_interval_ms),
            storage: Duration::from_millis(self.storage_interval_ms),
            prune: Duration::from_millis(self.prune_interval_ms),
        }
    }

    pub fn policy(&self) -> StoragePolicy {
        StoragePolicy { budget_bytes: self.storage_budget_bytes, high_pct: self.prune_high_pct, low_pct: self.prune_low_pct }
    }

    pub fn repo_root(&self) -> PathBuf {
        self.repo_root.clone().unwrap_or_else(|| find_repo_root(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))))
    }
}

fn find_repo_root(start: &std::path::Path) -> PathBuf {
    start.ancestors().find(|dir| dir.join("pnpm-workspace.yaml").exists()).unwrap_or(start).to_path_buf()
}
