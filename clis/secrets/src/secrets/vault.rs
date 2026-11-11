//! The vault write side: `bws secret create|edit` through an argv array (no shell, so free-form notes and values are never
//! interpreted), the separate edit credential, and the owner-only rotation-history log.
use super::audit::read_inventory;
use super::backend::resolve_backend_config;
use wf_kit::time::now_ms;
use wf_kit::time::iso;
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub struct VaultEntry {
    pub id: String,
    pub key: String,
    pub project_id: String,
}

fn run_bws(args: &[&str], token: &str, server_url: Option<&str>) -> Result<String, String> {
    let mut cmd = Command::new("bws");
    cmd.args(args).env("BWS_ACCESS_TOKEN", token).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(url) = server_url {
        cmd.env("BWS_SERVER_URL", url);
    }
    match cmd.output() {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err("ENOENT".into()),
        Err(e) => Err(e.to_string()),
        Ok(out) if out.status.success() => Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            // The TypeScript falls back to the command line in its message, which would carry the secret value.
            Err(if stderr.is_empty() { format!("Command failed with exit code {}", out.status.code().unwrap_or(-1)) } else { stderr })
        }
    }
}

fn has_word(detail: &str, words: &[&str]) -> bool {
    let lower = detail.to_lowercase();
    words.iter().any(|w| lower.contains(w))
}

struct Tokens {
    edit: Option<String>,
    read: Option<String>,
    server: Option<String>,
}

fn tokens() -> Result<Tokens, String> {
    let edit = resolve_backend_config("BWS_ADMIN_ACCESS_TOKEN", "bws-admin-access-token")?;
    let read = match resolve_backend_config("BWS_ADMIN_ACCESS_TOKEN", "bws-admin-access-token")? {
        Some(t) => Some(t),
        None => resolve_backend_config("BWS_ACCESS_TOKEN", "bws-access-token")?,
    };
    let server = resolve_backend_config("BWS_SERVER_URL", "bws-server-url")?;
    Ok(Tokens { edit, read, server })
}

fn missing_edit(what: &str) -> String {
    format!(
        "BWS_ADMIN_ACCESS_TOKEN is not set, so {what} cannot be performed. It needs the Bitwarden machine account `vm-admin` ('Can read, write' on both vault projects), which never goes into GitHub. Re-run without --apply to preview."
    )
}

/* ---------- create / rename / update ---------- */

pub struct VaultWriter {
    edit: Option<String>,
    server: Option<String>,
}

pub fn vault_writer() -> Result<VaultWriter, String> {
    let t = tokens()?;
    if t.edit.is_none() && t.read.is_none() {
        return Err("No Bitwarden token available. Set BWS_ADMIN_ACCESS_TOKEN to preview and apply mutations.".into());
    }
    Ok(VaultWriter { edit: t.edit, server: t.server })
}

impl VaultWriter {
    fn fail(&self, action: &str, detail: &str) -> String {
        let mut hints = String::new();
        if (detail.contains("invalid_client") || detail.contains("401")) && self.server.is_none() {
            hints.push_str(" If your Bitwarden account is EU-hosted, set BWS_SERVER_URL=https://vault.bitwarden.eu.");
        }
        if detail.contains("403") || detail.contains("404") || has_word(detail, &["forbidden", "not found"]) {
            hints.push_str(" A 403/404 on a write usually means the token authenticated but lacks 'Can read, write' on that project — Bitwarden reports an unauthorized write as 404, not 403.");
        }
        format!("Bitwarden {action} failed: {detail}{hints}")
    }

    fn write(&self, what: &str, action: &str, args: &[&str]) -> Result<(), String> {
        let token = self.edit.as_deref().ok_or_else(|| missing_edit(what))?;
        run_bws(args, token, self.server.as_deref()).map(|_| ()).map_err(|d| self.fail(action, &d))
    }

    /// Every secret any held credential can see, deduplicated by id; values are never read.
    pub fn list(&self) -> Result<Vec<VaultEntry>, String> {
        Ok(read_inventory()?.entries.into_iter().map(|(id, key, project_id)| VaultEntry { id, key, project_id }).collect())
    }

    pub fn create(&self, key: &str, value: &str, project_id: &str, note: &str) -> Result<(), String> {
        self.write(&format!("creating {key}"), &format!("secret create {key}"), &["secret", "create", "--note", note, "--output", "none", "--", key, value, project_id])
    }

    pub fn rename(&self, id: &str, new_key: &str) -> Result<(), String> {
        self.write(&format!("renaming {id}"), &format!("secret edit {id}"), &["secret", "edit", "--key", new_key, id, "--output", "none"])
    }

    pub fn move_to(&self, id: &str, project_id: &str) -> Result<(), String> {
        self.write(&format!("moving {id}"), &format!("secret edit {id}"), &["secret", "edit", "--project-id", project_id, id, "--output", "none"])
    }

    pub fn update_value(&self, id: &str, value: &str) -> Result<(), String> {
        self.write(&format!("updating the value of {id}"), &format!("secret edit {id}"), &["secret", "edit", "--value", value, "--output", "none", "--", id])
    }
}

/* ---------- notes ---------- */

pub struct VaultNote {
    pub id: String,
    pub key: String,
    pub note: String,
}

pub struct NoteWriter {
    edit: Option<String>,
    read: Option<String>,
    server: Option<String>,
}

pub fn note_writer() -> Result<NoteWriter, String> {
    let t = tokens()?;
    if t.edit.is_none() && t.read.is_none() {
        return Err("No Bitwarden token available. Set BWS_ADMIN_ACCESS_TOKEN to preview and apply note changes.".into());
    }
    Ok(NoteWriter { edit: t.edit, read: t.read, server: t.server })
}

impl NoteWriter {
    fn fail(&self, action: &str, detail: &str) -> String {
        let mut hints = String::new();
        if (detail.contains("invalid_client") || detail.contains("401")) && self.server.is_none() {
            hints.push_str(" If your Bitwarden account is EU-hosted, set BWS_SERVER_URL=https://vault.bitwarden.eu.");
        }
        if detail.contains("403") || has_word(detail, &["forbidden", "permission"]) {
            hints.push_str(" The token authenticated but was refused — grant its machine account 'Can read, write' on the project.");
        }
        format!("Bitwarden {action} failed: {detail}{hints}")
    }

    pub fn list(&self) -> Result<Vec<VaultNote>, String> {
        let token = self.read.as_deref().or(self.edit.as_deref()).expect("a token");
        let out = run_bws(&["secret", "list", "--output", "json"], token, self.server.as_deref()).map_err(|d| self.fail("secret list", &d))?;
        let parsed: Value = serde_json::from_str(&out).map_err(|_| format!("Bitwarden returned unparseable JSON: {}", out.chars().take(200).collect::<String>()))?;
        Ok(parsed
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|s| VaultNote {
                        id: s["id"].as_str().unwrap_or("").to_string(),
                        key: s["key"].as_str().unwrap_or("").to_string(),
                        note: s["note"].as_str().unwrap_or("").to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn set_note(&self, id: &str, note: &str) -> Result<(), String> {
        let Some(token) = self.edit.as_deref() else {
            return Err("BWS_ADMIN_ACCESS_TOKEN is not set, so notes cannot be written. It needs the Bitwarden machine account `vm-admin` ('Can read, write' on both vault projects): `wf secret set BWS_ADMIN_ACCESS_TOKEN`. Re-run without --apply to preview changes.".into());
        };
        run_bws(&["secret", "edit", "--note", note, id, "--output", "none"], token, self.server.as_deref())
            .map(|_| ())
            .map_err(|d| self.fail(&format!("secret edit {id}"), &d))
    }
}

/* ---------- rotation history ---------- */

pub fn history_path() -> PathBuf {
    let base = std::env::var("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|_| {
        PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "~".into())).join(".local").join("state")
    });
    base.join("wf-secrets").join("rotation-history.jsonl")
}

/// Appends the value about to be overwritten, owner-only. Never fails the caller: the outcome is the return value.
pub fn record_history(key: &str, backend: &str, previous: Option<&str>) -> (bool, PathBuf) {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
    let path = history_path();
    let ok = (|| -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        }
        let line = format!("{}\n", json!({ "timestamp": iso(now_ms()), "key": key, "backend": backend, "previousValue": previous }));
        let mut f = std::fs::OpenOptions::new().create(true).append(true).mode(0o600).open(&path)?;
        f.write_all(line.as_bytes())?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
    })()
    .is_ok();
    (ok, path)
}
