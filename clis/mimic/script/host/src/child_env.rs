//! The environment of the helper programs the host starts (`curl`, `tar`, a url target's mint command): an allowlist of paths, locale,
//! temp directories and CA/proxy settings. A variable of the caller that looks like a credential never reaches them.
use std::process::Command;

const EXACT: &[&str] = &[
    "PATH", "HOME", "USER", "LOGNAME", "TERM", "TZ", "LANG", "LANGUAGE", "TMPDIR", "TEMP", "TMP", "SSL_CERT_FILE", "SSL_CERT_DIR", "CURL_CA_BUNDLE", "HTTP_PROXY", "HTTPS_PROXY",
    "NO_PROXY", "http_proxy", "https_proxy", "no_proxy",
];

fn allowed(name: &str) -> bool {
    EXACT.contains(&name) || name.starts_with("LC_")
}

/// The part of `vars` a helper program gets.
pub fn minimal_from(vars: impl IntoIterator<Item = (String, String)>) -> Vec<(String, String)> {
    vars.into_iter().filter(|(k, _)| allowed(k)).collect()
}

/// A command that starts with nothing but the allowed part of this process's environment.
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    command.env_clear().envs(minimal_from(std::env::vars()));
    command
}
