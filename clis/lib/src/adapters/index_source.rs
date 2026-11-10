//! The index as readers get it: through the read proxy (a plain GET, no credential), and when the proxy does not answer, through the
//! bucket's read link if one resolves (a machine off the tailnet). A crate the proxy says has no file is not a failure and not retried.
use crate::domain::index;
use crate::ports::{Failure, IndexSource};
use blob_cache::domain::Key;
use blob_cache::ports::{BlobStore, StoreError};

pub struct ProxyThenLink {
    proxy: Box<dyn BlobStore>,
    link: Option<Box<dyn BlobStore>>,
}

impl ProxyThenLink {
    /// `proxy` is rooted at the proxy's index base; `link` at the bucket's `libs/` prefix.
    pub fn new(proxy: Box<dyn BlobStore>, link: Option<Box<dyn BlobStore>>) -> ProxyThenLink {
        ProxyThenLink { proxy, link }
    }
}

fn key(path: &str) -> Result<Key, Failure> {
    Key::at(path).map_err(|e| Failure::new(e.to_string()))
}

fn text(found: Option<Vec<u8>>) -> Option<String> {
    found.map(|b| String::from_utf8_lossy(&b).into_owned())
}

impl IndexSource for ProxyThenLink {
    fn file(&self, sparse_path: &str) -> Result<Option<String>, Failure> {
        let unreachable = |e: StoreError| Failure::new(format!("{sparse_path}: {e}")).next("the libs read proxy is on the tailnet (`wf vm net up`); off the tailnet set LIBS_READ_PAR_URL");
        match self.proxy.get(&key(sparse_path)?) {
            Ok(found) => Ok(text(found)),
            Err(proxy_error) => match &self.link {
                Some(link) => link.get(&key(&index::key(sparse_path))?).map(text).map_err(|e| Failure::new(format!("{sparse_path}: the proxy ({proxy_error}) and the read link ({e}) both failed"))),
                None => Err(unreachable(proxy_error)),
            },
        }
    }
}
