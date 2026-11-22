//! Why a task ran instead of replaying: the reason of every miss of a run, decided in the report job from the jobs' databases laid over the sealed history
//! (`wf ci cache-misses`), stored by the merge (`wf ci db merge`), and the lineage a job records for its tasks. The databases are real SQLite files with the
//! rows the jobs would have written; the sealed file is made by the merge verb itself.
use crate::support::{last_json, run_wf, RunOpts};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use wf_bucket::ci_db::{open_ledger, Connection};

pub(crate) const KEY: &str = "0707070707070707070707070707070707070707070707070707070707070707";
const WORKFLOW: &str = "Pipeline";

/// The CI database of two runs: `history`, finished and sealed, and `current`, the run being reported, as a job's own file in the downloaded artifacts.
pub(crate) struct Fixture {
    pub dir: tempfile::TempDir,
    pub current: String,
}

#[allow(clippy::too_many_arguments)]
fn task(c: &Connection, run: &str, ts: i64, job: &str, task: &str, hash: &str, outcome: &str, workflow: &str, stage: &str, facts: (Option<i64>, Option<i64>)) {
    c.execute(
        "INSERT INTO NX_TASKS (RUN_ID, SHA, TS, JOB, TASK, HASH, OUTCOME, MS, PASSED, WORKFLOW, CACHE_STAGE, DIR_HAS_HASH, DB_HAS_HASH) VALUES (?1, 's', ?2, ?3, ?4, ?5, ?6, 10000, 1, ?7, ?8, ?9, ?10)",
        rusqlite::params![run, ts, job, task, hash, outcome, workflow, stage, facts.0, facts.1],
    )
    .unwrap();
}

#[allow(clippy::too_many_arguments)]
fn event(c: &Connection, run: &str, ts: i64, workflow: &str, stage: &str, shard: &str, phase: &str, key: &str, matched: Option<&str>, outcome: &str, reason: Option<&str>) {
    c.execute(
        "INSERT INTO CI_CACHE_EVENTS (RUN_ID, TS, ATTEMPT, WORKFLOW, JOB, STAGE, SHARD, LAYER, PHASE, KEY_REQUESTED, KEY_MATCHED, MATCH_KIND, OUTCOME, REASON) VALUES (?1, ?2, '1', ?3, 'core-tests', ?4, ?5, 'nx', ?6, ?7, ?8, ?9, ?10, ?11)",
        rusqlite::params![run, ts, workflow, stage, shard, phase, key, matched, if matched.is_some() { "restore-key" } else { "none" }, outcome, reason],
    )
    .unwrap();
}

fn key_of(stage: &str, run: &str) -> String {
    format!("nx-{stage}-Linux-X64-{WORKFLOW}-{run}-1")
}

impl Fixture {
    /// Run `history` executed eight tasks and saved (or did not) the way each case below needs; run `current` ran them again.
    pub(crate) fn new(history: &str, current: &str) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let (h, c) = (history, current);
        let old = open_ledger(&dir.path().join("history.sqlite")).unwrap();
        for (job, name, hash, workflow, stage) in [
            ("core-tests", "a:flow", "h1", WORKFLOW, "core-tests-1of3"),
            ("core-tests", "b:flow", "h2", WORKFLOW, "core-tests-1of3"),
            ("core-tests", "c:flow", "h3", WORKFLOW, "core-tests-2of3"),
            ("core-tests", "d:flow", "h4", "Other", "core-tests-1of3"),
            ("integration-tests", "i:replay", "h8", WORKFLOW, "integration-tests-2of4"),
            ("core-tests", "z:flow", "h9", WORKFLOW, "core-tests-1of3"),
        ] {
            task(&old, h, 1_000, job, name, hash, "ran", workflow, stage, (None, None));
        }
        event(&old, h, 1_100, WORKFLOW, "core-tests", "1of3", "save", &key_of("core-tests-1of3", h), None, "skipped", Some("no new task results (809 restored, 809 now)"));
        event(&old, h, 1_100, WORKFLOW, "integration-tests", "2of4", "save", &key_of("integration-tests-2of4", h), None, "ok", None);
        drop(old);
        let jobs = dir.path().join("ledgers/ci-db-core-tests-1");
        std::fs::create_dir_all(&jobs).unwrap();
        let now = open_ledger(&jobs.join("rows.sqlite")).unwrap();
        for (job, name, hash, outcome, stage, facts) in [
            ("core-tests", "a:flow", "h1", "ran", "core-tests-1of3", (Some(0), Some(0))),
            ("core-tests", "b:flow", "h2", "ran", "core-tests-1of3", (Some(1), Some(1))),
            ("core-tests", "c:flow", "h3", "ran", "core-tests-1of3", (Some(0), Some(0))),
            ("core-tests", "d:flow", "h4", "ran", "core-tests-1of3", (Some(0), Some(0))),
            ("core-tests", "e:flow", "h5", "ran", "core-tests-1of3", (Some(0), Some(0))),
            ("core-tests", "z:flow", "h9", "hit", "core-tests-1of3", (Some(1), Some(1))),
            ("integration-tests", "i:replay", "h8", "ran", "integration-tests-2of4", (Some(0), Some(0))),
            ("integration-tests", "j:replay", "h6", "ran", "integration-tests-3of4", (Some(0), Some(0))),
            ("deployment-verification-tests", "crate-x:cargo-verify-deployed", "h7", "ran", "deployment-verification-tests", (None, None)),
        ] {
            task(&now, c, 2_000, job, name, hash, outcome, WORKFLOW, stage, facts);
        }
        event(&now, c, 1_900, WORKFLOW, "core-tests", "1of3", "restore", &key_of("core-tests-1of3", c), Some(&key_of("core-tests-1of3", h)), "ok", None);
        event(&now, c, 1_900, WORKFLOW, "integration-tests", "2of4", "restore", &key_of("integration-tests-2of4", c), Some(&key_of("integration-tests-2of4", "60")), "ok", None);
        event(&now, c, 1_900, WORKFLOW, "integration-tests", "3of4", "restore", &key_of("integration-tests-3of4", c), None, "error", Some("Failed to restore: HTTP 503"));
        drop(now);
        let fixture = Fixture { dir, current: c.to_string() };
        let r = run_wf(&["ci", "db", "merge", "--sealed-file", &fixture.sealed().display().to_string(), &fixture.path("history.sqlite").display().to_string()], &RunOpts::default().env("CI_DB_KEY", KEY));
        assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
        fixture
    }

    pub(crate) fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    pub(crate) fn sealed(&self) -> PathBuf {
        self.path("ci.sealed")
    }

    pub(crate) fn ledgers(&self) -> PathBuf {
        self.path("ledgers")
    }

    pub(crate) fn job_file(&self) -> PathBuf {
        self.path("ledgers/ci-db-core-tests-1/rows.sqlite")
    }

    /// The environment of a report job that holds the jobs' databases and the sealed history.
    pub(crate) fn env(&self, opts: RunOpts) -> RunOpts {
        opts.env("CI_DB_KEY", KEY).env("WF_CI_SEALED_FILE", &self.sealed().display().to_string()).env("WF_CI_LEDGERS", &self.ledgers().display().to_string()).env("GITHUB_RUN_ID", &self.current)
    }
}

fn reasons(out: &Value) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = out["misses"].as_array().unwrap().iter().map(|m| (m["task"].as_str().unwrap().to_string(), m["reason"].as_str().unwrap().to_string())).collect();
    rows.sort();
    rows
}

fn rows_of(sealed: &Path, sql: &str) -> Value {
    let r = run_wf(&["ci", "db", "query", "--sealed-file", &sealed.display().to_string(), sql], &RunOpts::default().env("CI_DB_KEY", KEY));
    last_json(&r)["rows"].clone()
}

#[test]
fn given_a_run_whose_tasks_ran_again_when_the_misses_are_explained_then_each_gets_the_reason_its_evidence_names() {
    // Given the sealed history of run 100 and the job database of run 200, whose tasks ran again for different reasons
    let f = Fixture::new("100", "200");

    // When the report job explains the misses of run 200
    let r = run_wf(&["ci", "cache-misses"], &f.env(RunOpts::default()));

    // Then every task that ran has one reason: the skipped save, the restored entry Nx ignored, another shard, another workflow, a failed restore, an older
    // entry, a hash nobody stored, and a target that is never cached
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let out = last_json(&r);
    let expected = [
        ("a:flow", "earlier-save-skipped"),
        ("b:flow", "restored-but-reran"),
        ("c:flow", "stored-by-other-shard"),
        ("crate-x:cargo-verify-deployed", "uncacheable-by-design"),
        ("d:flow", "stored-by-other-workflow"),
        ("e:flow", "never-stored"),
        ("i:replay", "restored-older-entry"),
        ("j:replay", "restore-failed"),
    ];
    assert_eq!(reasons(&out), expected.map(|(t, r)| (t.to_string(), r.to_string())), "{out}");
    let skipped = out["misses"].as_array().unwrap().iter().find(|m| m["task"] == "a:flow").unwrap();
    assert!(skipped["detail"].as_str().unwrap().contains("run 100 executed it and its save was skipped (no new task results (809 restored, 809 now))"), "{skipped}");

    // And the breakdown carries the seconds each reason cost, and marks the unavoidable ones
    let by = |reason: &str| out["breakdown"].as_array().unwrap().iter().find(|b| b["reason"] == reason).cloned().unwrap_or_else(|| panic!("no {reason}: {out}"));
    assert_eq!((by("never-stored")["avoidable"].clone(), by("uncacheable-by-design")["avoidable"].clone(), by("earlier-save-skipped")["avoidable"].clone()), (json!(false), json!(false), json!(true)));
    assert_eq!((by("earlier-save-skipped")["tasks"].clone(), by("earlier-save-skipped")["seconds"].clone()), (json!(1), json!(10.0)));

    // And a failed restore of the run is listed as a problem with its own words
    assert_eq!(out["problems"][0]["layer"], "nx", "{out}");
    assert!(out["problems"][0]["reason"].as_str().unwrap().contains("HTTP 503"), "{out}");

    // And the workflow warning counts the tasks whose hash had already run, and what they cost
    let warning = r.stdout.lines().find(|l| l.starts_with("::warning title=cache misses under a hash that ran before::")).unwrap_or_else(|| panic!("no warning: {}", r.stdout));
    assert!(warning.contains("6 task(s), 60 s lost") && warning.contains("earlier-save-skipped x1 (10 s)") && warning.contains("restore-failed x1 (10 s)"), "{warning}");
}

#[test]
fn given_the_last_runs_when_the_rates_are_read_then_inputs_changed_and_an_exact_hash_seen_before_are_told_apart_per_stage() {
    // Given the same two runs
    let f = Fixture::new("100", "200");

    // When the report job reads the per-stage rates
    let out = last_json(&run_wf(&["ci", "cache-misses"], &f.env(RunOpts::default())));

    // Then core-tests: 11 tasks over the two runs, 1 replayed, 6 ran under a new hash (the inputs changed) and 4 ran under a hash already executed, so 1 of 5 replayed
    let rate = out["rates"].as_array().unwrap().iter().find(|r| r["stage"] == "core-tests").cloned().unwrap_or_else(|| panic!("no core-tests: {out}"));
    assert_eq!((rate["tasks"].clone(), rate["replayed"].clone(), rate["inputsChanged"].clone(), rate["seenBefore"].clone()), (json!(11), json!(1), json!(6), json!(4)), "{rate}");
    assert!((rate["exactReplayPercent"].as_f64().unwrap() - 20.0).abs() < 0.01, "{rate}");
}

#[test]
fn given_the_merge_when_a_job_database_is_folded_in_then_each_task_that_ran_keeps_its_reason_in_the_sealed_database() {
    // Given the sealed history and the job database of run 200
    let f = Fixture::new("100", "200");

    // When the report job merges the job database into the sealed one
    let r = run_wf(&["ci", "db", "merge", "--sealed-file", &f.sealed().display().to_string(), &f.job_file().display().to_string()], &RunOpts::default().env("CI_DB_KEY", KEY));

    // Then the verb says how many it classified, and the sealed rows hold the reason and its sentence
    assert_eq!((r.exit_code, last_json(&r)["classified"].clone()), (0, json!(8)), "{}{}", r.stdout, r.stderr);
    let rows = rows_of(&f.sealed(), "SELECT TASK, MISS_REASON FROM NX_TASKS WHERE RUN_ID = '200' AND OUTCOME = 'ran' ORDER BY TASK");
    assert_eq!(rows[0], json!({ "TASK": "a:flow", "MISS_REASON": "earlier-save-skipped" }), "{rows}");
    assert_eq!(rows[7], json!({ "TASK": "j:replay", "MISS_REASON": "restore-failed" }), "{rows}");
    let detail = rows_of(&f.sealed(), "SELECT MISS_DETAIL FROM NX_TASKS WHERE TASK = 'b:flow' AND RUN_ID = '200'");
    assert!(detail[0]["MISS_DETAIL"].as_str().unwrap().contains("holds the hash"), "{detail}");
    // And the replayed task has none, and the restore and save events are rows beside the tasks
    assert_eq!(rows_of(&f.sealed(), "SELECT MISS_REASON FROM NX_TASKS WHERE TASK = 'z:flow' AND RUN_ID = '200'"), json!([{ "MISS_REASON": null }]));
    assert_eq!(rows_of(&f.sealed(), "SELECT COUNT(*) AS n FROM CI_CACHE_EVENTS WHERE OUTCOME = 'error'"), json!([{ "n": 1 }]));
}

#[test]
fn given_no_readable_history_when_the_misses_are_explained_then_the_verb_says_so_and_still_exits_zero() {
    // Given a report job whose sealed database does not open with its key
    let f = Fixture::new("100", "200");

    // When the misses are explained
    let r = run_wf(&["ci", "cache-misses"], &f.env(RunOpts::default()).env("CI_DB_KEY", &"08".repeat(32)));

    // Then no reason is guessed: the answer is null with the cause, and the run is not failed
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let out = last_json(&r);
    assert!(out["ok"].is_null() && out["reason"].as_str().unwrap().contains("does not open"), "{out}");
}

#[test]
fn given_a_job_that_restored_the_nx_cache_when_its_stage_runs_then_its_tasks_record_the_lineage_and_whether_the_restore_held_their_hash() {
    // Given a workspace whose Nx stub runs app:typecheck under hash 42, and a restore that left that hash in the cache directory but not in Nx's table
    let w = crate::pipeline::Workspace::new("wf-flow-lineage-");
    let (_plan, _) = w.plan("HEAD~1");
    let (db, snapshot) = (w.path("rows.sqlite"), w.path("nx-cache.restore.json"));
    std::fs::write(&snapshot, json!({ "outcome": "ok", "matchedKey": "nx-typecheck-lint-unit-tests-Linux-X64-Pipeline-70-1", "entries": ["42", "7"], "dbHashes": ["7"], "dbRows": 1, "walBytes": 0, "dbNote": "", "bytes": 5 }).to_string()).unwrap();

    // When the stage runs in a CI job of a workflow, with the lineage the restore exported
    let r = w.wf(
        &["test", "run", "--stage", "typecheck-lint-unit-tests", "--plan", &w.path("plan.json")],
        &[
            ("WF_CI_DB", db.clone()),
            ("GITHUB_RUN_ID", "777".to_string()),
            ("GITHUB_SHA", "abc1234".to_string()),
            ("GITHUB_JOB", "core-tests".to_string()),
            ("GITHUB_WORKFLOW", "Pipeline".to_string()),
            ("GITHUB_RUN_ATTEMPT", "2".to_string()),
            ("WF_CACHE_STAGE", "core-tests-1of3".to_string()),
            ("WF_CACHE_RESTORED_KEY", "nx-typecheck-lint-unit-tests-Linux-X64-Pipeline-70-1".to_string()),
            ("WF_CACHE_RESTORE_FILE", snapshot),
            ("WF_NX_WHY", "0".to_string()),
        ],
    );

    // Then the task's row names the workflow, job, attempt, cache stage and restored key, and says the directory held the hash while Nx's table did not
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    let rows = rows_of_local(&db, "SELECT WORKFLOW, GH_JOB, ATTEMPT, CACHE_STAGE, RESTORED_KEY, DIR_HAS_HASH, DB_HAS_HASH, MISS_REASON FROM NX_TASKS WHERE TASK = 'app:typecheck'");
    assert_eq!(
        rows,
        vec![json!({ "WORKFLOW": "Pipeline", "GH_JOB": "core-tests", "ATTEMPT": "2", "CACHE_STAGE": "core-tests-1of3", "RESTORED_KEY": "nx-typecheck-lint-unit-tests-Linux-X64-Pipeline-70-1", "DIR_HAS_HASH": 1, "DB_HAS_HASH": 0, "MISS_REASON": null })]
    );
}

fn rows_of_local(db: &str, sql: &str) -> Vec<Value> {
    let conn = Connection::open(db).unwrap();
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
