//! `git.branchDiff` and `git.commitDiff`: one file's two sides, each read as a blob of a commit through the executor's `git.blob`, in the
//! result the pinned host builds from them (`buildDiffResult`): text, or binary with the image or PDF preview, or text withheld over the render limit.
//! The sides come from rca's blob, which reads up to 8 MiB where the pinned host reads 10: a side between the two is `tooLarge` here.
use crate::{text, Repo};
use agent_api_git::BLOB;
use base64::Engine;
use orca_core::wire::Failure;
use orca_core::AgentError;
use serde_json::{json, Value};

const MAX_BLOB: u64 = 8 << 20;
const HOST_MAX: u64 = 10 << 20;
const PROBE: usize = 8192;
const MAX_LINES: usize = 120_000;
const MAX_CHARS: usize = 6_000_000;

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

/// The file at `rev`: a path the commit lacks (or any read the executor refuses) is an empty side, as the pinned host reads a missing blob.
fn read_side(repo: &Repo, rev: &str, path: &str, as_path: &str) -> Result<Side, Failure> {
    let blob = match repo.try_ask(BLOB, json!({"rev": rev, "path": path, "max_bytes": MAX_BLOB})) {
        Ok(blob) => blob,
        Err(AgentError::Refused { code, .. }) if code == "UNKNOWN_WORKSPACE" => return Err(Failure::new("selector_not_found", "selector_not_found")),
        Err(AgentError::Refused { .. }) => return Ok(absent()),
        Err(AgentError::Failed(_)) => return Err(Failure::new("runtime_unavailable", "runtime_unavailable")),
    };
    if blob["size"].as_u64().unwrap_or(0) > HOST_MAX || blob["truncated"] == true {
        return Ok(Side { content: String::new(), binary: true, too_large: true });
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(blob["data"].as_str().unwrap_or("")).unwrap_or_default();
    let binary = bytes.iter().take(PROBE).any(|b| *b == 0);
    let content = if binary {
        if preview_mime(as_path).is_some() { base64::engine::general_purpose::STANDARD.encode(&bytes) } else { String::new() }
    } else {
        String::from_utf8_lossy(&bytes).into_owned()
    };
    Ok(Side { content, binary, too_large: false })
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
    let old = old_path(params)?;
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
    let old = old_path(params)?;
    let left = match &parent {
        Some(p) => read_side(repo, p, old.as_deref().unwrap_or(&path), &path)?,
        None => absent(),
    };
    Ok(build(left, read_side(repo, &commit, &path, &path)?, &path))
}
