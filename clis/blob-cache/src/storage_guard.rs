//! The storage guard: measure every bucket of the tenancy against the Object Storage cap and, above the high-water mark, delete the
//! cheapest data until usage is under the low-water mark. It never fails: whatever goes wrong is a report that says what was skipped.
use crate::ports::{ObjectInfo, StoreError};
use crate::storage_policy::{candidates, choose, is_eas_pointer, layer_of, Candidate, Pins, Role, HIGH_WATER_PERCENT, LOW_WATER_PERCENT};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

const DELETE_CHUNK: usize = 100;
const WOULD_LISTED: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BucketSize {
    pub name: String,
    pub bytes: u64,
}

/// One object to delete: its name, and the version tag it was listed with, when the store gave one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Doomed {
    pub name: String,
    pub etag: Option<String>,
}

/// What a delete call did, object by object, so a failure half way still says what is gone.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Deleted {
    /// Deleted by this call.
    pub done: Vec<String>,
    /// Already gone when the call came.
    pub missing: usize,
    /// Left alone because the object was rewritten after it was listed.
    pub changed: usize,
    pub error: Option<StoreError>,
}

/// The tenancy's object store as the guard needs it: every bucket's size, and listing, reading and deleting inside one. Every call
/// that can take long stops at `deadline`.
pub trait Tenancy {
    fn buckets(&self) -> Result<Vec<BucketSize>, StoreError>;
    fn list(&self, bucket: &str, prefix: &str, deadline: Instant) -> Result<Vec<ObjectInfo>, StoreError>;
    /// `Ok(None)` is an object that is not there.
    fn read(&self, bucket: &str, name: &str, deadline: Instant) -> Result<Option<String>, StoreError>;
    fn delete(&self, bucket: &str, items: &[Doomed], deadline: Instant) -> Deleted;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Under the high-water mark: nothing to do.
    None,
    /// Over it, and the deletions brought usage under the low-water mark.
    Evicted,
    /// Over it, and everything that may go is gone or the time ran out, usage is still above the low-water mark.
    Short,
    /// A dry run over the high-water mark: what would be deleted, nothing deleted.
    WouldEvict,
    /// The tenancy could not be measured.
    Skipped,
}

impl Action {
    fn label(self) -> &'static str {
        match self {
            Action::None => "none",
            Action::Evicted => "evicted",
            Action::Short => "short",
            Action::WouldEvict => "would-evict",
            Action::Skipped => "skipped",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub action: Action,
    pub used_bytes: u64,
    pub cap_bytes: u64,
    pub deleted_objects: usize,
    pub freed_bytes: u64,
    pub changed_since_listing: usize,
    pub buckets: BTreeMap<String, u64>,
    pub layers: Value,
    pub evicted: BTreeMap<&'static str, (usize, u64)>,
    pub would_delete: Vec<Candidate>,
    /// A fixed code for why the run did less than it should: `unmeasurable`, `config-missing`, `listing-failed`, `pointer-unreadable`,
    /// `delete-failed`, `time-limit`, `relay-unreachable`, `relay-timeout`, `relay-no-result`.
    pub skip_code: Option<&'static str>,
    pub skip: Option<String>,
}

impl Report {
    pub fn skipped(cap_bytes: u64, code: &'static str, why: String) -> Report {
        Report {
            action: Action::Skipped,
            used_bytes: 0,
            cap_bytes,
            deleted_objects: 0,
            freed_bytes: 0,
            changed_since_listing: 0,
            buckets: BTreeMap::new(),
            layers: Value::Null,
            evicted: BTreeMap::new(),
            would_delete: Vec::new(),
            skip_code: Some(code),
            skip: Some(why),
        }
    }

    pub fn skip_reason(&self) -> Option<&str> {
        self.skip.as_deref()
    }

    pub fn skip_code(&self) -> Option<&'static str> {
        self.skip_code
    }

    /// The one line the run report and the nightly review read. The reason is cleaned of key material first: it ends up in logs and annotations.
    pub fn to_json(&self) -> Value {
        let evicted: Map<String, Value> = self.evicted.iter().map(|(class, (objects, bytes))| (class.to_string(), json!({ "objects": objects, "bytes": bytes }))).collect();
        let mut line = json!({
            "event": "storage-guard",
            "ok": self.action != Action::Skipped,
            "action": self.action.label(),
            "usedBytes": self.used_bytes,
            "usedBytesAfter": self.used_bytes.saturating_sub(self.freed_bytes),
            "capBytes": self.cap_bytes,
            "highWaterBytes": high_water(self.cap_bytes),
            "lowWaterBytes": low_water(self.cap_bytes),
            "deletedObjects": self.deleted_objects,
            "freedBytes": self.freed_bytes,
            "changedSinceListing": self.changed_since_listing,
            "buckets": self.buckets,
            "layers": self.layers,
            "evicted": evicted,
        });
        if self.action == Action::WouldEvict {
            let bytes: u64 = self.would_delete.iter().map(|c| c.bytes).sum();
            line["wouldDelete"] = json!({
                "objects": self.would_delete.len(),
                "bytes": bytes,
                "listed": self.would_delete.iter().take(WOULD_LISTED).map(|c| json!({ "bucket": c.bucket, "name": c.name, "bytes": c.bytes, "class": c.class.label() })).collect::<Vec<_>>(),
            });
        }
        if let Some(code) = self.skip_code {
            line["skipCode"] = json!(code);
        }
        if let Some(why) = &self.skip {
            line["skip"] = json!(sanitize(why).chars().take(600).collect::<String>());
        }
        line
    }
}

pub fn high_water(cap: u64) -> u64 {
    cap / 100 * HIGH_WATER_PERCENT
}

pub fn low_water(cap: u64) -> u64 {
    cap / 100 * LOW_WATER_PERCENT
}

/// Text from a tool's stderr or the vault, safe to print: PEM blocks and a passphrase's value removed, whitespace collapsed.
pub fn sanitize(text: &str) -> String {
    let mut clean = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("-----BEGIN") {
        clean.push_str(&rest[..start]);
        clean.push_str("[key redacted]");
        rest = match rest[start..].find("-----END") {
            Some(end) => {
                let from = start + end + "-----END".len();
                rest[from..].find("-----").map_or("", |close| &rest[from + close + 5..])
            }
            None => "",
        };
    }
    clean.push_str(rest);
    const KEYS: [&str; 3] = ["pass_phrase", "passphrase", "key_content"];
    let quotes = ['"', '\'', ','];
    let mut kept: Vec<String> = Vec::new();
    // 0: nothing pending, 1: a key was just named, 2: its value is the next word
    let mut pending = 0;
    for word in clean.split_whitespace() {
        let separator = word == "=" || word == ":";
        if pending == 2 || (pending == 1 && separator) {
            pending = if separator { 2 } else { 0 };
            kept.push(if separator { word.to_string() } else { "[redacted]".to_string() });
            continue;
        }
        pending = 0;
        let lower = word.to_ascii_lowercase();
        let Some(at) = KEYS.iter().find_map(|k| lower.find(k).map(|i| i + k.len())) else {
            kept.push(word.to_string());
            continue;
        };
        match word[at..].find(['=', ':']) {
            Some(sep) if !word[at + sep + 1..].trim_matches(quotes).is_empty() => kept.push(format!("{}[redacted]", &word[..at + sep + 1])),
            Some(_) => {
                kept.push(word.to_string());
                pending = 2;
            }
            None => {
                kept.push(word.to_string());
                pending = 1;
            }
        }
    }
    kept.join(" ")
}

/// The last `n` characters of `text`.
pub fn tail(text: &str, n: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    chars[chars.len().saturating_sub(n)..].iter().collect()
}

/// Sizes by layer, for the buckets the guard lists.
fn layers_of(listed: &[(String, Vec<ObjectInfo>)]) -> Value {
    let mut rows = Map::new();
    for (bucket, objects) in listed {
        let role = Role::of(bucket);
        let mut bytes: BTreeMap<&str, u64> = BTreeMap::new();
        for object in objects {
            *bytes.entry(layer_of(role, &object.name)).or_default() += object.bytes;
        }
        rows.insert(bucket.clone(), json!(bytes));
    }
    Value::Object(rows)
}

/// The update groups the evidence buckets' `latest-<branch>.json` pointers name. A pointer that cannot be read leaves `eas_known` false.
fn pins_of(tenancy: &impl Tenancy, listed: &[(String, Vec<ObjectInfo>)], deadline: Instant) -> Pins {
    let mut pins = Pins::none();
    for (bucket, objects) in listed.iter().filter(|(bucket, _)| Role::of(bucket) == Role::Evidence) {
        for pointer in objects.iter().filter(|o| is_eas_pointer(&o.name)) {
            let group = match tenancy.read(bucket, &pointer.name, deadline) {
                Ok(Some(text)) => serde_json::from_str::<Value>(&text).ok().and_then(|v| v["group"].as_str().map(str::to_string)).filter(|g| !g.is_empty() && !g.contains('/')),
                Ok(None) => continue,
                Err(_) => None,
            };
            match group {
                Some(group) => {
                    pins.eas_groups.insert(group);
                }
                None => pins.eas_known = false,
            }
        }
    }
    pins
}

/// Measures, and when usage is above the high-water mark evicts until it is under the low-water mark or `deadline` passes.
/// `now_ms` is the clock the age rules read. A dry run decides the same and deletes nothing. Usage is what the listings add up to
/// for every bucket the guard can list: a bucket's own size figure lags behind a delete by minutes to hours.
pub fn guard(tenancy: &impl Tenancy, cap_bytes: u64, now_ms: i64, deadline: Instant, dry_run: bool) -> Report {
    let sizes = match tenancy.buckets() {
        Ok(sizes) => sizes,
        Err(error) => return Report::skipped(cap_bytes, "unmeasurable", format!("the tenancy's buckets could not be measured: {error}")),
    };
    let mut listed = Vec::new();
    let mut measured: BTreeMap<String, u64> = sizes.iter().map(|b| (b.name.clone(), b.bytes)).collect();
    let mut skip: Option<(&'static str, String)> = None;
    for bucket in sizes.iter().filter(|b| Role::of(&b.name) != Role::Other) {
        match tenancy.list(&bucket.name, "", deadline) {
            Ok(objects) => {
                measured.insert(bucket.name.clone(), objects.iter().map(|o| o.bytes).sum());
                listed.push((bucket.name.clone(), objects));
            }
            Err(error) => skip = skip.or(Some(("listing-failed", format!("{} could not be listed: {error}", bucket.name)))),
        }
    }
    let used_bytes: u64 = measured.values().sum();
    let mut report = Report {
        action: Action::None,
        used_bytes,
        cap_bytes,
        deleted_objects: 0,
        freed_bytes: 0,
        changed_since_listing: 0,
        buckets: measured,
        layers: layers_of(&listed),
        evicted: BTreeMap::new(),
        would_delete: Vec::new(),
        skip_code: skip.as_ref().map(|(code, _)| *code),
        skip: skip.map(|(_, why)| why),
    };
    if used_bytes <= high_water(cap_bytes) {
        return report;
    }
    if Instant::now() >= deadline {
        report.action = Action::Skipped;
        report.skip_code = Some("time-limit");
        report.skip = Some("the time limit passed before the listings were done".into());
        return report;
    }
    let pins = pins_of(tenancy, &listed, deadline);
    if !pins.eas_known && report.skip.is_none() {
        report.skip_code = Some("pointer-unreadable");
        report.skip = Some("an update pointer could not be read, so no update group was a candidate".into());
    }
    let need = used_bytes.saturating_sub(low_water(cap_bytes));
    let chosen = choose(candidates(&listed, now_ms, &pins), need);
    if dry_run {
        report.freed_bytes = chosen.iter().map(|c| c.bytes).sum();
        for c in &chosen {
            let row = report.evicted.entry(c.class.label()).or_default();
            *row = (row.0 + 1, row.1 + c.bytes);
        }
        report.would_delete = chosen;
        report.action = Action::WouldEvict;
        return report;
    }
    let by_name: HashMap<(&str, &str), &Candidate> = chosen.iter().map(|c| ((c.bucket.as_str(), c.name.as_str()), c)).collect();
    let mut batches: Vec<&[Candidate]> = Vec::new();
    let mut start = 0;
    for end in 1..=chosen.len() {
        if end == chosen.len() || chosen[end].bucket != chosen[start].bucket || end - start == DELETE_CHUNK {
            batches.push(&chosen[start..end]);
            start = end;
        }
    }
    for batch in batches {
        if Instant::now() >= deadline {
            report.skip_code = Some("time-limit");
            report.skip = Some("the time limit passed before every chosen object was deleted".into());
            break;
        }
        let bucket = batch[0].bucket.as_str();
        let items: Vec<Doomed> = batch.iter().map(|c| Doomed { name: c.name.clone(), etag: c.etag.clone() }).collect();
        let outcome = tenancy.delete(bucket, &items, deadline);
        report.changed_since_listing += outcome.changed;
        for name in &outcome.done {
            let Some(c) = by_name.get(&(bucket, name.as_str())) else { continue };
            report.deleted_objects += 1;
            report.freed_bytes += c.bytes;
            let row = report.evicted.entry(c.class.label()).or_default();
            *row = (row.0 + 1, row.1 + c.bytes);
        }
        if let Some(error) = outcome.error {
            report.skip_code = Some("delete-failed");
            report.skip = Some(format!("a delete in {bucket} failed after {} of {} objects: {error}", outcome.done.len(), items.len()));
            break;
        }
        if outcome.done.len() + outcome.missing + outcome.changed < items.len() {
            report.skip_code = Some("time-limit");
            report.skip = Some(format!("the time limit passed with {} of {} objects of {bucket} deleted", outcome.done.len(), items.len()));
            break;
        }
    }
    report.action = if report.used_bytes.saturating_sub(report.freed_bytes) <= low_water(cap_bytes) { Action::Evicted } else { Action::Short };
    report
}
