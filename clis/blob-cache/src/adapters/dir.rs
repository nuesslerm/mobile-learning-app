//! `BlobStore` over a directory, for tests and local use. A first write wins atomically: the body is written beside its key and
//! hard-linked into place, which fails when the key is already there.
use crate::domain::Key;
use crate::ports::{BlobStore, PutOutcome, StoreError};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP: AtomicU64 = AtomicU64::new(0);

pub struct DirStore {
    root: PathBuf,
}

impl DirStore {
    pub fn new(root: impl Into<PathBuf>) -> DirStore {
        DirStore { root: root.into() }
    }

    /// Writes the body beside `key` with `fill`, then puts it in place.
    fn place(&self, key: &Key, if_absent: bool, fill: impl FnOnce(&Path) -> std::io::Result<()>) -> Result<PutOutcome, StoreError> {
        let path = self.root.join(key.as_str());
        let dir = path.parent().unwrap_or(&self.root);
        std::fs::create_dir_all(dir).map_err(unreachable)?;
        // a key never starts with a dot, so a temporary name cannot be one
        let temp = dir.join(format!(".tmp-{}-{}", std::process::id(), TEMP.fetch_add(1, Ordering::Relaxed)));
        if let Err(error) = fill(&temp) {
            let _ = std::fs::remove_file(&temp);
            return Err(unreachable(error));
        }
        let placed = if if_absent { std::fs::hard_link(&temp, &path).map(|()| std::fs::remove_file(&temp)) } else { std::fs::rename(&temp, &path).map(|()| Ok(())) };
        match placed {
            Ok(_) => Ok(PutOutcome::Stored),
            Err(error) => {
                let _ = std::fs::remove_file(&temp);
                if error.kind() == ErrorKind::AlreadyExists {
                    Ok(PutOutcome::Exists)
                } else {
                    Err(unreachable(error))
                }
            }
        }
    }
}

fn unreachable(error: std::io::Error) -> StoreError {
    StoreError::Unreachable(error.to_string())
}

impl BlobStore for DirStore {
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>, StoreError> {
        match std::fs::read(self.root.join(key.as_str())) {
            Ok(body) => Ok(Some(body)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(unreachable(error)),
        }
    }

    fn exists(&self, key: &Key) -> Result<bool, StoreError> {
        match std::fs::metadata(self.root.join(key.as_str())) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
            Err(error) => Err(unreachable(error)),
        }
    }

    fn put(&self, key: &Key, body: &[u8], if_absent: bool) -> Result<PutOutcome, StoreError> {
        self.place(key, if_absent, |temp| std::fs::write(temp, body))
    }

    fn get_file(&self, key: &Key, dest: &Path) -> Result<bool, StoreError> {
        let mut part = dest.as_os_str().to_owned();
        part.push(format!(".part-{}", std::process::id()));
        let part = PathBuf::from(part);
        match std::fs::copy(self.root.join(key.as_str()), &part) {
            Ok(_) => std::fs::rename(&part, dest).map(|()| true).map_err(|error| {
                let _ = std::fs::remove_file(&part);
                unreachable(error)
            }),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
            Err(error) => {
                let _ = std::fs::remove_file(&part);
                Err(unreachable(error))
            }
        }
    }

    fn put_file(&self, key: &Key, source: &Path, if_absent: bool) -> Result<PutOutcome, StoreError> {
        self.place(key, if_absent, |temp| std::fs::copy(source, temp).map(|_| ()))
    }
}
