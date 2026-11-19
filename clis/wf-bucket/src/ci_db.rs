//! The CI database: what CI used to send to HEALTH_RPT through ORDS (test runs, usage of the commands, health reports, the ledger of tasks and jobs),
//! kept as one SQLite file. A hosted job writes its rows to a plain local file (`local_path`); the report job merges the jobs' files into
//! the shared database, which lives sealed (XChaCha20-Poly1305 under `CI_DB_KEY`) on the state ref at `STATE_PATH`. Rows are never
//! secret, but the file is sealed so the state ref carries nothing readable without the key.
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rusqlite::Connection;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

pub const STATE_PATH: &str = "ci/ci.sqlite.sealed";
pub const KEY: &str = "CI_DB_KEY";
const MAGIC: &[u8] = b"WFCIDB1\n";
/// The tables a JSON row goes to; the ledger's own tables (`CI_TASKS`, `CI_JOBS`) are typed and made by `ensure_ledger`.
pub const ROW_TABLES: [&str; 3] = ["TEST_RUNS", "WF_USAGE", "HEALTH_REPORTS"];

/// The job's local database: `WF_CI_DB` when set (`off` for none), else `$RUNNER_TEMP/wf-ci-db/rows.sqlite` on a GitHub Actions runner,
/// else none.
pub fn local_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("WF_CI_DB").filter(|p| !p.is_empty()) {
        return (path != "off").then(|| PathBuf::from(path));
    }
    let temp = std::env::var_os("RUNNER_TEMP").filter(|_| std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true"))?;
    Some(PathBuf::from(temp).join("wf-ci-db").join("rows.sqlite"))
}

fn sql(e: rusqlite::Error) -> String {
    format!("ci database: {e}")
}

fn open_at(path: &Path) -> Result<Connection, String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let conn = Connection::open(path).map_err(sql)?;
    conn.busy_timeout(std::time::Duration::from_secs(10)).map_err(sql)?;
    Ok(conn)
}

fn checked_table(table: &str) -> Result<(), String> {
    if ROW_TABLES.contains(&table) {
        Ok(())
    } else {
        Err(format!("ci database: {table} is not one of {}", ROW_TABLES.join(", ")))
    }
}

fn ensure_rows(conn: &Connection, table: &str) -> Result<(), String> {
    checked_table(table)?;
    conn.execute_batch(&format!("CREATE TABLE IF NOT EXISTS {table} (KEY TEXT PRIMARY KEY, TS INTEGER NOT NULL, ROW TEXT NOT NULL)")).map_err(sql)
}

/// The typed tables of the CI ledger, as `HEALTH_RPT` had them.
pub fn ensure_ledger(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS CI_TASKS (RUN_ID TEXT NOT NULL, SHA TEXT NOT NULL, TS INTEGER NOT NULL, JOB TEXT NOT NULL, TASK TEXT NOT NULL,
           HASH TEXT NOT NULL, OUTCOME TEXT NOT NULL, MS INTEGER NOT NULL, PASSED INTEGER NOT NULL, PRIMARY KEY (RUN_ID, TASK, HASH));
         CREATE TABLE IF NOT EXISTS CI_JOBS (RUN_ID TEXT NOT NULL, SHA TEXT NOT NULL, TS INTEGER NOT NULL, KIND TEXT NOT NULL, JOB TEXT NOT NULL,
           SECS INTEGER NOT NULL, QUEUED INTEGER NOT NULL, PRIMARY KEY (RUN_ID, JOB));",
    )
    .map_err(sql)
}

/// Inserts `row` into `table` of the database at `path` once: the same row recorded twice (a retried step) is one row.
pub fn insert_at(path: &Path, table: &str, row: &Map<String, Value>) -> Result<(), String> {
    let conn = open_at(path)?;
    ensure_rows(&conn, table)?;
    let text = Value::Object(row.clone()).to_string();
    let key = {
        use sha2::Digest;
        hex::encode(sha2::Sha256::digest(text.as_bytes()))
    };
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    conn.execute(&format!("INSERT OR IGNORE INTO {table} (KEY, TS, ROW) VALUES (?1, ?2, ?3)"), rusqlite::params![key, ts, text]).map_err(sql)?;
    Ok(())
}

/// Records `row` in the job's local database when there is one (a hosted run); elsewhere it does nothing. Best effort: the failure is
/// returned for the caller to log, never to fail a command.
pub fn record(table: &str, row: &Map<String, Value>) -> Result<bool, String> {
    let Some(path) = local_path() else { return Ok(false) };
    insert_at(&path, table, row).map(|()| true)
}

/// One task of a CI run's Nx ledger (`CI_TASKS`).
pub struct LedgerTask {
    pub run: String,
    pub sha: String,
    pub ts: i64,
    pub job: String,
    pub task: String,
    pub hash: String,
    pub outcome: String,
    pub ms: i64,
    pub passed: bool,
}

/// Writes tasks to the typed `CI_TASKS` table of the job's local database, which the report job merges into the sealed one with the other rows. A task already
/// there (same run, task and hash) is kept once. Nothing is written, and `false` comes back, when this is not a CI job with a local database.
pub fn record_tasks_local(tasks: &[LedgerTask]) -> Result<bool, String> {
    let Some(path) = local_path() else { return Ok(false) };
    let mut conn = open_at(&path)?;
    ensure_ledger(&conn)?;
    let tx = conn.transaction().map_err(sql)?;
    for t in tasks {
        tx.execute(
            "INSERT OR IGNORE INTO CI_TASKS (RUN_ID, SHA, TS, JOB, TASK, HASH, OUTCOME, MS, PASSED) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![t.run, t.sha, t.ts, t.job, t.task, t.hash, t.outcome, t.ms, t.passed as i64],
        )
        .map_err(sql)?;
    }
    tx.commit().map_err(sql)?;
    Ok(true)
}

/// Copies every table of `from` into `into`, skipping rows `into` already holds (by primary key). A table `into` lacks is made with
/// `from`'s definition.
pub fn merge(into: &Connection, from: &Path) -> Result<usize, String> {
    into.execute("ATTACH DATABASE ?1 AS other", [from.to_string_lossy()]).map_err(sql)?;
    let result = (|| {
        let tables: Vec<(String, String)> = {
            let mut stmt = into.prepare("SELECT name, sql FROM other.sqlite_master WHERE type = 'table'").map_err(sql)?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(sql)?;
            rows.collect::<Result<_, _>>().map_err(sql)?
        };
        let mut copied = 0;
        for (name, create) in tables {
            into.execute_batch(&create.replacen("CREATE TABLE", "CREATE TABLE IF NOT EXISTS", 1)).map_err(sql)?;
            copied += into.execute(&format!("INSERT OR IGNORE INTO main.\"{name}\" SELECT * FROM other.\"{name}\""), []).map_err(sql)?;
        }
        Ok(copied)
    })();
    into.execute("DETACH DATABASE other", []).map_err(sql)?;
    result
}

/// The key `CI_DB_KEY` holds: 32 bytes as 64 hex characters, from the environment or the vault.
pub fn key() -> Result<[u8; 32], String> {
    let values = secrets::load_values(&[KEY]);
    let text = values.get(KEY).cloned().flatten().ok_or_else(|| format!("{KEY} is not set and the vault has none"))?;
    let bytes = hex::decode(text.trim()).map_err(|_| format!("{KEY} is not hex"))?;
    bytes.try_into().map_err(|_| format!("{KEY} is not 32 bytes"))
}

pub fn seal(key: &[u8; 32], plain: &[u8]) -> Result<Vec<u8>, String> {
    let mut nonce = [0u8; 24];
    getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
    let cipher = XChaCha20Poly1305::new(key.into());
    let sealed = cipher.encrypt(XNonce::from_slice(&nonce), plain).map_err(|_| "ci database: sealing failed".to_string())?;
    Ok([MAGIC, &nonce, &sealed].concat())
}

pub fn unseal(key: &[u8; 32], bytes: &[u8]) -> Result<Vec<u8>, String> {
    let body = bytes.strip_prefix(MAGIC).ok_or("ci database: not a sealed database")?;
    if body.len() < 24 {
        return Err("ci database: the sealed file is cut short".into());
    }
    let (nonce, sealed) = body.split_at(24);
    XChaCha20Poly1305::new(key.into()).decrypt(XNonce::from_slice(nonce), sealed).map_err(|_| format!("ci database: it does not open with {KEY} (another key, or changed)"))
}

/// A working copy of the shared database: unsealed into a private temporary directory (empty when the state ref has none yet).
pub struct Working {
    _dir: tempfile::TempDir,
    pub path: PathBuf,
    key: [u8; 32],
}

impl Working {
    pub fn fetch() -> Result<Working, String> {
        Working::from_sealed(key()?, crate::state::read_bytes(STATE_PATH)?)
    }

    pub fn from_sealed(key: [u8; 32], sealed: Option<Vec<u8>>) -> Result<Working, String> {
        let dir = tempfile::Builder::new().prefix("wf-ci-db-").tempdir().map_err(|e| e.to_string())?;
        let path = dir.path().join("ci.sqlite");
        if let Some(bytes) = sealed {
            std::fs::write(&path, unseal(&key, &bytes)?).map_err(|e| e.to_string())?;
        }
        Ok(Working { _dir: dir, path, key })
    }

    pub fn connect(&self) -> Result<Connection, String> {
        open_at(&self.path)
    }

    /// The working copy, compacted and sealed, as the bytes the state ref keeps.
    pub fn sealed(&self) -> Result<Vec<u8>, String> {
        self.connect()?.execute_batch("VACUUM").map_err(sql)?;
        seal(&self.key, &std::fs::read(&self.path).map_err(|e| e.to_string())?)
    }

    pub fn publish(&self, message: &str) -> Result<String, String> {
        crate::state::write(&[(STATE_PATH, &self.sealed()?)], message)
    }
}

/// Rows of a query as JSON objects, column name to value.
pub fn query(conn: &Connection, statement: &str) -> Result<Vec<Value>, String> {
    let mut stmt = conn.prepare(statement).map_err(sql)?;
    let names: Vec<String> = stmt.column_names().iter().map(|n| n.to_string()).collect();
    let rows = stmt
        .query_map([], |r| {
            let mut out = Map::new();
            for (i, name) in names.iter().enumerate() {
                let v = match r.get_ref(i)? {
                    rusqlite::types::ValueRef::Null => Value::Null,
                    rusqlite::types::ValueRef::Integer(n) => Value::from(n),
                    rusqlite::types::ValueRef::Real(f) => Value::from(f),
                    rusqlite::types::ValueRef::Text(t) => Value::from(String::from_utf8_lossy(t).into_owned()),
                    rusqlite::types::ValueRef::Blob(b) => Value::from(hex::encode(b)),
                };
                out.insert(name.clone(), v);
            }
            Ok(Value::Object(out))
        })
        .map_err(sql)?;
    rows.collect::<Result<_, _>>().map_err(sql)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(hash: &str) -> LedgerTask {
        LedgerTask { run: "1".into(), sha: "abc".into(), ts: 10, job: "flow-tests (1/6)".into(), task: "app:test".into(), hash: hash.into(), outcome: "ran".into(), ms: 4200, passed: true }
    }

    #[test]
    fn given_a_ci_job_database_when_the_same_task_is_recorded_twice_then_it_is_kept_once() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("rows.sqlite");
        std::env::set_var("WF_CI_DB", &db);

        let first = record_tasks_local(&[task("h1"), task("h2")]).unwrap();
        let again = record_tasks_local(&[task("h1")]).unwrap();

        std::env::remove_var("WF_CI_DB");
        let conn = Connection::open(&db).unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM CI_TASKS", [], |r| r.get(0)).unwrap();
        assert!(first && again);
        assert_eq!(n, 2);
    }
}
