//! The index as readers get it: a plain public GET of `<indexUrl><sparse path>`, no credential. A crate that has no file is not a failure.
use crate::ports::{Failure, IndexSource};
use blob_cache::domain::Key;
use blob_cache::ports::BlobStore;

pub struct PublicIndex {
    store: Box<dyn BlobStore>,
}

impl PublicIndex {
    /// `store` is rooted at the registry's `indexUrl`.
    pub fn new(store: Box<dyn BlobStore>) -> PublicIndex {
        PublicIndex { store }
    }
}

impl IndexSource for PublicIndex {
    fn file(&self, sparse_path: &str) -> Result<Option<String>, Failure> {
        let key = Key::at(sparse_path).map_err(|e| Failure::new(e.to_string()))?;
        self.store
            .get(&key)
            .map(|found| found.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()))
            .map_err(|e| Failure::new(format!("{sparse_path}: {e}")).next("the registry's indexUrl in libraries.json must answer a plain GET (`curl <indexUrl>config.json`)"))
    }
}
