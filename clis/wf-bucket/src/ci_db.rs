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
/// The tables a JSON row goes to; the ledger's own tables (`NX_TASKS`, `CI_JOBS`) are typed and made by `ensure_ledger`.
pub const ROW_TABLES: [&str; 3] = ["TEST_RESULTS", "WF_USAGE", "HEALTH_REPORTS"];

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

/// Tables renamed to say what they hold (old name, new name); a database written before the rename is carried over when it is opened or merged.
const RENAMED: [(&str, &str); 2] = [("TEST_RUNS", "TEST_RESULTS"), ("CI_TASKS", "NX_TASKS")];

fn renamed(name: &str) -> &str {
    RENAMED.iter().find(|(old, _)| *old == name).map_or(name, |(_, new)| new)
}

fn migrate(conn: &Connection) -> Result<(), String> {
    for (old, new) in RENAMED {
        let has = |t: &str| conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1", [t], |r| r.get::<_, i64>(0)).map(|n| n > 0);
        if !has(old).map_err(sql)? {
            continue;
        }
        if has(new).map_err(sql)? {
            conn.execute_batch(&format!("INSERT OR IGNORE INTO {new} SELECT * FROM {old}; DROP TABLE {old};")).map_err(sql)?;
        } else {
            conn.execute_batch(&format!("ALTER TABLE {old} RENAME TO {new};")).map_err(sql)?;
        }
    }
    for (table, cols) in TYPED {
        let blob = conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1", [table], |r| r.get::<_, i64>(0)).map_err(sql)? > 0 && has_column(conn, "main", table, "ROW")?;
        if blob {
            conn.execute_batch(&format!("ALTER TABLE {table} RENAME TO {table}_BLOB;")).map_err(sql)?;
            conn.execute_batch(&create_typed(table, cols)).map_err(sql)?;
            conn.execute_batch(&format!("{}; DROP TABLE {table}_BLOB;", blob_to_columns(table, cols, &format!("{table}_BLOB")))).map_err(sql)?;
        }
    }
    Ok(())
}

fn open_at(path: &Path) -> Result<Connection, String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let conn = Connection::open(path).map_err(sql)?;
    conn.busy_timeout(std::time::Duration::from_secs(10)).map_err(sql)?;
    migrate(&conn)?;
    Ok(conn)
}

fn checked_table(table: &str) -> Result<(), String> {
    if ROW_TABLES.contains(&table) {
        Ok(())
    } else {
        Err(format!("ci database: {table} is not one of {}", ROW_TABLES.join(", ")))
    }
}

/// The tables whose rows are real columns: (table, [(column, SQL type)]). A JSON key lands in the column of its upper-cased name; a key with no column is
/// kept in `EXTRA` as JSON, so nothing a writer sends is lost. A table not listed (HEALTH_REPORTS) keeps each row as a JSON blob in `ROW`.
const TYPED: [(&str, &[(&str, &str)]); 2] = [
    (
        "TEST_RESULTS",
        &[
            ("RUN_ID", "TEXT"), ("HARNESS", "TEXT"), ("SUITE_OR_RUNBOOK", "TEXT"), ("JOB_ID", "TEXT"), ("SHA", "TEXT"), ("METRO_COMMIT", "TEXT"), ("VERDICT", "TEXT"),
            ("EXIT_CODE", "INTEGER"), ("DURATION_S", "REAL"), ("ATTEMPT", "INTEGER"), ("RETRY_REASON", "TEXT"), ("DEVICE_OR_SCREEN", "TEXT"), ("PLATFORM", "TEXT"),
            ("RAN_ON", "TEXT"), ("STEPS", "INTEGER"), ("SCREENSHOTS", "INTEGER"), ("RECORDINGS", "INTEGER"), ("GOLDENS_TOTAL", "INTEGER"), ("GOLDENS_FAILED", "INTEGER"),
            ("WARNINGS", "INTEGER"), ("DEBUG_HITS", "INTEGER"), ("TESTS_TOTAL", "INTEGER"), ("TESTS_FAILED", "INTEGER"), ("REPORT_PATH", "TEXT"), ("TIMING_JSON", "TEXT"),
            ("NOTE", "TEXT"),
        ],
    ),
    ("WF_USAGE", &[("COMMAND", "TEXT"), ("EXIT_CODE", "INTEGER"), ("DURATION_MS", "INTEGER"), ("ERROR", "TEXT"), ("WF_VERSION", "TEXT"), ("OS", "TEXT")]),
];

fn typed(table: &str) -> Option<&'static [(&'static str, &'static str)]> {
    TYPED.iter().find(|(t, _)| *t == table).map(|(_, cols)| *cols)
}

fn create_typed(table: &str, cols: &[(&str, &str)]) -> String {
    let columns: Vec<String> = cols.iter().map(|(c, t)| format!("{c} {t}")).collect();
    format!("CREATE TABLE IF NOT EXISTS {table} (KEY TEXT PRIMARY KEY, TS INTEGER NOT NULL, {}, EXTRA TEXT)", columns.join(", "))
}

fn ensure_rows(conn: &Connection, table: &str) -> Result<(), String> {
    checked_table(table)?;
    match typed(table) {
        Some(cols) => conn.execute_batch(&create_typed(table, cols)).map_err(sql),
        None => conn.execute_batch(&format!("CREATE TABLE IF NOT EXISTS {table} (KEY TEXT PRIMARY KEY, TS INTEGER NOT NULL, ROW TEXT NOT NULL)")).map_err(sql),
    }
}

fn has_column(conn: &Connection, schema: &str, table: &str, column: &str) -> Result<bool, String> {
    let mut stmt = conn.prepare(&format!("SELECT name FROM pragma_table_info('{table}', '{schema}')")).map_err(sql)?;
    let names: Vec<String> = stmt.query_map([], |r| r.get(0)).map_err(sql)?.collect::<Result<_, _>>().map_err(sql)?;
    Ok(names.iter().any(|n| n == column))
}

/// The statement that fills `table`'s columns from a blob table's JSON `ROW` (`json_extract` per column; `EXTRA` is the keys no column holds).
fn blob_to_columns(table: &str, cols: &[(&str, &str)], source: &str) -> String {
    let names: Vec<&str> = cols.iter().map(|(c, _)| *c).collect();
    let picks: Vec<String> = names.iter().map(|c| format!("json_extract(ROW, '$.{}')", c.to_lowercase())).collect();
    let known: Vec<String> = names.iter().map(|c| format!("'{}'", c.to_lowercase())).collect();
    format!(
        "INSERT OR IGNORE INTO main.{table} (KEY, TS, {}, EXTRA) SELECT KEY, TS, {}, (SELECT json_group_object(key, value) FROM json_each(ROW) WHERE key NOT IN ({})) FROM {source}",
        names.join(", "),
        picks.join(", "),
        known.join(", ")
    )
}

/// The typed tables of the CI ledger, as `HEALTH_RPT` had them.
pub fn ensure_ledger(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS NX_TASKS (RUN_ID TEXT NOT NULL, SHA TEXT NOT NULL, TS INTEGER NOT NULL, JOB TEXT NOT NULL, TASK TEXT NOT NULL,
           HASH TEXT NOT NULL, OUTCOME TEXT NOT NULL, MS INTEGER NOT NULL, PASSED INTEGER NOT NULL, PRIMARY KEY (RUN_ID, TASK, HASH));
         CREATE TABLE IF NOT EXISTS CI_JOBS (RUN_ID TEXT NOT NULL, SHA TEXT NOT NULL, TS INTEGER NOT NULL, KIND TEXT NOT NULL, JOB TEXT NOT NULL,
           SECS INTEGER NOT NULL, QUEUED INTEGER NOT NULL, PRIMARY KEY (RUN_ID, JOB));",
    )
    .map_err(sql)
}

fn cell(v: &Value) -> rusqlite::types::Value {
    use rusqlite::types::Value as Sql;
    match v {
        Value::Null => Sql::Null,
        Value::Bool(b) => Sql::Integer(i64::from(*b)),
        Value::Number(n) => n.as_i64().map(Sql::Integer).or_else(|| n.as_f64().map(Sql::Real)).unwrap_or(Sql::Null),
        Value::String(s) => Sql::Text(s.clone()),
        other => Sql::Text(other.to_string()),
    }
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
    let Some(cols) = typed(table) else {
        conn.execute(&format!("INSERT OR IGNORE INTO {table} (KEY, TS, ROW) VALUES (?1, ?2, ?3)"), rusqlite::params![key, ts, text]).map_err(sql)?;
        return Ok(());
    };
    let mut params: Vec<rusqlite::types::Value> = vec![key.into(), ts.into()];
    params.extend(cols.iter().map(|(c, _)| row.get(&c.to_lowercase()).map_or(rusqlite::types::Value::Null, cell)));
    let extra: Map<String, Value> = row.iter().filter(|(k, _)| !cols.iter().any(|(c, _)| c.to_lowercase() == **k)).map(|(k, v)| (k.clone(), v.clone())).collect();
    params.push(if extra.is_empty() { rusqlite::types::Value::Null } else { Value::Object(extra).to_string().into() });
    let names: Vec<&str> = cols.iter().map(|(c, _)| *c).collect();
    let marks: Vec<String> = (1..=params.len()).map(|i| format!("?{i}")).collect();
    conn.execute(&format!("INSERT OR IGNORE INTO {table} (KEY, TS, {}, EXTRA) VALUES ({})", names.join(", "), marks.join(", ")), rusqlite::params_from_iter(params)).map_err(sql)?;
    Ok(())
}

/// Records `row` in the job's local database when there is one (a hosted run); elsewhere it does nothing. Best effort: the failure is
/// returned for the caller to log, never to fail a command.
pub fn record(table: &str, row: &Map<String, Value>) -> Result<bool, String> {
    let Some(path) = local_path() else { return Ok(false) };
    insert_at(&path, table, row).map(|()| true)
}

/// One task of a CI run's Nx ledger (`NX_TASKS`).
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

/// Writes tasks to the typed `NX_TASKS` table of the job's local database, which the report job merges into the sealed one with the other rows. A task already
/// there (same run, task and hash) is kept once. Nothing is written, and `false` comes back, when this is not a CI job with a local database.
pub fn record_tasks_local(tasks: &[LedgerTask]) -> Result<bool, String> {
    let Some(path) = local_path() else { return Ok(false) };
    let mut conn = open_at(&path)?;
    ensure_ledger(&conn)?;
    let tx = conn.transaction().map_err(sql)?;
    for t in tasks {
        tx.execute(
            "INSERT OR IGNORE INTO NX_TASKS (RUN_ID, SHA, TS, JOB, TASK, HASH, OUTCOME, MS, PASSED) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![t.run, t.sha, t.ts, t.job, t.task, t.hash, t.outcome, t.ms, t.passed as i64],
        )
        .map_err(sql)?;
    }
    tx.commit().map_err(sql)?;
    Ok(true)
}

/// One job of a CI run (`CI_JOBS`): how long it ran and how long it waited for a runner.
pub struct JobRow {
    pub run: String,
    pub sha: String,
    pub ts: i64,
    pub kind: String,
    pub job: String,
    pub secs: i64,
    pub queued: i64,
}

/// Writes jobs to the typed `CI_JOBS` table of the job's local database (the report job merges it); a job already there for the run is kept once.
/// `false` comes back, and nothing is written, when there is no local database.
pub fn record_jobs_local(jobs: &[JobRow]) -> Result<bool, String> {
    let Some(path) = local_path() else { return Ok(false) };
    let mut conn = open_at(&path)?;
    ensure_ledger(&conn)?;
    let tx = conn.transaction().map_err(sql)?;
    for j in jobs {
        tx.execute(
            "INSERT OR IGNORE INTO CI_JOBS (RUN_ID, SHA, TS, KIND, JOB, SECS, QUEUED) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![j.run, j.sha, j.ts, j.kind, j.job, j.secs, j.queued],
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
            let target = renamed(&name);
            if let Some(cols) = typed(target).filter(|_| matches!(has_column(into, "other", &name, "ROW"), Ok(true))) {
                into.execute_batch(&create_typed(target, cols)).map_err(sql)?;
                copied += into.execute(&blob_to_columns(target, cols, &format!("other.\"{name}\"")), []).map_err(sql)?;
                continue;
            }
            let create = if target == name { create } else { create.replacen(&name, target, 1) };
            into.execute_batch(&create.replacen("CREATE TABLE", "CREATE TABLE IF NOT EXISTS", 1)).map_err(sql)?;
            let mut stmt = into.prepare(&format!("SELECT name FROM pragma_table_info('{name}', 'other')")).map_err(sql)?;
            let theirs: Vec<String> = stmt.query_map([], |r| r.get(0)).map_err(sql)?.collect::<Result<_, _>>().map_err(sql)?;
            drop(stmt);
            let mut stmt = into.prepare(&format!("SELECT name FROM pragma_table_info('{target}', 'main')")).map_err(sql)?;
            let ours: Vec<String> = stmt.query_map([], |r| r.get(0)).map_err(sql)?.collect::<Result<_, _>>().map_err(sql)?;
            drop(stmt);
            let shared: Vec<String> = theirs.into_iter().filter(|c| ours.contains(c)).map(|c| format!("\"{c}\"")).collect();
            let list = shared.join(", ");
            copied += into.execute(&format!("INSERT OR IGNORE INTO main.\"{target}\" ({list}) SELECT {list} FROM other.\"{name}\""), []).map_err(sql)?;
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
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM NX_TASKS", [], |r| r.get(0)).unwrap();
        assert!(first && again);
        assert_eq!(n, 2);
    }

    #[test]
    fn given_a_database_with_the_old_table_names_when_it_is_opened_and_merged_then_the_rows_sit_under_the_new_names() {
        let dir = tempfile::tempdir().unwrap();
        let (old, sealed) = (dir.path().join("old.sqlite"), dir.path().join("sealed.sqlite"));
        let conn = Connection::open(&old).unwrap();
        conn.execute_batch(
            "CREATE TABLE CI_TASKS (RUN_ID TEXT NOT NULL, SHA TEXT NOT NULL, TS INTEGER NOT NULL, JOB TEXT NOT NULL, TASK TEXT NOT NULL, HASH TEXT NOT NULL, OUTCOME TEXT NOT NULL, MS INTEGER NOT NULL, PASSED INTEGER NOT NULL, PRIMARY KEY (RUN_ID, TASK, HASH));
             INSERT INTO CI_TASKS VALUES ('1','a',1,'j','t','h','ran',5,1);
             CREATE TABLE TEST_RUNS (KEY TEXT PRIMARY KEY, TS INTEGER NOT NULL, ROW TEXT NOT NULL);
             INSERT INTO TEST_RUNS VALUES ('k',1,'{\"run_id\":\"r1\",\"verdict\":\"fail\",\"duration_s\":1.5,\"custom\":\"x\"}');",
        )
        .unwrap();
        drop(conn);

        let opened = open_at(&old).unwrap();
        let moved: i64 = opened.query_row("SELECT (SELECT COUNT(*) FROM NX_TASKS) + (SELECT COUNT(*) FROM TEST_RESULTS)", [], |r| r.get(0)).unwrap();
        let legacy = dir.path().join("legacy.sqlite");
        let l = Connection::open(&legacy).unwrap();
        l.execute_batch("CREATE TABLE TEST_RUNS (KEY TEXT PRIMARY KEY, TS INTEGER NOT NULL, ROW TEXT NOT NULL); INSERT INTO TEST_RUNS VALUES ('z',2,'{}');").unwrap();
        drop(l);
        let into = open_at(&sealed).unwrap();
        merge(&into, &legacy).unwrap();
        let merged: i64 = into.query_row("SELECT COUNT(*) FROM TEST_RESULTS", [], |r| r.get(0)).unwrap();
        let old_left: i64 = into.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name IN ('TEST_RUNS','CI_TASKS')", [], |r| r.get(0)).unwrap();

        let (verdict, secs, extra): (String, f64, String) = opened.query_row("SELECT VERDICT, DURATION_S, EXTRA FROM TEST_RESULTS WHERE RUN_ID = 'r1'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();

        assert_eq!((moved, merged, old_left), (2, 1, 0));
        assert_eq!((verdict.as_str(), secs, extra.as_str()), ("fail", 1.5, "{\"custom\":\"x\"}"));
    }

    #[test]
    fn given_a_usage_row_when_it_is_recorded_then_its_fields_are_columns_and_unknown_keys_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("rows.sqlite");
        let row: Map<String, Value> = serde_json::from_str(r#"{"command":"ci comment","exit_code":0,"duration_ms":19927,"error":null,"os":"linux","new_field":true}"#).unwrap();

        insert_at(&db, "WF_USAGE", &row).unwrap();
        insert_at(&db, "WF_USAGE", &row).unwrap();

        let conn = Connection::open(&db).unwrap();
        let (n, command, ms, extra): (i64, String, i64, String) =
            conn.query_row("SELECT COUNT(*), MAX(COMMAND), MAX(DURATION_MS), MAX(EXTRA) FROM WF_USAGE", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap();
        assert_eq!((n, command.as_str(), ms, extra.as_str()), (1, "ci comment", 19927, "{\"new_field\":true}"));
    }
}
