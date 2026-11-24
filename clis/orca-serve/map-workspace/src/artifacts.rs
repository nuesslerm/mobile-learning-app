//! Files a terminal printed: `files.resolveTerminalPath`, `files.readTerminalArtifact`, `files.readTerminalArtifactPreview` and
//! `files.writeTerminalArtifact`. A path below the workspace opens as a workspace file; any other is granted to the device that tapped it, for ten
//! minutes after its last use, bound to the file's identity and content fingerprint as the executor described them, so a file swapped or rewritten
//! afterwards is refused as stale. What an executor serves outside its root is its owner's switch (`RCA_EXEC_ARTIFACT_ALLOW`, none by default):
//! with it off, such a path is refused as outside. The success shapes are the pinned host's source, authored here (no recording holds one).
use crate::agent::refusal;
use crate::files::{path_param_of, plain, preview_by, text_by};
use crate::target::{resolve, selector, text, Target};
use agent_api_workspace::edit::{ARTIFACT_READ, ARTIFACT_STAT, ARTIFACT_WRITE, FILE_STAT};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use orca_core::wire::Failure;
use orca_core::{AgentApi, AgentError};
use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const GRANT_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_GRANTS: usize = 256;
const WRITE_MAX: usize = 512 * 1024;
const CHUNK: usize = 1 << 20;
const OUTSIDE: &str = "Files outside this workspace are not granted on this host.";

struct Grant {
    id: String,
    owner: String,
    worktree: String,
    path: String,
    identity: String,
    fingerprint: Option<String>,
    expires: Instant,
}

/// The grants minted for taps, newest last.
pub struct Grants {
    all: Mutex<Vec<Grant>>,
    ttl: Duration,
}

impl Default for Grants {
    fn default() -> Self {
        Grants::with_ttl(GRANT_TTL)
    }
}

impl Grants {
    pub fn with_ttl(ttl: Duration) -> Grants {
        Grants { all: Mutex::new(Vec::new()), ttl }
    }

    fn locked(&self) -> std::sync::MutexGuard<'_, Vec<Grant>> {
        self.all.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn mint(&self, owner: &str, worktree: &str, path: &str, identity: String, fingerprint: Option<String>) -> String {
        let id = uuid();
        let mut all = self.locked();
        let now = Instant::now();
        all.retain(|g| g.expires > now);
        if all.len() >= MAX_GRANTS {
            all.remove(0);
        }
        all.push(Grant { id: id.clone(), owner: owner.into(), worktree: worktree.into(), path: path.into(), identity, fingerprint, expires: now + self.ttl });
        id
    }

    /// The grant a call names, still alive and made for this device, worktree and path, with its use renewed.
    fn require(&self, owner: &str, worktree: &str, id: &str, path: &str) -> Result<(String, Option<String>, String), Failure> {
        let mut all = self.locked();
        let now = Instant::now();
        all.retain(|g| g.expires > now);
        let grant = all.iter_mut().find(|g| g.id == id).ok_or_else(|| plain("terminal_file_grant_expired"))?;
        if grant.owner != owner || grant.worktree != worktree || grant.path != path {
            return Err(plain("terminal_file_grant_mismatch"));
        }
        grant.expires = now + self.ttl;
        Ok((grant.identity.clone(), grant.fingerprint.clone(), grant.path.clone()))
    }

    fn pin(&self, id: &str, identity: String, fingerprint: Option<String>) {
        if let Some(grant) = self.locked().iter_mut().find(|g| g.id == id) {
            grant.identity = identity;
            grant.fingerprint = fingerprint;
        }
    }
}

fn uuid() -> String {
    use std::io::Read;
    let mut b = [0u8; 16];
    let read = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b));
    if read.is_err() {
        use std::hash::{BuildHasher, Hasher};
        for half in b.chunks_mut(8) {
            half.copy_from_slice(&std::collections::hash_map::RandomState::new().build_hasher().finish().to_be_bytes());
        }
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

/// The executor's answer, with the refusals a grant knows by their host names.
fn exec(agent: &dyn AgentApi, method: &'static str, params: Value) -> Result<Value, Failure> {
    agent.request(method, params).map_err(|e| match &e {
        AgentError::Refused { code, .. } if code == "CHANGED" => plain("terminal_file_grant_stale"),
        AgentError::Refused { code, .. } if code == "PATH_OUTSIDE_ROOT" => plain(OUTSIDE),
        _ => refusal(e),
    })
}

/// The answer, or none when the file is not there.
fn probe(agent: &dyn AgentApi, method: &'static str, params: Value) -> Result<Option<Value>, Failure> {
    match agent.request(method, params) {
        Ok(v) => Ok(Some(v)),
        Err(AgentError::Refused { code, .. }) if code == "NOT_FOUND" => Ok(None),
        Err(e) => Err(match &e {
            AgentError::Refused { code, .. } if code == "PATH_OUTSIDE_ROOT" => plain(OUTSIDE),
            _ => refusal(e),
        }),
    }
}

/// `base` and `text` joined and folded: the host's `resolveTerminalAbsolutePath` for a plain path or a `file://` address.
fn joined(base: &str, text: &str) -> String {
    let text = text.strip_prefix("file://").map_or(text, |rest| rest.find('/').map_or(rest, |i| &rest[i..]));
    let full = if text.starts_with('/') || base.is_empty() { text.to_string() } else { format!("{}/{text}", base.trim_end_matches('/')) };
    let mut parts: Vec<&str> = Vec::new();
    for part in full.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|p| *p != "..") {
                    parts.pop();
                } else if !full.starts_with('/') {
                    parts.push("..");
                }
            }
            p => parts.push(p),
        }
    }
    format!("{}{}", if full.starts_with('/') { "/" } else { "" }, parts.join("/"))
}

fn below<'a>(root: &str, absolute: &'a str) -> Option<&'a str> {
    let root = root.trim_end_matches('/');
    if root.is_empty() {
        return None;
    }
    absolute.strip_prefix(root).and_then(|rest| if rest.is_empty() { Some("") } else { rest.strip_prefix('/') })
}

fn missing(t: &Target, relative: Option<&str>, absolute: Option<&str>) -> Value {
    json!({"worktree": t.id, "relativePath": relative, "absolutePath": absolute, "exists": false, "isDirectory": false})
}

pub(crate) fn resolve_path(grants: &Grants, agent: &dyn AgentApi, owner: &str, params: &Value) -> Result<Value, Failure> {
    let t = resolve(agent, selector(params)?)?;
    let path_text = text(params, "pathText").filter(|p| !p.is_empty()).ok_or_else(|| Failure::invalid("Missing path text"))?;
    let cwd = text(params, "cwd").filter(|c| !c.is_empty());
    if path_text.starts_with("~/") || path_text.starts_with("~\\") {
        return Ok(missing(&t, None, None));
    }
    let base = cwd.unwrap_or(&t.root_path);
    let absolute = joined(base, path_text);
    let inside = if absolute.starts_with('/') { below(&t.root_path, &absolute).map(str::to_string) } else { Some(absolute.clone()).filter(|p| crate::files::is_safe_path(p)) };
    if let Some(relative) = inside.filter(|r| r.is_empty() || crate::files::is_safe_path(r)) {
        let shown = absolute.as_str();
        let Some(stat) = probe(agent, FILE_STAT.name(), json!({"workspace": t.workspace, "path": if relative.is_empty() { "." } else { relative.as_str() }}))? else {
            return Ok(missing(&t, Some(&relative), Some(shown)));
        };
        let mut found = json!({"worktree": t.id, "relativePath": relative, "absolutePath": shown, "exists": true, "isDirectory": stat["kind"] == "dir"});
        if stat["kind"] != "dir" {
            found["openTarget"] = json!({"kind": "worktree-file", "provider": "local", "relativePath": relative, "absolutePath": shown});
        }
        return Ok(found);
    }
    let Some(stat) = probe(agent, ARTIFACT_STAT.name(), json!({"workspace": t.workspace, "path": absolute}))? else {
        return Ok(missing(&t, None, Some(&absolute)));
    };
    let canonical = stat["path"].as_str().unwrap_or(&absolute).to_string();
    if stat["kind"] == "dir" {
        return Ok(json!({"worktree": t.id, "relativePath": null, "absolutePath": canonical, "exists": true, "isDirectory": true}));
    }
    let grant = grants.mint(owner, &t.id, &canonical, stat["identity"].as_str().unwrap_or("").into(), stat["fingerprint"].as_str().map(str::to_string));
    let binary = stat["binary"] == true || crate::files::is_binary_path(&canonical);
    let mut target = json!({"kind": "absolute-file", "provider": "local", "absolutePath": canonical, "grantId": grant, "byteLength": stat["size"]});
    if binary {
        target["binary"] = json!(true);
    }
    Ok(json!({"worktree": t.id, "relativePath": null, "absolutePath": canonical, "exists": true, "isDirectory": false, "openTarget": target}))
}

struct Named<'a> {
    t: Target,
    grant: &'a str,
    path: &'a str,
}

fn named<'a>(agent: &dyn AgentApi, params: &'a Value) -> Result<Named<'a>, Failure> {
    let t = resolve(agent, selector(params)?)?;
    let grant = text(params, "grantId").filter(|g| !g.is_empty()).ok_or_else(|| Failure::invalid("Missing terminal artifact grant"))?;
    let path = path_param_of(params, "absolutePath", "Missing terminal artifact path")?;
    Ok(Named { t, grant, path })
}

/// Up to `max + 1` bytes of the granted file from its start; each chunk is read under the grant's identity and fingerprint.
fn read_granted(agent: &dyn AgentApi, n: &Named, identity: &str, fingerprint: &Option<String>, path: &str, max: usize) -> Result<Vec<u8>, Failure> {
    let mut bytes: Vec<u8> = Vec::new();
    loop {
        let want = CHUNK.min(max + 1 - bytes.len());
        let mut a = json!({"workspace": n.t.workspace, "path": path, "offset": bytes.len(), "length": want, "identity": identity});
        if let Some(f) = fingerprint {
            a["fingerprint"] = json!(f);
        }
        let r = exec(agent, ARTIFACT_READ.name(), a)?;
        let piece = STANDARD.decode(r["data"].as_str().unwrap_or("")).map_err(|_| plain("invalid_runtime_response"))?;
        bytes.extend_from_slice(&piece);
        if r["eof"] == true || bytes.len() > max || piece.is_empty() {
            return Ok(bytes);
        }
    }
}

pub(crate) fn read(grants: &Grants, agent: &dyn AgentApi, owner: &str, params: &Value) -> Result<Value, Failure> {
    let n = named(agent, params)?;
    let (identity, fingerprint, path) = grants.require(owner, &n.t.id, n.grant, n.path)?;
    let (content, truncated, byte_length) = text_by(&path, &|max| read_granted(agent, &n, &identity, &fingerprint, &path, max))?;
    Ok(json!({"worktree": n.t.id, "relativePath": path, "content": content, "truncated": truncated, "byteLength": byte_length}))
}

pub(crate) fn read_preview(grants: &Grants, agent: &dyn AgentApi, owner: &str, params: &Value) -> Result<Value, Failure> {
    let n = named(agent, params)?;
    let (identity, fingerprint, path) = grants.require(owner, &n.t.id, n.grant, n.path)?;
    preview_by(&path, &|limit| read_granted(agent, &n, &identity, &fingerprint, &path, limit))
}

pub(crate) fn write(grants: &Grants, agent: &dyn AgentApi, owner: &str, params: &Value) -> Result<Value, Failure> {
    let n = named(agent, params)?;
    let content = params["content"].as_str().ok_or_else(|| Failure::invalid("Missing file content"))?;
    if content.len() > WRITE_MAX {
        return Err(plain("file_too_large"));
    }
    let (identity, fingerprint, path) = grants.require(owner, &n.t.id, n.grant, n.path)?;
    if crate::files::is_binary_path(&path) {
        return Err(plain("binary_file"));
    }
    let mut a = json!({"workspace": n.t.workspace, "path": path, "content": content, "identity": identity});
    if let Some(f) = &fingerprint {
        a["fingerprint"] = json!(f);
    }
    let done = exec(agent, ARTIFACT_WRITE.name(), a)?;
    grants.pin(n.grant, done["identity"].as_str().unwrap_or("").into(), done["fingerprint"].as_str().map(str::to_string));
    Ok(json!({"ok": true}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_paths_when_joined_then_dots_fold_and_a_relative_base_stays_relative() {
        assert_eq!(joined("/w", "a/../b/./c"), "/w/b/c");
        assert_eq!(joined("/w", "/etc/../tmp/x"), "/tmp/x");
        assert_eq!(joined("", "a/b"), "a/b");
        assert_eq!(joined("", "../x"), "../x");
        assert_eq!(joined("/w", "file://host/tmp/y"), "/tmp/y");
    }

    #[test]
    fn given_a_root_when_a_path_is_checked_then_only_a_path_below_it_is_inside() {
        assert_eq!(below("/w", "/w/a/b"), Some("a/b"));
        assert_eq!(below("/w", "/w"), Some(""));
        assert_eq!(below("/w", "/wx/a"), None);
        assert_eq!(below("", "/a"), None);
    }
}
