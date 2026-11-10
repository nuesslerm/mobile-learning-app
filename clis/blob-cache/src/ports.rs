//! What the cache needs from a blob store. Two ports, so the credential that runs on every push can read and write but never
//! list or delete: `BlobStore` is the data plane, `BlobAdmin` the operator's.
use crate::domain::Key;
use std::fmt;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, PartialEq, Eq)]
pub enum StoreError {
    /// A timeout, a transport failure or a server error: the store cannot be relied on for the rest of the run.
    Unreachable(String),
    /// The link or key is expired or revoked.
    Credential(String),
    /// The store refused this one request and is otherwise fine.
    Rejected(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Unreachable(why) => write!(f, "unreachable: {why}"),
            StoreError::Credential(why) => write!(f, "credential: {why}"),
            StoreError::Rejected(why) => write!(f, "rejected: {why}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl StoreError {
    /// The HTTP status the store answered with, read from the adapter's already scrubbed words (`... HTTP 503`).
    pub fn status(&self) -> Option<u16> {
        let text = self.to_string();
        let digits: String = text.split("HTTP ").nth(1)?.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok()
    }

    /// What kind of failure this is, for a count: the tenancy's full storage is told apart from any other refusal.
    pub fn class(&self) -> &'static str {
        match self {
            _ if self.to_string().contains("StorageQuotaExceeded") => "quota-exceeded",
            StoreError::Unreachable(_) => "unreachable",
            StoreError::Credential(_) => "credential",
            StoreError::Rejected(_) => "rejected",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum PutOutcome {
    Stored,
    Exists,
}

/// Blocking reads and writes of whole objects; `get_file` and `put_file` stream one to or from a file for objects too large to
/// hold in memory.
pub trait BlobStore: Send + Sync {
    /// `Ok(None)` is a miss.
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>, StoreError>;

    /// `if_absent`: the first writer wins and a later one gets `Exists`. Otherwise the object is overwritten.
    fn put(&self, key: &Key, body: &[u8], if_absent: bool) -> Result<PutOutcome, StoreError>;

    fn exists(&self, key: &Key) -> Result<bool, StoreError>;

    /// The object into `dest`, never held in memory; `Ok(false)` is a miss, and a miss or a broken body leaves no file.
    fn get_file(&self, _key: &Key, _dest: &Path) -> Result<bool, StoreError> {
        Err(StoreError::Rejected("this store does not stream objects".into()))
    }

    /// `put` with the body read from `source` as it is sent.
    fn put_file(&self, _key: &Key, _source: &Path, _if_absent: bool) -> Result<PutOutcome, StoreError> {
        Err(StoreError::Rejected("this store does not stream objects".into()))
    }

    /// How many billed requests one `put` costs, for the run budget: a store with no conditional write checks first.
    fn requests_per_put(&self, _if_absent: bool) -> u64 {
        1
    }
}

impl<T: BlobStore + ?Sized> BlobStore for Arc<T> {
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>, StoreError> {
        (**self).get(key)
    }

    fn put(&self, key: &Key, body: &[u8], if_absent: bool) -> Result<PutOutcome, StoreError> {
        (**self).put(key, body, if_absent)
    }

    fn exists(&self, key: &Key) -> Result<bool, StoreError> {
        (**self).exists(key)
    }

    fn get_file(&self, key: &Key, dest: &Path) -> Result<bool, StoreError> {
        (**self).get_file(key, dest)
    }

    fn put_file(&self, key: &Key, source: &Path, if_absent: bool) -> Result<PutOutcome, StoreError> {
        (**self).put_file(key, source, if_absent)
    }

    fn requests_per_put(&self, if_absent: bool) -> u64 {
        (**self).requests_per_put(if_absent)
    }
}

/// A directory to and from one archive file: how the pnpm store becomes one object.
pub trait Packer {
    /// Everything under `dir` as an archive at `archive`.
    fn pack(&self, dir: &Path, archive: &Path) -> Result<(), String>;

    /// The archive's files into `dir`, which exists.
    fn unpack(&self, archive: &Path, dir: &Path) -> Result<(), String>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectInfo {
    pub name: String,
    pub bytes: u64,
    pub created_ms: i64,
    /// The store's version tag of the object as listed, when it reports one: a delete that names it fails if the object was rewritten since.
    pub etag: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Usage {
    pub stored_bytes: u64,
    pub objects: u64,
    /// `None` when the store reports no request count.
    pub requests_this_month: Option<u64>,
}

pub trait BlobAdmin {
    fn usage(&self) -> Result<Usage, StoreError>;
    fn list(&self, prefix: &str) -> Result<Vec<ObjectInfo>, StoreError>;
    /// Object names, as `list` gives them; returns how many were deleted.
    fn delete(&self, names: &[String]) -> Result<usize, StoreError>;
}
