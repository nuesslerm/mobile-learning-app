//! The mobile web bundle as a directory on disk: `manifest.json` and the assets it lists, which the host does not build (`ORCA_MOBILE_WEB_DIR`).
use orca_map_mobile::BundleSource;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

pub struct DirBundle(pub PathBuf);

impl BundleSource for DirBundle {
    fn manifest(&self) -> Result<Option<Value>, String> {
        let path = self.0.join("manifest.json");
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map(Some).map_err(|e| format!("{} is not JSON: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    fn digest(&self, path: &str) -> Result<(u64, String), String> {
        let mut file = std::fs::File::open(self.0.join(path)).map_err(|e| e.to_string())?;
        let (mut hash, mut len, mut block) = (Sha256::new(), 0u64, [0u8; 64 * 1024]);
        loop {
            let n = file.read(&mut block).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            hash.update(&block[..n]);
            len += n as u64;
        }
        Ok((len, hash.finalize().iter().map(|b| format!("{b:02x}")).collect()))
    }

    fn read(&self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>, String> {
        let mut file = std::fs::File::open(self.0.join(path)).map_err(|e| e.to_string())?;
        file.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
        let mut out = Vec::with_capacity(len);
        file.take(len as u64).read_to_end(&mut out).map_err(|e| e.to_string())?;
        Ok(out)
    }
}
