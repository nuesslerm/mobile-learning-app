//! `agentSessionAttachment.*` and `files.readChatImage` over rca's `attachments` group. A phone uploads a chat file in chunks: the chunks are
//! buffered here (offsets checked, at most the smaller of the host's limit and the group's), and the commit is one `upload.put` under the upload id.
//! The path the phone gets back is `/rca/agent-session-attachments/<upload id>/<name>`: the host's store layout, so the phone recognises it, with
//! the id the group keeps the bytes under. A read of that path is an `attachment.get` window; a send names the id in `turn.start.attachments`.
use crate::agent::{ask, refused_code};
use crate::ids::mint_upload;
use crate::journal::view;
use agent_api_attachments::{code, Kind, CHUNK_MAX, GET, MAX_BYTES, UPLOAD_PUT};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use orca_core::wire::{Failure, RUNTIME_ERROR};
use orca_core::AgentApi;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const STORE_ROOT: &str = "/rca/agent-session-attachments";
/// The host's `AGENT_SESSION_ATTACHMENT_MAX_BYTES`; the group's smaller limit wins.
const HOST_MAX_BYTES: u64 = 50 * 1024 * 1024;
/// The host's `AGENT_SESSION_ATTACHMENT_CHUNK_BASE64_CHARS`: 384 KiB of bytes.
const CHUNK_BASE64_CHARS: usize = 384 * 1024 / 3 * 4;
/// The host's `MAX_FILE_RANGE_READ_BYTES`.
const RANGE_MAX: u64 = 256 * 1024;
const NAME_MAX_BYTES: usize = 200;
const MAX_UPLOADS: usize = 8;
const IDLE: Duration = Duration::from_secs(5 * 60);
const PART_FILE: &str = ".upload.part";

const NOT_FOUND: &str = "Attachment upload was not found";
const NOT_AN_IMAGE: &str = "Not an attachment image";
const CHANGED: &str = "Attachment changed. Close it and try again.";

fn plain(message: &str) -> Failure {
    Failure::new(RUNTIME_ERROR, message)
}

/// The most bytes one attachment holds on this host.
pub fn max_bytes() -> u64 {
    MAX_BYTES.min(HOST_MAX_BYTES)
}

struct Upload {
    owner: String,
    name: String,
    expected: u64,
    data: Vec<u8>,
    touched: Instant,
}

/// The uploads in flight, each owned by the connection that started it.
#[derive(Default)]
pub struct Uploads(Mutex<BTreeMap<String, Upload>>);

impl Uploads {
    fn locked(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Upload>> {
        let mut all = self.0.lock().unwrap_or_else(|e| e.into_inner());
        all.retain(|_, u| u.touched.elapsed() < IDLE);
        all
    }
}

/// The params of a method, an object with exactly the keys it names (the host's schemas are strict).
fn strict<'a>(params: &'a Value, allowed: &[&str]) -> Result<&'a Map<String, Value>, Failure> {
    let object = params.as_object().ok_or_else(|| Failure::invalid("Invalid input: expected object"))?;
    match object.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(extra) => Err(Failure::invalid(format!("Invalid input: unrecognized key {extra}"))),
        None => Ok(object),
    }
}

fn uint(object: &Map<String, Value>, key: &str) -> Result<u64, Failure> {
    object.get(key).and_then(Value::as_u64).ok_or_else(|| Failure::invalid(format!("Invalid input: {key} must be a non-negative integer")))
}

fn string<'a>(object: &'a Map<String, Value>, key: &str, max: usize) -> Result<&'a str, Failure> {
    match object.get(key).and_then(Value::as_str) {
        Some(s) if !s.is_empty() && s.chars().count() <= max => Ok(s),
        _ => Err(Failure::invalid(format!("Invalid input: {key} must be 1 to {max} characters"))),
    }
}

/// The host's `sanitizeAgentSessionAttachmentName`: the last path segment, unsafe characters replaced, Windows-safe, at most 200 bytes.
pub fn sanitize_name(name: &str) -> String {
    let leaf = name.rsplit(['/', '\\']).find(|p| !p.is_empty()).unwrap_or("");
    let cleaned: String = leaf.chars().map(|c| if c.is_control() || "<>:\"|?*".contains(c) { '_' } else { c }).collect();
    let mut visible = cleaned.trim().trim_start_matches('.').trim_end_matches(['.', ' ']).to_string();
    if visible.is_empty() {
        return "attachment".into();
    }
    let stem = visible.split('.').next().filter(|_| visible.find('.').is_some_and(|i| i > 0)).unwrap_or(&visible).to_lowercase();
    let device = matches!(stem.as_str(), "con" | "prn" | "aux" | "nul") || ["com", "lpt"].iter().any(|p| stem.strip_prefix(p).is_some_and(|d| d.chars().count() == 1 && d.chars().all(|c| c.is_ascii_digit() || "\u{b9}\u{b2}\u{b3}".contains(c))));
    if device {
        visible.insert(0, '_');
    }
    if visible.len() <= NAME_MAX_BYTES {
        return visible;
    }
    let (stem, extension) = match visible.rfind('.') {
        Some(i) if i > 0 && visible.len() - i <= 16 => (&visible[..i], &visible[i..]),
        _ => (visible.as_str(), ""),
    };
    let mut kept = String::new();
    for c in stem.chars() {
        if kept.len() + c.len_utf8() + extension.len() > NAME_MAX_BYTES {
            break;
        }
        kept.push(c);
    }
    let out = format!("{kept}{extension}");
    let out = out.trim_end_matches(['.', ' ']);
    if out.is_empty() {
        "attachment".into()
    } else {
        out.into()
    }
}

fn is_upload_id(s: &str) -> bool {
    let groups: Vec<&str> = s.split('-').collect();
    groups.len() == 5 && groups.iter().zip([8, 4, 4, 4, 12]).all(|(g, n)| g.len() == n && g.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}

/// The attachment id and name of a committed store path; anything else is no attachment of this host.
pub(crate) fn parse_path(path: &str) -> Option<(String, String)> {
    let rest = path.strip_prefix(STORE_ROOT)?.strip_prefix('/')?;
    let (id, name) = rest.split_once('/')?;
    (is_upload_id(id) && !name.is_empty() && !name.contains(['/', '\\']) && name != PART_FILE && name != ".." && name != ".").then(|| (id.to_string(), name.to_string()))
}

fn store_path(id: &str, name: &str) -> String {
    format!("{STORE_ROOT}/{id}/{name}")
}

fn base64_ok(text: &str) -> bool {
    STANDARD.decode(text).is_ok()
}

pub(crate) fn start(uploads: &Uploads, owner: &str, agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let p = strict(params, &["sessionId", "name", "byteLength"])?;
    let session = string(p, "sessionId", 256)?;
    let name = string(p, "name", 1024)?;
    let length = uint(p, "byteLength")?;
    if length > max_bytes() {
        return Err(Failure::invalid("Attachment is too large"));
    }
    match view(agent, session) {
        Ok(_) => {}
        Err(f) if f.message == "agent_session_not_found" => return Err(plain("This chat is not on this host")),
        Err(f) => return Err(f),
    }
    if length == 0 {
        return Err(plain("Empty attachments are not served by this host"));
    }
    let mut all = uploads.locked();
    if all.len() >= MAX_UPLOADS {
        return Err(plain("Too many attachment uploads are in progress"));
    }
    let id = mint_upload();
    all.insert(id.clone(), Upload { owner: owner.into(), name: sanitize_name(name), expected: length, data: Vec::new(), touched: Instant::now() });
    Ok(json!({"uploadId": id}))
}

fn owned<'a>(all: &'a mut BTreeMap<String, Upload>, id: &str, owner: &str) -> Result<&'a mut Upload, Failure> {
    all.get_mut(id).filter(|u| u.owner == owner).ok_or_else(|| plain(NOT_FOUND))
}

pub(crate) fn append(uploads: &Uploads, owner: &str, params: &Value) -> Result<Value, Failure> {
    let p = strict(params, &["uploadId", "offset", "contentBase64"])?;
    let id = string(p, "uploadId", 128)?;
    let offset = uint(p, "offset")?;
    let content = p.get("contentBase64").and_then(Value::as_str).ok_or_else(|| Failure::invalid("Invalid input: contentBase64 must be a string"))?;
    if content.len() > CHUNK_BASE64_CHARS {
        return Err(Failure::invalid("Attachment chunk is too large"));
    }
    if !base64_ok(content) {
        return Err(Failure::invalid("Attachment chunk must be base64"));
    }
    let bytes = STANDARD.decode(content).unwrap_or_default();
    let mut all = uploads.locked();
    let upload = owned(&mut all, id, owner)?;
    if offset != upload.data.len() as u64 {
        return Err(plain("Attachment chunk offset is out of order"));
    }
    if upload.data.len() as u64 + bytes.len() as u64 > upload.expected {
        return Err(plain("Attachment upload exceeded its declared size"));
    }
    upload.data.extend_from_slice(&bytes);
    upload.touched = Instant::now();
    Ok(json!({"receivedBytes": upload.data.len()}))
}

fn id_params(params: &Value) -> Result<String, Failure> {
    let p = strict(params, &["uploadId"])?;
    Ok(string(p, "uploadId", 128)?.to_string())
}

pub(crate) fn commit(uploads: &Uploads, owner: &str, agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let id = id_params(params)?;
    let upload = {
        let mut all = uploads.locked();
        owned(&mut all, &id, owner)?;
        all.remove(&id).expect("an upload just found")
    };
    if upload.data.len() as u64 != upload.expected {
        return Err(plain("Attachment upload is incomplete"));
    }
    let image = orca_map_workspace::image_mime(&upload.name);
    let put = json!({"id": id, "name": upload.name, "mime_type": image.unwrap_or("application/octet-stream"), "kind": if image.is_some() { Kind::Image } else { Kind::File }, "data": STANDARD.encode(&upload.data)});
    ask(agent, UPLOAD_PUT.name(), put)?;
    Ok(json!({"path": store_path(&id, &upload.name), "name": upload.name, "byteLength": upload.data.len()}))
}

pub(crate) fn abort(uploads: &Uploads, owner: &str, params: &Value) -> Result<Value, Failure> {
    let id = id_params(params)?;
    let mut all = uploads.locked();
    if all.contains_key(&id) {
        owned(&mut all, &id, owner)?;
        all.remove(&id);
    }
    Ok(json!({"aborted": true}))
}

/// One `attachment.get` window: the attachment as kept and the bytes of the range; `None` when no such attachment.
fn window(agent: &dyn AgentApi, id: &str, offset: u64, length: u64) -> Result<Option<(Value, Vec<u8>)>, Failure> {
    match agent.request(GET.name(), json!({"id": id, "offset": offset, "length": length.min(CHUNK_MAX)})) {
        Ok(r) => {
            let bytes = STANDARD.decode(r["data"].as_str().unwrap_or_default()).map_err(|_| plain("runtime_unavailable"))?;
            Ok(Some((r["attachment"].clone(), bytes)))
        }
        Err(e) if refused_code(&e) == Some(code::ATTACHMENT_NOT_FOUND) => Ok(None),
        Err(e) => Err(crate::agent::refusal(e)),
    }
}

/// The whole of an image attachment as the phone's preview reply, or the host's uniform refusal.
fn image_preview(agent: &dyn AgentApi, path: &str, refusal: &str) -> Result<Value, Failure> {
    let (id, name) = parse_path(path).ok_or_else(|| plain(refusal))?;
    let mime = orca_map_workspace::image_mime(&name).ok_or_else(|| plain(refusal))?;
    let limit = orca_map_workspace::previewable_limit() as u64;
    let (view, mut bytes) = window(agent, &id, 0, CHUNK_MAX)?.ok_or_else(|| plain(refusal))?;
    let size = view["size_bytes"].as_u64().unwrap_or(bytes.len() as u64);
    if size > limit {
        return Err(plain("file_too_large"));
    }
    while (bytes.len() as u64) < size {
        let (_, more) = window(agent, &id, bytes.len() as u64, CHUNK_MAX)?.ok_or_else(|| plain(NOT_FOUND))?;
        if more.is_empty() {
            return Err(plain(CHANGED));
        }
        bytes.extend(more);
    }
    Ok(json!({"content": STANDARD.encode(&bytes), "isBinary": true, "isImage": true, "mimeType": mime}))
}

pub(crate) fn read(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let p = strict(params, &["path"])?;
    image_preview(agent, string(p, "path", 4096)?, NOT_AN_IMAGE)
}

pub(crate) fn read_chat_image(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let p = strict(params, &["worktree", "path"])?;
    orca_map_workspace::resolve(agent, string(p, "worktree", 4096)?)?;
    image_preview(agent, string(p, "path", 4096)?, "not_an_image")
}

pub(crate) fn read_text_chunk(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let p = strict(params, &["path", "offset", "length", "expectedSnapshot"])?;
    let (offset, length) = (uint(p, "offset")?, uint(p, "length")?);
    if length == 0 || length > RANGE_MAX {
        return Err(Failure::invalid(format!("fs.readFileRange length must be 1 to {RANGE_MAX} bytes")));
    }
    let expected = match p.get("expectedSnapshot") {
        None => None,
        Some(Value::String(s)) if s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) => Some(s.as_str()),
        Some(_) => return Err(Failure::invalid("Invalid input: expectedSnapshot must be 64 lowercase hex characters")),
    };
    let not_committed = || plain("Not a committed chat attachment");
    let (id, _) = parse_path(string(p, "path", 4096)?).ok_or_else(not_committed)?;
    let (view, bytes) = window(agent, &id, offset, length)?.ok_or_else(not_committed)?;
    let size = view["size_bytes"].as_u64().unwrap_or_default();
    let snapshot = view["sha256"].as_str().unwrap_or_default();
    if expected.is_some_and(|e| e != snapshot) {
        return Err(plain(CHANGED));
    }
    if offset > size {
        return Err(plain("Attachment offset is beyond its end"));
    }
    let read = bytes.len() as u64;
    Ok(json!({"size": size, "snapshot": snapshot, "contentBase64": STANDARD.encode(&bytes), "bytesRead": read, "eof": offset + read == size}))
}

/// The attachment ids a send body names, in order: an `image-ref` by a path of this host's store. A url is refused.
pub(crate) fn image_ids(blocks: &[&Value]) -> Result<Vec<String>, Failure> {
    blocks
        .iter()
        .map(|b| match (b["path"].as_str(), b["url"].as_str()) {
            (Some(path), None) => parse_path(path).map(|(id, _)| id).ok_or_else(|| Failure::invalid("Image path is not an attachment of this host")),
            _ => Err(Failure::invalid("Image urls are not served by this host: upload the image as an attachment")),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_names_the_host_sanitises_when_cleaned_then_each_is_a_safe_leaf_within_its_byte_limit() {
        // Given a path, control characters, a device name, a leading dot and an over-long name with an extension
        let long = format!("{}.png", "é".repeat(300));
        let cases = ["a/b\\c.txt", "we<ir>d:na|me?.txt", "con.txt", ".hidden", "///", "x.", long.as_str()];
        let cleaned: Vec<String> = cases.iter().map(|c| sanitize_name(c)).collect();
        // Then the leaf is kept, unsafe characters are replaced, a device name is prefixed, dots are dropped and a long name keeps its extension
        assert_eq!(cleaned[0], "c.txt");
        assert_eq!(cleaned[1], "we_ir_d_na_me_.txt");
        assert_eq!(cleaned[2], "_con.txt");
        assert_eq!(cleaned[3], "hidden");
        assert_eq!(cleaned[4], "attachment");
        assert_eq!(cleaned[5], "x");
        assert!(cleaned[6].len() <= NAME_MAX_BYTES && cleaned[6].ends_with(".png"));
    }

    #[test]
    fn given_store_paths_when_parsed_then_only_a_committed_file_directly_under_an_upload_id_is_one() {
        // Given a path made by the commit, and paths that escape, nest, name the part file or miss the id
        let id = mint_upload();
        let good = store_path(&id, "shot.png");
        let bad = [format!("{STORE_ROOT}/{id}/../x"), format!("{STORE_ROOT}/{id}/a/b.png"), format!("{STORE_ROOT}/{id}/{PART_FILE}"), format!("{STORE_ROOT}/not-an-id/x.png"), format!("/etc/{id}/x.png"), format!("{STORE_ROOT}/{id}")];
        // Then the good one gives its id and name and the others give none
        assert_eq!(parse_path(&good), Some((id, "shot.png".into())));
        assert!(bad.iter().all(|p| parse_path(p).is_none()), "{bad:?}");
    }

    #[test]
    fn given_the_pinned_host_when_its_limits_are_read_then_ours_are_never_larger() {
        // Given the host's attachment constants read from the pinned reference, when it is checked out
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../references/orca/src/shared/agent-session/agent-session-attachments.ts");
        let Ok(source) = std::fs::read_to_string(path) else { return };
        let number = |name: &str| -> u64 {
            let line = source.lines().find(|l| l.contains(&format!("const {name} ="))).expect("the constant");
            line.split('=').nth(1).unwrap().split('*').map(|t| t.trim().parse::<u64>().expect("a number")).product()
        };
        // Then the chunk size and the attachment limit we apply are the host's or smaller
        assert_eq!(number("AGENT_SESSION_ATTACHMENT_CHUNK_BYTES") / 3 * 4, CHUNK_BASE64_CHARS as u64);
        assert!(max_bytes() <= number("AGENT_SESSION_ATTACHMENT_MAX_BYTES"));
        assert_eq!(number("AGENT_SESSION_ATTACHMENT_NAME_MAX_BYTES"), NAME_MAX_BYTES as u64);
    }
}
