use super::backend::Backend;
use super::ci;
use super::registry::Entry;
use super::util::{is_placeholder, json_string};
use std::collections::HashMap;

#[derive(Default)]
pub struct SyncEnvResult {
    pub resolved: Vec<(String, String)>,
    pub missing: Vec<String>,
    pub skipped: Vec<String>,
    pub from_ambient_env: Vec<String>,
    pub placeholders: Vec<String>,
}

/// Process env as the TypeScript saw it: the real environment plus what `load` filled in.
pub struct Env {
    loaded: HashMap<String, String>,
    /// Whether the process environment (the shell and the env files `wf` layers) may supply a value.
    ambient: bool,
}

impl Env {
    pub fn new() -> Self {
        Env {
            loaded: HashMap::new(),
            ambient: true,
        }
    }
    /// Only what the vault supplied: the shell and the env files are never consulted, except for the `BWS_*` bootstrap settings,
    /// which cannot come from the vault they unlock.
    pub fn vault_only() -> Self {
        Env {
            loaded: HashMap::new(),
            ambient: false,
        }
    }
    pub fn get(&self, name: &str) -> Option<String> {
        self.loaded.get(name).cloned().or_else(|| if self.ambient || name.starts_with("BWS_") { std::env::var(name).ok() } else { None })
    }
    pub fn set(&mut self, name: &str, value: String) {
        self.loaded.insert(name.to_string(), value);
    }
}

pub fn lookup<'a>(registry: &'a [Entry], key: &str) -> Option<&'a Entry> {
    registry.iter().find(|e| e.key == key)
}

pub fn sync_env(
    keys: &[String],
    registry: &[Entry],
    backend: &mut dyn Backend,
    env: &Env,
) -> Result<SyncEnvResult, String> {
    let mut r = SyncEnvResult::default();
    for key in keys {
        let entry =
            lookup(registry, key).ok_or_else(|| format!("Unknown secrets registry key: {key}"))?;
        if entry.vault.as_deref().is_some_and(|v| v != "deploy") {
            r.skipped.push(key.clone());
            continue;
        }
        if let Some(why) = ci::refusal(entry) {
            return Err(why);
        }
        let ambient = env.get(&entry.env_var);
        let value = match &ambient {
            Some(v) => Some(v.clone()),
            None => backend.resolve(entry)?,
        };
        let usable = value.as_deref().is_some_and(|v| !v.is_empty());
        if usable && !is_placeholder(value.as_deref()) {
            if ambient.is_some() {
                r.from_ambient_env.push(key.clone());
            }
            r.resolved.push((entry.env_var.clone(), value.unwrap()));
        } else if is_placeholder(value.as_deref()) {
            r.placeholders.push(key.clone());
            r.missing.push(key.clone());
        } else {
            r.missing.push(key.clone());
        }
    }
    Ok(r)
}

pub fn format_dotenv(resolved: &[(String, String)]) -> String {
    let lines: Vec<String> = resolved
        .iter()
        .map(|(k, v)| format!("{k}={}", json_string(v)))
        .collect();
    format!("{}\n", lines.join("\n"))
}

pub fn compose_dotenv(
    base: Vec<(String, String)>,
    resolved: &[(String, String)],
) -> (String, Vec<String>) {
    let mut merged = base;
    let mut collisions = Vec::new();
    for (k, v) in resolved {
        match merged.iter_mut().find(|(mk, _)| mk == k) {
            Some(slot) => {
                collisions.push(k.clone());
                slot.1 = v.clone();
            }
            None => merged.push((k.clone(), v.clone())),
        }
    }
    (format_dotenv(&merged), collisions)
}
