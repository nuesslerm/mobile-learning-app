//! `clipboard.*`: the phone's pasted image written to a file at the workspace host, where the pinned host writes one to its temp folder, and that
//! file's absolute path answered. The bytes are kept as an attachment (`upload.put`) and placed in the executor's temporary directory
//! (`attachment.stage` with `temp`), so an agent or a terminal there reads the path. A chunked upload is held here until its commit, owned by the device that
//! started it. rca keeps at most 10 MiB per attachment where the host takes 24 MiB of base64 (about 18 MiB): a larger image is refused as too large.
use crate::agent::ask;
use crate::ids::mint_upload;
use agent_api_attachments::{MAX_BYTES, STAGE, UPLOAD_PUT};
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use base64::Engine;
use orca_core::wire::{received, Failure, RUNTIME_ERROR};
use orca_core::AgentApi;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_BASE64: u64 = 24 * 1024 * 1024;
const HOST_MAX_BYTES: usize = 24 * 1024 * 1024 / 4 * 3;
const CHUNK_BASE64: usize = 512 * 1024;
const MAX_UPLOADS: usize = 8;
const TTL: Duration = Duration::from_secs(5 * 60);
const TOO_LARGE: &str = "Clipboard image is too large";
const NOT_FOUND: &str = "Clipboard image upload was not found";
const NOT_BASE64: &str = "Clipboard image content must be base64";
const SSH_NOT_SERVED: &str = "Remote connection dropped. Click Reconnect on the SSH target before retrying.";

/// Node's base64 reading: padding optional, stray bits ignored.
const LENIENT: GeneralPurpose = GeneralPurpose::new(&base64::alphabet::STANDARD, GeneralPurposeConfig::new().with_decode_allow_trailing_bits(true).with_decode_padding_mode(DecodePaddingMode::Indifferent));

struct Clip {
    owner: String,
    expected: u64,
    connection: Option<String>,
    file_name: Option<String>,
    chunks: String,
    touched: Instant,
}

/// The uploads in flight; one past its five minutes of quiet is gone.
#[derive(Default)]
pub struct Clips(Mutex<BTreeMap<String, Clip>>);

impl Clips {
    fn locked(&self) -> MutexGuard<'_, BTreeMap<String, Clip>> {
        let mut all = self.0.lock().unwrap_or_else(|e| e.into_inner());
        all.retain(|_, c| c.touched.elapsed() < TTL);
        all
    }
}

fn plain(message: &str) -> Failure {
    Failure::new(RUNTIME_ERROR, message)
}

fn object(params: &Value) -> Result<(), Failure> {
    match params {
        Value::Null | Value::Object(_) => Ok(()),
        other => Err(Failure::invalid(format!("Invalid input: expected object, received {}", received(other)))),
    }
}

fn js_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `isValidBase64`: the standard alphabet, at most two `=` at the end, and a length that can be base64.
fn base64_ok(s: &str) -> bool {
    let body = s.trim_end_matches('=');
    s.len() - body.len() <= 2 && s.len() % 4 != 1 && body.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
}

/// `clipboardImageBase64Payload`: text, then within `max` characters, then base64.
fn payload<'a>(value: Option<&'a Value>, max: usize, too_large: &str) -> Result<&'a str, Failure> {
    let Some(Value::String(s)) = value else { return Err(Failure::invalid("Missing image content")) };
    if js_len(s) > max {
        return Err(Failure::invalid(too_large));
    }
    if !base64_ok(s) {
        return Err(Failure::invalid(NOT_BASE64));
    }
    Ok(s)
}

/// A string of at least one character (and at most `max`), or absent; `nullable` also takes `null` as absent.
fn opt_string(value: Option<&Value>, max: Option<usize>, nullable: bool) -> Result<Option<String>, Failure> {
    match value {
        None => Ok(None),
        Some(Value::Null) if nullable => Ok(None),
        Some(Value::String(s)) if s.is_empty() => Err(Failure::invalid("Too small: expected string to have >=1 characters")),
        Some(Value::String(s)) if max.is_some_and(|m| js_len(s) > m) => Err(Failure::invalid(format!("Too big: expected string to have <={} characters", max.unwrap_or(0)))),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(other) => Err(Failure::invalid(format!("Invalid input: expected string, received {}", received(other)))),
    }
}

fn upload_id(params: &Value) -> Result<String, Failure> {
    match params.get("uploadId") {
        None => Err(Failure::invalid("Invalid input: expected string, received undefined")),
        some => opt_string(some, None, false).map(|id| id.unwrap_or_default()),
    }
}

/// A non-negative integer up to `max` (with `over` as the words past it), in zod's order of checks.
fn count(value: Option<&Value>, max: u64, over: &str) -> Result<u64, Failure> {
    let n = match value {
        Some(Value::Number(n)) => n,
        other => return Err(Failure::invalid(format!("Invalid input: expected number, received {}", other.map_or("undefined", received)))),
    };
    let f = n.as_f64().unwrap_or(f64::NAN);
    if f.fract() != 0.0 || !f.is_finite() {
        return Err(Failure::invalid("Invalid input: expected int, received number"));
    }
    if f < 0.0 {
        return Err(Failure::invalid("Too small: expected number to be >=0"));
    }
    if f > max as f64 {
        return Err(Failure::invalid(over));
    }
    Ok(f as u64)
}

/// `detectImageFileExtension` for the formats an agent takes, and the ISO media brands of a phone's photos.
fn extension(bytes: &[u8]) -> Option<&'static str> {
    let at = |offset: usize, sig: &[u8]| bytes.len() >= offset + sig.len() && &bytes[offset..offset + sig.len()] == sig;
    if at(0, &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Some("png");
    }
    if at(0, &[0xff, 0xd8, 0xff]) {
        return Some("jpg");
    }
    if at(0, b"GIF87a") || at(0, b"GIF89a") {
        return Some("gif");
    }
    if at(0, b"RIFF") && at(8, b"WEBP") {
        return Some("webp");
    }
    if at(4, b"ftyp") && bytes.len() >= 12 {
        return match &bytes[8..12] {
            b"heic" | b"heix" | b"heim" | b"heis" | b"hevc" | b"hevx" => Some("heic"),
            b"mif1" | b"msf1" => Some("heif"),
            b"avif" => Some("avif"),
            _ => None,
        };
    }
    if at(0, &[0x49, 0x49, 0x2a, 0x00]) || at(0, &[0x4d, 0x4d, 0x00, 0x2a]) {
        return Some("tiff");
    }
    (at(0, b"BM") && at(6, &[0, 0, 0, 0])).then_some("bmp")
}

fn mime(ext: Option<&str>) -> &'static str {
    match ext {
        Some("png") => "image/png",
        Some("jpg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("heic") => "image/heic",
        Some("heif") => "image/heif",
        Some("avif") => "image/avif",
        Some("tiff") => "image/tiff",
        Some("bmp") => "image/bmp",
        _ => "application/octet-stream",
    }
}

/// The host's file name: a picked file keeps its own (with no character a shell would expand), a paste is named by its time and image kind.
fn file_name(picked: Option<&str>, bytes: &[u8], unique: &str) -> String {
    match picked {
        Some(name) => agent_api_attachments::safe_file_name(name).chars().map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '.' | '_' | '-') { c } else { '_' }).collect(),
        None => {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
            match extension(bytes) {
                Some(ext) => format!("orca-paste-{now}-{unique}.{ext}"),
                None => format!("orca-paste-{now}-{unique}"),
            }
        }
    }
}

/// The image kept and placed in the root workspace; its path there.
fn write(agent: &dyn AgentApi, bytes: &[u8], connection: Option<&str>, picked: Option<&str>) -> Result<Value, Failure> {
    if bytes.len() > HOST_MAX_BYTES || bytes.len() as u64 > MAX_BYTES {
        return Err(plain(TOO_LARGE));
    }
    if connection.is_some() {
        return Err(plain(SSH_NOT_SERVED));
    }
    let all = orca_map_workspace::targets(agent)?;
    let root = all.iter().find(|t| t.kind == "root").or(all.first()).ok_or_else(|| plain("no executor is attached"))?;
    let id = mint_upload();
    let name = file_name(picked, bytes, &id);
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    let detected = extension(bytes).or(match ext.as_deref() {
        Some("png") => Some("png"),
        Some("jpg" | "jpeg") => Some("jpg"),
        Some("gif") => Some("gif"),
        Some("webp") => Some("webp"),
        _ => None,
    });
    ask(agent, UPLOAD_PUT.name(), json!({"id": id, "name": name, "mime_type": mime(detected), "kind": "image", "data": base64::engine::general_purpose::STANDARD.encode(bytes)}))?;
    let staged = ask(agent, STAGE.name(), json!({"workspace": root.workspace, "ids": [id], "temp": true}))?;
    let path = staged["files"][0]["path"].as_str().ok_or_else(|| plain("attachment.stage answered no path"))?;
    Ok(json!(path))
}

/// `clipboard.saveImageAsTempFile`: one image in one request.
pub(crate) fn save(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    object(params)?;
    let content = payload(params.get("contentBase64"), MAX_BASE64 as usize, TOO_LARGE)?;
    let connection = opt_string(params.get("connectionId"), None, true)?;
    let bytes = LENIENT.decode(content).map_err(|_| Failure::invalid(NOT_BASE64))?;
    write(agent, &bytes, connection.as_deref(), None)
}

pub(crate) fn start(clips: &Clips, owner: &str, params: &Value) -> Result<Value, Failure> {
    object(params)?;
    let expected = count(params.get("expectedBase64Length"), MAX_BASE64, TOO_LARGE)?;
    let connection = opt_string(params.get("connectionId"), None, true)?;
    let file_name = opt_string(params.get("fileName"), Some(1024), false)?;
    let mut all = clips.locked();
    if all.len() >= MAX_UPLOADS {
        return Err(plain("Too many clipboard image uploads are in progress"));
    }
    let id = mint_upload();
    all.insert(id.clone(), Clip { owner: owner.into(), expected, connection, file_name, chunks: String::new(), touched: Instant::now() });
    Ok(json!({"uploadId": id}))
}

pub(crate) fn append(clips: &Clips, owner: &str, params: &Value) -> Result<Value, Failure> {
    object(params)?;
    let id = upload_id(params)?;
    let offset = count(params.get("offset"), u64::MAX, "")?;
    let content = payload(params.get("contentBase64"), CHUNK_BASE64, "Clipboard image chunk is too large")?;
    let mut all = clips.locked();
    let clip = all.get_mut(&id).filter(|c| c.owner == owner).ok_or_else(|| plain(NOT_FOUND))?;
    if offset != clip.chunks.len() as u64 {
        return Err(plain("Clipboard image chunk offset is out of order"));
    }
    if (clip.chunks.len() + content.len()) as u64 > clip.expected {
        return Err(plain("Clipboard image upload exceeded expected size"));
    }
    clip.chunks.push_str(content);
    clip.touched = Instant::now();
    Ok(json!({"receivedBase64Length": clip.chunks.len()}))
}

pub(crate) fn commit(clips: &Clips, owner: &str, agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    object(params)?;
    let id = upload_id(params)?;
    let clip = {
        let mut all = clips.locked();
        all.get(&id).filter(|c| c.owner == owner).ok_or_else(|| plain(NOT_FOUND))?;
        all.remove(&id).expect("an upload just found")
    };
    if clip.chunks.len() as u64 != clip.expected {
        return Err(plain("Clipboard image upload is incomplete"));
    }
    // padding may only end the whole upload, as the host decodes the chunks as one
    let body = clip.chunks.trim_end_matches('=');
    if body.contains('=') || clip.chunks.len() - body.len() > 2 || clip.chunks.len() % 4 == 1 {
        return Err(plain(NOT_BASE64));
    }
    let bytes = LENIENT.decode(&clip.chunks).map_err(|_| plain(NOT_BASE64))?;
    write(agent, &bytes, clip.connection.as_deref(), clip.file_name.as_deref())
}

pub(crate) fn abort(clips: &Clips, owner: &str, params: &Value) -> Result<Value, Failure> {
    object(params)?;
    let id = upload_id(params)?;
    let mut all = clips.locked();
    if let Some(clip) = all.get(&id) {
        if clip.owner != owner {
            return Err(plain(NOT_FOUND));
        }
        all.remove(&id);
    }
    Ok(json!({"aborted": true}))
}
