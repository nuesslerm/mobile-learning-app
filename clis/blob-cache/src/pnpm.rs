//! The pnpm store as one cache object (`pnpm/g<generation>/store-<layout>.tar.gz`, `store-<layout>.json` beside it): restored into
//! an empty local store before `pnpm install`, saved after a green install of a lockfile the bucket has not seen. A bucket
//! problem is `Nothing` with its reason; only a step on this machine that broke is `Reason::Failed`.
use crate::domain::pnpm::{recorded_for, Layout, Sidecar};
use crate::domain::Kind;
use crate::ports::{BlobStore, Packer};
use crate::{Cache, Put, Skip, Writes};
use std::fmt;
use std::path::{Path, PathBuf};

/// Why a verb did nothing, or what broke.
#[derive(Debug, PartialEq, Eq)]
pub enum Reason {
    StoreNotEmpty,
    /// The app-build directory already has content, so there is nothing to restore into.
    DirNotEmpty,
    StoreEmpty,
    NoObject,
    LockfileUnchanged,
    /// The saved object already holds what is on disk.
    Unchanged,
    /// The installed tree is not the one `pnpm-lock.yaml` describes, so the install was not a green one of this lockfile.
    InstallNotCurrent,
    GuardRefused(String),
    TooLarge,
    /// The tenancy's Object Storage is full: a probe write was refused with `StorageQuotaExceeded`, so the archive was not packed.
    StorageFull,
    /// The bucket was not used: why, as `Skip::as_str` says it.
    Cold(&'static str),
    /// A step on this machine failed.
    Failed(String),
}

impl Reason {
    pub fn failed(&self) -> bool {
        matches!(self, Reason::Failed(_))
    }

    fn cold(skip: Skip) -> Reason {
        Reason::Cold(skip.as_str())
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reason::StoreNotEmpty => f.write_str("local store not empty"),
            Reason::DirNotEmpty => f.write_str("local directory not empty"),
            Reason::StoreEmpty => f.write_str("local store empty, nothing to save"),
            Reason::NoObject => f.write_str("no object"),
            Reason::LockfileUnchanged => f.write_str("lockfile unchanged since the saved object"),
            Reason::Unchanged => f.write_str("unchanged since the saved object"),
            Reason::InstallNotCurrent => f.write_str("node_modules does not hold this lockfile, so there was no green install of it"),
            Reason::GuardRefused(why) => write!(f, "guard refused the write, {why}"),
            Reason::TooLarge => f.write_str("the archive is over the size cap"),
            Reason::StorageFull => f.write_str("the tenancy's Object Storage is full (StorageQuotaExceeded), so the store was not packed"),
            Reason::Cold(why) => write!(f, "store not used, {why}"),
            Reason::Failed(why) => f.write_str(why),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Done { bytes: u64 },
    /// Nothing was done, and why; `Reason::Failed` is the one that broke.
    Nothing(Reason),
}

/// Removes what a verb left on disk beside the store, whichever way it ends.
struct Scratch(Vec<PathBuf>);

impl Drop for Scratch {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_dir_all(path).or_else(|_| std::fs::remove_file(path));
        }
    }
}

fn failed(error: impl fmt::Display) -> Outcome {
    Outcome::Nothing(Reason::Failed(error.to_string()))
}

/// A directory with nothing in it, or none. One that cannot be read is left alone.
fn empty(dir: &Path) -> bool {
    match std::fs::read_dir(dir) {
        Ok(mut entries) => entries.next().is_none(),
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
    }
}

/// Scratch names beside the store, on its own file system, so a rename into place cannot cross one.
fn beside(store: &Path, layout: &Layout, what: &str) -> Option<PathBuf> {
    Some(store.parent()?.join(format!(".{layout}-{what}-{}", std::process::id())))
}

/// What is known about a restore without asking the bucket, so a caller can stop before it resolves a link or sends a request.
pub fn restore_precheck(store: &Path) -> Option<Reason> {
    (!empty(store)).then_some(Reason::StoreNotEmpty)
}

/// Fills `store` from the bucket when it is empty. The archive is unpacked next to the store and moved into place whole, so a
/// body or an archive that breaks half way leaves the store as it was.
pub fn restore<S: BlobStore>(cache: &Cache<S>, packer: &dyn Packer, store: &Path, layout: &Layout) -> Outcome {
    if let Some(reason) = restore_precheck(store) {
        return Outcome::Nothing(reason);
    }
    let key = match cache.key(Kind::Pnpm, &layout.archive_name()) {
        Ok(key) => key,
        Err(error) => return failed(error),
    };
    let (Some(parent), Some(archive), Some(staging)) = (store.parent(), beside(store, layout, "restore.tar.gz"), beside(store, layout, "restore")) else {
        return failed(format!("{} has no parent directory", store.display()));
    };
    if let Err(error) = std::fs::create_dir_all(parent) {
        return failed(format!("{}: {error}", parent.display()));
    }
    let _scratch = Scratch(vec![archive.clone(), staging.clone()]);
    let bytes = match cache.get_file(&key, &archive) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Outcome::Nothing(Reason::NoObject),
        Err(skip) => return Outcome::Nothing(Reason::cold(skip)),
    };
    if let Err(error) = std::fs::create_dir_all(&staging) {
        return failed(format!("{}: {error}", staging.display()));
    }
    if let Err(error) = packer.unpack(&archive, &staging) {
        return failed(error);
    }
    // an empty directory is replaced by the rename; one that filled up meanwhile is not
    match std::fs::rename(&staging, store) {
        Ok(()) => Outcome::Done { bytes },
        Err(error) => failed(format!("{}: {error}", store.display())),
    }
}

/// What a save needs to know about this checkout; the caller found it out.
pub struct SaveFacts<'a> {
    pub store: &'a Path,
    pub layout: &'a Layout,
    /// sha256 of `pnpm-lock.yaml`.
    pub lockfile_sha256: &'a str,
    /// sha256 of the lockfile the last install recorded in `node_modules/.pnpm/lock.yaml`, if there is one.
    pub installed_lockfile_sha256: Option<&'a str>,
}

/// What is known about a save without asking the bucket: the guard, the install and the store, in that order.
pub fn save_precheck(writes: &Writes, facts: &SaveFacts) -> Option<Reason> {
    if let Writes::ReadOnly(why) = writes {
        return Some(Reason::GuardRefused(why.clone()));
    }
    if facts.installed_lockfile_sha256 != Some(facts.lockfile_sha256) {
        return Some(Reason::InstallNotCurrent);
    }
    empty(facts.store).then_some(Reason::StoreEmpty)
}

/// No store of npm packages compresses better than this, so a tree this many times over the entry cap cannot fit it packed.
const COMPRESSION_FLOOR: u64 = 4;

/// Bytes under `dir`, counted until they pass `limit`.
fn tree_bytes_over(dir: &Path, limit: u64) -> bool {
    let mut total = 0u64;
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(&next).into_iter().flatten().flatten() {
            match entry.metadata() {
                Ok(meta) if meta.is_dir() => pending.push(entry.path()),
                Ok(meta) => {
                    total = total.saturating_add(meta.len());
                    if total > limit {
                        return true;
                    }
                }
                Err(_) => {}
            }
        }
    }
    false
}

/// What can be known before the archive is packed, which takes minutes: the tree is not too big for the entry cap, and the store
/// accepts a write at all (a one-word probe object), so a store that would be refused is not packed for nothing.
fn before_packing<S: BlobStore>(cache: &Cache<S>, store: &Path) -> Option<Reason> {
    let cap = cache.max_entry_bytes(Kind::Pnpm);
    if tree_bytes_over(store, cap.saturating_mul(COMPRESSION_FLOOR)) {
        return Some(Reason::TooLarge);
    }
    let probe = match cache.key(Kind::Probe, "pnpm-save") {
        Ok(key) => key,
        Err(error) => return Some(Reason::Failed(error.to_string())),
    };
    match cache.put(&probe, b"probe", false) {
        Put::Stored | Put::Exists => None,
        Put::Skipped(_) if cache.stats().quota_exceeded > 0 => Some(Reason::StorageFull),
        Put::Skipped(skip) => Some(Reason::cold(skip)),
    }
}

/// Uploads the store as its one object, overwritten in place, and then the sidecar that says which lockfile it holds. Only when
/// the write is allowed, the install is current, and the bucket has no object for this lockfile.
pub fn save<S: BlobStore>(cache: &Cache<S>, packer: &dyn Packer, facts: &SaveFacts) -> Outcome {
    if let Some(reason) = save_precheck(cache.writes(), facts) {
        return Outcome::Nothing(reason);
    }
    let (archive_key, sidecar_key) = match (cache.key(Kind::Pnpm, &facts.layout.archive_name()), cache.key(Kind::Pnpm, &facts.layout.sidecar_name())) {
        (Ok(archive), Ok(sidecar)) => (archive, sidecar),
        (Err(error), _) | (_, Err(error)) => return failed(error),
    };
    let recorded = match cache.fetch(&sidecar_key) {
        Ok(found) => found.as_deref().and_then(Sidecar::parse),
        Err(skip) => return Outcome::Nothing(Reason::cold(skip)),
    };
    if recorded_for(recorded.as_ref(), facts.lockfile_sha256) {
        match cache.exists(&archive_key) {
            Ok(true) => return Outcome::Nothing(Reason::LockfileUnchanged),
            Ok(false) => {}
            Err(skip) => return Outcome::Nothing(Reason::cold(skip)),
        }
    }
    if let Some(reason) = before_packing(cache, facts.store) {
        return Outcome::Nothing(reason);
    }
    let Some(archive) = beside(facts.store, facts.layout, "save.tar.gz") else {
        return failed(format!("{} has no parent directory", facts.store.display()));
    };
    let _scratch = Scratch(vec![archive.clone()]);
    if let Err(error) = packer.pack(facts.store, &archive) {
        return failed(error);
    }
    let bytes = match std::fs::metadata(&archive) {
        Ok(meta) => meta.len(),
        Err(error) => return failed(format!("{}: {error}", archive.display())),
    };
    match cache.put_file(&archive_key, &archive, false) {
        Put::Stored => {}
        Put::Exists => return Outcome::Nothing(Reason::LockfileUnchanged),
        Put::Skipped(Skip::TooLarge) => return Outcome::Nothing(Reason::TooLarge),
        Put::Skipped(skip) => return Outcome::Nothing(Reason::cold(skip)),
    }
    let sidecar = Sidecar { lockfile_sha256: facts.lockfile_sha256.to_string(), bytes };
    match cache.put(&sidecar_key, sidecar.to_json().as_bytes(), false) {
        Put::Stored | Put::Exists => Outcome::Done { bytes },
        Put::Skipped(skip) => Outcome::Nothing(Reason::cold(skip)),
    }
}
