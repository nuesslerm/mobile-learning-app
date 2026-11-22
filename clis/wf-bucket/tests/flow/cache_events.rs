//! `wf cache event`: every restore and save of a CI cache layer is a row of CI_CACHE_EVENTS, and a failed one is shouted (an error annotation, a red summary line,
//! a gauge) without ever failing the job. The verb is entered through the thin `wf-bucket` binary's argv; the job's database is a temp file, the step summary and
//! `GITHUB_ENV` are temp files the runner would own, the metrics source a loopback double.
use crate::support::otlp::{gauges, Gauge};
use crate::support::{last_json, run_wf, MockServer, RunOpts, RunResult};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const KEY: &str = "nx-core-tests-1of3-Linux-X64-CI-core-4242-1";

/// A CI job: its own database, step summary and environment file, the lineage variables, and a metrics source.
struct Job {
    dir: tempfile::TempDir,
    source: MockServer,
}

impl Job {
    fn new() -> Job {
        Job { dir: tempfile::tempdir().unwrap(), source: MockServer::start(None) }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn home(&self) -> PathBuf {
        self.path("home")
    }

    fn event(&self, args: &[&str]) -> RunResult {
        let mut argv = vec!["cache", "event"];
        argv.extend(args);
        let opts = RunOpts::default()
            .env("WF_CI_DB", &self.path("rows.sqlite").display().to_string())
            .env("HOME", &self.home().display().to_string())
            .env("RUNNER_TEMP", &self.path("temp").display().to_string())
            .env("GITHUB_STEP_SUMMARY", &self.path("summary.md").display().to_string())
            .env("GITHUB_ENV", &self.path("github.env").display().to_string())
            .env("GITHUB_RUN_ID", "4242")
            .env("GITHUB_RUN_ATTEMPT", "1")
            .env("GITHUB_WORKFLOW", "CI core")
            .env("GITHUB_JOB", "core-tests")
            .env("WF_CI_METRICS_BASE", &format!("http://127.0.0.1:{}", self.source.port));
        run_wf(&argv, &opts)
    }

    fn rows(&self, sql: &str) -> Vec<Value> {
        let conn = Connection::open(self.path("rows.sqlite")).unwrap();
        let mut stmt = conn.prepare(sql).unwrap();
        let names: Vec<String> = stmt.column_names().iter().map(|n| n.to_string()).collect();
        stmt.query_map([], |r| {
            let mut row = serde_json::Map::new();
            for (i, name) in names.iter().enumerate() {
                row.insert(name.clone(), match r.get_ref(i)? {
                    rusqlite::types::ValueRef::Null => Value::Null,
                    rusqlite::types::ValueRef::Integer(n) => json!(n),
                    rusqlite::types::ValueRef::Real(f) => json!(f),
                    rusqlite::types::ValueRef::Text(t) => json!(String::from_utf8_lossy(t)),
                    rusqlite::types::ValueRef::Blob(_) => Value::Null,
                });
            }
            Ok(Value::Object(row))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
    }

    fn gauges(&self) -> Vec<Gauge> {
        gauges(&self.source.requests())
    }

    /// An Nx local cache as a restore leaves it: entries under `~/.nx/ws/cache`, and `cache_outputs` rows in a database beside it.
    fn nx_home(&self, entries: &[&str], db_hashes: &[&str]) {
        let (cache, databases) = (self.home().join(".nx/ws/cache"), self.home().join(".nx/ws/databases"));
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::create_dir_all(&databases).unwrap();
        for e in entries {
            std::fs::create_dir_all(cache.join(e)).unwrap();
            std::fs::write(cache.join(e).join("terminalOutput"), "x").unwrap();
        }
        let conn = Connection::open(databases.join("abc-v3.db")).unwrap();
        conn.execute_batch("CREATE TABLE cache_outputs (hash TEXT PRIMARY KEY, code INTEGER)").unwrap();
        for h in db_hashes {
            conn.execute("INSERT INTO cache_outputs VALUES (?1, 0)", [h]).unwrap();
        }
    }
}

fn has(seen: &[Gauge], metric: &str, scope: &str, name: &str) -> Option<f64> {
    seen.iter().find(|g| g.metric == metric && g.scope == scope && g.name == name).map(|g| g.value)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

#[test]
fn given_a_restore_that_failed_when_it_is_recorded_then_the_job_stays_green_and_the_failure_is_an_error_annotation_a_red_summary_line_a_row_and_a_gauge() {
    // Given a CI job whose Nx cache restore step failed with the cache service's own message
    let job = Job::new();

    // When the failure is recorded
    let r = job.event(&["nx", "restore", "--outcome", "error", "--key", KEY, "--stage", "core-tests-1of3", "--reason", "Failed to restore: HTTP 503 from the cache service", "--duration-ms", "2100"]);

    // Then the verb does not fail the job, and prints the error annotation with the layer, the key and the reason before its JSON
    assert_eq!(r.exit_code, 0, "{r:?}");
    let annotation = format!("::error title=cache restore failed::layer=nx key={KEY} reason=Failed to restore: HTTP 503 from the cache service");
    assert_eq!(r.stdout.lines().next(), Some(annotation.as_str()), "{r:?}");
    let out = last_json(&r);
    assert_eq!((out["outcome"].clone(), out["problem"].clone(), out["recorded"].clone()), (json!("error"), json!("restore-error"), json!(true)), "{out}");

    // And the database holds one row with the lineage, the key, the outcome and the reason
    let rows = job.rows("SELECT RUN_ID, ATTEMPT, WORKFLOW, JOB, STAGE, SHARD, LAYER, PHASE, KEY_REQUESTED, KEY_MATCHED, MATCH_KIND, OUTCOME, REASON, DURATION_MS FROM CI_CACHE_EVENTS");
    assert_eq!(
        rows,
        vec![json!({ "RUN_ID": "4242", "ATTEMPT": "1", "WORKFLOW": "CI core", "JOB": "core-tests", "STAGE": "core-tests", "SHARD": "1of3", "LAYER": "nx", "PHASE": "restore", "KEY_REQUESTED": KEY, "KEY_MATCHED": null, "MATCH_KIND": "none", "OUTCOME": "error", "REASON": "Failed to restore: HTTP 503 from the cache service", "DURATION_MS": 2100 })]
    );

    // And the step summary has a red line, the log line is a warn event with the reason, and the source got the problem gauge
    assert!(read(&job.path("summary.md")).contains("🔴 restore failed: **nx** (core-tests) Failed to restore: HTTP 503"), "{}", read(&job.path("summary.md")));
    let line = r.stderr.lines().find(|l| l.contains("source=ci-cache") && l.contains("msg=\"cache.event\"")).unwrap_or_else(|| panic!("no cache.event line: {r:?}"));
    assert!(line.contains("level=warn") && line.contains("outcome=error") && line.contains("HTTP 503"), "{line}");
    let seen = job.gauges();
    assert_eq!(has(&seen, "ci.cache.problems", "nx", "restore-error"), Some(1.0), "{seen:?}");
    assert_eq!(has(&seen, "ci.cache.events", "nx", "restore-error"), Some(1.0), "{seen:?}");
}

#[test]
fn given_a_restore_cancelled_by_its_time_limit_when_it_is_recorded_then_it_is_a_timeout_with_the_reason_text() {
    // Given a restore step that ended cancelled, with no message of its own
    let job = Job::new();

    // When it is recorded as a timeout
    let r = job.event(&["sccache-flow-tests", "restore", "--outcome", "timeout", "--key", "sccache-flow-tests-Linux-X64-2026-W47-abc", "--started-ms", "1"]);

    // Then the annotation is an error naming the layer, the row says timeout with a reason that says why, and the stage is split from the layer
    assert_eq!(r.exit_code, 0, "{r:?}");
    assert!(r.stdout.lines().next().unwrap().starts_with("::error title=cache restore failed::layer=sccache key=sccache-flow-tests-Linux-X64-2026-W47-abc reason=the cache step was cancelled"), "{r:?}");
    let rows = job.rows("SELECT LAYER, STAGE, SHARD, OUTCOME, REASON, DURATION_MS > 0 AS timed FROM CI_CACHE_EVENTS");
    assert_eq!((rows[0]["LAYER"].clone(), rows[0]["STAGE"].clone(), rows[0]["OUTCOME"].clone(), rows[0]["timed"].clone()), (json!("sccache"), json!("flow-tests"), json!("timeout"), json!(1)), "{rows:?}");
    assert!(rows[0]["REASON"].as_str().unwrap().contains("time limit"), "{rows:?}");
    assert_eq!(has(&job.gauges(), "ci.cache.problems", "sccache", "restore-timeout"), Some(1.0));
}

#[test]
fn given_a_restore_that_matched_a_key_when_it_is_recorded_then_nx_s_directory_and_table_are_snapshotted_and_exported_to_the_job() {
    // Given a restore that matched a restore key and left two entries in Nx's cache directory and two rows in its table
    let job = Job::new();
    job.nx_home(&["h1", "h2"], &["h1", "h2"]);

    // When it is recorded
    let r = job.event(&["nx", "restore", "--outcome", "ok", "--hit", "false", "--key", KEY, "--matched-key", "nx-core-tests-1of3-Linux-X64-CI-core-4000-1", "--stage", "core-tests-1of3"]);

    // Then there is no annotation, and the row is a restore-key match with its entries and rows
    assert_eq!(r.exit_code, 0, "{r:?}");
    assert!(!r.stdout.contains("::error") && !r.stdout.contains("::warning"), "{r:?}");
    let rows = job.rows("SELECT MATCH_KIND, KEY_MATCHED, OUTCOME, ENTRIES, DB_ROWS, SIZE_BYTES > 0 AS sized FROM CI_CACHE_EVENTS");
    assert_eq!(rows, vec![json!({ "MATCH_KIND": "restore-key", "KEY_MATCHED": "nx-core-tests-1of3-Linux-X64-CI-core-4000-1", "OUTCOME": "ok", "ENTRIES": 2, "DB_ROWS": 2, "sized": 1 })]);

    // And the snapshot holds the hashes of both, and the job's environment names it, its stage and the matched key
    let snapshot: Value = serde_json::from_str(&read(&job.path("temp/nx-cache.restore.json"))).unwrap();
    assert_eq!((snapshot["entries"].clone(), snapshot["dbHashes"].clone(), snapshot["outcome"].clone()), (json!(["h1", "h2"]), json!(["h1", "h2"]), json!("ok")), "{snapshot}");
    let env = read(&job.path("github.env"));
    assert!(env.contains(&format!("WF_CACHE_RESTORE_FILE={}", job.path("temp/nx-cache.restore.json").display())), "{env}");
    assert!(env.contains("WF_CACHE_STAGE=core-tests-1of3") && env.contains("WF_CACHE_RESTORED_KEY=nx-core-tests-1of3-Linux-X64-CI-core-4000-1"), "{env}");
    assert_eq!(has(&job.gauges(), "ci.cache.events", "nx", "restore-ok"), Some(1.0));
}

#[test]
fn given_a_restore_that_matched_but_restored_no_entries_when_it_is_recorded_then_it_is_an_error() {
    // Given a restore that matched an entry and left Nx's cache directory empty
    let job = Job::new();
    job.nx_home(&[], &[]);

    // When it is recorded as ok
    let r = job.event(&["nx", "restore", "--outcome", "ok", "--key", KEY, "--matched-key", "nx-core-tests-1of3-Linux-X64-CI-core-4000-1", "--stage", "core-tests-1of3"]);

    // Then the verb turns it into an error that says what happened, shouted like any failed restore
    assert!(r.stdout.lines().next().unwrap().contains("::error title=cache restore failed::layer=nx key="), "{r:?}");
    assert!(r.stdout.lines().next().unwrap().contains("matched nx-core-tests-1of3-Linux-X64-CI-core-4000-1 and restored 0 entries"), "{r:?}");
    assert_eq!(last_json(&r)["problem"], "restore-empty");
    assert_eq!(job.rows("SELECT OUTCOME FROM CI_CACHE_EVENTS"), vec![json!({ "OUTCOME": "error" })]);
}

#[test]
fn given_a_save_skipped_while_tasks_ran_and_passed_when_it_is_recorded_then_it_warns_that_their_results_are_lost() {
    // Given a job whose database holds two tasks that ran and passed and one that replayed
    let job = Job::new();
    let conn = wf_bucket::ci_db::open_ledger(&job.path("rows.sqlite")).unwrap();
    for (task, outcome) in [("a:test", "ran"), ("b:test", "ran"), ("c:test", "hit")] {
        conn.execute("INSERT INTO NX_TASKS (RUN_ID, SHA, TS, JOB, TASK, HASH, OUTCOME, MS, PASSED) VALUES ('4242', 's', 1, 'core-tests', ?1, 'h', ?2, 5, 1)", [task, outcome]).unwrap();
    }
    drop(conn);

    // When the save decision "no new entries" is recorded
    let r = job.event(&["nx", "save", "--outcome", "skipped", "--key", KEY, "--stage", "core-tests-1of3", "--reason", "no new task results (809 restored, 809 now)", "--entries", "809"]);

    // Then it is a warning that names the layer, the reason and the number of tasks left behind
    let first = r.stdout.lines().next().unwrap();
    assert_eq!(first, format!("::warning title=cache save skipped::layer=nx key={KEY} reason=no new task results (809 restored, 809 now); 2 task(s) ran and passed, and their results are not saved"), "{r:?}");
    assert_eq!(last_json(&r)["problem"], "save-skipped-tasks-ran");
    assert_eq!(has(&job.gauges(), "ci.cache.problems", "nx", "save-skipped-tasks-ran"), Some(1.0));
    assert!(read(&job.path("summary.md")).contains("🟠 save skipped: **nx** (core-tests)"));
}

#[test]
fn given_a_save_that_failed_when_it_is_recorded_then_the_annotation_says_save() {
    // Given a save step that failed
    let job = Job::new();

    // When it is recorded
    let r = job.event(&["pnpm", "save", "--outcome", "error", "--key", "pnpm-Linux-X64-abc", "--reason", "Unable to reserve cache with key pnpm-Linux-X64-abc"]);

    // Then the annotation is an error titled cache save failed
    assert_eq!(r.stdout.lines().next(), Some("::error title=cache save failed::layer=pnpm key=pnpm-Linux-X64-abc reason=Unable to reserve cache with key pnpm-Linux-X64-abc"), "{r:?}");
    assert_eq!(has(&job.gauges(), "ci.cache.problems", "pnpm", "save-error"), Some(1.0));
}

#[test]
fn given_a_clean_miss_when_it_is_recorded_then_it_is_a_row_and_nothing_is_shouted() {
    // Given a restore that found no key at all
    let job = Job::new();

    // When it is recorded
    let r = job.event(&["metro-app-web", "restore", "--outcome", "miss", "--key", "metro-app-web-Linux-X64-2026-W47-abc"]);

    // Then the row is a miss with no match, and no annotation or summary line was written
    assert!(!r.stdout.contains("::"), "{r:?}");
    assert_eq!(job.rows("SELECT LAYER, STAGE, MATCH_KIND, OUTCOME, REASON FROM CI_CACHE_EVENTS"), vec![json!({ "LAYER": "metro", "STAGE": "app-web", "MATCH_KIND": "none", "OUTCOME": "miss", "REASON": null })]);
    assert_eq!(read(&job.path("summary.md")), "");
}

#[test]
fn given_no_job_database_when_an_event_is_recorded_then_nothing_is_kept_and_the_failure_is_still_shouted() {
    // Given a run outside a CI job (the kit's default WF_CI_DB=off)
    // When a failed restore is recorded
    let r = run_wf(&["cache", "event", "nx", "restore", "--outcome", "error", "--reason", "boom"], &RunOpts::default());

    // Then the exit is 0, nothing was recorded, and the annotation is printed all the same
    assert_eq!(r.exit_code, 0, "{r:?}");
    assert_eq!(last_json(&r)["recorded"], json!(false));
    assert!(r.stdout.starts_with("::error title=cache restore failed::layer=nx key=none reason=boom"), "{r:?}");
}

#[test]
fn given_an_outcome_the_table_does_not_know_when_it_is_recorded_then_the_command_line_is_refused() {
    // Given an outcome that is not one of ok, miss, timeout, error or skipped
    // When it is recorded
    let r = run_wf(&["cache", "event", "nx", "restore", "--outcome", "great"], &RunOpts::default());

    // Then the only non-zero exit of the verb says what is accepted
    assert_eq!(r.exit_code, 1, "{r:?}");
    assert!(last_json(&r)["error"].as_str().unwrap().contains("ok, miss, timeout, error, skipped"), "{r:?}");
}
