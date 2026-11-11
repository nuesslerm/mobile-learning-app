use super::cache;
use super::ci;
use super::registry::{Entry, Scope};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};

const SYSTEM_CA_BUNDLE_PATH: &str = "/etc/ssl/certs/ca-certificates.crt";

pub struct Failure {
    pub status: Option<i32>,
    pub detail: String,
}

/// Runs `file args` with no shell. `bws_env` applies the child env `bws` needs; otherwise the parent env is inherited.
pub fn run_cli(
    file: &str,
    args: &[&str],
    extra_env: &[(String, String)],
    plain_color: bool,
) -> Result<String, Failure> {
    let mut cmd = Command::new(file);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if plain_color {
        // `bws --output json` honours colour-forcing vars and wraps the JSON in ANSI codes.
        cmd.env("NO_COLOR", "1");
        for var in ["FORCE_COLOR", "FORCE_TTY", "CLICOLOR_FORCE"] {
            cmd.env_remove(var);
        }
    }
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    match cmd.output() {
        Err(e) => {
            // A missing binary reads like the shell's, which is what the TypeScript reported.
            let detail = if e.kind() == std::io::ErrorKind::NotFound {
                format!("/bin/sh: 1: {file}: not found")
            } else {
                e.to_string()
            };
            Err(Failure {
                status: Some(127),
                detail,
            })
        }
        Ok(out) if out.status.success() => Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let status = out.status.code();
            let detail = if stderr.is_empty() {
                format!("Command failed with exit code {}", status.unwrap_or(-1))
            } else {
                stderr.clone()
            };
            Err(Failure { status, detail })
        }
    }
}

fn account() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "user".into())
}

/// macOS Keychain lookup through the `security` CLI; a no-op off darwin.
pub fn keychain_lookup(service: &str) -> Result<Option<String>, String> {
    if !cfg!(target_os = "macos") {
        return Ok(None);
    }
    match run_cli(
        "security",
        &[
            "find-generic-password",
            "-a",
            &account(),
            "-s",
            service,
            "-w",
        ],
        &[],
        false,
    ) {
        Ok(out) => Ok(Some(out.trim().to_string()).filter(|v| !v.is_empty())),
        Err(f) if f.status == Some(44) => Ok(None),
        Err(f) => Err(format!(
            "macOS Keychain lookup failed for service \"{service}\" (exit {}): {}",
            f.status.map_or("?".to_string(), |s| s.to_string()),
            f.detail
        )),
    }
}

/// Backend bootstrap config: the environment wins, then the macOS keychain.
pub fn resolve_backend_config(
    env_var: &str,
    keychain_service: &str,
) -> Result<Option<String>, String> {
    if let Ok(v) = std::env::var(env_var) {
        if !v.is_empty() {
            return Ok(Some(v));
        }
    }
    keychain_lookup(keychain_service)
}

pub trait Backend {
    fn name(&self) -> &str;
    /// True when this backend is the one source of truth for the registry: a registered secret is read from it and from nothing
    /// else, never from the shell or an env file. False when no vault is configured (a deployed `.env` is then the source).
    fn authoritative(&self) -> bool {
        false
    }
    fn resolve(&mut self, entry: &Entry) -> Result<Option<String>, String>;
    /// The names of every secret the backend's read token can list; `None` when it cannot say (no vault project to list).
    fn names(&mut self) -> Result<Option<Vec<String>>, String> {
        Ok(None)
    }
    fn store(&mut self, service: &str, value: &str) -> Result<(), String>;
    fn remove(&mut self, service: &str) -> Result<(), String>;
}

pub struct Macos;

impl Backend for Macos {
    fn name(&self) -> &str {
        "macos"
    }
    fn resolve(&mut self, entry: &Entry) -> Result<Option<String>, String> {
        keychain_lookup(&entry.service)
    }
    fn store(&mut self, service: &str, value: &str) -> Result<(), String> {
        if !cfg!(target_os = "macos") {
            return Ok(());
        }
        let status = Command::new("security")
            .args(["add-generic-password", "-a", &account(), "-s", service, "-w", value, "-U"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("Command failed: security add-generic-password (exit {})", status.code().unwrap_or(-1)))
        }
    }
    fn remove(&mut self, service: &str) -> Result<(), String> {
        if cfg!(target_os = "macos") {
            // An item that is not there is a no-op.
            let _ = Command::new("security")
                .args(["delete-generic-password", "-a", &account(), "-s", service])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Source {
    Read,
    Admin,
    GhCi,
    CiOpen,
    CiDeploy,
}

struct Memo {
    token: Option<String>,
    label: &'static str,
    cache: Option<Result<HashMap<String, String>, String>>,
}

pub struct Bitwarden {
    server_url: Option<String>,
    read: Memo,
    admin: Option<Memo>,
    gh_ci: Option<Memo>,
    ci_open: Option<Memo>,
    ci_deploy: Option<Memo>,
}

fn memo(token: Option<String>, label: &'static str) -> Memo {
    Memo {
        token,
        label,
        cache: None,
    }
}

impl Bitwarden {
    pub fn new() -> Result<Self, String> {
        let read = match resolve_backend_config(
            "BWS_ADMIN_ACCESS_TOKEN",
            "bws-admin-access-token",
        )? {
            Some(t) => Some(t),
            None => resolve_backend_config("BWS_ACCESS_TOKEN", "bws-access-token")?,
        };
        let admin = resolve_backend_config(
            "BWS_ADMIN_ACCESS_TOKEN",
            "bws-admin-access-token",
        )?;
        let gh_ci = resolve_backend_config(
            "BWS_CI_DEPLOY_ACCESS_TOKEN",
            "bws-ci-deploy-access-token",
        )?;
        let server_url = resolve_backend_config("BWS_SERVER_URL", "bws-server-url")?;
        Ok(Bitwarden {
            server_url,
            read: memo(read, "read token"),
            admin: admin.map(|t| memo(Some(t), "admin token")),
            gh_ci: gh_ci.map(|t| memo(Some(t), "gh-ci token")),
            ci_open: ci::scope_token(Scope::CiOpen).map(|t| memo(Some(t), "ci-open token")),
            ci_deploy: ci::scope_token(Scope::CiDeploy).map(|t| memo(Some(t), "ci-deploy token")),
        })
    }

    fn list_with(
        server_url: &Option<String>,
        token: &Option<String>,
        label: &str,
    ) -> Result<HashMap<String, String>, String> {
        let mut env: Vec<(String, String)> = Vec::new();
        if let Some(t) = token {
            env.push(("BWS_ACCESS_TOKEN".into(), t.clone()));
        }
        if let Some(u) = server_url {
            env.push(("BWS_SERVER_URL".into(), u.clone()));
        }
        // bws's TLS stack rejects the sandbox proxy's small CA bundle; the full OS store works.
        if std::env::var_os("SSL_CERT_FILE").is_some() && Path::new(SYSTEM_CA_BUNDLE_PATH).exists()
        {
            env.push(("SSL_CERT_FILE".into(), SYSTEM_CA_BUNDLE_PATH.into()));
        }
        // a dropped connection to the vault is common from the VM and the sandbox and passes on a retry; a bad token does not
        let transient = |d: &str| ["connection error", "close_notify", "error sending request", "error decoding response body", "timed out"].iter().any(|m| d.contains(m));
        let mut attempt = run_cli("bws", &["secret", "list", "--output", "json"], &env, true);
        for wait in [2, 5, 10] {
            match &attempt {
                Err(f) if transient(&f.detail) => {
                    std::thread::sleep(std::time::Duration::from_secs(wait));
                    attempt = run_cli("bws", &["secret", "list", "--output", "json"], &env, true);
                }
                _ => break,
            }
        }
        let stdout = attempt.map_err(|f| {
            let region_hint = if (f.detail.contains("invalid_client") || f.detail.contains("401")) && server_url.is_none() {
                " If your Bitwarden account is EU-hosted, set BWS_SERVER_URL=https://vault.bitwarden.eu — bws defaults to the US cloud, which rejects EU tokens with exactly this error."
            } else {
                ""
            };
            format!(
                "Bitwarden Secrets Manager list failed ({label}) — is bws installed and the token valid? {}{region_hint}",
                f.detail
            )
        })?;
        let parsed: Value = serde_json::from_str(&stdout).map_err(|_| {
            let head: String = stdout.chars().take(200).collect();
            format!("Bitwarden Secrets Manager returned unparseable JSON: {head}")
        })?;
        let mut map = HashMap::new();
        for s in parsed.as_array().cloned().unwrap_or_default() {
            if let (Some(k), Some(v)) = (
                s.get("key").and_then(Value::as_str),
                s.get("value").and_then(Value::as_str),
            ) {
                map.insert(k.to_string(), v.to_string());
            }
        }
        Ok(map)
    }

    /// The list through the local encrypted copy (see `cache`): a fresh copy is used without calling the vault, and a copy up to a
    /// week old stands in when the vault cannot be reached. The write-capable source is always fetched live.
    fn list_through_copy(
        which: Source,
        server_url: &Option<String>,
        token: &Option<String>,
        label: &str,
    ) -> Result<HashMap<String, String>, String> {
        let mode = cache::mode();
        if which == Source::Admin || mode == cache::Mode::Off {
            return Self::list_with(server_url, token, label);
        }
        let copy = cache::load(label, token, server_url);
        if let Some(c) = copy.as_ref().filter(|c| mode == cache::Mode::Use && c.is_fresh()) {
            return Ok(c.secrets.clone());
        }
        match Self::list_with(server_url, token, label) {
            Ok(map) => {
                cache::store(label, token, server_url, &map);
                Ok(map)
            }
            Err(e) => match copy.filter(cache::Copy::is_usable_when_the_vault_is_down) {
                Some(c) => {
                    eprintln!("[secrets] Bitwarden unreachable; using the vault copy from {} min ago ({label})", c.age_sec / 60);
                    Ok(c.secrets)
                }
                None => Err(e),
            },
        }
    }

    /// Lists once per source; a failed list is cached too, so a broken backend fails fast on every key.
    fn ensure(&mut self, which: Source) -> Result<bool, String> {
        let server_url = self.server_url.clone();
        let slot = match which {
            Source::Read => Some(&mut self.read),
            Source::Admin => self.admin.as_mut(),
            Source::GhCi => self.gh_ci.as_mut(),
            Source::CiOpen => self.ci_open.as_mut(),
            Source::CiDeploy => self.ci_deploy.as_mut(),
        };
        let Some(m) = slot else { return Ok(false) };
        if m.cache.is_none() {
            m.cache = Some(Self::list_through_copy(which, &server_url, &m.token, m.label));
        }
        match m.cache.as_ref().expect("cache set") {
            Ok(_) => Ok(true),
            Err(e) => Err(e.clone()),
        }
    }

    fn get(&self, which: Source, key: &str) -> Option<String> {
        let m = match which {
            Source::Read => Some(&self.read),
            Source::Admin => self.admin.as_ref(),
            Source::GhCi => self.gh_ci.as_ref(),
            Source::CiOpen => self.ci_open.as_ref(),
            Source::CiDeploy => self.ci_deploy.as_ref(),
        }?;
        m.cache.as_ref()?.as_ref().ok()?.get(key).cloned()
    }
}

impl Backend for Bitwarden {
    fn authoritative(&self) -> bool {
        self.read.token.is_some() || ci::scoped_mode()
    }

    fn name(&self) -> &str {
        "bitwarden"
    }

    fn store(&mut self, _service: &str, _value: &str) -> Result<(), String> {
        Err("The bitwarden backend is read-only — add or update the secret in Bitwarden Secrets Manager (bws secret create/edit or the web vault), not via this CLI.".into())
    }
    fn remove(&mut self, _service: &str) -> Result<(), String> {
        Err("The bitwarden backend is read-only — remove the secret in Bitwarden Secrets Manager, not via this CLI.".into())
    }
    fn names(&mut self) -> Result<Option<Vec<String>>, String> {
        if self.read.token.is_none() && ci::scoped_mode() {
            return Ok(None);
        }
        self.ensure(Source::Read)?;
        Ok(self.read.cache.as_ref().and_then(|c| c.as_ref().ok()).map(|m| m.keys().cloned().collect()))
    }
    fn resolve(&mut self, entry: &Entry) -> Result<Option<String>, String> {
        let key = entry
            .bitwarden_key
            .clone()
            .unwrap_or_else(|| entry.env_var.clone());
        if let Some(why) = ci::refusal(entry) {
            return Err(why);
        }
        if entry.scope == Scope::Public && ci::in_ci() {
            if let Some(value) = ci::public_value(entry) {
                return Ok(Some(value));
            }
        }
        let chain: Vec<Source> = match entry.vault.as_deref() {
            _ if ci::scoped_mode() => vec![if entry.scope == Scope::CiOpen { Source::CiOpen } else { Source::CiDeploy }],
            Some("admin") => vec![Source::Admin],
            Some("gh-ci") => vec![Source::GhCi, Source::Read, Source::Admin],
            _ => vec![Source::Read, Source::Admin],
        };
        // A source is listed only when the ones before it did not hold the key, so a hit in the read source never waits on (or
        // fails with) the write-capable one.
        for s in &chain {
            self.ensure(*s)?;
            if let Some(found) = self.get(*s, &key) {
                ci::announce(&found);
                return Ok(Some(found));
            }
        }
        Ok(None)
    }
}

pub fn build(name: &str) -> Result<Box<dyn Backend>, String> {
    match name {
        "bitwarden" => Ok(Box::new(Bitwarden::new()?)),
        _ => Ok(Box::new(Macos)),
    }
}

pub fn default_backend_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else {
        "bitwarden"
    }
}

pub fn backend_name(flag: Option<&str>) -> String {
    flag.map(str::to_string)
        .or_else(|| std::env::var("SECRETS_BACKEND").ok())
        .unwrap_or_else(|| default_backend_name().to_string())
}
