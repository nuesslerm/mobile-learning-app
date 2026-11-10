//! What `prune` may delete: a version no lockfile on dev pins, that is not the latest of its crate, and is older than the grace.
use super::version::latest;
use std::collections::{BTreeMap, HashSet};

/// One stored `.crate`, from a listing of the bucket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stored {
    pub name: String,
    pub version: String,
    /// The object's name in the bucket, as the listing gave it.
    pub object: String,
    pub bytes: u64,
    pub created_ms: i64,
}

/// `libs/crates/<name>/<version>.crate` as a `Stored`; anything else under the prefix is not a crate.
pub fn stored(object: &str, bytes: u64, created_ms: i64) -> Option<Stored> {
    let rest = object.strip_prefix("libs/crates/")?;
    let (name, file) = rest.split_once('/')?;
    let version = file.strip_suffix(".crate")?;
    Some(Stored { name: name.to_string(), version: version.to_string(), object: object.to_string(), bytes, created_ms })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kept {
    pub name: String,
    pub version: String,
    pub reason: &'static str,
}

#[derive(Debug, Default)]
pub struct Plan {
    pub delete: Vec<Stored>,
    pub kept: Vec<Kept>,
}

pub const DAY_MS: i64 = 86_400_000;

/// `pinned` holds `(crate, version)` of every registry entry of every lockfile at origin/dev.
pub fn plan(all: &[Stored], pinned: &HashSet<(String, String)>, now_ms: i64, grace_days: i64) -> Plan {
    let mut by_crate: BTreeMap<&str, Vec<&Stored>> = BTreeMap::new();
    for one in all {
        by_crate.entry(&one.name).or_default().push(one);
    }
    let mut plan = Plan::default();
    for (name, versions) in by_crate {
        let newest = latest(versions.iter().map(|s| s.version.as_str()));
        for one in versions {
            let reason = if newest.as_deref() == Some(one.version.as_str()) {
                Some("latest")
            } else if pinned.contains(&(name.to_string(), one.version.clone())) {
                Some("pinned")
            } else if now_ms - one.created_ms < grace_days.saturating_mul(DAY_MS) {
                Some("grace")
            } else {
                None
            };
            match reason {
                Some(reason) => plan.kept.push(Kept { name: name.to_string(), version: one.version.clone(), reason }),
                None => plan.delete.push(one.clone()),
            }
        }
    }
    plan
}
