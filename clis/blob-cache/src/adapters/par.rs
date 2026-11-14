//! `BlobStore` over OCI Object Storage pre-authenticated requests: `GET`, `PUT` and `HEAD` on `<par url><object name>`. A PAR
//! carries no identity, cannot list or delete, and one link reads where another writes. The HTTP work is `HttpStore` with `Auth::None`.
use super::http::{Auth, HttpStore, Side};
use crate::domain::Key;
use crate::ports::{BlobStore, PutOutcome, StoreError};
use cli_http::Via;
use std::path::Path;

pub struct OciParStore(HttpStore);

impl OciParStore {
    /// Both links end at the bucket's `/o/`; with no write link the store is read-only.
    pub fn new(read_par_url: &str, write_par_url: Option<&str>) -> OciParStore {
        OciParStore(HttpStore::new(Side::new(read_par_url, Auth::None), write_par_url.map(|url| Side::new(url, Auth::None))))
    }

    /// Write with `if-none-match: *` instead of checking first. Only for a bucket whose PAR is known to accept it.
    pub fn conditional_put(self, on: bool) -> OciParStore {
        OciParStore(self.0.conditional_put(on))
    }

    pub fn via(self, via: Via) -> OciParStore {
        OciParStore(self.0.via(via))
    }
}

impl BlobStore for OciParStore {
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>, StoreError> {
        self.0.get(key)
    }

    fn exists(&self, key: &Key) -> Result<bool, StoreError> {
        self.0.exists(key)
    }

    fn put(&self, key: &Key, body: &[u8], if_absent: bool) -> Result<PutOutcome, StoreError> {
        self.0.put(key, body, if_absent)
    }

    fn get_file(&self, key: &Key, dest: &Path) -> Result<bool, StoreError> {
        self.0.get_file(key, dest)
    }

    fn put_file(&self, key: &Key, source: &Path, if_absent: bool) -> Result<PutOutcome, StoreError> {
        self.0.put_file(key, source, if_absent)
    }

    fn requests_per_put(&self, if_absent: bool) -> u64 {
        self.0.requests_per_put(if_absent)
    }
}
