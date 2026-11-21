//! The operator's verbs of `wf cache`: where the bucket stands against its caps, making room, and starting a new empty generation.
//! `status` and `gc` go through the `oci` CLI and the admin key in `~/.oci` (the sandbox has them); `purge` needs only the two
//! PAR links. Every verb prints one JSON object on stdout and never a link.
use super::open_stores;
use wf_kit::args::{Parsed, Positional, Spec};
use blob_cache::adapters::oci_cli::BUCKET;
use blob_cache::adapters::OciCli;
use blob_cache::stores::ConfigError;
use blob_cache::admin::{clear, standing, Clear, Standing};
use blob_cache::domain::{Budget, Generation, Kind};
use blob_cache::ports::BlobAdmin;
use blob_cache::{Cache, Writes};
use serde_json::{json, Value};

pub const STATUS: Spec = Spec { positional: Positional::None, ..Spec::NONE };
pub const GC: Spec = Spec { values: &["evict-to", "prefix"], flags: &["status"], positional: Positional::None, ..Spec::NONE };
pub const PURGE: Spec = Spec { positional: Positional::Exactly(1), ..Spec::NONE };

fn print(value: &Value) {
    println!("{value}");
}

/// The line on stderr and the same on stdout, so a caller that reads only the JSON still gets the reason.
fn fail(verb: &str, error: &str) -> Option<i32> {
    wf_kit::wf_err!("wf cache {verb}: {error}");
    print(&json!({ "ok": false, "error": error }));
    Some(1)
}

fn usage(verb: &str, error: &str) -> Option<i32> {
    wf_kit::wf_err!("wf cache {verb}: {error}");
    Some(2)
}

pub fn status(_: &Parsed) -> Option<i32> {
    let budget = Budget::committed();
    match standing(&OciCli::new(BUCKET), budget) {
        Ok(Standing { usage, over }) => {
            print(&json!({
                "ok": over.is_empty(),
                "storedBytes": usage.stored_bytes,
                "storedBytesCap": budget.stored_bytes,
                "objects": usage.objects,
                "requestsThisMonth": usage.requests_this_month,
                "requestsPerMonthCap": budget.requests_per_month,
                "layers": layers(),
                "over": over,
            }));
            Some(i32::from(!over.is_empty()))
        }
        Err(error) => fail("status", &error.to_string()),
    }
}

/// What each cache layer (kind) holds: bytes and objects by prefix, one listing. `null` when the listing is not available.
fn layers() -> Value {
    let Ok(all) = OciCli::new(BUCKET).list("") else { return Value::Null };
    let mut rows = serde_json::Map::new();
    for kind in Kind::ALL {
        let prefix = format!("{}/", kind.prefix());
        let mine: Vec<_> = all.iter().filter(|object| object.name.starts_with(&prefix)).collect();
        rows.insert(kind.prefix().to_string(), json!({ "bytes": mine.iter().map(|o| o.bytes).sum::<u64>(), "objects": mine.len(), "ttlDays": kind.ttl_days() }));
    }
    Value::Object(rows)
}

pub fn gc(p: &Parsed) -> Option<i32> {
    let (report, evict, prefix) = (p.flag("status"), p.val("evict-to"), p.val("prefix"));
    if [report, evict.is_some(), prefix.is_some()].into_iter().filter(|given| *given).count() != 1 {
        return usage("gc", "give exactly one of --status, --evict-to <bytes> or --prefix <p>");
    }
    let admin = OciCli::new(BUCKET);
    if report {
        return match admin.list("") {
            Ok(all) => {
                print(&json!({ "ok": true, "storedBytes": all.iter().map(|object| object.bytes).sum::<u64>(), "objects": all.len() }));
                Some(0)
            }
            Err(error) => fail("gc", &error.to_string()),
        };
    }
    let how = match (evict, prefix) {
        (Some(bytes), _) => match bytes.parse::<u64>() {
            Ok(limit) => Clear::EvictTo(limit),
            Err(_) => return usage("gc", &format!("--evict-to takes a number of bytes, got {bytes:?}")),
        },
        (None, Some("")) => return usage("gc", "--prefix needs a non-empty prefix: an empty one names every object"),
        (None, Some(prefix)) => Clear::Prefix(prefix),
        (None, None) => return None,
    };
    let limit = if let Clear::EvictTo(limit) = how { Some(limit) } else { None };
    match clear(&admin, how) {
        Ok(cleared) => {
            let reached = limit.is_none_or(|limit| cleared.stored_bytes <= limit);
            print(&json!({ "ok": reached, "deleted": cleared.deleted, "freedBytes": cleared.freed_bytes, "storedBytes": cleared.stored_bytes }));
            Some(i32::from(!reached))
        }
        Err(error) => fail("gc", &error.to_string()),
    }
}

/// Moves every reader and writer to the next generation. The generation is one number for every kind, so the kind is checked
/// but a purge starts all of them over; the old entries are left to the bucket's lifecycle. It is the way out of a cached wrong
/// result, so unlike a run it does not ask for a clean tree on `origin/dev`.
pub fn purge(p: &Parsed) -> Option<i32> {
    let name = p.positional.first()?;
    let Some(kind) = Kind::from_prefix(name).filter(|kind| matches!(kind, Kind::Nx | Kind::Pnpm)) else {
        return usage("purge", &format!("kind is nx or pnpm, got {name:?}"));
    };
    let stores = match open_stores(true) {
        Ok(Some(stores)) => stores,
        Ok(None) => return fail("purge", "no build cache store is declared (every entry of the store list is disabled)"),
        Err(error @ ConfigError::Missing(_)) => return fail("purge", &format!("{error} (the environment, then the vault)")),
        Err(error) => return fail("purge", &error.to_string()),
    };
    let mut cache = Cache::with_generation(stores, Budget::committed().clone(), Writes::Allowed, Generation::default());
    match cache.purge() {
        Ok(next) => {
            print(&json!({ "ok": true, "kind": kind.prefix(), "generation": next.0 }));
            Some(0)
        }
        Err(error) => fail("purge", &error.to_string()),
    }
}
