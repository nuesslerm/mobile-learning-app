//! `attachments.*` and `assets.*`: what a phone sends with a message and fetches back. The bytes are the agent API's (`upload.put`,
//! `attachment.get`, `attachment.delete`); this server mints the signed URLs a phone uploads to and fetches from, as the pin does, and checks them
//! when the bytes arrive. A URL carries its claims and an HMAC-SHA-256 of them under a key kept in the store, so a restart keeps every URL valid
//! until it expires. Only attachments are assets here: the other resources of the pin's asset URLs (workspace files, media, favicons) are
//! refused with that reason.
use super::queries::QueryFailure;
use super::service::Service;
use crate::ports::{AgentError, Op};
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use t3_contracts::wire;

pub const UPLOAD_PREFIX: &str = "/api/attachments/upload";
pub const ASSET_PREFIX: &str = "/api/assets";
const UPLOAD_TTL_MS: i64 = 10 * 60_000;
const ASSET_TTL_MS: i64 = 60 * 60_000;
/// The most bytes one upload carries: the agent API's cap (`agent_api_attachments::MAX_BYTES`, asserted equal by a test of the adapter).
pub const MAX_UPLOAD_BYTES: u64 = 10 << 20;
const IMAGE_MAX_BYTES: u64 = 10 << 20;
const FILE_MAX_BYTES: u64 = 50 << 20;
const IMAGE_TYPES: [&str; 4] = ["image/gif", "image/jpeg", "image/png", "image/webp"];
const PENDING: &str = "pending-";
const KEY: &str = "attachment-signing-key";

/// An HTTP answer that is a plain text with a status, as the pin's routes answer.
#[derive(Debug, PartialEq)]
pub struct HttpFailure {
    pub status: u16,
    pub text: String,
}

fn http(status: u16, text: impl Into<String>) -> HttpFailure {
    HttpFailure { status, text: text.into() }
}

/// An asset opened for sending: its headers' facts and the first chunk of its bytes.
#[derive(Debug)]
pub struct Asset {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub size: u64,
    pub inline: bool,
    pub first: Vec<u8>,
    pub eof: bool,
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut block = [0u8; 64];
    if key.len() > 64 {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let pad = |byte: u8| -> Vec<u8> { block.iter().map(|b| b ^ byte).collect() };
    let inner = Sha256::new().chain_update(pad(0x36)).chain_update(message).finalize();
    Sha256::new().chain_update(pad(0x5c)).chain_update(inner).finalize().to_vec()
}

fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn tag_error(tag: &str, message: &str) -> QueryFailure {
    QueryFailure::Error(json!({"_tag": tag, "message": message}))
}

fn unsupported(method: &str, reason: &str) -> QueryFailure {
    QueryFailure::Error(json!({"_tag": "UnsupportedOperationError", "message": reason, "method": method}))
}

fn defect(path: &str, what: &str) -> QueryFailure {
    QueryFailure::Defect(format!("Expected {what}\n  at [\"{path}\"]"))
}

fn text<'a>(payload: &'a Value, key: &str, max: usize) -> Result<&'a str, QueryFailure> {
    let value = payload.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    value.filter(|s| s.len() <= max).ok_or_else(|| defect(key, &format!("a non-empty string of at most {max} characters")))
}

fn code_of(e: &AgentError) -> &str {
    match e {
        AgentError::Refused { code, .. } => code,
        _ => "",
    }
}

fn hex_of(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Service {
    fn signing_key(&self) -> Result<Vec<u8>, String> {
        let _held = self.2.signing.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(key) = self.0.meta.get(KEY).map_err(|e| e.0)? {
            return hex::decode(key).map_err(|e| e.to_string());
        }
        let key = self.0.ids.random_bytes(32);
        self.0.meta.set(KEY, &hex_of(&key)).map_err(|e| e.0)?;
        Ok(key)
    }

    /// `<payload>.<signature>`, both base64url, over the claims.
    fn sign(&self, claims: &Value) -> Result<String, String> {
        let payload = URL_SAFE_NO_PAD.encode(claims.to_string());
        let signature = hmac_sha256(&self.signing_key()?, payload.as_bytes());
        Ok(format!("{payload}.{}", URL_SAFE_NO_PAD.encode(signature)))
    }

    /// The claims of a token this server signed for `kind` that has not expired, else none: a wrong, altered, expired or foreign token alike.
    fn verify(&self, token: &str, kind: &str) -> Option<Value> {
        let (payload, signature) = token.split_once('.')?;
        if signature.contains('.') {
            return None;
        }
        let expected = hmac_sha256(&self.signing_key().ok()?, payload.as_bytes());
        if !same(&URL_SAFE_NO_PAD.decode(signature).ok()?, &expected) {
            return None;
        }
        let claims: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()?;
        (claims["kind"] == kind && claims["version"] == 1 && claims["expiresAt"].as_i64()? > self.0.clock.now_ms()).then_some(claims)
    }

    fn attachment_op(&self, method: &'static str, params: Value) -> Result<Value, AgentError> {
        self.agent().run(&Op::Attachment { method, params })
    }

    /// `attachments.createUploadUrl`: an id for the attachment and the URL its bytes are posted to, good for ten minutes.
    pub fn attachment_upload_url(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let kind = payload.get("type").and_then(Value::as_str).unwrap_or("image");
        let name = text(payload, "name", 255)?;
        let mime = text(payload, "mimeType", 100)?;
        let size = payload.get("sizeBytes").and_then(Value::as_u64).filter(|n| *n >= 1).ok_or_else(|| defect("sizeBytes", "an integer of at least 1"))?;
        let limit = match kind {
            "image" if IMAGE_TYPES.contains(&mime) => IMAGE_MAX_BYTES,
            "image" => return Err(defect("mimeType", "\"image/gif\" | \"image/jpeg\" | \"image/png\" | \"image/webp\"")),
            "file" => FILE_MAX_BYTES,
            _ => return Err(defect("type", "\"image\" | \"file\"")),
        };
        if size > limit {
            return Err(defect("sizeBytes", &format!("an integer of at most {limit}")));
        }
        if size > MAX_UPLOAD_BYTES {
            return Err(unsupported("attachments.createUploadUrl", &format!("this server keeps attachments of at most {MAX_UPLOAD_BYTES} bytes; this one is {size}")));
        }
        let id = format!("{PENDING}{}", hex_of(&self.0.ids.random_bytes(12)));
        let expires_at = self.0.clock.now_ms() + UPLOAD_TTL_MS;
        let token = self.sign(&json!({"version": 1, "kind": "attachment-upload", "type": kind, "attachmentId": id, "name": name, "mimeType": mime, "sizeBytes": size, "expiresAt": expires_at})).map_err(|e| tag_error("AttachmentUploadSigningKeyError", &format!("Failed to load the attachment upload signing key: {e}")))?;
        Ok(json!({"attachmentId": id, "relativeUrl": format!("{UPLOAD_PREFIX}/{token}"), "expiresAt": expires_at}))
    }

    /// `attachments.delete`: an upload that was never attached to a message is dropped, and anything else is left alone, as the pin does.
    pub fn attachment_delete(&self, payload: &Value) -> Result<(), QueryFailure> {
        let id = text(payload, "attachmentId", 256)?;
        if !id.starts_with(PENDING) {
            return Ok(());
        }
        self.attachment_op("attachment.delete", json!({"id": id})).map(drop).map_err(|e| QueryFailure::Error(wire::host_error(&e.message())))
    }

    /// `assets.createUrl`: the URL an attachment's bytes are fetched from, good for an hour. The attachment is asked of the agent API first, so
    /// one that is not there is the pin's `AssetAttachmentNotFoundError`.
    pub fn asset_create_url(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let resource = payload.get("resource").filter(|r| r.is_object()).ok_or_else(|| defect("resource", "an object"))?;
        let tag = resource["_tag"].as_str().unwrap_or("");
        if tag != "attachment" {
            return Err(unsupported("assets.createUrl", &format!("this server serves attachment assets only; {tag:?} assets are not served")));
        }
        let id = text(resource, "attachmentId", 256)?;
        let got = self.attachment_op("attachment.get", json!({"id": id, "offset": 0, "length": 1})).map_err(|e| match code_of(&e) {
            "ATTACHMENT_NOT_FOUND" => QueryFailure::Error(json!({"_tag": "AssetAttachmentNotFoundError", "resource": resource})),
            _ => QueryFailure::Error(wire::host_error(&e.message())),
        })?;
        let view = &got["attachment"];
        let name = resource.get("fileName").and_then(Value::as_str).or(view["name"].as_str()).unwrap_or("attachment");
        let inline = resource["disposition"] == "inline" && IMAGE_TYPES.contains(&view["mime_type"].as_str().unwrap_or(""));
        let expires_at = self.0.clock.now_ms() + ASSET_TTL_MS;
        let token = self.sign(&json!({"version": 1, "kind": "asset", "attachmentId": id, "inline": inline, "expiresAt": expires_at})).map_err(|e| QueryFailure::Error(wire::host_error(&format!("the asset signing key could not be loaded: {e}"))))?;
        let mut file = String::new();
        for b in name.bytes() {
            if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_') {
                file.push(b as char);
            } else {
                file.push_str(&format!("%{b:02X}"));
            }
        }
        Ok(json!({"relativeUrl": format!("{ASSET_PREFIX}/{token}/{file}"), "expiresAt": expires_at, "fileRevision": view["sha256"]}))
    }

    /// `assets.persistChatAttachments`: images a client has as data URLs are kept by the agent API under ids, and answered as the message's attachments.
    pub fn persist_chat_attachments(&self, payload: &Value) -> Result<Value, QueryFailure> {
        text(payload, "threadId", 256)?;
        text(payload, "messageId", 256)?;
        let uploads = payload.get("attachments").and_then(Value::as_array).ok_or_else(|| defect("attachments", "an array"))?;
        let fail = |message: String| tag_error("PersistChatAttachmentsError", &message);
        let mut kept = Vec::new();
        for upload in uploads {
            let name = text(upload, "name", 255)?;
            let mime = text(upload, "mimeType", 100)?;
            let size = upload.get("sizeBytes").and_then(Value::as_u64).ok_or_else(|| defect("sizeBytes", "a non-negative integer"))?;
            let url = text(upload, "dataUrl", 14_000_000)?;
            let (head, data) = url.split_once(',').filter(|(head, _)| head.starts_with("data:") && head.ends_with(";base64")).ok_or_else(|| fail(format!("{name}: the data URL is not a base64 data URL")))?;
            if !mime.to_ascii_lowercase().starts_with("image/") || !head.to_ascii_lowercase().starts_with("data:image/") {
                return Err(fail(format!("{name}: only images are kept from data URLs")));
            }
            let bytes = STANDARD.decode(data).map_err(|_| fail(format!("{name}: the data URL is not base64")))?;
            if bytes.len() as u64 != size {
                return Err(fail(format!("{name}: the data URL holds {} bytes, not the {size} the attachment says", bytes.len())));
            }
            if size == 0 || size > MAX_UPLOAD_BYTES {
                return Err(fail(format!("{name}: an attachment is 1 to {MAX_UPLOAD_BYTES} bytes")));
            }
            let id = match upload.get("id").and_then(Value::as_str) {
                Some(id) => id.to_string(),
                None => format!("att-{}", hex_of(&self.0.ids.random_bytes(12))),
            };
            self.attachment_op("upload.put", json!({"id": id, "name": name, "mime_type": mime, "kind": "image", "data": data})).map_err(|e| fail(e.message()))?;
            kept.push(json!({"type": "image", "id": id, "name": name, "mimeType": mime, "sizeBytes": size}));
        }
        Ok(json!({"attachments": kept}))
    }

    /// `POST /api/attachments/upload/<token>`: the body is the attachment's bytes. The token is the credential, as the pin has it.
    pub fn attachment_upload(&self, token: &str, body: &[u8], content_length: Option<u64>) -> Result<(), HttpFailure> {
        let claims = self.verify(token, "attachment-upload").ok_or_else(|| http(404, "Not Found"))?;
        let size = claims["sizeBytes"].as_u64().unwrap_or(0);
        if content_length.is_some_and(|n| n != size) {
            return Err(http(400, "Content-Length must match the upload size."));
        }
        if body.len() as u64 != size {
            return Err(http(400, format!("Body was {} bytes, expected {size}.", body.len())));
        }
        let params = json!({"id": claims["attachmentId"], "name": claims["name"], "mime_type": claims["mimeType"], "kind": claims["type"], "data": STANDARD.encode(body)});
        match self.attachment_op("upload.put", params) {
            Ok(_) => Ok(()),
            Err(e) if code_of(&e) == "ATTACHMENT_EXISTS" => Err(http(409, "Attachment already uploaded.")),
            Err(e) => {
                self.log("attachment.upload.failed", &[("error", e.message())]);
                Err(http(502, "Failed to persist upload."))
            }
        }
    }

    /// `GET /api/assets/<token>/<name>`: the first chunk of the attachment the token names, and what the headers say of it.
    pub fn asset_open(&self, token: &str) -> Result<Asset, HttpFailure> {
        let claims = self.verify(token, "asset").ok_or_else(|| http(404, "Not Found"))?;
        let id = claims["attachmentId"].as_str().ok_or_else(|| http(404, "Not Found"))?;
        let (view, first, eof) = self.chunk(id, 0)?;
        Ok(Asset { id: id.to_string(), name: view["name"].as_str().unwrap_or("attachment").to_string(), mime_type: view["mime_type"].as_str().unwrap_or("application/octet-stream").to_string(), size: view["size_bytes"].as_u64().unwrap_or(0), inline: claims["inline"] == true, first, eof })
    }

    /// The bytes of the attachment after `offset`, and whether they reach its end.
    pub fn asset_read(&self, id: &str, offset: u64) -> Result<(Vec<u8>, bool), HttpFailure> {
        self.chunk(id, offset).map(|(_, bytes, eof)| (bytes, eof))
    }

    fn chunk(&self, id: &str, offset: u64) -> Result<(Value, Vec<u8>, bool), HttpFailure> {
        let got = self.attachment_op("attachment.get", json!({"id": id, "offset": offset})).map_err(|e| match code_of(&e) {
            "ATTACHMENT_NOT_FOUND" => http(404, "Not Found"),
            _ => http(502, "Failed to read the attachment."),
        })?;
        let bytes = STANDARD.decode(got["data"].as_str().unwrap_or("")).map_err(|_| http(502, "Failed to read the attachment."))?;
        Ok((got["attachment"].clone(), bytes, got["eof"] == true))
    }
}
