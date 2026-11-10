use super::backend::{resolve_backend_config, run_cli, Backend};
use super::registry::Entry;
use super::util::is_placeholder;
use serde_json::Value;
use std::collections::{BTreeSet, HashSet};

pub const SCOPES: &[(&str, &str, &str)] = &[
    (
        "deploy",
        "BWS_DEPLOY_SECRETS_READ_ACCESS_TOKEN",
        "bws-deploy-secrets-read-access-token",
    ),
    (
        "gh-ci",
        "BWS_GH_CI_SECRETS_READ_ACCESS_TOKEN",
        "bws-gh-ci-secrets-read-access-token",
    ),
    (
        "admin",
        "BWS_DEPLOY_SECRETS_EDIT_ACCESS_TOKEN",
        "bws-deploy-secrets-edit-access-token",
    ),
];

pub struct Inventory {
    pub keys: Vec<String>,
    /// Id, key and project of every listed secret; values are never read.
    pub entries: Vec<(String, String, String)>,
    pub unreachable_scopes: Vec<String>,
}

pub fn has_any_scope_credential() -> Result<bool, String> {
    for (_, env_var, service) in SCOPES {
        if resolve_backend_config(env_var, service)?.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Unions the vault listing across every scope credential held, deduplicated by vault id. Names only: values are never read.
pub fn read_inventory() -> Result<Inventory, String> {
    let server_url = resolve_backend_config("BWS_SERVER_URL", "bws-server-url")?;
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut keys = Vec::new();
    let mut entries = Vec::new();
    let mut unreachable = Vec::new();
    for (scope, env_var, service) in SCOPES {
        let Some(token) = resolve_backend_config(env_var, service)? else {
            unreachable.push((*scope).to_string());
            continue;
        };
        let mut env = vec![("BWS_ACCESS_TOKEN".to_string(), token)];
        if let Some(u) = &server_url {
            env.push(("BWS_SERVER_URL".into(), u.clone()));
        }
        let listed = run_cli("bws", &["secret", "list", "--output", "json"], &env, false)
            .ok()
            .and_then(|out| serde_json::from_str::<Value>(&out).ok());
        // A credential that exists but cannot list is the same blind spot as a missing one.
        let Some(Value::Array(items)) = listed else {
            unreachable.push((*scope).to_string());
            continue;
        };
        for s in items {
            let (Some(id), Some(key)) = (
                s.get("id").and_then(Value::as_str),
                s.get("key").and_then(Value::as_str),
            ) else {
                continue;
            };
            if seen_ids.insert(id.to_string()) {
                keys.push(key.to_string());
                let project = s.get("projectId").and_then(Value::as_str).unwrap_or("").to_string();
                entries.push((id.to_string(), key.to_string(), project));
            }
        }
    }
    if unreachable.len() == SCOPES.len() {
        let names: Vec<&str> = SCOPES.iter().map(|(_, e, _)| *e).collect();
        return Err(format!(
            "No Bitwarden credential could list any scope, so there is nothing to audit against. Set at least one of: {}.",
            names.join(", ")
        ));
    }
    Ok(Inventory {
        keys,
        entries,
        unreachable_scopes: unreachable,
    })
}

const STRUCTURAL: &[&str] = &[
    "ACCESS",
    "API",
    "AUTH",
    "CONFIG",
    "CREDENTIAL",
    "DEFAULT",
    "ENV",
    "EXPO",
    "HOST",
    "KEY",
    "NAME",
    "PASS",
    "PASSWORD",
    "PORT",
    "PRIVATE",
    "PROD",
    "PUBLIC",
    "SECRET",
    "SECRETS",
    "SERVER",
    "TABLE",
    "TEST",
    "TOKEN",
    "URL",
    "USER",
];

fn distinctive_parts(v: &str) -> BTreeSet<String> {
    v.to_uppercase()
        .split(|c: char| !c.is_ascii_uppercase() && !c.is_ascii_digit())
        .filter(|t| t.chars().count() > 2 && !STRUCTURAL.contains(t))
        .map(str::to_string)
        .collect()
}

fn looks_related(a: &str, b: &str) -> bool {
    let pb = distinctive_parts(b);
    distinctive_parts(a).iter().any(|t| pb.contains(t))
}

pub struct Mismatch {
    pub key: String,
    pub wanted: String,
    pub candidates: Vec<String>,
}

pub struct Report {
    pub mismatch_suspects: Vec<Mismatch>,
    pub unregistered: Vec<String>,
    pub not_provisioned: Vec<String>,
    pub not_assessed: Vec<String>,
    pub placeholders: Vec<String>,
    pub registry_count: usize,
    pub vault_count: usize,
    pub unreachable_scopes: Vec<String>,
}

pub fn audit_drift(registry: &[Entry], backend: &mut dyn Backend) -> Result<Report, String> {
    let wanted: HashSet<String> = registry
        .iter()
        .map(|e| e.bitwarden_key.clone().unwrap_or_else(|| e.env_var.clone()))
        .collect();
    let inventory = read_inventory()?;
    let mut unregistered: Vec<String> = inventory
        .keys
        .iter()
        .filter(|k| !wanted.contains(*k))
        .cloned()
        .collect();
    unregistered.sort();
    let mut report = Report {
        mismatch_suspects: Vec::new(),
        unregistered: unregistered.clone(),
        not_provisioned: Vec::new(),
        not_assessed: Vec::new(),
        placeholders: Vec::new(),
        registry_count: registry.len(),
        vault_count: inventory.keys.len(),
        unreachable_scopes: inventory.unreachable_scopes.clone(),
    };
    for entry in registry {
        // Bootstrap credentials come from env/keychain, never the vault.
        if entry.bootstrap {
            continue;
        }
        let scope = entry.vault.as_deref().unwrap_or("deploy");
        if inventory.unreachable_scopes.iter().any(|s| s == scope) {
            report.not_assessed.push(entry.key.clone());
            continue;
        }
        let value = backend.resolve(entry)?;
        if let Some(v) = value.as_deref().filter(|v| !v.is_empty()) {
            if is_placeholder(Some(v)) {
                report.placeholders.push(entry.key.clone());
            }
            continue;
        }
        let wanted_key = entry
            .bitwarden_key
            .clone()
            .unwrap_or_else(|| entry.env_var.clone());
        let candidates: Vec<String> = unregistered
            .iter()
            .filter(|k| looks_related(k, &wanted_key))
            .cloned()
            .collect();
        if candidates.is_empty() {
            report.not_provisioned.push(entry.key.clone());
        } else {
            report.mismatch_suspects.push(Mismatch {
                key: entry.key.clone(),
                wanted: wanted_key,
                candidates,
            });
        }
    }
    Ok(report)
}

/// Human-readable drift report. Key names only, never a value.
pub fn format_report(r: &Report) -> String {
    let mut lines: Vec<String> = vec![format!(
        "registry: {} entr(ies)   vault: {} secret(s)",
        r.registry_count, r.vault_count
    )];
    if !r.mismatch_suspects.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "NAME MISMATCH (likely) — registered, unresolvable, but a similar vault secret exists ({}):",
            r.mismatch_suspects.len()
        ));
        for m in &r.mismatch_suspects {
            let cands: Vec<String> = m.candidates.iter().map(|c| format!("\"{c}\"")).collect();
            lines.push(format!(
                "  {}: wants \"{}\", vault has {}",
                m.key,
                m.wanted,
                cands.join(", ")
            ));
        }
        lines.push(
            "  Bridge with `bitwardenKey` in registry.json. Until then the value is silently"
                .into(),
        );
        lines.push("  omitted from every generated .env.".into());
    }
    if !r.unregistered.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "UNREGISTERED — in the vault, reachable by nothing ({}):",
            r.unregistered.len()
        ));
        lines.extend(r.unregistered.iter().map(|k| format!("  {k}")));
        lines.push(
            "  These cannot appear in `wf secret list` and no sync-env will ever deliver them."
                .into(),
        );
        lines.push("  Add a registry entry, or delete the secret if it is genuinely dead.".into());
    }
    if !r.not_provisioned.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "NOT PROVISIONED (advisory, does not fail) — registered, absent, nothing similar in the vault ({}):",
            r.not_provisioned.len()
        ));
        lines.extend(r.not_provisioned.iter().map(|k| format!("  {k}")));
        lines.push(
            "  A provisioning backlog, not drift: `wf secret list` says where each value comes from.".into(),
        );
    }
    if !r.placeholders.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "PLACEHOLDER (advisory, does not fail) — in the vault, created but never filled in ({}):",
            r.placeholders.len()
        ));
        lines.extend(r.placeholders.iter().map(|k| format!("  {k}")));
        lines.push("  These RESOLVE, so every presence check passes and `wf secret list` shows them as set,".into());
        lines.push("  while being exactly as unconfigured as an absent value. sync-env treats them as missing.".into());
        lines.push("  Fill with `wf secret set <KEY>`.".into());
    }
    if !r.unreachable_scopes.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "PARTIAL INVENTORY — no credential could list {} scope(s): {}",
            r.unreachable_scopes.len(),
            r.unreachable_scopes.join(", ")
        ));
        lines.push(format!(
            "  {} registry entr(ies) belong to those scopes and were NOT assessed.",
            r.not_assessed.len()
        ));
        lines.push(
            "  Secrets in those projects are invisible to this run, so a clean result does NOT"
                .into(),
        );
        lines.push(
            "  rule out drift. Export that scope's token and re-run for a complete audit.".into(),
        );
    }
    if r.mismatch_suspects.is_empty() && r.unregistered.is_empty() {
        lines.push(String::new());
        lines.push(if r.unreachable_scopes.is_empty() {
            "ok: no drift between registry and vault".into()
        } else {
            "no drift found in the scopes that were readable — inventory incomplete, see above"
                .into()
        });
    }
    lines.join("\n")
}

pub fn is_list_failure(message: &str) -> bool {
    message.to_lowercase().contains("list failed")
}
