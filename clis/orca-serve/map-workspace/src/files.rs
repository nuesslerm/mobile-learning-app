//! `files.*`: the phone's file explorer, previews and search over `file.*`. The reply shapes are the pinned host's
//! (`references/orca/src/main/runtime/files`), and its limits are kept: a text read stops at 512 KiB, a binary preview at what one reply carries.
use crate::agent::ask;
use crate::natural::{compare, compare_names, Entry};
use crate::target::{resolve, selector, text, Target};
use agent_api_workspace::edit::{FILE_CREATE, FILE_READ_CHUNK, FILE_SEARCH, FILE_STAT};
use agent_api_workspace::FILE_LIST;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use orca_core::wire::{Failure, RUNTIME_ERROR};
use orca_core::AgentApi;
use serde_json::{json, Value};

const READ_MAX: usize = 512 * 1024;
/// `REMOTE_RUNTIME_MAX_OUTBOUND_JSON_BYTES` less the envelope reserve: the content one reply may carry.
const REPLY_BUDGET: usize = 4 * 1024 * 1024 - 4 * 1024;
const LIST_LIMIT: usize = 5000;
/// `list_dir` stops at this many entries, so a listing that reaches it may be cut.
const EXECUTOR_LIST_CAP: usize = 5000;
const CHUNK: u64 = 1 << 20;

const BINARY: &[&str] = &[".avif", ".bmp", ".gif", ".heic", ".ico", ".jpeg", ".jpg", ".mov", ".mp3", ".mp4", ".pdf", ".png", ".webp", ".zip"];
const PREVIEW_MIME: &[(&str, &str)] = &[
    (".png", "image/png"),
    (".jpg", "image/jpeg"),
    (".jpeg", "image/jpeg"),
    (".gif", "image/gif"),
    (".svg", "image/svg+xml"),
    (".webp", "image/webp"),
    (".bmp", "image/bmp"),
    (".ico", "image/x-icon"),
    (".avif", "image/avif"),
    (".pdf", "application/pdf"),
];

/// The mime type of an image file by its name; a pdf previews but is no image.
pub fn image_mime(name: &str) -> Option<&'static str> {
    let ext = extension(name);
    PREVIEW_MIME.iter().find(|(e, m)| *e == ext && m.starts_with("image/")).map(|(_, m)| *m)
}

fn plain(message: &str) -> Failure {
    Failure::new(RUNTIME_ERROR, message)
}

fn extension(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rfind('.') {
        Some(i) if i > 0 => name[i..].to_lowercase(),
        _ => String::new(),
    }
}

fn is_binary_path(path: &str) -> bool {
    BINARY.contains(&extension(path).as_str())
}

pub(crate) fn is_safe_path(path: &str) -> bool {
    let path = path.trim_end_matches('/');
    if path.is_empty() || path.starts_with('/') {
        return false;
    }
    let b = path.as_bytes();
    if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'/' || b[2] == b'\\') {
        return false;
    }
    path.replace('\\', "/").split('/').all(|p| !p.is_empty() && p != "." && p != "..")
}

/// The host's `normalizeRuntimeRelativePath`: trailing slashes go, the rest must be a safe relative path; `root_ok` lets `.` through.
fn clean(path: &str, root_ok: bool) -> Result<&str, Failure> {
    let path = path.trim_end_matches('/');
    if (root_ok && path == ".") || is_safe_path(path) {
        Ok(path)
    } else {
        Err(plain("invalid_relative_path"))
    }
}

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn has_nul_in_probe(bytes: &[u8]) -> bool {
    bytes.iter().take(8192).any(|b| *b == 0)
}

fn path_param<'a>(params: &'a Value, key: &str) -> Result<&'a str, Failure> {
    text(params, key).ok_or_else(|| Failure::invalid(format!("Invalid input: {key} must be a string")))
}

fn uint(params: &Value, key: &str) -> Result<Option<u64>, Failure> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().map(Some).ok_or_else(|| Failure::invalid(format!("Invalid input: {key} must be a non-negative integer"))),
    }
}

fn args(t: &Target, path: &str) -> Value {
    json!({"workspace": t.workspace, "path": path})
}

/// Up to `max + 1` bytes of the file from its start, and its size: one more byte than the limit tells a longer file from one that fits.
pub(crate) fn read_prefix(agent: &dyn AgentApi, t: &Target, path: &str, max: usize) -> Result<(Vec<u8>, u64), Failure> {
    let (mut bytes, mut total) = (Vec::new(), 0);
    loop {
        let want = (CHUNK as usize).min(max + 1 - bytes.len());
        let mut a = args(t, path);
        a["offset"] = json!(bytes.len());
        a["length"] = json!(want);
        let r = ask(agent, FILE_READ_CHUNK.name(), a)?;
        let piece = STANDARD.decode(r["data"].as_str().unwrap_or("")).map_err(|_| plain("invalid_runtime_response"))?;
        total = r["total_bytes"].as_u64().unwrap_or(total);
        bytes.extend_from_slice(&piece);
        if r["eof"] == true || bytes.len() > max || piece.is_empty() {
            return Ok((bytes, total));
        }
    }
}

pub(crate) fn read(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let t = resolve(agent, selector(params)?)?;
    let path = clean(path_param(params, "relativePath")?, false)?;
    if is_binary_path(path) {
        return Err(plain("binary_file"));
    }
    let (bytes, _) = read_prefix(agent, &t, path, READ_MAX)?;
    if has_nul_in_probe(&bytes) {
        return Err(plain("binary_file"));
    }
    let truncated = bytes.len() > READ_MAX;
    let kept = if truncated { &bytes[..READ_MAX] } else { &bytes[..] };
    Ok(json!({"worktree": t.id, "relativePath": path, "content": String::from_utf8_lossy(kept), "truncated": truncated, "byteLength": bytes.len()}))
}

/// The most raw bytes whose base64 fits one reply beside the rest of a preview result (`previewableBinaryByteLimit`).
pub fn previewable_limit() -> usize {
    let empty = json!({"content": "", "isBinary": true, "isImage": true, "mimeType": "application/octet-stream"}).to_string().len();
    (REPLY_BUDGET.saturating_sub(empty) / 4) * 3
}

pub(crate) fn read_preview(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let t = resolve(agent, selector(params)?)?;
    let path = clean(path_param(params, "relativePath")?, false)?;
    let ext = extension(path);
    let mime = PREVIEW_MIME.iter().find(|(e, _)| *e == ext).map(|(_, m)| *m);
    let limit = if mime.is_some() { previewable_limit() } else { READ_MAX };
    let (bytes, _) = read_prefix(agent, &t, path, limit)?;
    if bytes.len() > limit {
        return Err(plain("file_too_large"));
    }
    Ok(match mime {
        Some(mime) => json!({"content": STANDARD.encode(&bytes), "isBinary": true, "isImage": true, "mimeType": mime}),
        None if has_nul_in_probe(&bytes) => json!({"content": "", "isBinary": true}),
        None => json!({"content": String::from_utf8_lossy(&bytes), "isBinary": false}),
    })
}

pub(crate) fn read_chunk(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let t = resolve(agent, selector(params)?)?;
    let path = clean(path_param(params, "relativePath")?, false)?;
    let (offset, length) = (uint(params, "offset")?, uint(params, "length")?);
    let (Some(offset), Some(length)) = (offset, length) else { return Err(Failure::invalid("Invalid input: offset and length are required")) };
    if !(1..=READ_MAX as u64).contains(&length) {
        return Err(Failure::invalid("Invalid file read range"));
    }
    let mut a = args(&t, path);
    a["offset"] = json!(offset);
    a["length"] = json!(length);
    let r = ask(agent, FILE_READ_CHUNK.name(), a)?;
    Ok(json!({"contentBase64": r["data"], "bytesRead": r["bytes"], "eof": r["eof"]}))
}

fn listing(agent: &dyn AgentApi, t: &Target, path: &str, depth: u64) -> Result<Vec<Value>, Failure> {
    let mut a = args(t, path);
    a["depth"] = json!(depth);
    Ok(ask(agent, FILE_LIST.name(), a)?["entries"].as_array().cloned().unwrap_or_default())
}

pub(crate) fn read_dir(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let t = resolve(agent, selector(params)?)?;
    let path = clean(text(params, "relativePath").filter(|p| !p.is_empty()).unwrap_or("."), true)?;
    let limit = uint(params, "limit")?;
    if limit.is_some_and(|l| !(1..=5000).contains(&l)) {
        return Err(Failure::invalid("Invalid input: limit must be between 1 and 5000"));
    }
    let after = match params.get("after") {
        None | Some(Value::Null) => None,
        Some(a) => match (a["name"].as_str().filter(|n| !n.is_empty()), a["isDirectory"].as_bool()) {
            (Some(name), Some(is_directory)) => Some(Entry { name: name.to_string(), is_directory }),
            _ => return Err(Failure::invalid("Invalid input: after needs a name and isDirectory")),
        },
    };
    let mut entries: Vec<Entry> = listing(agent, &t, path, 1)?.iter().filter_map(|e| Some(Entry { name: e["name"].as_str()?.to_string(), is_directory: e["kind"] == "dir" })).collect();
    entries.sort_by(compare);
    let row = |e: &Entry| json!({"name": e.name, "isDirectory": e.is_directory, "isSymlink": false});
    if after.is_none() && limit.is_none() {
        return Ok(Value::Array(entries.iter().map(row).collect()));
    }
    let start = after.map_or(0, |a| entries.partition_point(|e| compare(e, &a) != std::cmp::Ordering::Greater));
    let limit = limit.unwrap_or(5000) as usize;
    let page: Vec<Value> = entries.iter().skip(start).take(limit).map(row).collect();
    Ok(json!({"entries": page, "total": entries.len(), "hasMore": start + limit < entries.len()}))
}

pub(crate) fn stat(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let t = resolve(agent, selector(params)?)?;
    let path = clean(text(params, "relativePath").filter(|p| !p.is_empty()).unwrap_or("."), true)?;
    let r = ask(agent, FILE_STAT.name(), args(&t, path))?;
    Ok(json!({"size": r["size"], "isDirectory": r["kind"] == "dir", "mtime": r["modified_ms"]}))
}

pub(crate) fn create_file(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let t = resolve(agent, selector(params)?)?;
    let path = clean(path_param(params, "relativePath")?, false)?;
    let mut a = args(&t, path);
    a["content"] = json!("");
    ask(agent, FILE_CREATE.name(), a)?;
    Ok(json!({"ok": true}))
}

fn file_row(path: &str) -> Value {
    json!({"relativePath": path, "basename": basename(path), "kind": if is_binary_path(path) { "binary" } else { "text" }})
}

fn list_reply(t: &Target, files: &[String], total: usize, truncated: bool) -> Value {
    json!({"worktree": t.id, "rootPath": t.root_path, "files": files.iter().map(|p| file_row(p)).collect::<Vec<_>>(), "totalCount": total, "truncated": truncated})
}

/// Every file of the workspace the phone may open, sorted, and whether the listing was cut.
fn inventory(agent: &dyn AgentApi, t: &Target) -> Result<(Vec<String>, bool), Failure> {
    let raw = listing(agent, t, ".", 64)?;
    let cut = raw.len() >= EXECUTOR_LIST_CAP;
    let mut files: Vec<String> = raw.iter().filter(|e| e["kind"] == "file").filter_map(|e| e["name"].as_str()).filter(|p| is_safe_path(p)).map(str::to_string).collect();
    files.sort_by(|a, b| compare_names(&a.to_lowercase(), &b.to_lowercase()).then_with(|| a.cmp(b)));
    Ok((files, cut))
}

pub(crate) fn list(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let t = resolve(agent, selector(params)?)?;
    let (files, cut) = inventory(agent, &t)?;
    let total = files.len();
    Ok(list_reply(&t, &files[..total.min(LIST_LIMIT)], total, cut || total > LIST_LIMIT))
}

pub(crate) fn search_paths(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let t = resolve(agent, selector(params)?)?;
    let query = text(params, "query").unwrap_or("");
    let limit = uint(params, "limit")?.unwrap_or(16);
    if !(1..=32).contains(&limit) {
        return Err(Failure::invalid("Invalid input: limit must be between 1 and 32"));
    }
    let limit = limit as usize;
    let quick_open = params.get("mode").and_then(Value::as_str) == Some("quick-open");
    if query.trim().is_empty() {
        if quick_open {
            return Ok(list_reply(&t, &[], 0, false));
        }
        let (files, cut) = inventory(agent, &t)?;
        return Ok(list_reply(&t, &files[..files.len().min(limit)], files.len(), cut || files.len() > limit));
    }
    let excluded: Vec<&str> = params["excludePaths"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let found = ask(agent, FILE_SEARCH.name(), json!({"workspace": t.workspace, "query": query, "limit": 500}))?;
    let files: Vec<String> = found["entries"].as_array().map(|a| a.iter().filter(|e| e["kind"] == "file").filter_map(|e| e["path"].as_str()).filter(|p| is_safe_path(p) && !excluded.contains(p)).map(str::to_string).collect()).unwrap_or_default();
    let truncated = found["truncated"] == true || files.len() > limit;
    Ok(list_reply(&t, &files[..files.len().min(limit)], files.len(), truncated))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_paths_when_checked_then_only_relative_paths_without_dots_or_empty_parts_are_safe() {
        // Given the paths the host's own rule names, safe and not
        let safe = ["a.txt", "src/a/b.rs", "a b/c", "a/", "a//"];
        let unsafe_ = ["", "/etc/passwd", "C:\\x", "c:/x", "a/../b", "./a", "a//b", ".."];
        // Then the host's rule is kept
        assert!(safe.iter().all(|p| is_safe_path(p)), "{safe:?}");
        assert!(unsafe_.iter().all(|p| !is_safe_path(p)), "{unsafe_:?}");
    }

    #[test]
    fn given_the_reply_budget_when_the_preview_limit_is_read_then_a_result_of_that_many_bytes_fits_one_reply_and_three_more_do_not() {
        // Given the most raw bytes a preview may carry
        let limit = previewable_limit();
        let result = |n: usize| json!({"content": STANDARD.encode(vec![0u8; n]), "isBinary": true, "isImage": true, "mimeType": "application/octet-stream"}).to_string().len();
        // Then a result holding that many bytes fits one reply, and three more bytes do not
        assert!(result(limit) <= REPLY_BUDGET);
        assert!(result(limit + 3) > REPLY_BUDGET);
    }
}
