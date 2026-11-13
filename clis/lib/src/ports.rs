//! What the verbs need from outside. The blob store is blob-cache's `BlobStore` (data plane) and `BlobAdmin` (list, delete); the
//! two ports here are the registry a release goes to and the files of a checkout.
use crate::domain::libraries::Library;
use std::path::Path;

/// Why a verb stopped, and what to do next. Printed as `{ok: false, error, next}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub error: String,
    pub next: Option<String>,
}

impl Failure {
    pub fn new(error: impl Into<String>) -> Failure {
        Failure { error: error.into(), next: None }
    }

    pub fn next(mut self, step: impl Into<String>) -> Failure {
        self.next = Some(step.into());
        self
    }
}

impl From<String> for Failure {
    fn from(error: String) -> Failure {
        Failure::new(error)
    }
}

/// One library version, with its manifest already holding the version.
pub struct Release<'a> {
    pub library: &'a Library,
    pub manifest: &'a Path,
    pub version: &'a str,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Published {
    /// What the registry now holds for this version, as names the adapter chooses.
    pub objects: Vec<String>,
    pub checksum: Option<String>,
    pub bytes: Option<u64>,
}

/// A registry a library can be released to. A new registry kind is one adapter; the verbs do not change.
pub trait Publisher {
    fn id(&self) -> &'static str;

    /// Whether this version is already there, checked before the manifest is touched. A registry with no cheap check says no and
    /// refuses the duplicate itself when `publish` runs.
    fn already_released(&self, name: &str, version: &str) -> Result<bool, Failure>;

    fn publish(&self, release: &Release) -> Result<Published, Failure>;

    /// Whether `prune` may delete from this registry: a bucket can, crates.io cannot.
    fn can_delete(&self) -> bool {
        false
    }
}

/// The registry's sparse index as a reader sees it (through the read proxy): one crate's file by its sparse path, `None` when the
/// crate has none yet.
pub trait IndexSource {
    fn file(&self, sparse_path: &str) -> Result<Option<String>, Failure>;
}

/// Whether a library may be published now: its files committed, pushed and free of credentials.
pub trait Guard {
    fn check(&self, library: &Library) -> Result<(), Failure>;
}

/// The tracked files of one name (`Cargo.lock`, `Cargo.toml`) in a checkout or at a git revision: `(repo-relative path, text)`.
pub trait Checkout {
    fn files(&self, file_name: &str) -> Result<Vec<(String, String)>, Failure>;
}
