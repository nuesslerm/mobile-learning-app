//! `wf cache restore metro` and `wf cache save metro`: the app's Metro transform cache and `tsc` build info
//! (`frontend/mla-tamagui/node_modules/.cache/app-build`) as one restore-latest object in the build-cache bucket, so a clean runner starts a
//! web export, an EAS Update or a typecheck warm. Both print one JSON object and exit 0 whenever the bucket is the problem (no link,
//! unreachable, no object, a refused write): the cache is cold then, never red. Only a failing step on this machine exits 1. A restore
//! into a directory that has content does nothing, which is the usual case on a runner whose checkout persists.
use super::pnpm::{finish, open};
use super::{writes_for, Scope};
use blob_cache::adapters::TarGz;
use blob_cache::app_build::{self, SaveFacts};
use blob_cache::domain::Kind;
use blob_cache::pnpm::Outcome;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Instant;
use wf_kit::args::Parsed;

/// Where Metro's `FileStore` and `tsc`'s `tsBuildInfoFile` both write (`metro.config.js`, `tsconfig.json`).
pub fn dir(root: &Path) -> PathBuf {
    root.join("frontend/mla-tamagui/node_modules/.cache/app-build")
}

/// A digest of every file's relative name and size under `dir`: the cheap answer to "did the build add anything".
pub fn fingerprint(dir: &Path) -> String {
    fn walk(base: &Path, dir: &Path, into: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.metadata() {
                Ok(meta) if meta.is_dir() => walk(base, &path, into),
                Ok(meta) => into.push(format!("{}:{}", path.strip_prefix(base).unwrap_or(&path).display(), meta.len())),
                Err(_) => {}
            }
        }
    }
    let mut files = Vec::new();
    walk(dir, dir, &mut files);
    files.sort();
    hex::encode(Sha256::digest(files.join("\n").as_bytes()))
}

pub fn restore(_: &Parsed) -> Option<i32> {
    let started = Instant::now();
    let target = dir(&wf_kit::root::repo_root());
    // a directory that has content needs neither a link nor a request
    if let Some(reason) = app_build::restore_precheck(&target) {
        return finish("metro", "restore", Outcome::Nothing(reason), started, vec![], None);
    }
    let (outcome, stats) = match open(None, Kind::Metro) {
        Ok(cache) => (app_build::restore(&cache, &TarGz, &target), Some(cache.stats())),
        Err(cold) => (cold, None),
    };
    finish("metro", "restore", outcome, started, vec![], stats.as_ref())
}

pub fn save(_: &Parsed) -> Option<i32> {
    let started = Instant::now();
    let root = wf_kit::root::repo_root();
    let target = dir(&root);
    let writes = writes_for(&root, Scope::Tree, "metro save");
    let digest = fingerprint(&target);
    let facts = SaveFacts { dir: &target, fingerprint: &digest };
    let extra = vec![("fingerprint", json!(digest))];
    if let Some(reason) = app_build::save_precheck(&writes, &facts) {
        return finish("metro", "save", Outcome::Nothing(reason), started, extra, None);
    }
    let (outcome, stats) = match open(Some(writes), Kind::Metro) {
        Ok(cache) => (app_build::save(&cache, &TarGz, &facts), Some(cache.stats())),
        Err(cold) => (cold, None),
    };
    finish("metro", "save", outcome, started, extra, stats.as_ref())
}

