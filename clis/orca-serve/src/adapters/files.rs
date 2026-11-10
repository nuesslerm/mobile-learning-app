//! The host's x25519 identity and the paired devices, as JSON files in the state directory, readable by the owner only.
use orca_crypto::ports::{Device, DeviceRegistry, KeyPairStore, StoreError};
use orca_crypto::{b64, KeyPair};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

fn write_private(path: &Path, text: &str) -> Result<(), StoreError> {
    use std::os::unix::fs::OpenOptionsExt;
    let fail = |e: std::io::Error| StoreError(format!("{}: {e}", path.display()));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(fail)?;
    }
    let tmp = path.with_extension("tmp");
    let mut file = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp).map_err(fail)?;
    file.write_all(text.as_bytes()).and_then(|()| file.sync_all()).map_err(fail)?;
    std::fs::rename(&tmp, path).map_err(fail)
}

/// Compares without stopping at the first difference, so the time of a lookup does not say how much of a token was right.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub struct FileKeyPair(pub PathBuf);

impl KeyPairStore for FileKeyPair {
    fn load(&self) -> Result<Option<KeyPair>, StoreError> {
        let text = match std::fs::read_to_string(&self.0) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(StoreError(format!("{}: {e}", self.0.display()))),
        };
        let bad = || StoreError(format!("{} does not hold a host key", self.0.display()));
        let value: Value = serde_json::from_str(&text).map_err(|_| bad())?;
        let secret = value["secretKeyB64"].as_str().and_then(b64::decode_canonical_exact::<32>).ok_or_else(bad)?;
        Ok(Some(KeyPair::from_secret(secret)))
    }

    fn save(&self, pair: &KeyPair) -> Result<(), StoreError> {
        write_private(&self.0, &json!({"secretKeyB64": b64::encode(&pair.secret)}).to_string())
    }
}

/// The paired devices; the file is read on each lookup, so a `pair` run by hand is known to a server already listening.
pub struct FileDevices {
    path: PathBuf,
    write: Mutex<()>,
}

impl FileDevices {
    pub fn new(path: PathBuf) -> Self {
        Self { path, write: Mutex::new(()) }
    }

    fn read(&self) -> Result<Vec<Device>, StoreError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(StoreError(format!("{}: {e}", self.path.display()))),
        };
        let value: Value = serde_json::from_str(&text).map_err(|_| StoreError(format!("{} is not JSON", self.path.display())))?;
        Ok(value.as_array().into_iter().flatten().filter_map(|d| Some(Device { id: d["id"].as_str()?.to_string(), token: d["token"].as_str()?.to_string() })).collect())
    }

    fn write(&self, devices: &[Device]) -> Result<(), StoreError> {
        let rows: Vec<Value> = devices.iter().map(|d| json!({"id": d.id, "token": d.token})).collect();
        write_private(&self.path, &Value::Array(rows).to_string())
    }
}

/// An exclusive advisory lock on a sibling lock file, released when dropped (or when the process dies).
struct FileLock(#[allow(dead_code)] std::fs::File);

impl FileLock {
    fn take(path: &Path) -> Result<Self, StoreError> {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::OpenOptionsExt;
        let fail = |e: std::io::Error| StoreError(format!("{}: {e}", path.display()));
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(fail)?;
        }
        let file = std::fs::OpenOptions::new().write(true).create(true).truncate(false).mode(0o600).open(path).map_err(fail)?;
        // SAFETY: the descriptor is open for the life of `file`.
        while unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() != std::io::ErrorKind::Interrupted {
                return Err(fail(e));
            }
        }
        Ok(Self(file))
    }
}

impl FileDevices {
    /// Serializes a read-modify-write against other processes and other instances on the same file.
    fn exclusive(&self) -> Result<(std::sync::MutexGuard<'_, ()>, FileLock), StoreError> {
        let guard = self.write.lock().unwrap_or_else(|e| e.into_inner());
        let lock = FileLock::take(&self.path.with_extension("lock"))?;
        Ok((guard, lock))
    }
}

impl DeviceRegistry for FileDevices {
    fn find_by_token(&self, token: &str) -> Option<Device> {
        self.read().ok()?.into_iter().find(|d| same(&d.token, token))
    }

    fn register(&self, device: Device) -> Result<(), StoreError> {
        let _held = self.exclusive()?;
        let mut devices = self.read()?;
        devices.retain(|d| d.id != device.id);
        devices.push(device);
        self.write(&devices)
    }

    fn revoke(&self, device_id: &str) -> Result<bool, StoreError> {
        let _held = self.exclusive()?;
        let mut devices = self.read()?;
        let before = devices.len();
        devices.retain(|d| d.id != device_id);
        self.write(&devices)?;
        Ok(devices.len() != before)
    }
}

/// A device is paired while the registry still holds its token under its id.
pub struct RegistryPrincipals(pub std::sync::Arc<dyn DeviceRegistry + Send + Sync>);

impl orca_core::Principals for RegistryPrincipals {
    fn still_paired(&self, device_id: &str, token: &str) -> bool {
        self.0.find_by_token(token).is_some_and(|d| d.id == device_id)
    }
}
