use serde::{Deserialize, Serialize};
use std::fmt;

/// The directory name pnpm keeps its store layout in (`v11`). An archive of one layout is never unpacked as another, so the
/// layout is part of the object names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout(String);

impl Layout {
    /// `v` and digits, as `pnpm store path` ends; anything else is not a layout this cache knows.
    pub fn parse(dir_name: &str) -> Option<Layout> {
        let digits = dir_name.strip_prefix('v')?;
        (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then(|| Layout(dir_name.to_string()))
    }

    pub fn archive_name(&self) -> String {
        format!("store-{}.tar.gz", self.0)
    }

    pub fn sidecar_name(&self) -> String {
        format!("store-{}.json", self.0)
    }
}

impl fmt::Display for Layout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What is stored beside the archive: the lockfile it was saved for, and its size.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Sidecar {
    pub lockfile_sha256: String,
    pub bytes: u64,
}

impl Sidecar {
    /// Anything that is not a sidecar is no record, and the store is saved again.
    pub fn parse(stored: &[u8]) -> Option<Sidecar> {
        serde_json::from_slice(stored).ok()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a sidecar is plain data")
    }
}

/// Whether the archive on record was made for this lockfile; only then is a save skipped (and only if the archive is still there).
pub fn recorded_for(sidecar: Option<&Sidecar>, lockfile_sha256: &str) -> bool {
    sidecar.is_some_and(|s| s.lockfile_sha256 == lockfile_sha256)
}
