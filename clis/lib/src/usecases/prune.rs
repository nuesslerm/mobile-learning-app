use crate::domain::index;
use crate::domain::libraries::Catalog;
use crate::domain::lockfile::pins;
use crate::domain::prune::{plan, stored, DAY_MS};
use crate::ports::{Checkout, Failure};
use blob_cache::domain::Key;
use blob_cache::ports::{BlobAdmin, BlobStore};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

pub struct Options {
    pub dry_run: bool,
    pub grace_days: i64,
    pub now_ms: i64,
}

/// Deletes (or, with `dry_run`, lists) the stored versions no lockfile at `dev` pins that are not the latest of their crate and are
/// older than the grace. Only registries whose publisher can delete are pruned; the rest are listed in `skipped` with the reason.
pub fn prune(catalog: &Catalog, dev: &dyn Checkout, store: &dyn BlobStore, admin: &dyn BlobAdmin, deletable: &dyn Fn(&str) -> bool, options: &Options) -> Result<Value, Failure> {
    let mut pinned = HashSet::new();
    for (path, text) in dev.files("Cargo.lock")? {
        for pin in pins(&text).map_err(|e| Failure::new(format!("{path}: {e}")))? {
            pinned.insert((pin.name, pin.version));
        }
    }
    let skipped: Vec<Value> = catalog
        .libraries
        .iter()
        .filter(|l| !catalog.registry(&l.registry).is_some_and(|r| r.publish.as_deref().is_some_and(deletable)))
        .map(|l| json!({ "library": l.name, "registry": l.registry, "why": "this registry's adapter cannot delete versions; its releases are kept" }))
        .collect();
    let listed = admin.list("crates/").map_err(|e| Failure::new(format!("listing crates/: {e}")).next("prune needs the oci CLI and ~/.oci (the operator's key)"))?;
    let all: Vec<_> = listed.iter().filter_map(|o| stored(&o.name, o.bytes, o.created_ms)).collect();
    let plan = plan(&all, &pinned, options.now_ms, options.grace_days);
    if !options.dry_run && !plan.delete.is_empty() {
        let mut doomed: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for one in &plan.delete {
            doomed.entry(&one.name).or_default().push(one.version.clone());
        }
        for (name, versions) in doomed {
            let key = Key::at(&index::key(&index::path(name))).map_err(|e| Failure::new(e.to_string()))?;
            if let Some(text) = store.get(&key).map_err(|e| Failure::new(format!("{name} index: {e}")))? {
                let yanked = index::yank(&String::from_utf8_lossy(&text), &versions);
                store.put(&key, yanked.as_bytes(), false).map_err(|e| Failure::new(format!("{name} index: {e}")).next("nothing was deleted; rerun"))?;
            }
        }
        let names: Vec<String> = plan.delete.iter().map(|o| o.object.clone()).collect();
        admin.delete(&names).map_err(|e| Failure::new(format!("delete: {e}")).next("the versions are yanked in the index; rerun to finish deleting"))?;
    }
    let age = |created: i64| (options.now_ms - created) / DAY_MS;
    Ok(json!({
        "ok": true,
        "dryRun": options.dry_run,
        "graceDays": options.grace_days,
        (if options.dry_run { "wouldDelete" } else { "deleted" }): plan.delete.iter().map(|o| json!({ "crate": o.name, "version": o.version, "bytes": o.bytes, "ageDays": age(o.created_ms) })).collect::<Vec<_>>(),
        "freedBytes": plan.delete.iter().map(|o| o.bytes).sum::<u64>(),
        "kept": plan.kept.iter().map(|k| json!({ "crate": k.name, "version": k.version, "reason": k.reason })).collect::<Vec<_>>(),
        "skipped": skipped,
    }))
}
