//! `git.diff`, `git.branchDiff` and `git.commitDiff`: one file's two sides in the result the pinned host builds from them (`buildDiffResult`): text, or
//! binary with the image or PDF preview, or text withheld over the render limit. A commit's or the index's side is the executor's `git.blob`, which
//! reads up to 8 MiB where the pinned host reads 10 (a side between the two is `tooLarge` here); the working tree's side is `file.stat` and
//! `file.readChunk`, capped at the host's 10 MiB. Not reproduced: submodule routing, textconv drivers, and a symlink that leaves the workspace
//! (rca refuses it, so the side is empty where the host reads through it).
use crate::{text, Repo};
use agent_api_git::BLOB;
use orca_map_workspace::{FILE_READ_CHUNK, FILE_STAT};
use base64::Engine;
use orca_core::wire::Failure;
use orca_core::AgentError;
use serde_json::{json, Value};

const MAX_BLOB: u64 = 8 << 20;
const HOST_MAX: u64 = 10 << 20;
const PROBE: usize = 8192;
const MAX_LINES: usize = 120_000;
const MAX_CHARS: usize = 6_000_000;
const CHUNK: u64 = 1 << 20;
/// `MAX_REMOTE_RPC_RESPONSE_BYTES` less its envelope headroom: a remote reply larger than this is refused rather than sent.
const REPLY_MAX: usize = 4 * 1024 * 1024 - 4 * 1024;

struct Side {
    content: String,
    binary: bool,
    too_large: bool,
}

fn preview_mime(path: &str) -> Option<&'static str> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or("");
    let ext = match name.rfind('.') {
        Some(dot) if dot > 0 => name[dot..].to_ascii_lowercase(),
        _ => return None,
    };
    Some(match ext.as_str() {
        ".png" => "image/png",
        ".jpg" | ".jpeg" => "image/jpeg",
        ".gif" => "image/gif",
        ".svg" => "image/svg+xml",
        ".webp" => "image/webp",
        ".bmp" => "image/bmp",
        ".ico" => "image/x-icon",
        ".avif" => "image/avif",
        ".pdf" => "application/pdf",
        _ => return None,
    })
}

fn absent() -> Side {
    Side { content: String::new(), binary: false, too_large: false }
}

/// One side as read: its bytes, or proven absent, or unreadable (both of the last are an empty side; only a proven absence counts as deleted).
enum Read {
    Found(Side),
    Missing,
    Failed,
}

impl Read {
    fn side(self) -> Side {
        match self {
            Read::Found(side) => side,
            Read::Missing | Read::Failed => absent(),
        }
    }
}

fn too_large() -> Side {
    Side { content: String::new(), binary: true, too_large: true }
}

/// `bufferToBlob`: a NUL in the first 8192 bytes is binary, kept in base64 only for a path the phone can preview.
fn side_of(bytes: &[u8], as_path: &str) -> Side {
    let binary = bytes.iter().take(PROBE).any(|b| *b == 0);
    let content = if binary {
        if preview_mime(as_path).is_some() { base64::engine::general_purpose::STANDARD.encode(bytes) } else { String::new() }
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    Side { content, binary, too_large: false }
}

/// The file at `rev`, or as staged when no revision is named: a path the executor does not find is absent, any other refusal unreadable.
fn read_git(repo: &Repo, rev: Option<&str>, path: &str, as_path: &str) -> Result<Read, Failure> {
    let mut args = json!({"path": path, "max_bytes": MAX_BLOB});
    if let Some(rev) = rev {
        args["rev"] = json!(rev);
    }
    let blob = match repo.try_ask(BLOB, args) {
        Ok(blob) => blob,
        Err(AgentError::Refused { code, .. }) if code == "UNKNOWN_WORKSPACE" => return Err(Failure::new("selector_not_found", "selector_not_found")),
        Err(AgentError::Refused { code, .. }) if code == "NOT_FOUND" => return Ok(Read::Missing),
        Err(AgentError::Refused { .. }) => return Ok(Read::Failed),
        Err(AgentError::Failed(_)) => return Err(Failure::new("runtime_unavailable", "runtime_unavailable")),
    };
    if blob["size"].as_u64().unwrap_or(0) > HOST_MAX || blob["truncated"] == true {
        return Ok(Read::Found(too_large()));
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(blob["data"].as_str().unwrap_or("")).unwrap_or_default();
    Ok(Read::Found(side_of(&bytes, as_path)))
}

/// The file at `rev`: a path the commit lacks (or any read the executor refuses) is an empty side, as the pinned host reads a missing blob.
fn read_side(repo: &Repo, rev: &str, path: &str, as_path: &str) -> Result<Side, Failure> {
    read_git(repo, Some(rev), path, as_path).map(Read::side)
}

/// `readWorkingTreeFile`: a path that is not there, or is a directory, is absent; one over 10 MiB is too large and not read.
fn read_worktree(repo: &Repo, path: &str) -> Result<Read, Failure> {
    let stat = match repo.try_ask(FILE_STAT, json!({"path": path})) {
        Ok(stat) => stat,
        Err(AgentError::Refused { code, .. }) if code == "UNKNOWN_WORKSPACE" => return Err(Failure::new("selector_not_found", "selector_not_found")),
        Err(AgentError::Refused { code, .. }) if code == "NOT_FOUND" => return Ok(Read::Missing),
        Err(AgentError::Refused { .. }) => return Ok(Read::Failed),
        Err(AgentError::Failed(_)) => return Err(Failure::new("runtime_unavailable", "runtime_unavailable")),
    };
    if stat["kind"] == "dir" {
        return Ok(Read::Missing);
    }
    if stat["size"].as_u64().unwrap_or(0) > HOST_MAX {
        return Ok(Read::Found(too_large()));
    }
    let mut bytes = Vec::new();
    loop {
        let length = CHUNK.min(HOST_MAX + 1 - bytes.len() as u64);
        let piece = match repo.try_ask(FILE_READ_CHUNK, json!({"path": path, "offset": bytes.len(), "length": length})) {
            Ok(r) => r,
            Err(AgentError::Failed(_)) => return Err(Failure::new("runtime_unavailable", "runtime_unavailable")),
            Err(_) => return Ok(Read::Failed),
        };
        let Ok(data) = base64::engine::general_purpose::STANDARD.decode(piece["data"].as_str().unwrap_or("")) else { return Ok(Read::Failed) };
        bytes.extend_from_slice(&data);
        if bytes.len() as u64 > HOST_MAX {
            return Ok(Read::Found(too_large()));
        }
        if piece["eof"] == true || data.is_empty() {
            return Ok(Read::Found(side_of(&bytes, path)));
        }
    }
}

/// `countLinesEmptyAsZeroUpToLimit`: 0 for nothing, else one more than the newlines, stopping once past the limit.
fn lines_up_to(content: &str) -> (usize, bool) {
    if content.is_empty() {
        return (0, false);
    }
    let mut count = 1;
    for b in content.bytes() {
        if b == b'\n' {
            count += 1;
            if count > MAX_LINES {
                return (count, true);
            }
        }
    }
    (count, false)
}

fn build(left: Side, right: Side, path: &str) -> Value {
    if left.binary || right.binary {
        let mut out = json!({"kind": "binary", "originalContent": left.content, "modifiedContent": right.content, "originalIsBinary": left.binary, "modifiedIsBinary": right.binary});
        if let Some(mime) = preview_mime(path) {
            out["isImage"] = true.into();
            out["mimeType"] = mime.into();
        }
        if left.too_large || right.too_large {
            out["tooLarge"] = true.into();
        }
        return out;
    }
    let chars = left.content.encode_utf16().count() + right.content.encode_utf16().count();
    let limits = json!({"maxLinesPerSide": MAX_LINES, "maxCombinedCharacters": MAX_CHARS});
    let limited = if chars > MAX_CHARS {
        Some(json!({"limited": true, "reason": "character-count", "lineCounts": null, "characterCount": chars, "limits": limits}))
    } else {
        let ((lo, eo), (lm, em)) = (lines_up_to(&left.content), lines_up_to(&right.content));
        (eo || em).then(|| json!({"limited": true, "reason": "line-count", "lineCounts": {"original": lo, "modified": lm}, "lineCountsAreMinimum": {"original": eo, "modified": em}, "characterCount": chars, "limits": limits}))
    };
    match limited {
        Some(limit) => json!({"kind": "text", "originalContent": "", "modifiedContent": "", "originalIsBinary": false, "modifiedIsBinary": false, "largeDiffRenderLimit": limit}),
        None => json!({"kind": "text", "originalContent": left.content, "modifiedContent": right.content, "originalIsBinary": false, "modifiedIsBinary": false}),
    }
}

/// `normalizeRuntimeGitRelativePath`: backslashes become `/`, a trailing `/` goes, and an empty, absolute or `.`/`..` path is the host's plain error.
fn relative(path: &str) -> Result<String, Failure> {
    let slashed = path.replace('\\', "/");
    let trimmed = slashed.trim_end_matches('/');
    let drive = trimmed.len() >= 2 && trimmed.as_bytes()[0].is_ascii_alphabetic() && trimmed.as_bytes()[1] == b':' && trimmed.len() > 2 && trimmed.as_bytes()[2] == b'/';
    if trimmed.is_empty() || trimmed.starts_with('/') || drive || trimmed.split('/').any(|s| matches!(s, "" | "." | "..")) {
        return Err(Failure::new(orca_core::wire::RUNTIME_ERROR, "invalid_relative_path"));
    }
    Ok(trimmed.to_string())
}

fn file_path(params: &Value) -> Result<String, Failure> {
    text(params, "filePath").map(str::to_string).ok_or_else(|| Failure::invalid("Missing file path"))
}

fn full_id(v: Option<&Value>) -> Result<String, Failure> {
    match v {
        None => Err(Failure::invalid("Invalid input: expected string, received undefined")),
        Some(Value::String(s)) if matches!(s.len(), 40 | 64) && s.bytes().all(|b| b.is_ascii_hexdigit()) => Ok(s.clone()),
        Some(Value::String(_)) => Err(Failure::invalid("Expected a full git object id")),
        Some(other) => Err(Failure::invalid(format!("Invalid input: expected string, received {}", orca_core::wire::received(other)))),
    }
}

fn old_path(params: &Value) -> Result<Option<String>, Failure> {
    match params.get("oldPath") {
        None => Ok(None),
        // an empty `oldPath` is no `oldPath`, as the host reads it by truthiness
        Some(Value::String(s)) if s.is_empty() => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(other) => Err(Failure::invalid(format!("Invalid input: expected string, received {}", orca_core::wire::received(other)))),
    }
}

/// `git.branchDiff`: the file at the merge base against the file at the head.
pub(crate) fn branch(repo: &Repo, params: &Value) -> Result<Value, Failure> {
    let path = file_path(params)?;
    let compare = match params.get("compare") {
        None => return Err(Failure::invalid("Invalid input: expected object, received undefined")),
        Some(c) if c.is_object() => c,
        Some(other) => return Err(Failure::invalid(format!("Invalid input: expected object, received {}", orca_core::wire::received(other)))),
    };
    let (head, base) = (full_id(compare.get("headOid"))?, full_id(compare.get("mergeBase"))?);
    let old = old_path(params)?.map(|p| relative(&p)).transpose()?;
    let path = relative(&path)?;
    let left_path = old.as_deref().unwrap_or(&path);
    // the pinned host answers an empty text result for any read it cannot make
    let sides = read_side(repo, &base, left_path, &path).and_then(|l| read_side(repo, &head, &path, &path).map(|r| (l, r)));
    match sides {
        Ok((l, r)) => Ok(build(l, r, &path)),
        Err(e) if e.code == "selector_not_found" => Err(e),
        Err(_) => Ok(build(absent(), absent(), &path)),
    }
}

/// `git.commitDiff`: the file at the commit's parent (nothing for a root commit) against the file at the commit.
pub(crate) fn commit(repo: &Repo, params: &Value) -> Result<Value, Failure> {
    let path = file_path(params)?;
    let commit = full_id(params.get("commitOid"))?;
    let parent = match params.get("parentOid") {
        None | Some(Value::Null) => None,
        other => Some(full_id(other)?),
    };
    let old = old_path(params)?.map(|p| relative(&p)).transpose()?;
    let path = relative(&path)?;
    let left = match &parent {
        Some(p) => read_side(repo, p, old.as_deref().unwrap_or(&path), &path)?,
        None => absent(),
    };
    Ok(build(left, read_side(repo, &commit, &path, &path)?, &path))
}

/// The params of `git.diff`, checked in the pinned schema's order before the selector is resolved (only the first problem is answered).
pub(crate) struct WorkingParams {
    file_path: String,
    staged: bool,
    against_head: bool,
    old_path: Option<String>,
}

fn kind_of(v: Option<&Value>) -> &'static str {
    v.map_or("undefined", orca_core::wire::received)
}

pub(crate) fn working_params(params: &Value) -> Result<WorkingParams, Failure> {
    text(params, "worktree").ok_or_else(|| Failure::invalid("Missing worktree selector"))?;
    let file_path = text(params, "filePath").ok_or_else(|| Failure::invalid("Missing file path"))?.to_string();
    let staged = match params.get("staged") {
        Some(Value::Bool(b)) => *b,
        other => return Err(Failure::invalid(format!("Invalid input: expected boolean, received {}", kind_of(other)))),
    };
    let against_head = match params.get("compareAgainstHead") {
        None => false,
        Some(Value::Bool(b)) => *b,
        other => return Err(Failure::invalid(format!("Invalid input: expected boolean, received {}", kind_of(other)))),
    };
    Ok(WorkingParams { file_path, staged, against_head, old_path: old_path(params)? })
}

/// `git.diff`: staged, the file at HEAD (at its old path for a rename) against the index; unstaged, the index (or HEAD for a file the index lacks, or
/// HEAD when asked) against the working tree. A binary result whose new side is proven gone says so, and a reply over the remote budget is refused.
pub(crate) fn working(repo: &Repo, p: &WorkingParams, request_id: &str) -> Result<Value, Failure> {
    let path = relative(&p.file_path)?;
    let old = p.old_path.as_deref().map(relative).transpose()?;
    let (left, right) = if p.staged {
        let left_path = old.as_deref().unwrap_or(&path);
        (read_git(repo, Some("HEAD"), left_path, left_path)?, read_git(repo, None, &path, &path)?)
    } else {
        let left = if p.against_head {
            read_git(repo, Some("HEAD"), &path, &path)?
        } else {
            match read_git(repo, None, &path, &path)? {
                Read::Missing => read_git(repo, Some("HEAD"), &path, &path)?,
                read => read,
            }
        };
        (left, read_worktree(repo, &path)?)
    };
    let deleted = matches!(right, Read::Missing);
    let mut result = build(left.side(), right.side(), &path);
    if deleted && result["kind"] == "binary" {
        result["modifiedDeleted"] = true.into();
    }
    let budget = REPLY_MAX - serde_json::to_string(request_id).map_or(0, |s| s.len());
    if serde_json::to_string(&result).map_or(0, |s| s.len()) > budget {
        return Err(Failure { code: "diff_too_large".into(), message: "This diff is too large to open over a remote connection.".into(), data: Some(json!({"maxBytes": budget})) });
    }
    Ok(result)
}
