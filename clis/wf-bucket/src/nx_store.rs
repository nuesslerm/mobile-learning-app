//! What Nx's local cache held right after the CI restore: the entries of `~/.nx/<id>/cache` and the rows of the `cache_outputs` table in
//! `~/.nx/<id>/databases/*.db`, which is what Nx answers a hit from. `wf cache event nx restore` takes the snapshot and leaves it beside the restore
//! outcome in a file (`WF_CACHE_RESTORE_FILE`); a task that ran is then told apart as "the hash was there and Nx ran it anyway" or "the hash was not there".
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const FILE_ENV: &str = "WF_CACHE_RESTORE_FILE";
pub const STAGE_ENV: &str = "WF_CACHE_STAGE";
pub const KEY_ENV: &str = "WF_CACHE_RESTORED_KEY";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    /// `ok`, `miss`, `timeout`, `error` or `skipped`, with the matched key and the reason, as the restore recorded them.
    pub outcome: String,
    pub matched_key: Option<String>,
    pub reason: Option<String>,
    pub entries: BTreeSet<String>,
    /// The hashes of `cache_outputs`; `None` when no database, no such table or no `hash` column was found (`db_note` says which).
    pub db_hashes: Option<BTreeSet<String>>,
    pub db_rows: Option<i64>,
    /// The newest `created_at` of `cache_outputs` as text (a re-run over an existing hash replaces its row and moves this), `None` without such a column.
    pub db_created: Option<String>,
    pub wal_bytes: u64,
    pub db_note: String,
    pub bytes: u64,
}

fn dirs(home: &Path, leaf: &str) -> Vec<PathBuf> {
    let Ok(root) = std::fs::read_dir(home.join(".nx")) else { return Vec::new() };
    let mut out: Vec<PathBuf> = root.filter_map(Result::ok).map(|e| e.path().join(leaf)).filter(|p| p.is_dir()).collect();
    out.sort();
    out
}

fn size_of(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
    if meta.is_dir() {
        std::fs::read_dir(path).into_iter().flatten().filter_map(Result::ok).map(|e| size_of(&e.path())).sum()
    } else {
        meta.len()
    }
}

/// The bytes under `paths`, files and directories alike.
pub fn bytes_of(paths: &[PathBuf]) -> u64 {
    paths.iter().map(|p| size_of(p)).sum()
}

/// The hashes of one Nx database, `None` with the reason when it has no table of them.
fn read_db(path: &Path) -> Result<(BTreeSet<String>, i64, Option<String>), String> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| format!("{}: {e}", path.display()))?;
    let _ = conn.busy_timeout(std::time::Duration::from_secs(2));
    let columns: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('cache_outputs')")
        .and_then(|mut s| s.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<_, _>>())
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if columns.is_empty() {
        return Err(format!("{} has no cache_outputs table", path.display()));
    }
    let Some(hash) = columns.iter().find(|c| c.eq_ignore_ascii_case("hash")) else {
        return Err(format!("{}: cache_outputs has the columns {} and no hash", path.display(), columns.join(",")));
    };
    let mut stmt = conn.prepare(&format!("SELECT \"{hash}\" FROM cache_outputs")).map_err(|e| format!("{}: {e}", path.display()))?;
    let hashes: BTreeSet<String> = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?.filter_map(Result::ok).collect();
    let rows = conn.query_row("SELECT COUNT(*) FROM cache_outputs", [], |r| r.get::<_, i64>(0)).map_err(|e| e.to_string())?;
    let created = columns.iter().find(|c| c.eq_ignore_ascii_case("created_at")).and_then(|c| {
        conn.query_row(&format!("SELECT MAX(CAST(\"{c}\" AS TEXT)) FROM cache_outputs"), [], |r| r.get::<_, Option<String>>(0)).ok().flatten()
    });
    Ok((hashes, rows, created))
}

/// One database's WAL flush: `Ok` with the frames moved into the database file, `Err` with why it could not be flushed (a locked database; a database not in WAL mode is not an error).
pub struct Flush {
    pub db: String,
    pub result: Result<i64, String>,
}

/// Moves every row of the Nx databases under `home` out of the `-wal` file into the database file (`PRAGMA wal_checkpoint(TRUNCATE)`), so an archive of the
/// `.db` file alone holds what Nx wrote. Never fails: what it could not flush is in the result.
pub fn checkpoint(home: &Path) -> Vec<Flush> {
    let mut out = Vec::new();
    for dir in dirs(home, "databases") {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).into_iter().flatten().filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "db")).collect();
        files.sort();
        for file in files {
            let result = Connection::open_with_flags(&file, OpenFlags::SQLITE_OPEN_READ_WRITE)
                .and_then(|conn| {
                    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
                    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(2)?)))
                })
                .map_err(|e| e.to_string())
                .and_then(|(busy, moved)| if busy == 0 { Ok(moved.max(0)) } else { Err("busy: another connection held the WAL".to_string()) });
            out.push(Flush { db: file.display().to_string(), result });
        }
    }
    out
}

/// What is in Nx's local cache under `home` now.
pub fn take(home: &Path) -> Snapshot {
    let cache = dirs(home, "cache");
    let mut snapshot = Snapshot::default();
    for dir in &cache {
        snapshot.entries.extend(std::fs::read_dir(dir).into_iter().flatten().filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()));
    }
    let databases = dirs(home, "databases");
    snapshot.bytes = bytes_of(&cache) + bytes_of(&databases);
    let mut notes = Vec::new();
    let (mut hashes, mut rows, mut any, mut created): (BTreeSet<String>, i64, bool, Option<String>) = (BTreeSet::new(), 0, false, None);
    for dir in &databases {
        for file in std::fs::read_dir(dir).into_iter().flatten().filter_map(Result::ok).map(|e| e.path()) {
            let name = file.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
            if name.ends_with("-wal") {
                snapshot.wal_bytes += std::fs::metadata(&file).map_or(0, |m| m.len());
            } else if name.ends_with(".db") {
                match read_db(&file) {
                    Ok((found, n, newest)) => {
                        hashes.extend(found);
                        rows += n;
                        created = created.max(newest);
                        any = true;
                    }
                    Err(note) => notes.push(note),
                }
            }
        }
    }
    if any {
        snapshot.db_hashes = Some(hashes);
        snapshot.db_rows = Some(rows);
        snapshot.db_created = created;
    } else if notes.is_empty() {
        notes.push("no Nx database under ~/.nx/*/databases".to_string());
    }
    snapshot.db_note = notes.join("; ");
    snapshot
}

impl Snapshot {
    /// Whether the cache directory held `hash` (`None` when the snapshot is empty because the restore failed to put anything there to check against).
    pub fn has_dir(&self, hash: &str) -> Option<bool> {
        Some(self.entries.contains(hash))
    }

    /// Whether `cache_outputs` held a row for `hash`; `None` when the table could not be read.
    pub fn has_db(&self, hash: &str) -> Option<bool> {
        self.db_hashes.as_ref().map(|h| h.contains(hash))
    }

    pub fn to_json(&self) -> Value {
        json!({
            "outcome": self.outcome,
            "matchedKey": self.matched_key,
            "reason": self.reason,
            "entries": self.entries,
            "dbHashes": self.db_hashes,
            "dbRows": self.db_rows,
            "dbCreated": self.db_created,
            "walBytes": self.wal_bytes,
            "dbNote": self.db_note,
            "bytes": self.bytes,
        })
    }

    pub fn from_json(v: &Value) -> Snapshot {
        let set = |key: &str| v[key].as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect::<BTreeSet<String>>());
        Snapshot {
            outcome: v["outcome"].as_str().unwrap_or_default().to_string(),
            matched_key: v["matchedKey"].as_str().map(str::to_string),
            reason: v["reason"].as_str().map(str::to_string),
            entries: set("entries").unwrap_or_default(),
            db_hashes: set("dbHashes"),
            db_rows: v["dbRows"].as_i64(),
            db_created: v["dbCreated"].as_str().map(str::to_string),
            wal_bytes: v["walBytes"].as_u64().unwrap_or(0),
            db_note: v["dbNote"].as_str().unwrap_or_default().to_string(),
            bytes: v["bytes"].as_u64().unwrap_or(0),
        }
    }

    pub fn write(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(path, self.to_json().to_string()).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// The snapshot the job's restore left (`WF_CACHE_RESTORE_FILE`), when this is a job that restored.
pub fn restored() -> Option<Snapshot> {
    let path = std::env::var_os(FILE_ENV).filter(|p| !p.is_empty())?;
    let raw = std::fs::read_to_string(path).ok()?;
    Some(Snapshot::from_json(&serde_json::from_str(&raw).ok()?))
}
