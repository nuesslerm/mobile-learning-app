//! The environment of every child `wf mimic` starts, built in this one place: an explicit allowlist, no inheritance. A variable of the caller
//! reaches a child only when it is on the list (paths, locale, toolchain homes, the repository's own `MIMIC_*`/`WF_*` knobs) and its name does
//! not look like a credential, or when a set names it (`env_pass`, `[reference] env_from_vault`). `BWS_*` and `GITHUB_*` never
//! enter a child, named or not. Every spawn goes through `command`, `ChildEnv::command` or `ChildEnv::apply`; nothing else may spawn.
use std::collections::BTreeMap;
use std::process::Command;

const EXACT: &[&str] = &[
    "PATH", "HOME", "USER", "LOGNAME", "SHELL", "TERM", "TZ", "LANG", "LANGUAGE", "TMPDIR", "TEMP", "TMP", "CI", "LD_LIBRARY_PATH", "pythonLocation", "Python_ROOT_DIR", "Python3_ROOT_DIR",
    "SSL_CERT_FILE", "SSL_CERT_DIR", "CURL_CA_BUNDLE", "REQUESTS_CA_BUNDLE", "HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY", "http_proxy", "https_proxy", "no_proxy",
    "GOROOT", "GOPATH", "GOCACHE", "GOMODCACHE", "GOTOOLCHAIN", "COREPACK_HOME",
];
const PREFIXES: &[&str] = &["LC_", "XDG_", "GIT_CONFIG_", "CARGO_", "RUSTUP_", "PNPM_", "NODE_", "npm_config_", "NPM_CONFIG_", "T3_", "MIMIC_", "WF_"];

/// Whether a variable name looks like a credential.
pub fn credential_like(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    ["TOKEN", "SECRET", "PASSWORD", "CREDENTIAL", "PAR_URL", "API_KEY", "PRIVATE_KEY"].iter().any(|s| n.contains(s)) || n.ends_with("_KEY") || n.starts_with("BWS_")
}

/// The names no child ever gets: the vault token and the GitHub Actions context and token.
pub fn never(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    n.starts_with("BWS_") || n.starts_with("GITHUB_")
}

fn allowed(name: &str) -> bool {
    !never(name) && !credential_like(name) && (EXACT.contains(&name) || PREFIXES.iter().any(|p| name.starts_with(p)))
}

#[derive(Clone, Debug, Default)]
pub struct ChildEnv {
    vars: BTreeMap<String, String>,
}

impl ChildEnv {
    /// What a child gets of this process's environment and nothing more.
    pub fn base() -> Self {
        Self::base_from(std::env::vars())
    }

    pub fn base_from(vars: impl IntoIterator<Item = (String, String)>) -> Self {
        Self { vars: vars.into_iter().filter(|(k, _)| allowed(k)).collect() }
    }

    /// Values the caller computed (the `[env]` of a provision block, a pairing link): added as given, except the names no child gets.
    #[must_use]
    pub fn with(mut self, vars: impl IntoIterator<Item = (String, String)>) -> Self {
        self.vars.extend(vars.into_iter().filter(|(k, _)| !never(k)));
        self
    }

    /// The variables a set names (`env_pass`), taken from this process's environment when it has them.
    #[must_use]
    pub fn pass(self, names: &[String]) -> Self {
        let named = names.iter().filter(|n| !never(n)).filter_map(|n| Some((n.clone(), std::env::var(n).ok()?)));
        self.with(named)
    }

    pub fn pairs(&self) -> Vec<(String, String)> {
        self.vars.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    pub fn apply(&self, command: &mut Command) {
        command.env_clear().envs(&self.vars);
    }

    pub fn command(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
        let mut command = Command::new(program);
        self.apply(&mut command);
        command
    }
}

/// A command with the base environment only.
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    ChildEnv::base().command(program)
}

/// A key a set declared, read: the variable the program gets, the registry key it came from and its value.
pub struct Declared {
    pub env: String,
    pub key: String,
    pub value: String,
}

/// The values the vault (or, without one, the caller's environment) holds for the registry keys a set declares (`[reference] env_from_vault`), read
/// in memory with `secrets::load_values`. Only the declared keys are asked for: nothing else of the vault is read. A missing one fails naming the
/// key, never a value.
pub fn resolve_declared(declared: &BTreeMap<String, String>) -> Result<Vec<Declared>, String> {
    if declared.is_empty() {
        return Ok(Vec::new());
    }
    let keys: Vec<&str> = declared.values().map(String::as_str).collect();
    let mut loaded = secrets::load_values(&keys);
    declared
        .iter()
        .map(|(env, key)| loaded.remove(key).flatten().map(|value| Declared { env: env.clone(), key: key.clone(), value }).ok_or_else(|| format!("{key} is declared by the set ([reference] env_from_vault, for {env}) and the vault holds no value for it")))
        .collect()
}

/// `text` with every value of `secrets` (at least 6 characters, so an empty or tiny value never rewrites ordinary text) replaced by its name.
pub fn redact(text: &str, secrets: &[(String, String)]) -> String {
    secrets.iter().filter(|(_, v)| v.len() >= 6).fold(text.to_string(), |t, (k, v)| t.replace(v.as_str(), &format!("<{k}>")))
}
