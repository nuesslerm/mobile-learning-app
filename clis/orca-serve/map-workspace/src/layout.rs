//! The server-kept layout: a worktree's tabs that are no terminal (editor, diff, markdown, chat), opened by `files.open`, `files.openDiff` and
//! `agentSession.reveal`, listed by `session.tabs.*`, kept as `tabs.extra.<workspace>` of scope `orca`; a file's content is read when asked.
use crate::agent::{ask, refusal};
use crate::files::{basename, extension, image_mime, is_binary_path, is_safe_path, plain, read_prefix};
use crate::target::{resolve, selector, text, Target};
use agent_api_state::SET;
use agent_api_workspace::edit::{FILE_STAT, FILE_WRITE};
use orca_core::wire::Failure;
use orca_core::AgentApi;
use serde_json::{json, Value};

const SCOPE: &str = "orca";
const EDIT_MAX: usize = 256 * 1024;
const READ_MAX: usize = 2 * 1024 * 1024;

pub fn extras_key(workspace: &str) -> String {
    format!("tabs.extra.{workspace}")
}

pub fn active_key(workspace: &str) -> String {
    format!("tabs.active.{workspace}")
}

/// The tabs kept for a workspace, in the order they were opened.
pub fn kept(agent: &dyn AgentApi, workspace: &str) -> Result<Vec<Value>, Failure> {
    match agent.state_get(SCOPE, &extras_key(workspace)).map_err(refusal)? {
        Some(Value::Array(tabs)) => Ok(tabs),
        _ => Ok(Vec::new()),
    }
}

fn keep(agent: &dyn AgentApi, key: &str, value: Value) -> Result<(), Failure> {
    ask(agent, SET.name(), json!({"scope": SCOPE, "key": key, "value": value})).map(|_| ())
}

/// Opens `record` (replacing a tab of the same id) and puts it in front.
fn open(agent: &dyn AgentApi, workspace: &str, record: Value) -> Result<(), Failure> {
    let mut tabs = kept(agent, workspace)?;
    let id = record["id"].clone();
    match tabs.iter_mut().find(|t| t["id"] == id) {
        Some(existing) => *existing = record,
        None => tabs.push(record),
    }
    keep(agent, &extras_key(workspace), Value::Array(tabs))?;
    keep(agent, &active_key(workspace), id)
}

/// Closes a kept tab: whether there was one. The front tab falls back to the default pick when it was this one.
pub fn close(agent: &dyn AgentApi, workspace: &str, tab: &str) -> Result<bool, Failure> {
    let mut tabs = kept(agent, workspace)?;
    let before = tabs.len();
    tabs.retain(|t| t["id"] != tab);
    if tabs.len() == before {
        return Ok(false);
    }
    keep(agent, &extras_key(workspace), Value::Array(tabs))?;
    if agent.state_get(SCOPE, &active_key(workspace)).map_err(refusal)?.is_some_and(|v| v == tab) {
        keep(agent, &active_key(workspace), Value::Null)?;
    }
    Ok(true)
}

/// A chat's tab, opened by `agentSession.reveal`: the chat's own id names it.
pub fn open_chat(agent: &dyn AgentApi, workspace: &str, session: &str, chat_agent: &str, title: &str) -> Result<(), Failure> {
    open(agent, workspace, json!({"id": session, "kind": "agent-session", "sessionId": session, "agent": chat_agent, "title": title}))
}

fn language(path: &str) -> &'static str {
    match extension(path).as_str() {
        ".ts" | ".tsx" | ".mts" | ".cts" => "typescript",
        ".js" | ".jsx" | ".mjs" | ".cjs" => "javascript",
        ".json" => "json",
        ".py" => "python",
        ".rs" => "rust",
        ".go" => "go",
        ".md" | ".mdx" | ".markdown" => "markdown",
        ".css" => "css",
        ".html" => "html",
        ".yaml" | ".yml" => "yaml",
        ".sh" | ".bash" => "shell",
        _ => "plaintext",
    }
}

pub fn is_markdown(path: &str) -> bool {
    matches!(extension(path).as_str(), ".md" | ".mdx" | ".markdown")
}

fn absolute(target: &Target, relative: &str) -> String {
    if target.root_path.is_empty() {
        relative.to_string()
    } else {
        format!("{}/{relative}", target.root_path.trim_end_matches('/'))
    }
}

/// A kept tab as the phone's snapshot lists it.
pub fn project(target: &Target, record: &Value, active: Option<&str>) -> Value {
    let id = record["id"].as_str().unwrap_or_default();
    let is_active = active == Some(id);
    if record["kind"] == "agent-session" {
        return json!({"type": "agent-session", "id": id, "title": record["title"], "sessionId": record["sessionId"], "agent": record["agent"], "color": null, "isPinned": false, "isActive": is_active});
    }
    let relative = record["relativePath"].as_str().unwrap_or_default();
    let path = absolute(target, relative);
    let common = json!({"id": id, "title": basename(relative), "filePath": path, "relativePath": relative, "isDirty": false, "isActive": is_active, "color": null, "isPinned": false});
    let mut tab = common.as_object().cloned().unwrap_or_default();
    if record["kind"] == "markdown" {
        for (k, v) in [("type", json!("markdown")), ("language", json!("markdown")), ("mode", json!("edit")), ("sourceFileId", json!(path)), ("sourceFilePath", json!(path)), ("sourceRelativePath", json!(relative)), ("documentVersion", json!(format!("file:{path}")))] {
            tab.insert(k.into(), v);
        }
    } else {
        tab.insert("type".into(), json!("file"));
        tab.insert("language".into(), json!(language(relative)));
        tab.insert("mode".into(), record["mode"].clone());
        if record["mode"] == "diff" {
            tab.insert("diffSource".into(), record["diffSource"].clone());
            if let Some(old) = record.get("oldPath").filter(|o| o.is_string()) {
                tab.insert("oldPath".into(), old.clone());
            }
        }
    }
    Value::Object(tab)
}

fn relative_param(params: &Value) -> Result<&str, Failure> {
    let path = text(params, "relativePath").ok_or_else(|| Failure::invalid("Invalid input: relativePath must be a string"))?;
    if is_safe_path(path) { Ok(path) } else { Err(plain("invalid_relative_path")) }
}

fn kind_of(path: &str, diff: bool) -> &'static str {
    if !diff && image_mime(path).is_some_and(|m| m != "image/svg+xml") {
        "image"
    } else if is_binary_path(path) {
        "binary"
    } else if is_markdown(path) {
        "markdown"
    } else {
        "text"
    }
}

/// `files.open`: the file must be there and no directory before the phone is told it opened.
pub(crate) fn open_file(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let target = resolve(agent, selector(params)?)?;
    let relative = relative_param(params)?;
    let stat = agent.request(FILE_STAT.name(), json!({"workspace": target.workspace, "path": relative}));
    match stat {
        Ok(s) if s["kind"] == "dir" => return Err(plain(&format!("EISDIR: illegal operation on a directory, open '{}'", absolute(&target, relative)))),
        Ok(_) => {}
        Err(orca_core::AgentError::Refused { code, .. }) if code == "NOT_FOUND" => return Err(plain(&format!("ENOENT: no such file or directory, open '{}'", absolute(&target, relative)))),
        Err(e) => return Err(refusal(e)),
    }
    let kind = kind_of(relative, false);
    let (id, record_kind) = if kind == "markdown" { (format!("markdown:{relative}"), "markdown") } else { (format!("file:{relative}"), "file") };
    open(agent, &target.workspace, json!({"id": id, "kind": record_kind, "relativePath": relative, "mode": "edit"}))?;
    Ok(json!({"worktree": target.id, "relativePath": relative, "kind": kind, "opened": true}))
}

/// `files.openDiff`: a diff tab keeps no file, so the path need not exist (a deleted file has a diff).
pub(crate) fn open_diff(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let target = resolve(agent, selector(params)?)?;
    let relative = relative_param(params)?;
    let old = text(params, "oldPath").filter(|o| !o.is_empty());
    if old.is_some_and(|o| !is_safe_path(o)) {
        return Err(plain("invalid_relative_path"));
    }
    let staged = params["staged"] == true;
    let source = if staged { "staged" } else { "unstaged" };
    let mut record = json!({"id": format!("diff:{source}:{relative}"), "kind": "file", "relativePath": relative, "mode": "diff", "diffSource": source});
    if let Some(old) = old {
        record["oldPath"] = json!(old);
    }
    open(agent, &target.workspace, record)?;
    Ok(json!({"worktree": target.id, "relativePath": relative, "kind": kind_of(relative, true), "opened": true}))
}

/// The host's content hash of a markdown document: 64-bit FNV-1a over its UTF-16 code units, with the UTF-8 length.
pub fn content_version(content: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for unit in content.encode_utf16() {
        hash ^= u64::from(unit);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("content:{}:{hash:016x}", content.len())
}

fn markdown_tab(agent: &dyn AgentApi, target: &Target, params: &Value) -> Result<String, Failure> {
    let id = text(params, "tabId").filter(|t| !t.is_empty()).ok_or_else(|| Failure::invalid("Invalid input: tabId must be a string"))?;
    kept(agent, &target.workspace)?
        .into_iter()
        .find(|t| t["id"] == id && t["kind"] == "markdown")
        .and_then(|t| t["relativePath"].as_str().map(str::to_string))
        .ok_or_else(|| plain("tab_not_found"))
}

/// The file's text, up to `max` bytes of it (cut on a character boundary), and its full size.
fn disk(agent: &dyn AgentApi, target: &Target, path: &str, max: usize) -> Result<(String, u64), Failure> {
    let (bytes, total) = read_prefix(agent, target, path, max)?;
    if is_binary_path(path) || bytes.iter().take(8192).any(|b| *b == 0) {
        return Err(plain("binary_file"));
    }
    let kept = &bytes[..bytes.len().min(max)];
    let text = match std::str::from_utf8(kept) {
        Ok(t) => t,
        Err(e) => std::str::from_utf8(&kept[..e.valid_up_to()]).unwrap_or_default(),
    };
    Ok((text.to_string(), total.max(bytes.len() as u64)))
}

pub(crate) fn read_tab(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let target = resolve(agent, selector(params)?)?;
    let relative = markdown_tab(agent, &target, params)?;
    let (content, size) = disk(agent, &target, &relative, READ_MAX)?;
    let mut result = json!({"tabId": params["tabId"], "filePath": absolute(&target, &relative), "relativePath": relative, "content": content, "isDirty": false, "version": content_version(&content), "source": "file", "editable": size as usize <= EDIT_MAX});
    if size as usize > EDIT_MAX {
        result["readOnlyReason"] = json!("file_too_large");
    }
    if size as usize > content.len() {
        result["truncated"] = json!(true);
        result["byteLength"] = json!(size);
    }
    Ok(result)
}

/// A save against the version the phone read: a changed file answers `conflict`; the file is re-read right before the unconditional write.
pub(crate) fn save_tab(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let target = resolve(agent, selector(params)?)?;
    let content = params["content"].as_str().ok_or_else(|| Failure::invalid("Invalid input: content must be a string"))?;
    let base = text(params, "baseVersion").ok_or_else(|| Failure::invalid("Invalid input: baseVersion must be a string"))?;
    if content.len() > EDIT_MAX {
        return Err(plain("file_too_large"));
    }
    let relative = markdown_tab(agent, &target, params)?;
    let saved = json!({"tabId": params["tabId"], "isDirty": false, "version": content_version(content), "content": content});
    let (current, size) = disk(agent, &target, &relative, EDIT_MAX)?;
    if size as usize > EDIT_MAX {
        return Err(plain("file_too_large"));
    }
    if content_version(&current) != base {
        return if current == content { Ok(saved) } else { Err(plain("conflict")) };
    }
    if disk(agent, &target, &relative, EDIT_MAX)?.0 != current {
        return Err(plain("conflict"));
    }
    ask(agent, FILE_WRITE.name(), json!({"workspace": target.workspace, "path": relative, "content": content, "create_dirs": false}))?;
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::content_version;

    #[test]
    fn given_the_hosts_hash_of_a_known_text_when_computed_then_it_is_the_same_fnv_over_utf16_units() {
        // Given the empty text and a text with a character outside the basic plane
        // Then the first is the FNV offset basis under a zero length, and the second hashes its surrogate pair as two units
        assert_eq!(content_version(""), "content:0:cbf29ce484222325");
        assert_eq!(content_version("a"), "content:1:af63dc4c8601ec8c");
        assert!(content_version("\u{1F600}").starts_with("content:4:"));
    }
}
