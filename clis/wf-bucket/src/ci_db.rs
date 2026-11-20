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

/// The typed tables of the CI ledger, as `HEALTH_RPT` had them, and the parallelism telemetry (`docs/solutions/ci/parallelism-telemetry.md`).
pub fn ensure_ledger(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS NX_TASKS (RUN_ID TEXT NOT NULL, SHA TEXT NOT NULL, TS INTEGER NOT NULL, JOB TEXT NOT NULL, TASK TEXT NOT NULL,
           HASH TEXT NOT NULL, OUTCOME TEXT NOT NULL, MS INTEGER NOT NULL, PASSED INTEGER NOT NULL, PRIMARY KEY (RUN_ID, TASK, HASH));
         CREATE TABLE IF NOT EXISTS CI_METRICS (RUN_ID TEXT NOT NULL, TS INTEGER NOT NULL, METRIC TEXT NOT NULL, SCOPE TEXT NOT NULL, NAME TEXT NOT NULL, VALUE REAL NOT NULL,
           PRIMARY KEY (RUN_ID, TS, METRIC, SCOPE, NAME));
         CREATE TABLE IF NOT EXISTS CI_JOBS (RUN_ID TEXT NOT NULL, SHA TEXT NOT NULL, TS INTEGER NOT NULL, KIND TEXT NOT NULL, JOB TEXT NOT NULL,
           SECS INTEGER NOT NULL, QUEUED INTEGER NOT NULL, PRIMARY KEY (RUN_ID, JOB));
         CREATE TABLE IF NOT EXISTS CI_JOB_RESOURCES (RUN_ID TEXT NOT NULL, JOB TEXT NOT NULL, WORKFLOW TEXT, RUNNER TEXT, CORES INTEGER, SAMPLES INTEGER, CPU_AVG REAL, CPU_P95 REAL,
           CPU_MAX REAL, IOWAIT_AVG REAL, MEM_AVG_MB REAL, MEM_PEAK_MB REAL, MEM_TOTAL_MB REAL, SWAP_PEAK_MB REAL, LOAD1_MAX REAL, PSI_CPU_MAX REAL, PSI_MEM_MAX REAL, PSI_IO_MAX REAL,
           READ_MB REAL, WRITE_MB REAL, DURATION_S REAL, IDLE_S REAL, SATURATED_S REAL, PRIMARY KEY (RUN_ID, JOB));
         CREATE TABLE IF NOT EXISTS CI_TASK_RESOURCES (RUN_ID TEXT NOT NULL, JOB TEXT NOT NULL, TASK TEXT NOT NULL, STARTED INTEGER NOT NULL, ENDED INTEGER, SECONDS REAL, CPU_S REAL,
           CORES_USED REAL, CONCURRENT INTEGER, CPU_AVG REAL, MEM_PEAK_MB REAL, PRIMARY KEY (RUN_ID, JOB, TASK, STARTED));
         CREATE TABLE IF NOT EXISTS CI_STEPS (RUN_ID TEXT NOT NULL, JOB TEXT NOT NULL, STEP_NO INTEGER NOT NULL, STEP TEXT, STARTED INTEGER, COMPLETED INTEGER, DURATION_S INTEGER,
           CONCLUSION TEXT, PRIMARY KEY (RUN_ID, JOB, STEP_NO));
         CREATE TABLE IF NOT EXISTS CI_RUN_PROFILE (RUN_ID TEXT NOT NULL PRIMARY KEY, WORKFLOW TEXT, WALL_S REAL, RUNNER_S REAL, PEAK_JOBS INTEGER, AVG_JOBS REAL, QUEUE_S_TOTAL REAL,
           QUEUE_S_MAX REAL, JOBS INTEGER, CRITICAL_PATH_JOB TEXT, CRITICAL_PATH_S REAL, PARALLEL_EFFICIENCY REAL);
         CREATE TABLE IF NOT EXISTS CI_CONCURRENCY (TS INTEGER NOT NULL, RUN_ID TEXT NOT NULL, PHASE TEXT NOT NULL, WORKFLOW TEXT NOT NULL, BUSY INTEGER NOT NULL, QUEUED INTEGER NOT NULL,
           RUNS_ACTIVE INTEGER NOT NULL, CAP INTEGER NOT NULL, HEADROOM INTEGER NOT NULL, PRIMARY KEY (RUN_ID, PHASE, TS, WORKFLOW));",
    )
    .map_err(sql)?;
    // columns added later, to a database that had the first ones: what a job ended as, when it started (0: never), the concurrency group it queued in, the wait
    // from creation to start (QUEUED_S, null when it never started), the runner it ran on; a task's interval (epoch ms) and its share of the stage's CPU seconds
    for (table, column, kind) in [
        ("CI_JOBS", "CONCLUSION", "TEXT"),
        ("CI_JOBS", "STARTED", "INTEGER"),
        ("CI_JOBS", "GROUP_KEY", "TEXT"),
        ("CI_JOBS", "STEP", "TEXT"),
        ("CI_JOBS", "EXCERPT", "TEXT"),
        ("CI_JOBS", "QUEUED_S", "INTEGER"),
        ("CI_JOBS", "RUNNER", "TEXT"),
        ("NX_TASKS", "STARTED_MS", "INTEGER"),
        ("NX_TASKS", "ENDED_MS", "INTEGER"),
        ("NX_TASKS", "CPU_S", "REAL"),
    ] {
        if !has_column(conn, "main", table, column)? {
            conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {kind}")).map_err(sql)?;
        }
    }
    Ok(())
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
    /// The interval Nx reports (epoch ms; 0 when unknown) and the task's share of the stage's CPU seconds.
    pub started_ms: i64,
    pub ended_ms: i64,
    pub cpu_s: f64,
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
            "INSERT OR IGNORE INTO NX_TASKS (RUN_ID, SHA, TS, JOB, TASK, HASH, OUTCOME, MS, PASSED, STARTED_MS, ENDED_MS, CPU_S) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            rusqlite::params![t.run, t.sha, t.ts, t.job, t.task, t.hash, t.outcome, t.ms, t.passed as i64, t.started_ms, t.ended_ms, t.cpu_s],
        )
        .map_err(sql)?;
    }
    tx.commit().map_err(sql)?;
    Ok(true)
}

/// One gauge reading (`CI_METRICS`): the same record the gauges send to Better Stack, two labels (`scope`, `name`), so both transports hold one thing.
pub struct MetricRow {
    pub run: String,
    pub ts: i64,
    pub metric: String,
    pub scope: String,
    pub name: String,
    pub value: f64,
}

/// Writes readings to the typed `CI_METRICS` table of the job's local database (the report job merges it); `false`, and nothing written, without one.
pub fn record_metrics_local(rows: &[MetricRow]) -> Result<bool, String> {
    let Some(path) = local_path() else { return Ok(false) };
    let mut conn = open_at(&path)?;
    ensure_ledger(&conn)?;
    let tx = conn.transaction().map_err(sql)?;
    for r in rows {
        tx.execute(
            "INSERT OR IGNORE INTO CI_METRICS (RUN_ID, TS, METRIC, SCOPE, NAME, VALUE) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![r.run, r.ts, r.metric, r.scope, r.name, r.value],
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
    pub conclusion: String,
    pub started: i64,
    pub group_key: Option<String>,
    pub step: Option<String>,
    pub excerpt: Option<String>,
    /// Seconds from the job's creation to its start (`None` when it never started) and the runner that ran it.
    pub queued_s: Option<i64>,
    pub runner: Option<String>,
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
            "INSERT OR IGNORE INTO CI_JOBS (RUN_ID, SHA, TS, KIND, JOB, SECS, QUEUED, CONCLUSION, STARTED, GROUP_KEY, STEP, EXCERPT, QUEUED_S, RUNNER) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            rusqlite::params![j.run, j.sha, j.ts, j.kind, j.job, j.secs, j.queued, j.conclusion, j.started, j.group_key, j.step, j.excerpt, j.queued_s, j.runner],
        )
        .map_err(sql)?;
    }
    tx.commit().map_err(sql)?;
    Ok(true)
}

/// The tasks of the job-local database at `path` (the job's own `NX_TASKS`), the columns a database written before the intervals existed lacks reading as 0.
pub fn tasks_of(path: &Path) -> Result<Vec<LedgerTask>, String> {
    let conn = open_at(path)?;
    ensure_ledger(&conn)?;
    let mut stmt = conn
        .prepare("SELECT RUN_ID, SHA, TS, JOB, TASK, HASH, OUTCOME, MS, PASSED, COALESCE(STARTED_MS, 0), COALESCE(ENDED_MS, 0), COALESCE(CPU_S, 0) FROM NX_TASKS")
        .map_err(sql)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(LedgerTask {
                run: r.get(0)?,
                sha: r.get(1)?,
                ts: r.get(2)?,
                job: r.get(3)?,
                task: r.get(4)?,
                hash: r.get(5)?,
                outcome: r.get(6)?,
                ms: r.get(7)?,
                passed: r.get::<_, i64>(8)? != 0,
                started_ms: r.get(9)?,
                ended_ms: r.get(10)?,
                cpu_s: r.get(11)?,
            })
        })
        .map_err(sql)?;
    rows.collect::<Result<_, _>>().map_err(sql)
}

type Cells = Vec<rusqlite::types::Value>;

fn real(v: Option<f64>) -> rusqlite::types::Value {
    v.map_or(rusqlite::types::Value::Null, rusqlite::types::Value::Real)
}

fn text(v: &str) -> rusqlite::types::Value {
    rusqlite::types::Value::Text(v.to_string())
}

fn int(v: i64) -> rusqlite::types::Value {
    rusqlite::types::Value::Integer(v)
}

/// Runs one `INSERT OR IGNORE` per row of cells into the job's local database; `false`, and nothing written, without one.
fn insert_local(statement: &str, rows: Vec<Cells>) -> Result<bool, String> {
    let Some(path) = local_path() else { return Ok(false) };
    let mut conn = open_at(&path)?;
    ensure_ledger(&conn)?;
    let tx = conn.transaction().map_err(sql)?;
    for cells in rows {
        tx.execute(statement, rusqlite::params_from_iter(cells)).map_err(sql)?;
    }
    tx.commit().map_err(sql)?;
    Ok(true)
}

/// One job's runner samples boiled down (`CI_JOB_RESOURCES`): what the machine did while the job ran.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JobResources {
    pub run: String,
    pub job: String,
    pub workflow: String,
    pub runner: String,
    pub cores: i64,
    pub samples: i64,
    pub cpu_avg: f64,
    pub cpu_p95: f64,
    pub cpu_max: f64,
    pub iowait_avg: f64,
    pub mem_avg_mb: f64,
    pub mem_peak_mb: f64,
    pub mem_total_mb: f64,
    pub swap_peak_mb: f64,
    pub load1_max: f64,
    pub psi_cpu_max: f64,
    pub psi_mem_max: f64,
    pub psi_io_max: f64,
    pub read_mb: f64,
    pub write_mb: f64,
    pub duration_s: f64,
    pub idle_s: f64,
    pub saturated_s: f64,
}

pub fn record_job_resources_local(rows: &[JobResources]) -> Result<bool, String> {
    let cells = rows
        .iter()
        .map(|r| {
            let f = |v: f64| real(Some(v));
            vec![
                text(&r.run), text(&r.job), text(&r.workflow), text(&r.runner), int(r.cores), int(r.samples), f(r.cpu_avg), f(r.cpu_p95), f(r.cpu_max), f(r.iowait_avg), f(r.mem_avg_mb),
                f(r.mem_peak_mb), f(r.mem_total_mb), f(r.swap_peak_mb), f(r.load1_max), f(r.psi_cpu_max), f(r.psi_mem_max), f(r.psi_io_max), f(r.read_mb), f(r.write_mb), f(r.duration_s),
                f(r.idle_s), f(r.saturated_s),
            ]
        })
        .collect();
    insert_local(
        "INSERT OR IGNORE INTO CI_JOB_RESOURCES (RUN_ID, JOB, WORKFLOW, RUNNER, CORES, SAMPLES, CPU_AVG, CPU_P95, CPU_MAX, IOWAIT_AVG, MEM_AVG_MB, MEM_PEAK_MB, MEM_TOTAL_MB, SWAP_PEAK_MB, LOAD1_MAX, \
         PSI_CPU_MAX, PSI_MEM_MAX, PSI_IO_MAX, READ_MB, WRITE_MB, DURATION_S, IDLE_S, SATURATED_S) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
        cells,
    )
}

/// One Nx task that ran, with what the job's samples say about its interval (`CI_TASK_RESOURCES`); the sample columns are null when no sample overlaps it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TaskResources {
    pub run: String,
    pub job: String,
    pub task: String,
    pub started: i64,
    pub ended: i64,
    pub seconds: f64,
    pub cpu_s: f64,
    pub cores_used: f64,
    pub concurrent: i64,
    pub cpu_avg: Option<f64>,
    pub mem_peak_mb: Option<f64>,
}

pub fn record_task_resources_local(rows: &[TaskResources]) -> Result<bool, String> {
    let cells = rows
        .iter()
        .map(|r| vec![text(&r.run), text(&r.job), text(&r.task), int(r.started), int(r.ended), real(Some(r.seconds)), real(Some(r.cpu_s)), real(Some(r.cores_used)), int(r.concurrent), real(r.cpu_avg), real(r.mem_peak_mb)])
        .collect();
    insert_local("INSERT OR IGNORE INTO CI_TASK_RESOURCES (RUN_ID, JOB, TASK, STARTED, ENDED, SECONDS, CPU_S, CORES_USED, CONCURRENT, CPU_AVG, MEM_PEAK_MB) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)", cells)
}

/// One step of one job (`CI_STEPS`): the fixed per-runner overhead (checkout, toolchain, cache restore) is the sum of the setup steps.
#[derive(Debug, Clone, PartialEq)]
pub struct StepRow {
    pub run: String,
    pub job: String,
    pub step_no: i64,
    pub step: String,
    pub started: Option<i64>,
    pub completed: Option<i64>,
    pub duration_s: Option<i64>,
    pub conclusion: Option<String>,
}

pub fn record_steps_local(rows: &[StepRow]) -> Result<bool, String> {
    let opt = |v: Option<i64>| v.map_or(rusqlite::types::Value::Null, int);
    let cells = rows
        .iter()
        .map(|r| vec![text(&r.run), text(&r.job), int(r.step_no), text(&r.step), opt(r.started), opt(r.completed), opt(r.duration_s), r.conclusion.as_deref().map_or(rusqlite::types::Value::Null, text)])
        .collect();
    insert_local("INSERT OR IGNORE INTO CI_STEPS (RUN_ID, JOB, STEP_NO, STEP, STARTED, COMPLETED, DURATION_S, CONCLUSION) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)", cells)
}

/// One run's parallelism (`CI_RUN_PROFILE`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunProfile {
    pub run: String,
    pub workflow: String,
    pub wall_s: f64,
    pub runner_s: f64,
    pub peak_jobs: i64,
    pub avg_jobs: f64,
    pub queue_s_total: f64,
    pub queue_s_max: f64,
    pub jobs: i64,
    pub critical_path_job: String,
    pub critical_path_s: f64,
    pub parallel_efficiency: f64,
}

pub fn record_run_profile_local(row: &RunProfile) -> Result<bool, String> {
    let f = |v: f64| real(Some(v));
    let cells = vec![vec![
        text(&row.run), text(&row.workflow), f(row.wall_s), f(row.runner_s), int(row.peak_jobs), f(row.avg_jobs), f(row.queue_s_total), f(row.queue_s_max), int(row.jobs),
        text(&row.critical_path_job), f(row.critical_path_s), f(row.parallel_efficiency),
    ]];
    insert_local(
        "INSERT OR IGNORE INTO CI_RUN_PROFILE (RUN_ID, WORKFLOW, WALL_S, RUNNER_S, PEAK_JOBS, AVG_JOBS, QUEUE_S_TOTAL, QUEUE_S_MAX, JOBS, CRITICAL_PATH_JOB, CRITICAL_PATH_S, PARALLEL_EFFICIENCY) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        cells,
    )
}

/// One reading of the account's runners (`CI_CONCURRENCY`): jobs in progress (`busy`) and queued of one workflow, or of all (`workflow` = `all`), against the cap.
#[derive(Debug, Clone, PartialEq)]
pub struct ConcurrencyRow {
    pub ts: i64,
    pub run: String,
    pub phase: String,
    pub workflow: String,
    pub busy: i64,
    pub queued: i64,
    pub runs_active: i64,
    pub cap: i64,
    pub headroom: i64,
}

/// The readings of `CI_CONCURRENCY` in a database file (the plan job's rows, shipped as an artifact), oldest first.
pub fn concurrency_of(path: &Path) -> Result<Vec<ConcurrencyRow>, String> {
    let conn = open_at(path)?;
    let mut stmt = conn.prepare("SELECT TS, RUN_ID, PHASE, WORKFLOW, BUSY, QUEUED, RUNS_ACTIVE, CAP, HEADROOM FROM CI_CONCURRENCY ORDER BY TS").map_err(sql)?;
    let rows = stmt
        .query_map([], |r| Ok(ConcurrencyRow { ts: r.get(0)?, run: r.get(1)?, phase: r.get(2)?, workflow: r.get(3)?, busy: r.get(4)?, queued: r.get(5)?, runs_active: r.get(6)?, cap: r.get(7)?, headroom: r.get(8)? }))
        .map_err(sql)?;
    rows.collect::<Result<_, _>>().map_err(sql)
}

pub fn record_concurrency_local(rows: &[ConcurrencyRow]) -> Result<bool, String> {
    let cells = rows.iter().map(|r| vec![int(r.ts), text(&r.run), text(&r.phase), text(&r.workflow), int(r.busy), int(r.queued), int(r.runs_active), int(r.cap), int(r.headroom)]).collect();
    insert_local("INSERT OR IGNORE INTO CI_CONCURRENCY (TS, RUN_ID, PHASE, WORKFLOW, BUSY, QUEUED, RUNS_ACTIVE, CAP, HEADROOM) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)", cells)
}

/// Copies every table of `from` into `into`, skipping rows `into` already holds (by primary key). A table `into` lacks is made with
/// `from`'s definition.
pub fn merge(into: &Connection, from: &Path) -> Result<usize, String> {
    // a sealed database made before a column existed gets it now, or the rows that carry it would lose it in the copy below
    ensure_ledger(into)?;
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

    /// `WF_CI_DB` is the process's: the tests that set it run one at a time.
    static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn task(hash: &str) -> LedgerTask {
        LedgerTask { run: "1".into(), sha: "abc".into(), ts: 10, job: "flow-tests (1/6)".into(), task: "app:test".into(), hash: hash.into(), outcome: "ran".into(), ms: 4200, passed: true, started_ms: 1_000, ended_ms: 5_200, cpu_s: 3.5 }
    }

    #[test]
    fn given_a_ci_job_database_when_the_same_task_is_recorded_twice_then_it_is_kept_once() {
        let _env = ENV.lock().unwrap_or_else(|p| p.into_inner());
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
    fn given_gauge_readings_when_they_are_sent_then_the_same_readings_are_rows_of_ci_metrics() {
        let _env = ENV.lock().unwrap_or_else(|p| p.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("rows.sqlite");
        std::env::set_var("WF_CI_DB", &db);
        std::env::set_var("WF_CI_METRICS", "off");
        std::env::set_var("GITHUB_RUN_ID", "4242");

        crate::ci_metrics::Gauges::new().set("ci.job.duration_s", "flow-tests", "1/6", 551.0).set("ci.run.wall_s", "4242", "all", 1900.0).send();

        std::env::remove_var("WF_CI_DB");
        let conn = Connection::open(&db).unwrap();
        let (n, run, value): (i64, String, f64) = conn.query_row("SELECT COUNT(*), MAX(RUN_ID), MAX(VALUE) FROM CI_METRICS", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!((n, run.as_str(), value), (2, "4242", 1900.0));
    }

    #[test]
    fn given_a_sealed_database_from_before_the_job_columns_when_a_job_database_is_merged_then_the_new_columns_are_added_and_filled() {
        let dir = tempfile::tempdir().unwrap();
        let (old, job) = (dir.path().join("old.sqlite"), dir.path().join("job.sqlite"));
        let conn = Connection::open(&old).unwrap();
        conn.execute_batch("CREATE TABLE CI_JOBS (RUN_ID TEXT NOT NULL, SHA TEXT NOT NULL, TS INTEGER NOT NULL, KIND TEXT NOT NULL, JOB TEXT NOT NULL, SECS INTEGER NOT NULL, QUEUED INTEGER NOT NULL, PRIMARY KEY (RUN_ID, JOB)); INSERT INTO CI_JOBS VALUES ('1','a',1,'k','j',5,0);").unwrap();
        let j = open_at(&job).unwrap();
        ensure_ledger(&j).unwrap();
        j.execute("INSERT INTO CI_JOBS (RUN_ID, SHA, TS, KIND, JOB, SECS, QUEUED, CONCLUSION, STARTED, GROUP_KEY, STEP, EXCERPT) VALUES ('2','b',2,'k','j',7,1,'failure',9,'g','Flow tests','boom')", []).unwrap();
        drop(j);

        merge(&conn, &job).unwrap();

        let (n, conclusion, excerpt): (i64, String, String) = conn.query_row("SELECT COUNT(*), MAX(CONCLUSION), MAX(EXCERPT) FROM CI_JOBS", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!((n, conclusion.as_str(), excerpt.as_str()), (2, "failure", "boom"));
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

    #[test]
    fn given_a_job_that_measured_its_runner_when_the_rows_are_recorded_and_merged_then_each_telemetry_table_holds_them_once() {
        let _env = ENV.lock().unwrap_or_else(|p| p.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let (db, sealed) = (dir.path().join("rows.sqlite"), dir.path().join("sealed.sqlite"));
        std::env::set_var("WF_CI_DB", &db);
        let job = JobResources { run: "9".into(), job: "core-tests".into(), workflow: "CI core".into(), runner: "GitHub Actions 7".into(), cores: 4, samples: 12, cpu_p95: 97.0, saturated_s: 35.0, ..Default::default() };
        let task = TaskResources { run: "9".into(), job: "core-tests".into(), task: "app:test".into(), started: 100, ended: 130, seconds: 30.0, cpu_s: 60.0, cores_used: 2.0, concurrent: 2, cpu_avg: Some(55.0), mem_peak_mb: None };
        let step = StepRow { run: "9".into(), job: "core-tests".into(), step_no: 3, step: "Set up job".into(), started: Some(10), completed: Some(14), duration_s: Some(4), conclusion: Some("success".into()) };
        let profile = RunProfile { run: "9".into(), workflow: "CI core".into(), wall_s: 600.0, runner_s: 1800.0, peak_jobs: 4, parallel_efficiency: 0.75, critical_path_job: "plan > core-tests".into(), ..Default::default() };
        let busy = ConcurrencyRow { ts: 5, run: "9".into(), phase: "start".into(), workflow: "all".into(), busy: 17, queued: 3, runs_active: 4, cap: 20, headroom: 3 };

        let first = (
            record_job_resources_local(std::slice::from_ref(&job)),
            record_task_resources_local(std::slice::from_ref(&task)),
            record_steps_local(std::slice::from_ref(&step)),
            record_run_profile_local(&profile),
            record_concurrency_local(std::slice::from_ref(&busy)),
        );
        record_job_resources_local(&[job]).unwrap();
        record_steps_local(&[step]).unwrap();

        std::env::remove_var("WF_CI_DB");
        let into = open_at(&sealed).unwrap();
        merge(&into, &db).unwrap();
        let count = |t: &str| into.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get::<_, i64>(0)).unwrap();
        let (p95, saturated, runner): (f64, f64, String) = into.query_row("SELECT CPU_P95, SATURATED_S, RUNNER FROM CI_JOB_RESOURCES", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        let (cores, mem): (f64, Option<f64>) = into.query_row("SELECT CORES_USED, MEM_PEAK_MB FROM CI_TASK_RESOURCES", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        let headroom: i64 = into.query_row("SELECT HEADROOM FROM CI_CONCURRENCY", [], |r| r.get(0)).unwrap();
        assert!(matches!(first, (Ok(true), Ok(true), Ok(true), Ok(true), Ok(true))));
        assert_eq!(["CI_JOB_RESOURCES", "CI_TASK_RESOURCES", "CI_STEPS", "CI_RUN_PROFILE", "CI_CONCURRENCY"].map(count), [1, 1, 1, 1, 1]);
        assert_eq!((p95, saturated, runner.as_str(), cores, mem, headroom), (97.0, 35.0, "GitHub Actions 7", 2.0, None, 3));
    }

    #[test]
    fn given_a_sealed_database_from_before_the_parallelism_columns_when_a_job_database_is_merged_then_jobs_and_tasks_gain_their_columns() {
        let dir = tempfile::tempdir().unwrap();
        let (old, job) = (dir.path().join("old.sqlite"), dir.path().join("job.sqlite"));
        let conn = Connection::open(&old).unwrap();
        conn.execute_batch(
            "CREATE TABLE CI_JOBS (RUN_ID TEXT NOT NULL, SHA TEXT NOT NULL, TS INTEGER NOT NULL, KIND TEXT NOT NULL, JOB TEXT NOT NULL, SECS INTEGER NOT NULL, QUEUED INTEGER NOT NULL, PRIMARY KEY (RUN_ID, JOB));
             INSERT INTO CI_JOBS VALUES ('1','a',1,'k','j',5,0);
             CREATE TABLE NX_TASKS (RUN_ID TEXT NOT NULL, SHA TEXT NOT NULL, TS INTEGER NOT NULL, JOB TEXT NOT NULL, TASK TEXT NOT NULL, HASH TEXT NOT NULL, OUTCOME TEXT NOT NULL, MS INTEGER NOT NULL, PASSED INTEGER NOT NULL, PRIMARY KEY (RUN_ID, TASK, HASH));
             INSERT INTO NX_TASKS VALUES ('1','a',1,'j','t','h','ran',5,1);",
        )
        .unwrap();
        let j = open_at(&job).unwrap();
        ensure_ledger(&j).unwrap();
        j.execute("INSERT INTO CI_JOBS (RUN_ID, SHA, TS, KIND, JOB, SECS, QUEUED, QUEUED_S, RUNNER) VALUES ('2','b',2,'k','j',7,1,4,'GitHub Actions 3')", []).unwrap();
        j.execute("INSERT INTO NX_TASKS (RUN_ID, SHA, TS, JOB, TASK, HASH, OUTCOME, MS, PASSED, STARTED_MS, ENDED_MS, CPU_S) VALUES ('2','b',2,'j','t','h2','ran',9000,1,1000,10000,18.0)", []).unwrap();
        drop(j);

        merge(&conn, &job).unwrap();

        let (n, queued_s, runner): (i64, i64, String) = conn.query_row("SELECT COUNT(*), MAX(QUEUED_S), MAX(RUNNER) FROM CI_JOBS", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        let tasks = tasks_of(&old).unwrap();
        assert_eq!((n, queued_s, runner.as_str()), (2, 4, "GitHub Actions 3"));
        assert_eq!(tasks.iter().map(|t| (t.started_ms, t.ended_ms, t.cpu_s)).collect::<Vec<_>>(), [(0, 0, 0.0), (1000, 10000, 18.0)]);
    }
}
