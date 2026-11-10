//! The app's build state (Metro's transform cache and `tsc`'s build info, one directory) as one restore-latest cache object
//! (`metro/g<generation>/app-build.tar.gz`, `app-build.json` beside it): restored into an empty local directory before a build, saved
//! after one when the directory changed. Unlike an Nx output it does not need the same inputs to come back: Metro and `tsc` key their
//! own entries by file content, so an older tree's state is still mostly right for a newer one. A bucket problem is `Nothing` with its
//! reason; only a step on this machine that broke is `Reason::Failed`.
use crate::domain::Kind;
use crate::pnpm::{Outcome, Reason};
use crate::ports::{BlobStore, Packer};
use crate::{Cache, Put, Skip, Writes};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const ARCHIVE: &str = "app-build.tar.gz";
pub const SIDECAR: &str = "app-build.json";

/// Removes what a verb left beside the directory, whichever way it ends.
struct Scratch(Vec<PathBuf>);

impl Drop for Scratch {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_dir_all(path).or_else(|_| std::fs::remove_file(path));
        }
    }
}

fn failed(error: impl std::fmt::Display) -> Outcome {
    Outcome::Nothing(Reason::Failed(error.to_string()))
}

fn cold(skip: Skip) -> Outcome {
    Outcome::Nothing(Reason::Cold(skip.as_str()))
}

/// A directory with nothing in it, or none. One that cannot be read is left alone.
pub fn is_empty(dir: &Path) -> bool {
    match std::fs::read_dir(dir) {
        Ok(mut entries) => entries.next().is_none(),
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
    }
}

fn beside(dir: &Path, what: &str) -> Option<PathBuf> {
    Some(dir.parent()?.join(format!(".app-build-{what}-{}", std::process::id())))
}

/// What is known about a restore without asking the bucket.
pub fn restore_precheck(dir: &Path) -> Option<Reason> {
    (!is_empty(dir)).then_some(Reason::DirNotEmpty)
}

/// Fills `dir` from the bucket when it is empty. The archive is unpacked next to it and moved into place whole, so a body or an
/// archive that breaks half way leaves the directory as it was.
pub fn restore<S: BlobStore>(cache: &Cache<S>, packer: &dyn Packer, dir: &Path) -> Outcome {
    if let Some(reason) = restore_precheck(dir) {
        return Outcome::Nothing(reason);
    }
    let key = match cache.key(Kind::Metro, ARCHIVE) {
        Ok(key) => key,
        Err(error) => return failed(error),
    };
    let (Some(parent), Some(archive), Some(staging)) = (dir.parent(), beside(dir, "restore.tar.gz"), beside(dir, "restore")) else {
        return failed(format!("{} has no parent directory", dir.display()));
    };
    if let Err(error) = std::fs::create_dir_all(parent) {
        return failed(format!("{}: {error}", parent.display()));
    }
    let _scratch = Scratch(vec![archive.clone(), staging.clone()]);
    let bytes = match cache.get_file(&key, &archive) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Outcome::Nothing(Reason::NoObject),
        Err(skip) => return cold(skip),
    };
    if let Err(error) = std::fs::create_dir_all(&staging) {
        return failed(format!("{}: {error}", staging.display()));
    }
    if let Err(error) = packer.unpack(&archive, &staging) {
        return failed(error);
    }
    match std::fs::rename(&staging, dir) {
        Ok(()) => Outcome::Done { bytes },
        Err(error) => failed(format!("{}: {error}", dir.display())),
    }
}

/// What a save needs to know; the caller found it out.
pub struct SaveFacts<'a> {
    pub dir: &'a Path,
    /// A digest of the directory's file names and sizes: unchanged means nothing new to upload.
    pub fingerprint: &'a str,
}

fn recorded_fingerprint(sidecar: &[u8]) -> Option<String> {
    serde_json::from_slice::<Value>(sidecar).ok()?["fingerprint"].as_str().map(str::to_string)
}

/// What is known about a save without asking the bucket: the guard, then the directory.
pub fn save_precheck(writes: &Writes, facts: &SaveFacts) -> Option<Reason> {
    if let Writes::ReadOnly(why) = writes {
        return Some(Reason::GuardRefused(why.clone()));
    }
    is_empty(facts.dir).then_some(Reason::StoreEmpty)
}

/// Uploads the directory as its one object, overwritten in place, and then the sidecar that records its fingerprint. Only when the
/// write is allowed, the directory has content, and the saved object is not already this directory.
pub fn save<S: BlobStore>(cache: &Cache<S>, packer: &dyn Packer, facts: &SaveFacts) -> Outcome {
    if let Some(reason) = save_precheck(cache.writes(), facts) {
        return Outcome::Nothing(reason);
    }
    let (archive_key, sidecar_key) = match (cache.key(Kind::Metro, ARCHIVE), cache.key(Kind::Metro, SIDECAR)) {
        (Ok(archive), Ok(sidecar)) => (archive, sidecar),
        (Err(error), _) | (_, Err(error)) => return failed(error),
    };
    let recorded = match cache.fetch(&sidecar_key) {
        Ok(found) => found.as_deref().and_then(recorded_fingerprint),
        Err(skip) => return cold(skip),
    };
    if recorded.as_deref() == Some(facts.fingerprint) {
        match cache.exists(&archive_key) {
            Ok(true) => return Outcome::Nothing(Reason::Unchanged),
            Ok(false) => {}
            Err(skip) => return cold(skip),
        }
    }
    let Some(archive) = beside(facts.dir, "save.tar.gz") else {
        return failed(format!("{} has no parent directory", facts.dir.display()));
    };
    let _scratch = Scratch(vec![archive.clone()]);
    if let Err(error) = packer.pack(facts.dir, &archive) {
        return failed(error);
    }
    let bytes = match std::fs::metadata(&archive) {
        Ok(meta) => meta.len(),
        Err(error) => return failed(format!("{}: {error}", archive.display())),
    };
    match cache.put_file(&archive_key, &archive, false) {
        Put::Stored => {}
        Put::Exists => return Outcome::Nothing(Reason::Unchanged),
        Put::Skipped(Skip::TooLarge) => return Outcome::Nothing(Reason::TooLarge),
        Put::Skipped(skip) => return cold(skip),
    }
    let sidecar = json!({ "fingerprint": facts.fingerprint, "bytes": bytes }).to_string();
    match cache.put(&sidecar_key, sidecar.as_bytes(), false) {
        Put::Stored | Put::Exists => Outcome::Done { bytes },
        Put::Skipped(skip) => cold(skip),
    }
}
