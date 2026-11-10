//! The adapters of the attachments group's ports: the blob store as one directory per attachment on the state volume (`<id>/blob` and
//! `<id>/meta.json`, readable by the service's user only), the system clock, and ids from the system's randomness.
use agent_api_attachments::{valid_id, AttachmentView};
use rca_service_core::application::agent_groups::{AttachmentClock, AttachmentIds, BlobPut, BlobStore};
use rca_service_core::domain::util::{now_ms, random_hex};
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long an attachment directory may stay without its `meta.json` before a put of the same id takes it over: a put that crashed.
const ABANDONED_AFTER: Duration = Duration::from_secs(60);

pub struct BlobDir {
    root: PathBuf,
}

fn io(path: &Path, e: std::io::Error) -> String {
    format!("{}: {e}", path.display())
}

impl BlobDir {
    pub fn at(root: PathBuf) -> BlobDir {
        BlobDir { root }
    }

    fn dir(&self, id: &str) -> Result<PathBuf, String> {
        if !valid_id(id) {
            return Err(format!("{id:?} is not an attachment id"));
        }
        Ok(self.root.join(id))
    }

    fn read_meta(dir: &Path) -> Result<Option<AttachmentView>, String> {
        let file = dir.join("meta.json");
        match fs::read_to_string(&file) {
            Ok(text) => serde_json::from_str(&text).map(Some).map_err(|e| io(&file, std::io::Error::other(e))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io(&file, e)),
        }
    }

    fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
        let part = path.with_extension("part");
        let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&part).map_err(|e| io(&part, e))?;
        file.write_all(bytes).and_then(|()| file.sync_all()).map_err(|e| io(&part, e))?;
        fs::rename(&part, path).map_err(|e| io(path, e))
    }
}

impl BlobStore for BlobDir {
    fn put(&self, view: &AttachmentView, bytes: &[u8]) -> Result<BlobPut, String> {
        let dir = self.dir(&view.id)?;
        fs::DirBuilder::new().recursive(true).mode(0o700).create(&self.root).map_err(|e| io(&self.root, e))?;
        // The directory is the claim on the id: whoever creates it writes the attachment, everyone else reads what is there.
        for attempt in 0..2 {
            match fs::DirBuilder::new().mode(0o700).create(&dir) {
                Ok(()) => {
                    let written = Self::write_private(&dir.join("blob"), bytes).and_then(|()| Self::write_private(&dir.join("meta.json"), serde_json::to_string(view).map_err(|e| e.to_string())?.as_bytes()));
                    if written.is_err() {
                        let _ = fs::remove_dir_all(&dir);
                    }
                    return written.map(|()| BlobPut::Created);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if let Some(held) = Self::read_meta(&dir)? {
                        return Ok(BlobPut::Taken(held));
                    }
                    let age = fs::metadata(&dir).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).unwrap_or_default();
                    if attempt == 1 || age < ABANDONED_AFTER {
                        return Err(format!("attachment {} is being written", view.id));
                    }
                    fs::remove_dir_all(&dir).map_err(|e| io(&dir, e))?;
                }
                Err(e) => return Err(io(&dir, e)),
            }
        }
        unreachable!("the second attempt returns")
    }

    fn meta(&self, id: &str) -> Result<Option<AttachmentView>, String> {
        Self::read_meta(&self.dir(id)?)
    }

    fn read(&self, id: &str, offset: u64, length: u64) -> Result<Option<(AttachmentView, Vec<u8>)>, String> {
        let dir = self.dir(id)?;
        let Some(view) = Self::read_meta(&dir)? else { return Ok(None) };
        let file = dir.join("blob");
        let mut blob = fs::File::open(&file).map_err(|e| io(&file, e))?;
        let mut bytes = Vec::new();
        if offset < view.size_bytes {
            blob.seek(SeekFrom::Start(offset)).map_err(|e| io(&file, e))?;
            blob.take(length).read_to_end(&mut bytes).map_err(|e| io(&file, e))?;
        }
        Ok(Some((view, bytes)))
    }

    fn delete(&self, id: &str) -> Result<bool, String> {
        let dir = self.dir(id)?;
        if !dir.exists() {
            return Ok(false);
        }
        fs::remove_dir_all(&dir).map_err(|e| io(&dir, e))?;
        Ok(true)
    }
}

pub struct SystemClock;

impl AttachmentClock for SystemClock {
    fn now_ms(&self) -> u64 {
        now_ms()
    }
}

pub struct RandomIds;

impl AttachmentIds for RandomIds {
    fn new_id(&self) -> String {
        format!("att-{}", random_hex(12))
    }
}
