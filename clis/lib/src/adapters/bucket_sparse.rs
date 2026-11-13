//! Our registry: the crate uploaded once under `crates/` of the public libs bucket, its line appended to the sparse index file
//! under `index/` and `index/config.json` kept current, all through the one write store. Written in that order so the index never
//! names a `.crate` that is not there. Readers fetch all of it with a plain GET; the existing index is read the same way.
use super::cargo::package;
use crate::domain::index;
use crate::ports::{Failure, IndexSource, Published, Publisher, Release};
use blob_cache::domain::Key;
use blob_cache::ports::{BlobStore, PutOutcome};

pub struct BucketSparse {
    store: Box<dyn BlobStore>,
    index: Box<dyn IndexSource>,
    /// What `config.json` says, derived from the registry's `indexUrl`.
    config: String,
}

impl BucketSparse {
    pub fn new(store: Box<dyn BlobStore>, index: Box<dyn IndexSource>, index_url: &str) -> Result<BucketSparse, Failure> {
        let config = index::config(index_url).map_err(|e| Failure::new(e).next("set indexUrl to the bucket's public `<base>/index/` in libraries.json"))?;
        Ok(BucketSparse { store, index, config })
    }

    fn key(path: &str) -> Result<Key, Failure> {
        Key::at(path).map_err(|e| Failure::new(e.to_string()))
    }

    fn io<T>(what: &str, result: Result<T, blob_cache::ports::StoreError>) -> Result<T, Failure> {
        result.map_err(|e| Failure::new(format!("{what}: {e}")).next("check LIBS_WRITE_PAR_URL (`wf lib status` shows whether the index answers)"))
    }

    fn read_index(&self, sparse_path: &str) -> Result<String, Failure> {
        Ok(self.index.file(sparse_path)?.unwrap_or_default())
    }
}

impl Publisher for BucketSparse {
    fn id(&self) -> &'static str {
        "bucket-sparse"
    }

    fn already_released(&self, name: &str, version: &str) -> Result<bool, Failure> {
        if Self::io("crate lookup", self.store.exists(&Self::key(&index::crate_path(name, version))?))? {
            return Ok(true);
        }
        Ok(index::versions(&self.read_index(&index::path(name))?).iter().any(|(v, _)| v == version))
    }

    fn publish(&self, release: &Release) -> Result<Published, Failure> {
        let scratch = tempfile::Builder::new().prefix("wf-lib-").tempdir().map_err(|e| Failure::new(e.to_string()))?;
        let packaged = package(release.manifest, scratch.path())?;
        let (name, version) = (packaged.entry.name.clone(), packaged.entry.vers.clone());
        let crate_path = index::crate_path(&name, &version);
        match Self::io(&crate_path, self.store.put(&Self::key(&crate_path)?, &packaged.bytes, true))? {
            PutOutcome::Stored => {}
            PutOutcome::Exists => return Err(Failure::new(format!("{name} {version} is already released; versions are immutable")).next("release a new version instead")),
        }
        let sparse_path = index::path(&name);
        let index_key = index::key(&sparse_path);
        let updated = index::append(&self.read_index(&sparse_path)?, &packaged.entry).map_err(|e| Failure::new(e).next("the .crate is uploaded but the index was not written; release the next patch version"))?;
        Self::io(&index_key, self.store.put(&Self::key(&index_key)?, updated.as_bytes(), false))?;
        Self::io(index::CONFIG, self.store.put(&Self::key(index::CONFIG)?, self.config.as_bytes(), false))?;
        Ok(Published { objects: vec![crate_path, index_key, index::CONFIG.to_string()], checksum: Some(packaged.entry.cksum), bytes: Some(packaged.bytes.len() as u64) })
    }

    fn can_delete(&self) -> bool {
        true
    }
}
