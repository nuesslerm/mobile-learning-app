//! Real agent CLIs in the integration chain, opt in (`T3_CHAIN_CLIS`, default scripted): which CLIs run for real, their credentials resolved in memory
//! (`secrets::load_values`, the vault or the caller's environment) and handed to each CLI alone through `ChildEnv`, and the exact-value scan of
//! whatever a run writes before it is published. A missing key fails naming the key, never a value.
use crate::child_env::ChildEnv;
use crate::leak_scan;
use base64::Engine;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The variable that switches the chain to real CLIs: unset or `scripted` keeps the scripted ones, `real` runs all three, `real:claude,codex` the named.
pub const MODE_ENV: &str = "T3_CHAIN_CLIS";

/// Registry keys in, their values (or none) out: the vault's `secrets::load_values`, or a stand-in.
pub type Lookup<'a> = &'a dyn Fn(&[&str]) -> HashMap<String, Option<String>>;

pub struct Cli {
    pub name: &'static str,
    /// The registry key of its credential.
    pub key: &'static str,
    /// The variable the CLI reads it from; none for a login that is a file (Codex's `auth.json`).
    pub env: Option<&'static str>,
}

pub const CLIS: [Cli; 3] = [
    Cli { name: "claude", key: "CLAUDE_CODE_OAUTH_TOKEN", env: Some("CLAUDE_CODE_OAUTH_TOKEN") },
    Cli { name: "codex", key: "CODEX_OAUTH_JSON_B64", env: None },
    Cli { name: "opencode", key: "OPENCODE_GO_API_KEY", env: Some("OPENCODE_API_KEY") },
];

/// The CLIs `value` of `MODE_ENV` runs for real: none for the default.
pub fn parse_mode(value: Option<&str>) -> Result<Vec<&'static Cli>, String> {
    let value = value.map(str::trim).unwrap_or_default();
    let names = match value {
        "" | "scripted" => return Ok(Vec::new()),
        "real" => return Ok(CLIS.iter().collect()),
        other => other.strip_prefix("real:").ok_or_else(|| format!("{MODE_ENV}={other:?}: say scripted, real or real:<claude,codex,opencode>"))?,
    };
    let mut out: Vec<&'static Cli> = Vec::new();
    for name in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
        let cli = CLIS.iter().find(|c| c.name == name).ok_or_else(|| format!("{MODE_ENV}: {name:?} is no agent CLI (claude, codex, opencode)"))?;
        if out.iter().any(|c| c.name == name) {
            return Err(format!("{MODE_ENV}: {name} twice"));
        }
        out.push(cli);
    }
    if out.is_empty() {
        return Err(format!("{MODE_ENV}=real: names no CLI"));
    }
    Ok(out)
}

/// The credentials of the CLIs that run for real, held in memory and, for Codex, as a file in a directory the caller owns.
pub struct Resolved {
    home: PathBuf,
    creds: Vec<(&'static Cli, String)>,
    secrets: Vec<(String, String)>,
}

/// Long strings whose own field is credential-named (token, secret, key, password); ids, plans and timestamps are not secrets and a log may hold them.
fn leaves(value: &serde_json::Value, secret_field: bool, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) if s.len() >= 16 && secret_field => out.push(s.clone()),
        serde_json::Value::Array(a) => a.iter().for_each(|v| leaves(v, secret_field, out)),
        serde_json::Value::Object(o) => o.iter().for_each(|(k, v)| {
            let k = k.to_ascii_lowercase();
            leaves(v, ["token", "secret", "key", "password"].iter().any(|w| k.contains(w)), out)
        }),
        _ => {}
    }
}

impl Resolved {
    /// `clis` with their values from `lookup` (the registry keys in, the values out). The Codex login is decoded into `home/.codex/auth.json`.
    pub fn resolve(clis: &[&'static Cli], home: &Path, lookup: Lookup) -> Result<Resolved, String> {
        let keys: Vec<&str> = clis.iter().map(|c| c.key).collect();
        let mut loaded = lookup(&keys);
        let mut resolved = Resolved { home: home.to_path_buf(), creds: Vec::new(), secrets: Vec::new() };
        for cli in clis {
            let value = loaded.remove(cli.key).flatten().ok_or_else(|| format!("{} is needed by the real {} and the vault holds no value for it", cli.key, cli.name))?;
            resolved.secrets.push((cli.key.to_string(), value.clone()));
            if cli.env.is_none() {
                let bytes = base64::engine::general_purpose::STANDARD.decode(value.trim()).map_err(|_| format!("{} is not base64 (the real {}'s login)", cli.key, cli.name))?;
                let json: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| format!("{} does not decode to a JSON login", cli.key))?;
                let mut found = Vec::new();
                leaves(&json, false, &mut found);
                resolved.secrets.extend(found.into_iter().map(|v| (cli.key.to_string(), v)));
                let dir = home.join(".codex");
                std::fs::create_dir_all(&dir).map_err(|e| format!("the Codex home {}: {}", dir.display(), e.kind()))?;
                std::fs::write(dir.join("auth.json"), &bytes).map_err(|e| format!("the Codex login file: {}", e.kind()))?;
            }
            resolved.creds.push((cli, value));
        }
        Ok(resolved)
    }

    /// `resolve` from the vault, or from the caller's environment when no vault is configured.
    pub fn load(clis: &[&'static Cli], home: &Path) -> Result<Resolved, String> {
        Resolved::resolve(clis, home, &|keys| secrets::load_values(keys))
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.creds.iter().map(|(c, _)| c.name).collect()
    }

    /// Every value a published output must not hold, named by its registry key.
    pub fn secrets(&self) -> &[(String, String)] {
        &self.secrets
    }

    /// The environment of one real CLI: `base` and its own credential (Codex: a `CODEX_HOME` holding its login, a path and never the value), no other CLI's.
    pub fn child_env(&self, cli: &Cli, base: ChildEnv) -> ChildEnv {
        let own = self.creds.iter().find(|(c, _)| c.name == cli.name);
        match (own, cli.env) {
            (Some((_, value)), Some(env)) => base.with([(env.to_string(), value.clone())]),
            (Some(_), None) => base.with([("CODEX_HOME".to_string(), self.home.join(".codex").display().to_string())]),
            (None, _) => base,
        }
    }

    /// The variables of the chain's one executor, one process that every real CLI runs under, so each CLI can read the others' env keys (per-CLI isolation holds in `child_env`, not in the chain; a gap).
    pub fn executor_env(&self) -> Vec<(String, String)> {
        self.creds.iter().filter_map(|(c, v)| Some((c.env?.to_string(), v.clone()))).collect()
    }
}

/// What under `dirs` holds the exact value of a credential or a token shape: one line per finding, the file and the key, never the value.
pub fn scan(dirs: &[&Path], secrets: &[(String, String)]) -> Vec<String> {
    dirs.iter().flat_map(|dir| leak_scan::scan_tree(dir, dir, secrets)).collect()
}
