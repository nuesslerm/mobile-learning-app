//! Which objects of the tenancy's buckets may go, and in what order, when the Object Storage cap is near. The tenancy shares one
//! cap across every bucket, and a full tenancy rejects every PUT (`StorageQuotaExceeded`), so the cheapest data goes first and
//! what cannot be rebuilt (terraform state, goldens, the crate registry, the generation pointer) is never a candidate.
use crate::ports::ObjectInfo;
use std::collections::BTreeSet;

pub const HIGH_WATER_PERCENT: u64 = 85;
pub const LOW_WATER_PERCENT: u64 = 70;
const DAY_MS: i64 = 86_400_000;
const FRESH_MS: i64 = DAY_MS;
const EVIDENCE_MS: i64 = 3 * DAY_MS;

/// What a bucket is for, by the name `infra/oracle-cloud` gives it (`${name_prefix}-<role>`). The state bucket is measured by its listing and never evicted; any other bucket is measured, never evicted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Cache,
    Evidence,
    /// The VM state backups (`vm-state.tf`): listed, so the figure is exact, and counted against the cap, but no object is ever a candidate.
    State,
    Other,
}

impl Role {
    pub fn of(bucket: &str) -> Role {
        if bucket.ends_with("-build-cache") {
            Role::Cache
        } else if bucket.ends_with("-verification-artifacts") {
            Role::Evidence
        } else if bucket.ends_with("-vm-state") {
            Role::State
        } else {
            Role::Other
        }
    }
}

/// An evictable kind of object, cheapest to lose first: the derived caches, then the old Nx results and package stores, then
/// evidence that only a report reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    Sccache,
    Metro,
    Probe,
    Nx,
    PnpmOld,
    TestRuns,
    EasUpdates,
    DeviceLab,
}

impl Class {
    pub fn label(self) -> &'static str {
        match self {
            Class::Sccache => "sccache",
            Class::Metro => "metro",
            Class::Probe => "probe",
            Class::Nx => "nx",
            Class::PnpmOld => "pnpm-old",
            Class::TestRuns => "test-runs",
            Class::EasUpdates => "eas-updates",
            Class::DeviceLab => "device-lab",
        }
    }

    /// Evidence is kept longer than a cache entry: a report of a recent run may still link it.
    fn min_age_ms(self) -> i64 {
        match self {
            Class::TestRuns | Class::EasUpdates | Class::DeviceLab => EVIDENCE_MS,
            _ => FRESH_MS,
        }
    }
}

/// The sub-path after `prefix`, when it names a file inside a folder of that prefix (`<run>/bundle.json`, not an index beside the folders).
fn nested<'a>(name: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = name.strip_prefix(prefix)?;
    rest.contains('/').then_some(rest)
}

/// `None` is an object the guard never deletes. A pnpm object is `PnpmOld` here and `pnpm_keep` spares the newest store.
pub fn class_of(role: Role, name: &str) -> Option<Class> {
    match role {
        Role::Cache => [("sccache/", Class::Sccache), ("metro/", Class::Metro), ("probe/", Class::Probe), ("nx/", Class::Nx), ("pnpm/", Class::PnpmOld)]
            .into_iter()
            .find_map(|(prefix, class)| name.starts_with(prefix).then_some(class)),
        Role::Evidence => {
            if let Some(rest) = nested(name, "verification/eas-updates/") {
                return (!rest.starts_with("latest-")).then_some(Class::EasUpdates);
            }
            if nested(name, "verification/test-runs/").is_some() {
                return Some(Class::TestRuns);
            }
            nested(name, "verification/device-lab/").map(|_| Class::DeviceLab)
        }
        Role::State | Role::Other => None,
    }
}

fn pnpm_stem(name: &str) -> &str {
    name.strip_suffix(".tar.gz").or_else(|| name.strip_suffix(".json")).unwrap_or(name)
}

/// The newest pnpm store (its archive, and the sidecar that names the lockfile it was saved for): the one object a cold run cannot rebuild cheaply.
pub fn pnpm_keep(objects: &[ObjectInfo]) -> BTreeSet<String> {
    let pnpm: Vec<&ObjectInfo> = objects.iter().filter(|o| o.name.starts_with("pnpm/")).collect();
    let archives: Vec<&&ObjectInfo> = pnpm.iter().filter(|o| o.name.ends_with(".tar.gz")).collect();
    let newest = if archives.is_empty() { pnpm.iter().max_by_key(|o| o.created_ms) } else { archives.into_iter().max_by_key(|o| o.created_ms) };
    let Some(newest) = newest else { return BTreeSet::new() };
    let stem = pnpm_stem(&newest.name);
    pnpm.iter().filter(|o| pnpm_stem(&o.name) == stem).map(|o| o.name.clone()).collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub bucket: String,
    pub name: String,
    pub bytes: u64,
    pub created_ms: i64,
    pub etag: Option<String>,
    pub class: Class,
}

pub const EAS_PREFIX: &str = "verification/eas-updates/";

/// What the evidence bucket's pointers name, which age alone would not spare: the update group a `latest-<branch>.json` points at is
/// what the Android e2e job loads, however quiet the branch has been.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pins {
    pub eas_groups: BTreeSet<String>,
    /// False when a pointer could not be read or understood: no update group may then be deleted.
    pub eas_known: bool,
}

impl Pins {
    pub fn none() -> Pins {
        Pins { eas_groups: BTreeSet::new(), eas_known: true }
    }

    fn spares(&self, class: Class, name: &str) -> bool {
        class == Class::EasUpdates && (!self.eas_known || self.eas_groups.iter().any(|group| name.strip_prefix(EAS_PREFIX).and_then(|rest| rest.strip_prefix(group.as_str())).is_some_and(|rest| rest.starts_with('/'))))
    }
}

/// The pointer objects of the evidence bucket: `verification/eas-updates/latest-<branch>.json`.
pub fn is_eas_pointer(name: &str) -> bool {
    name.strip_prefix(EAS_PREFIX).is_some_and(|rest| rest.starts_with("latest-") && !rest.contains('/'))
}

/// Everything the guard may delete, in the order it deletes: by class, oldest first within one. An object younger than its class's
/// minimum age, whose creation time is unknown, or that a pin spares, is not here.
pub fn candidates(buckets: &[(String, Vec<ObjectInfo>)], now_ms: i64, pins: &Pins) -> Vec<Candidate> {
    let mut found = Vec::new();
    for (bucket, objects) in buckets {
        let role = Role::of(bucket);
        let keep = if role == Role::Cache { pnpm_keep(objects) } else { BTreeSet::new() };
        for object in objects {
            let Some(class) = class_of(role, &object.name).filter(|_| !keep.contains(&object.name)) else { continue };
            if object.created_ms <= now_ms - class.min_age_ms() && !pins.spares(class, &object.name) {
                found.push(Candidate { bucket: bucket.clone(), name: object.name.clone(), bytes: object.bytes, created_ms: object.created_ms, etag: object.etag.clone(), class });
            }
        }
    }
    found.sort_by(|a, b| (a.class, a.created_ms, &a.bucket, &a.name).cmp(&(b.class, b.created_ms, &b.bucket, &b.name)));
    found
}

/// The first candidates, in order, that free at least `need` bytes (all of them when they cannot).
pub fn choose(ordered: Vec<Candidate>, need: u64) -> Vec<Candidate> {
    let mut freed = 0u64;
    ordered
        .into_iter()
        .take_while(|c| {
            let more = freed < need;
            freed += c.bytes;
            more
        })
        .collect()
}

/// The layer an object counts under in the report: the cache's prefix, or the evidence folder under `verification/`.
pub fn layer_of(role: Role, name: &str) -> &'static str {
    const CACHE: [&str; 6] = ["nx", "pnpm", "sccache", "metro", "probe", "meta"];
    const EVIDENCE: [&str; 4] = ["test-runs", "eas-updates", "device-lab", "verify"];
    let first = |prefix: &str| name.strip_prefix(prefix).and_then(|rest| rest.split('/').next()).unwrap_or("");
    match role {
        Role::Cache => CACHE.into_iter().find(|layer| name.strip_prefix(layer).is_some_and(|rest| rest.starts_with('/'))).unwrap_or("other"),
        Role::Evidence => EVIDENCE.into_iter().find(|layer| first("verification/").starts_with(layer)).unwrap_or("other"),
        Role::State | Role::Other => "other",
    }
}
