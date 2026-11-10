use clap::Parser;
use std::fmt;

/// The read link: a secret, so it has no `Display` and `Debug` hides it.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: String) -> Secret {
        Secret(value)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl std::str::FromStr for Secret {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Secret, Self::Err> {
        Ok(Secret(s.to_string()))
    }
}

#[derive(Debug, Parser)]
#[command(name = "libs-proxy-rs", about = "Read-only Cargo registry proxy over the libs bucket")]
pub struct Config {
    #[arg(long, env = "PORT", default_value_t = 8793)]
    pub port: u16,
    #[arg(long, env = "LIBS_PROXY_BIND", default_value = "0.0.0.0")]
    pub bind: String,
    /// Pre-authenticated read URL of the libs bucket, ending where the object name starts.
    #[arg(long, env = "LIBS_READ_PAR_URL", hide_env_values = true, required_unless_present = "healthcheck")]
    pub read_par_url: Option<Secret>,
    /// The URL clients reach this proxy at; `config.json`'s `dl` is built from it.
    #[arg(long, env = "LIBS_PROXY_BASE_URL", required_unless_present = "healthcheck")]
    pub base_url: Option<String>,
    #[arg(long)]
    pub healthcheck: bool,
}
