//! A local, encrypted copy of what `bws secret list` returned, so a wf command does not wait on the vault (about a second and
//! a network round trip each) and a vault outage does not stop a deploy: a fresh copy is used as is, a stale one stands in when
//! the vault cannot be reached.
//!
//! The copy is sealed with XChaCha20-Poly1305 under a key derived from the access token that read it, so it opens only for
//! the holder of that token and is useless when the token is rotated. The file is 0600 in a 0700 directory. Only the read
//! sources are kept; the write-capable (admin) list is always fetched live.
//!
//! `WF_SECRETS_CACHE=off` skips it, `refresh` fetches and rewrites it; `wf secret sync-env` refreshes, and a successful vault
//! write clears it.
use wf_kit::time::now_ms;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

const MAGIC: &[u8; 4] = b"WFS1";
const NONCE_LEN: usize = 24;
const DEFAULT_TTL_SEC: i64 = 3600;
/// Past this a copy is not trusted even when the vault is down: a secret may have been rotated long since.
const MAX_STALE_SEC: i64 = 7 * 24 * 3600;

static REFRESH: AtomicBool = AtomicBool::new(false);

/// Makes this process fetch from the vault and rewrite the copy, whatever its age.
pub fn refresh_this_run() {
    REFRESH.store(true, Ordering::Relaxed);
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    Use,
    Refresh,
    Off,
}

pub fn mode() -> Mode {
    match std::env::var("WF_SECRETS_CACHE").ok().as_deref() {
        Some("off") | Some("0") => Mode::Off,
        Some("refresh") => Mode::Refresh,
        _ if REFRESH.load(Ordering::Relaxed) => Mode::Refresh,
        _ => Mode::Use,
    }
}

fn ttl_sec() -> i64 {
    std::env::var("WF_SECRETS_CACHE_TTL_SEC").ok().and_then(|v| v.parse().ok()).unwrap_or(DEFAULT_TTL_SEC)
}

fn dir() -> Option<PathBuf> {
    let base = std::env::var_os("WF_CACHE_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CACHE_HOME").map(|d| PathBuf::from(d).join("wf")))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/wf")))?;
    Some(base)
}

fn digest_hex(text: &str, len: usize) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))[..len].to_string()
}

fn file_for(label: &str, token: &str, server_url: &Option<String>) -> Option<PathBuf> {
    let slug: String = label.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let server = server_url.as_deref().unwrap_or("");
    Some(dir()?.join(format!("secrets-{slug}-{}-{}.bin", digest_hex(token, 12), digest_hex(server, 6))))
}

fn key_for(token: &str, server_url: &Option<String>) -> [u8; 32] {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(token.as_bytes()).expect("hmac accepts any key length");
    mac.update(b"wf-secrets-cache-v1\0");
    mac.update(server_url.as_deref().unwrap_or("").as_bytes());
    mac.finalize().into_bytes().into()
}

pub fn seal(token: &str, server_url: &Option<String>, secrets: &HashMap<String, String>, fetched_at_ms: i64) -> Result<Vec<u8>, String> {
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
    let plain = json!({ "fetchedAtMs": fetched_at_ms, "secrets": secrets }).to_string();
    let cipher = XChaCha20Poly1305::new(&key_for(token, server_url).into());
    let sealed = cipher.encrypt(XNonce::from_slice(&nonce), plain.as_bytes()).map_err(|_| "cannot seal the secrets cache".to_string())?;
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&sealed);
    Ok(out)
}

/// `(fetched_at_ms, secrets)`; `None` for a file that is not ours, was sealed under another token or was altered.
pub fn open(token: &str, server_url: &Option<String>, bytes: &[u8]) -> Option<(i64, HashMap<String, String>)> {
    if bytes.len() < MAGIC.len() + NONCE_LEN || &bytes[..MAGIC.len()] != MAGIC {
        return None;
    }
    let (nonce, sealed) = bytes[MAGIC.len()..].split_at(NONCE_LEN);
    let cipher = XChaCha20Poly1305::new(&key_for(token, server_url).into());
    let plain = cipher.decrypt(XNonce::from_slice(nonce), sealed).ok()?;
    let parsed: Value = serde_json::from_slice(&plain).ok()?;
    let secrets = parsed["secrets"].as_object()?.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect();
    Some((parsed["fetchedAtMs"].as_i64()?, secrets))
}

pub struct Copy {
    pub age_sec: i64,
    pub secrets: HashMap<String, String>,
}

impl Copy {
    pub fn is_fresh(&self) -> bool {
        self.age_sec <= ttl_sec()
    }

    pub fn is_usable_when_the_vault_is_down(&self) -> bool {
        self.age_sec <= MAX_STALE_SEC
    }
}

pub fn load(label: &str, token: &Option<String>, server_url: &Option<String>) -> Option<Copy> {
    let token = token.as_deref()?;
    let bytes = std::fs::read(file_for(label, token, server_url)?).ok()?;
    let (fetched_at, secrets) = open(token, server_url, &bytes)?;
    Some(Copy { age_sec: ((now_ms() - fetched_at) / 1000).max(0), secrets })
}

/// Best effort: a cache that cannot be written only costs the next call a fetch.
pub fn store(label: &str, token: &Option<String>, server_url: &Option<String>, secrets: &HashMap<String, String>) {
    let Some(token) = token.as_deref() else { return };
    let Some(file) = file_for(label, token, server_url) else { return };
    let Ok(bytes) = seal(token, server_url, secrets, now_ms()) else { return };
    let Some(parent) = file.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
    let tmp = file.with_extension(format!("tmp{}", std::process::id()));
    let written = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp).and_then(|mut f| std::io::Write::write_all(&mut f, &bytes));
    if written.is_err() || std::fs::rename(&tmp, &file).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Removes every copy; how many files went.
pub fn clear() -> usize {
    let Some(dir) = dir() else { return 0 };
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("secrets-") && std::fs::remove_file(e.path()).is_ok())
        .count()
}
