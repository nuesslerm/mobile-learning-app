//! The ledger of what CI did, kept across runs: every Nx task with its input hash and what became of it (`CI_TASKS`), and every job's time
//! (`CI_JOBS`). `wf ci audit` judges runs against it: a task that ran although the same hash was already produced is a miss that
//! should have been a hit, and a task's time under one hash is comparable run to run whatever the cache did.
//!
//! Two stores behind one port: HEALTH_RPT in the Always Free Autonomous Database through ORDS's SQL endpoint (the CI's), and a JSON
//! lines file (`WF_CI_LEDGER_FILE`) for replaying a history offline.
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// One Nx task of one run.
#[derive(Debug, Clone, PartialEq)]
pub struct Observed {
    pub run: String,
    pub sha: String,
    pub ts: i64,
    pub job: String,
    pub task: String,
    pub hash: String,
    /// `ran` or `hit`.
    pub outcome: String,
    pub ms: i64,
    pub passed: bool,
}

/// One job of one run.
#[derive(Debug, Clone, PartialEq)]
pub struct JobRow {
    pub run: String,
    pub sha: String,
    pub ts: i64,
    /// `push` or `cold` (a forced run, where nothing is replayed).
    pub kind: String,
    pub job: String,
    pub secs: i64,
    pub queued: i64,
}

/// The sha and the task hashes of one run.
pub type RunHashes = (String, BTreeMap<String, String>);

pub trait Ledger {
    fn ensure(&self) -> Result<(), String>;
    fn record_tasks(&self, rows: &[Observed]) -> Result<(), String>;
    fn record_jobs(&self, rows: &[JobRow]) -> Result<(), String>;
    /// Earlier runs' rows (not `run`'s) for these hashes.
    fn priors(&self, hashes: &[String], run: &str) -> Result<Vec<Observed>, String>;
    /// The tasks among `tasks` that were replayed from a cache at least once: proof that a task can be cached at all.
    fn ever_hit(&self, tasks: &[String]) -> Result<BTreeSet<String>, String>;
    /// The sha and the task hashes of the newest run before `run`.
    fn previous(&self, run: &str) -> Result<Option<RunHashes>, String>;
    /// Each job's seconds in the newest `runs` cold runs, newest first.
    fn cold_jobs(&self, runs: usize) -> Result<BTreeMap<String, Vec<i64>>, String>;
}

/// The store this environment names: a file when `WF_CI_LEDGER_FILE` is set, else the database.
pub fn open() -> Result<Box<dyn Ledger>, String> {
    if let Some(path) = crate::ci_comment::var("WF_CI_LEDGER_FILE") {
        return Ok(Box::new(FileLedger { path: path.into() }));
    }
    Ok(Box::new(Ords::from_env()?))
}

// ---- file ----

pub struct FileLedger {
    pub path: std::path::PathBuf,
}

impl FileLedger {
    fn lines(&self) -> Vec<Value> {
        std::fs::read_to_string(&self.path).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
    }

    fn append(&self, lines: Vec<Value>) -> Result<(), String> {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&self.path).map_err(|e| format!("{}: {e}", self.path.display()))?;
        lines.iter().try_for_each(|l| writeln!(file, "{l}")).map_err(|e| e.to_string())
    }

    fn tasks(&self) -> Vec<Observed> {
        self.lines().iter().filter(|l| l["t"] == "task").filter_map(observed_of).collect()
    }
}

fn observed_of(v: &Value) -> Option<Observed> {
    let s = |k: &str| v[k].as_str().map(str::to_string);
    Some(Observed { run: s("run")?, sha: s("sha")?, ts: v["ts"].as_i64()?, job: s("job")?, task: s("task")?, hash: s("hash")?, outcome: s("outcome")?, ms: v["ms"].as_i64()?, passed: v["passed"].as_bool()? })
}

impl Ledger for FileLedger {
    fn ensure(&self) -> Result<(), String> {
        Ok(())
    }

    fn record_tasks(&self, rows: &[Observed]) -> Result<(), String> {
        self.append(rows.iter().map(|o| json!({ "t": "task", "run": o.run, "sha": o.sha, "ts": o.ts, "job": o.job, "task": o.task, "hash": o.hash, "outcome": o.outcome, "ms": o.ms, "passed": o.passed })).collect())
    }

    fn record_jobs(&self, rows: &[JobRow]) -> Result<(), String> {
        self.append(rows.iter().map(|j| json!({ "t": "job", "run": j.run, "sha": j.sha, "ts": j.ts, "kind": j.kind, "job": j.job, "secs": j.secs, "queued": j.queued })).collect())
    }

    fn priors(&self, hashes: &[String], run: &str) -> Result<Vec<Observed>, String> {
        Ok(self.tasks().into_iter().filter(|o| o.run != run && hashes.contains(&o.hash)).collect())
    }

    fn ever_hit(&self, tasks: &[String]) -> Result<BTreeSet<String>, String> {
        Ok(self.tasks().into_iter().filter(|o| o.outcome == "hit" && tasks.contains(&o.task)).map(|o| o.task).collect())
    }

    fn previous(&self, run: &str) -> Result<Option<RunHashes>, String> {
        let rows: Vec<Observed> = self.tasks().into_iter().filter(|o| o.run != run).collect();
        let Some(newest) = rows.iter().map(|o| o.ts).max() else { return Ok(None) };
        let last: Vec<&Observed> = rows.iter().filter(|o| o.ts == newest).collect();
        Ok(Some((last[0].sha.clone(), last.iter().map(|o| (o.task.clone(), o.hash.clone())).collect())))
    }

    fn cold_jobs(&self, runs: usize) -> Result<BTreeMap<String, Vec<i64>>, String> {
        let jobs: Vec<Value> = self.lines().into_iter().filter(|l| l["t"] == "job" && l["kind"] == "cold").collect();
        Ok(newest_runs(&jobs, runs, |j| (j["run"].as_str().unwrap_or("").to_string(), j["ts"].as_i64().unwrap_or(0), j["job"].as_str().unwrap_or("").to_string(), j["secs"].as_i64().unwrap_or(0))))
    }
}

/// Per job, the seconds of the newest `runs` runs, newest first.
fn newest_runs(rows: &[Value], runs: usize, pick: impl Fn(&Value) -> (String, i64, String, i64)) -> BTreeMap<String, Vec<i64>> {
    let picked: Vec<(String, i64, String, i64)> = rows.iter().map(pick).collect();
    let mut order: Vec<(i64, String)> = picked.iter().map(|(run, ts, _, _)| (*ts, run.clone())).collect();
    order.sort_unstable();
    order.dedup();
    let keep: BTreeSet<String> = order.into_iter().rev().take(runs).map(|(_, run)| run).collect();
    let mut out: BTreeMap<String, Vec<(i64, i64)>> = BTreeMap::new();
    for (run, ts, job, secs) in picked.into_iter().filter(|(run, ..)| keep.contains(run)) {
        let _ = run;
        out.entry(job).or_default().push((ts, secs));
    }
    out.into_iter()
        .map(|(job, mut v)| {
            v.sort_unstable_by_key(|(ts, _)| std::cmp::Reverse(*ts));
            (job, v.into_iter().map(|(_, secs)| secs).collect())
        })
        .collect()
}

// ---- database ----

pub const SCHEMA: [&str; 2] = [
    "CREATE TABLE IF NOT EXISTS CI_TASKS (
       RUN_ID VARCHAR2(40) NOT NULL, SHA VARCHAR2(40) NOT NULL, TS NUMBER(19) NOT NULL, JOB VARCHAR2(200) NOT NULL, TASK VARCHAR2(300) NOT NULL,
       HASH VARCHAR2(40) NOT NULL, OUTCOME VARCHAR2(8) NOT NULL, MS NUMBER(19) NOT NULL, PASSED NUMBER(1) NOT NULL,
       CONSTRAINT CI_TASKS_PK PRIMARY KEY (RUN_ID, TASK, HASH)
     )",
    "CREATE TABLE IF NOT EXISTS CI_JOBS (
       RUN_ID VARCHAR2(40) NOT NULL, SHA VARCHAR2(40) NOT NULL, TS NUMBER(19) NOT NULL, KIND VARCHAR2(8) NOT NULL, JOB VARCHAR2(200) NOT NULL,
       SECS NUMBER(19) NOT NULL, QUEUED NUMBER(19) NOT NULL,
       CONSTRAINT CI_JOBS_PK PRIMARY KEY (RUN_ID, JOB)
     )",
];

/// The SQL endpoint of the schema an auto-REST URL such as `https://host/ords/health/health_reports/` belongs to.
pub fn sql_url(base_url: &str) -> Option<String> {
    let (host, rest) = base_url.split_once("/ords/")?;
    let alias = rest.split('/').next().filter(|a| !a.is_empty())?;
    Some(format!("{host}/ords/{alias}/_/sql"))
}

/// A value as a SQL string literal: quotes doubled, and `&` replaced, which the SQL endpoint would read as a substitution variable.
pub fn literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''").replace('&', "_"))
}

fn in_list(values: &[String]) -> String {
    values.iter().map(|v| literal(v)).collect::<Vec<_>>().join(",")
}

/// One `MERGE` that inserts the rows a run has not stored yet (a task and hash is stored once per run).
pub fn merge_tasks(rows: &[Observed]) -> Option<String> {
    let mut seen = BTreeSet::new();
    let selects: Vec<String> = rows
        .iter()
        .filter(|o| seen.insert((o.run.clone(), o.task.clone(), o.hash.clone())))
        .map(|o| format!("SELECT {} RUN_ID, {} SHA, {} TS, {} JOB, {} TASK, {} HASH, {} OUTCOME, {} MS, {} PASSED FROM DUAL", literal(&o.run), literal(&o.sha), o.ts, literal(&o.job), literal(&o.task), literal(&o.hash), literal(&o.outcome), o.ms, i32::from(o.passed)))
        .collect();
    (!selects.is_empty()).then(|| {
        format!(
            "MERGE INTO CI_TASKS t USING ({}) s ON (t.RUN_ID = s.RUN_ID AND t.TASK = s.TASK AND t.HASH = s.HASH) WHEN NOT MATCHED THEN INSERT (RUN_ID, SHA, TS, JOB, TASK, HASH, OUTCOME, MS, PASSED) VALUES (s.RUN_ID, s.SHA, s.TS, s.JOB, s.TASK, s.HASH, s.OUTCOME, s.MS, s.PASSED)",
            selects.join(" UNION ALL ")
        )
    })
}

pub fn merge_jobs(rows: &[JobRow]) -> Option<String> {
    let selects: Vec<String> = rows.iter().map(|j| format!("SELECT {} RUN_ID, {} SHA, {} TS, {} KIND, {} JOB, {} SECS, {} QUEUED FROM DUAL", literal(&j.run), literal(&j.sha), j.ts, literal(&j.kind), literal(&j.job), j.secs, j.queued)).collect();
    (!selects.is_empty()).then(|| format!("MERGE INTO CI_JOBS t USING ({}) s ON (t.RUN_ID = s.RUN_ID AND t.JOB = s.JOB) WHEN NOT MATCHED THEN INSERT (RUN_ID, SHA, TS, KIND, JOB, SECS, QUEUED) VALUES (s.RUN_ID, s.SHA, s.TS, s.KIND, s.JOB, s.SECS, s.QUEUED)", selects.join(" UNION ALL ")))
}

/// The rows of one ORDS SQL answer; a failure is told by `errorDetails` alone (`result` is the row count of a DML).
pub fn rows_of(reply: &Value) -> Result<Vec<Value>, String> {
    let item = reply["items"].get(0).ok_or("ords: the reply has no items")?;
    if item.get("errorDetails").is_some() {
        let text = item["errorDetails"].as_str().or_else(|| item["response"].get(0).and_then(Value::as_str)).unwrap_or("the statement failed");
        return Err(format!("ords: {}", text.trim().lines().next().unwrap_or("the statement failed")));
    }
    Ok(item["resultSet"]["items"].as_array().cloned().unwrap_or_default())
}

pub struct Ords {
    url: String,
    password: String,
}

impl Ords {
    pub fn new(base_url: &str, password: &str) -> Result<Ords, String> {
        Ok(Ords { url: sql_url(base_url).ok_or_else(|| format!("not an ORDS auto-REST url: {base_url}"))?, password: password.to_string() })
    }

    /// From `HEALTH_REPORTS_ORDS_BASE_URL` and `HEALTH_REPORTS_ORDS_PASSWORD`: the environment, then the vault.
    pub fn from_env() -> Result<Ords, String> {
        let values = secrets::load_values(&["HEALTH_REPORTS_ORDS_BASE_URL", "HEALTH_REPORTS_ORDS_PASSWORD"]);
        let get = |key: &str| values.get(key).cloned().flatten().ok_or_else(|| format!("{key} is not set and the vault has none"));
        Ords::new(&get("HEALTH_REPORTS_ORDS_BASE_URL")?, &get("HEALTH_REPORTS_ORDS_PASSWORD")?)
    }

    pub fn query(&self, statement: &str) -> Result<Vec<Value>, String> {
        let response = cli_http::Request::post(&self.url)
            .basic_auth("health_rpt", &self.password)
            .header("Content-Type", "application/sql")
            .header("Accept", "application/json")
            .body(statement.as_bytes().to_vec())
            .timeout(std::time::Duration::from_secs(60))
            .send()?;
        if !response.ok() {
            return Err(format!("ords answered {}", response.status));
        }
        rows_of(&serde_json::from_str(&response.text()).map_err(|e| format!("ords: not JSON ({e})"))?)
    }
}

fn int(v: &Value, key: &str) -> i64 {
    v[key].as_i64().or_else(|| v[key].as_f64().map(|f| f as i64)).unwrap_or(0)
}

fn text_of(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or_default().to_string()
}

impl Ledger for Ords {
    fn ensure(&self) -> Result<(), String> {
        SCHEMA.iter().try_for_each(|s| self.query(s).map(|_| ()))
    }

    fn record_tasks(&self, rows: &[Observed]) -> Result<(), String> {
        rows.chunks(100).filter_map(merge_tasks).try_for_each(|s| self.query(&s).map(|_| ()))
    }

    fn record_jobs(&self, rows: &[JobRow]) -> Result<(), String> {
        rows.chunks(100).filter_map(merge_jobs).try_for_each(|s| self.query(&s).map(|_| ()))
    }

    fn priors(&self, hashes: &[String], run: &str) -> Result<Vec<Observed>, String> {
        let mut out = Vec::new();
        for chunk in hashes.chunks(200) {
            let sql = format!("SELECT RUN_ID, SHA, TS, JOB, TASK, HASH, OUTCOME, MS, PASSED FROM CI_TASKS WHERE RUN_ID <> {} AND HASH IN ({})", literal(run), in_list(chunk));
            out.extend(self.query(&sql)?.iter().map(|r| Observed { run: text_of(r, "run_id"), sha: text_of(r, "sha"), ts: int(r, "ts"), job: text_of(r, "job"), task: text_of(r, "task"), hash: text_of(r, "hash"), outcome: text_of(r, "outcome"), ms: int(r, "ms"), passed: int(r, "passed") == 1 }));
        }
        Ok(out)
    }

    fn ever_hit(&self, tasks: &[String]) -> Result<BTreeSet<String>, String> {
        let mut out = BTreeSet::new();
        for chunk in tasks.chunks(200) {
            let sql = format!("SELECT DISTINCT TASK FROM CI_TASKS WHERE OUTCOME = 'hit' AND TASK IN ({})", in_list(chunk));
            out.extend(self.query(&sql)?.iter().map(|r| text_of(r, "task")));
        }
        Ok(out)
    }

    fn previous(&self, run: &str) -> Result<Option<RunHashes>, String> {
        let sql = format!("SELECT SHA, TASK, HASH FROM CI_TASKS WHERE TS = (SELECT MAX(TS) FROM CI_TASKS WHERE RUN_ID <> {})", literal(run));
        let rows = self.query(&sql)?;
        Ok(rows.first().map(|r| (text_of(r, "sha"), rows.iter().map(|r| (text_of(r, "task"), text_of(r, "hash"))).collect())))
    }

    fn cold_jobs(&self, runs: usize) -> Result<BTreeMap<String, Vec<i64>>, String> {
        let sql = format!("SELECT RUN_ID, TS, JOB, SECS FROM CI_JOBS WHERE KIND = 'cold' AND TS IN (SELECT TS FROM (SELECT DISTINCT TS FROM CI_JOBS WHERE KIND = 'cold' ORDER BY TS DESC) WHERE ROWNUM <= {runs})");
        let rows = self.query(&sql)?;
        Ok(newest_runs(&rows, runs, |r| (text_of(r, "run_id"), int(r, "ts"), text_of(r, "job"), int(r, "secs"))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(run: &str, task: &str, hash: &str, outcome: &str, ts: i64) -> Observed {
        Observed { run: run.into(), sha: format!("sha-{run}"), ts, job: "flow".into(), task: task.into(), hash: hash.into(), outcome: outcome.into(), ms: 1000, passed: true }
    }

    #[test]
    fn given_rows_when_stored_in_a_file_then_priors_previous_and_proven_cacheable_tasks_come_back() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = FileLedger { path: dir.path().join("l.jsonl") };
        ledger.record_tasks(&[row("1", "a:lint", "h1", "ran", 10), row("2", "a:lint", "h1", "hit", 20), row("2", "b:lint", "h2", "ran", 20)]).unwrap();
        assert_eq!(ledger.priors(&["h1".into()], "3").unwrap().len(), 2);
        assert_eq!(ledger.priors(&["h1".into()], "1").unwrap().len(), 1);
        assert_eq!(ledger.ever_hit(&["a:lint".into(), "b:lint".into()]).unwrap().into_iter().collect::<Vec<_>>(), ["a:lint"]);
        let (sha, hashes) = ledger.previous("3").unwrap().unwrap();
        assert_eq!((sha.as_str(), hashes.len()), ("sha-2", 2));
        assert!(ledger.previous("9").unwrap().is_some() && FileLedger { path: dir.path().join("none") }.previous("1").unwrap().is_none());
    }

    #[test]
    fn given_cold_runs_when_asked_then_each_job_has_the_seconds_of_the_newest_runs_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = FileLedger { path: dir.path().join("l.jsonl") };
        let job = |run: &str, ts: i64, secs: i64| JobRow { run: run.into(), sha: "s".into(), ts, kind: "cold".into(), job: "flow".into(), secs, queued: 0 };
        ledger.record_jobs(&[job("1", 1, 100), job("2", 2, 200), job("3", 3, 300)]).unwrap();
        assert_eq!(ledger.cold_jobs(2).unwrap()["flow"], [300, 200]);
    }

    #[test]
    fn given_rows_and_replies_when_turned_into_sql_then_values_are_quoted_and_a_failure_is_told_by_its_error_alone() {
        let merged = merge_tasks(&[row("1", "it's:lint", "h&1", "ran", 5), row("1", "it's:lint", "h&1", "ran", 5)]).unwrap();
        assert!(merged.contains("'it''s:lint'") && merged.contains("'h_1'") && merged.matches("UNION ALL").count() == 0, "{merged}");
        assert_eq!(sql_url("https://h/ords/health/health_reports/").as_deref(), Some("https://h/ords/health/_/sql"));
        assert!(rows_of(&json!({ "items": [{ "result": 37, "response": ["37 rows"] }] })).unwrap().is_empty());
        assert!(rows_of(&json!({ "items": [{ "result": 1, "errorDetails": "\nORA-00942: no table\nmore" }] })).unwrap_err().contains("ORA-00942"));
    }

    /// Against the real database (`cargo test -p pipeline --lib live_ledger -- --ignored`, with the vault readable): a round trip of every
    /// call, then its own rows removed.
    #[test]
    #[ignore = "talks to the real autonomous database"]
    fn live_ledger_round_trips_through_ords() {
        let ords = Ords::from_env().expect("HEALTH_REPORTS_ORDS_* readable");
        ords.ensure().unwrap();
        let run = format!("live-{}", wf_kit::time::now_ms());
        let task = format!("live:{run}");
        let obs = |run: &str, outcome: &str, ts: i64| Observed { run: run.into(), sha: "live".into(), ts, job: "j".into(), task: task.clone(), hash: format!("h{run}"), outcome: outcome.into(), ms: 1500, passed: true };
        let (older, newer) = (format!("{run}a"), format!("{run}b"));
        ords.record_tasks(&[obs(&older, "hit", 10_000_000_000_000), obs(&newer, "ran", 10_000_000_000_001)]).unwrap();
        ords.record_tasks(&[obs(&newer, "ran", 10_000_000_000_001)]).unwrap();
        assert_eq!(ords.priors(&[format!("h{older}")], &newer).unwrap().len(), 1);
        assert!(ords.ever_hit(std::slice::from_ref(&task)).unwrap().contains(&task));
        let (sha, hashes) = ords.previous(&older).unwrap().unwrap();
        assert_eq!((sha.as_str(), hashes.get(&task).map(String::as_str)), ("live", Some(format!("h{newer}").as_str())));
        let job = JobRow { run: older.clone(), sha: "live".into(), ts: 10_000_000_000_000, kind: "cold".into(), job: task.clone(), secs: 77, queued: 0 };
        ords.record_jobs(&[job]).unwrap();
        assert_eq!(ords.cold_jobs(4).unwrap().get(&task).cloned(), Some(vec![77]));
        ords.query(&format!("DELETE FROM CI_TASKS WHERE TASK = {}", literal(&task))).unwrap();
        ords.query(&format!("DELETE FROM CI_JOBS WHERE JOB = {}", literal(&task))).unwrap();
    }
}
