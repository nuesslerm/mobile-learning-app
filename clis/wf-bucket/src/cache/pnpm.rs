//! `wf cache restore pnpm` and `wf cache save pnpm`: the pnpm store as one object in the build-cache bucket, so a recreated VM or
//! a fresh checkout fetches nothing it already has. Both print one JSON object and exit 0 whenever the bucket is the problem
//! (no link, unreachable, no object, a refused write): the cache is cold then, never red. Only a failing step on this machine
//! exits 1, and the workflow steps that call them do not fail the job for that either.
use super::{writes_for, Scope, READ_PAR, WRITE_PAR};
use crate::ci_metrics::{self, Gauges};
use blob_cache::domain::Stats;
use wf_kit::args::{Parsed, Spec, Positional};
use blob_cache::adapters::{OciParStore, TarGz};
use blob_cache::domain::pnpm::Layout;
use blob_cache::domain::{Budget, Kind};
use blob_cache::pnpm::{restore_precheck, save_precheck, Outcome, Reason, SaveFacts};
use blob_cache::{Cache, Writes};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

pub const SPEC: Spec = Spec { positional: Positional::Exactly(1), ..Spec::NONE };

/// Where the store is and which layout it is, from pnpm itself, so `npm_config_store_dir` and the default agree with the install.
fn locate(root: &Path) -> Result<(PathBuf, Layout), String> {
    let output = Command::new("pnpm").args(["store", "path"]).current_dir(root).stdin(Stdio::null()).output().map_err(|e| format!("pnpm did not start: {e}"))?;
    if !output.status.success() {
        return Err(format!("pnpm store path failed ({})", output.status));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let store = PathBuf::from(text.lines().rev().find(|line| !line.trim().is_empty()).ok_or("pnpm store path printed nothing")?.trim());
    let dir = store.file_name().and_then(|name| name.to_str()).unwrap_or_default();
    let layout = Layout::parse(dir).ok_or_else(|| format!("{} does not end in a store layout such as v11", store.display()))?;
    Ok((store, layout))
}

pub(super) fn sha256_of(path: &Path) -> Option<String> {
    std::fs::read(path).ok().map(|bytes| hex::encode(Sha256::digest(bytes)))
}

fn failed(error: impl std::fmt::Display) -> Outcome {
    Outcome::Nothing(Reason::Failed(error.to_string()))
}

/// The bucket for this verb: its read link, and the write link when the verb writes. A link that does not resolve is a cold run.
pub(super) fn open(write: Option<Writes>, kind: Kind) -> Result<Cache<OciParStore>, Outcome> {
    if let Some(why) = super::switched_off(Budget::committed()) {
        super::disabled::<()>(why);
        return Err(Outcome::Nothing(Reason::Cold("disabled")));
    }
    let wanted: &[&str] = if write.is_some() { &[READ_PAR, WRITE_PAR] } else { &[READ_PAR] };
    let links = secrets::load_values(wanted);
    let link = |key: &str| links.get(key).cloned().flatten();
    let missing: Vec<&str> = wanted.iter().copied().filter(|key| link(key).is_none()).collect();
    if !missing.is_empty() {
        eprintln!("wf: build cache not used, {} does not resolve (the environment, then the vault)", missing.join(" and "));
        return Err(Outcome::Nothing(Reason::Cold("no link")));
    }
    let read = link(READ_PAR).unwrap_or_default();
    let write_link = if write.is_some() { link(WRITE_PAR) } else { None };
    let writes = write.unwrap_or_else(|| Writes::ReadOnly("restore reads only".into()));
    // one pnpm entry is the whole job of this run, so its byte limit is the entry's, not an Nx run's
    let budget = Budget::committed().for_one_entry_of(kind);
    Ok(Cache::open(OciParStore::new(&read, write_link.as_deref()), budget, writes))
}

/// Prints the verb's one JSON object and picks the exit code: 1 only for a step on this machine that failed. `stats` is what the
/// run did against the store, when it got as far as opening one; the verb's event and gauges leave from here too.
pub(super) fn finish(kind: &str, verb: &str, outcome: Outcome, started: Instant, extra: Vec<(&str, Value)>, stats: Option<&Stats>) -> Option<i32> {
    let (done, bytes, reason) = match &outcome {
        Outcome::Done { bytes } => (true, *bytes, None),
        Outcome::Nothing(reason) => (false, 0, Some(reason)),
    };
    let broke = reason.is_some_and(Reason::failed);
    let failure = stats.and_then(|s| s.last_failure);
    let mut out = Map::new();
    out.insert("ok".into(), json!(!broke));
    out.insert("kind".into(), json!(kind));
    out.insert(if verb == "restore" { "restored" } else { "saved" }.into(), json!(done));
    out.insert("bytes".into(), json!(bytes));
    for (key, value) in extra {
        out.insert(key.into(), value);
    }
    out.insert("reason".into(), json!(reason.map(ToString::to_string)));
    if let Some(failure) = failure {
        out.insert("storeError".into(), json!({ "class": failure.class, "status": failure.status }));
    }
    let elapsed = started.elapsed();
    out.insert("ms".into(), json!(elapsed.as_millis() as u64));
    println!("{}", Value::Object(out));
    report(kind, verb, done, bytes, reason, stats, elapsed.as_secs_f64());
    if let Some(reason) = reason.filter(|r| r.failed()) {
        wf_kit::wf_err!("wf cache {verb}: {reason}");
    }
    Some(i32::from(broke))
}

fn report(kind: &str, verb: &str, done: bool, bytes: u64, reason: Option<&Reason>, stats: Option<&Stats>, seconds: f64) {
    let failure = stats.and_then(|s| s.last_failure);
    let went_cold = matches!(reason, Some(Reason::Cold(_) | Reason::StorageFull)) || reason.is_some_and(Reason::failed) || failure.is_some();
    let mut fields = vec![
        ("kind", kind.to_string()),
        ("verb", verb.to_string()),
        ("done", done.to_string()),
        ("bytes", bytes.to_string()),
        ("reason", reason.map_or_else(String::new, ToString::to_string)),
        ("seconds", format!("{seconds:.1}")),
    ];
    if let Some(failure) = failure {
        fields.push(("error_class", failure.class.to_string()));
        fields.push(("error_status", failure.status.map_or_else(String::new, |s| s.to_string())));
    }
    ci_metrics::event(if went_cold { "warn" } else { "info" }, &format!("cache.{verb}"), &fields);
    if let Some(stats) = stats.filter(|s| s.dropped_puts > 0) {
        super::store_events(kind, stats);
    }
    let mut gauges = Gauges::new();
    match stats {
        Some(stats) => gauges.store(kind, stats),
        None => gauges.set("ci.cache.requests", kind, "not-asked", 1.0),
    };
    gauges.set("ci.cache.step_seconds", kind, verb, seconds).send();
}

fn only_pnpm(verb: &str, p: &Parsed) -> Option<i32> {
    let kind = p.positional.first()?;
    (kind != "pnpm").then(|| {
        wf_kit::wf_err!("wf cache {verb}: kind is pnpm, got {kind:?}");
        2
    })
}

pub fn restore(p: &Parsed) -> Option<i32> {
    if let Some(code) = only_pnpm("restore", p) {
        return Some(code);
    }
    let started = Instant::now();
    let root = wf_kit::root::repo_root();
    let (store, layout) = match locate(&root) {
        Ok(found) => found,
        Err(error) => return finish("pnpm", "restore", failed(error), started, vec![], None),
    };
    // a store that has content needs neither a link nor a request
    if let Some(reason) = restore_precheck(&store) {
        return finish("pnpm", "restore", Outcome::Nothing(reason), started, vec![], None);
    }
    let (outcome, stats) = match open(None, Kind::Pnpm) {
        Ok(cache) => (blob_cache::pnpm::restore(&cache, &TarGz, &store, &layout), Some(cache.stats())),
        Err(cold) => (cold, None),
    };
    finish("pnpm", "restore", outcome, started, vec![], stats.as_ref())
}

/// One job of a run saves the store: every job installs from the same lockfile, so packing it again in each costs minutes and the same object.
fn saved_by_another_job() -> bool {
    matches!((std::env::var("WF_PNPM_SAVE_JOB"), std::env::var("GITHUB_JOB")), (Ok(only), Ok(job)) if crate::job_name::stage(&only) != crate::job_name::stage(&job))
}

pub fn save(p: &Parsed) -> Option<i32> {
    if let Some(code) = only_pnpm("save", p) {
        return Some(code);
    }
    let started = Instant::now();
    if saved_by_another_job() {
        return finish("pnpm", "save", Outcome::Nothing(Reason::Cold("another job saves")), started, vec![], None);
    }
    let root = wf_kit::root::repo_root();
    let (store, layout) = match locate(&root) {
        Ok(found) => found,
        Err(error) => return finish("pnpm", "save", failed(error), started, vec![], None),
    };
    let Some(lockfile) = sha256_of(&root.join("pnpm-lock.yaml")) else {
        return finish("pnpm", "save", failed("pnpm-lock.yaml is not readable"), started, vec![], None);
    };
    // pnpm records the lockfile it installed from as the last act of a successful install; a failed one leaves none or an old one
    let installed = sha256_of(&root.join("node_modules/.pnpm/lock.yaml"));
    let writes = writes_for(&root, Scope::Tree, "pnpm save");
    let facts = SaveFacts { store: &store, layout: &layout, lockfile_sha256: &lockfile, installed_lockfile_sha256: installed.as_deref() };
    let extra = vec![("lockfileSha256", json!(lockfile))];
    if let Some(reason) = save_precheck(&writes, &facts) {
        return finish("pnpm", "save", Outcome::Nothing(reason), started, extra, None);
    }
    let (outcome, stats) = match open(Some(writes), Kind::Pnpm) {
        Ok(cache) => (blob_cache::pnpm::save(&cache, &TarGz, &facts), Some(cache.stats())),
        Err(cold) => (cold, None),
    };
    finish("pnpm", "save", outcome, started, extra, stats.as_ref())
}
