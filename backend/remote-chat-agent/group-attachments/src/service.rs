//! The use cases of the group over its ports and the executors.
use crate::ports::{BlobStore, Clock, Ids, Put, Refusal};
use agent_api_attachments::{code, message, staged_path, STAGE_IGNORE_CONTENT, STAGE_IGNORE_PATH, AttachmentView, DeleteResult, GetParams, GetResult, IdParams, PutParams, PutResult, StageParams, StageResult, StagedFile, MAX_BYTES};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use exec_proto::Code;
use rca_exec_channel::Executors;
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub struct Attachments {
    store: Arc<dyn BlobStore>,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn Ids>,
    executors: Arc<dyn Executors>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Attachments {
    pub fn new(store: Arc<dyn BlobStore>, clock: Arc<dyn Clock>, ids: Arc<dyn Ids>, executors: Arc<dyn Executors>) -> Arc<Attachments> {
        Arc::new(Attachments { store, clock, ids, executors })
    }

    pub fn put(&self, p: PutParams) -> Result<PutResult, Refusal> {
        let bytes = STANDARD.decode(&p.data).map_err(|_| Refusal::new("INVALID_PARAMS", "data is not base64"))?;
        if bytes.is_empty() {
            return Err(Refusal::new("INVALID_PARAMS", "data is required"));
        }
        if bytes.len() as u64 > MAX_BYTES {
            return Err(Refusal::new(code::ATTACHMENT_TOO_LARGE, message::too_large(bytes.len() as u64, MAX_BYTES)));
        }
        let id = p.id.unwrap_or_else(|| self.ids.new_id());
        let view = AttachmentView { id: id.clone(), name: p.name, mime_type: p.mime_type, kind: p.kind, size_bytes: bytes.len() as u64, sha256: hex(&Sha256::digest(&bytes)), created_at_ms: self.clock.now_ms() };
        match self.store.put(&view, &bytes).map_err(Refusal::unavailable)? {
            Put::Created => Ok(PutResult { attachment: view }),
            // An attachment never changes: the same bytes again is the same attachment, other bytes under the id are refused.
            Put::Taken(held) if held.sha256 == view.sha256 => Ok(PutResult { attachment: held }),
            Put::Taken(_) => Err(Refusal::new(code::ATTACHMENT_EXISTS, message::exists(&id))),
        }
    }

    pub fn get(&self, p: GetParams) -> Result<GetResult, Refusal> {
        let (view, bytes) = self.store.read(&p.id, p.offset(), p.length()).map_err(Refusal::unavailable)?.ok_or_else(|| Refusal::new(code::ATTACHMENT_NOT_FOUND, message::not_found(&p.id)))?;
        let read = bytes.len() as u64;
        Ok(GetResult { eof: p.offset().saturating_add(read) >= view.size_bytes, attachment: view, data: STANDARD.encode(&bytes), offset: p.offset(), bytes: read })
    }

    pub fn delete(&self, p: IdParams) -> Result<DeleteResult, Refusal> {
        let deleted = self.store.delete(&p.id).map_err(Refusal::unavailable)?;
        Ok(DeleteResult { id: p.id, deleted })
    }

    /// Writes each attachment into the workspace at its staged path. All ids are looked up before anything is written, so an unknown one
    /// writes nothing; a file already in place (an earlier stage of the same attachment) is the same file, because an attachment never changes.
    pub async fn stage(&self, p: StageParams) -> Result<StageResult, Refusal> {
        let channel = self.executors.get(&p.workspace).ok_or_else(|| Refusal::new(Code::NoExecutor.as_str(), format!("no executor is attached for workspace {}", p.workspace)))?;
        let place = self.executors.place(&p.workspace);
        let mut found = Vec::new();
        for id in &p.ids {
            let (view, bytes) = self.store.read(id, 0, u64::MAX).map_err(Refusal::unavailable)?.ok_or_else(|| Refusal::new(code::ATTACHMENT_NOT_FOUND, message::not_found(id)))?;
            found.push((view, bytes));
        }
        let mut files = Vec::new();
        let put = |path: String, bytes: Vec<u8>| {
            let channel = channel.clone();
            let place = place.clone();
            async move {
                let at = place.path(&path).map_err(|e| Refusal::new(Code::InvalidArgs.as_str(), e))?;
                match channel.call(exec_fs::PUT_BLOB, serde_json::json!({"path": at, "data": STANDARD.encode(&bytes)})).await {
                    Ok(_) => Ok(()),
                    Err(rca_exec_channel::remote::RemoteError::Action { code, .. }) if code == Code::AlreadyExists.as_str() => Ok(()),
                    Err(e) => Err(Refusal::new(e.code(), e.message())),
                }
            }
        };
        put(STAGE_IGNORE_PATH.to_string(), STAGE_IGNORE_CONTENT.as_bytes().to_vec()).await?;
        for (view, bytes) in found {
            let path = staged_path(&view.id, &view.name);
            put(path.clone(), bytes).await?;
            files.push(StagedFile { id: view.id, name: view.name, path, size_bytes: view.size_bytes });
        }
        Ok(StageResult { files })
    }
}
