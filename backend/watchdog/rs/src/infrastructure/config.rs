use clap::Parser;

/// Everything the daemon reads from its environment (or the same-named flags).
#[derive(Parser)]
#[command(name = "watchdog-rs", about = "Recreates unhealthy or restart-looping compose containers")]
pub struct Config {
    #[arg(long, env = "HOST_LABEL", default_value = "unknown")]
    pub host_label: String,

    #[arg(long, env = "WATCHDOG_INTERVAL_MS", default_value_t = 15_000, value_parser = positive)]
    pub interval_ms: u64,
}

fn positive(value: &str) -> Result<u64, String> {
    value.parse::<u64>().ok().filter(|ms| *ms > 0).ok_or_else(|| format!("{value} is not a positive number of milliseconds"))
}
