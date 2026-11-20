//! `wf ci plan shard-weights` through the built binary, against a sealed CI database made the way the report job makes it (job-local files merged by `wf ci db
//! merge`): `--write` regenerates the weights file from the shard runner's unit spans (`CI_UNITS`) and falls back to the Nx tasks (`NX_TASKS`), `--report` sets one run's
//! split against what it measured.
use crate::support::{last_json, run_wf, RunOpts};
use serde_json::{json, Value};

const KEY: &str = "0707070707070707070707070707070707070707070707070707070707070707";

fn opts() -> RunOpts {
    RunOpts::default().env("CI_DB_KEY", KEY).env("WF_CI_DB", "off")
}

/// A job-local database with the rows a job leaves, merged into a sealed file the verbs read with `--sealed-file`.
fn sealed(dir: &std::path::Path, statements: &str) -> String {
    let local = dir.join("rows.sqlite");
    let conn = rusqlite::Connection::open(&local).unwrap();
    wf_bucket::ci_db::ensure_ledger(&conn).unwrap();
    conn.execute_batch(statements).unwrap();
    drop(conn);
    let sealed = dir.join("ci.sealed").display().to_string();
    let r = run_wf(&["ci", "db", "merge", "--sealed-file", &sealed, &local.display().to_string()], &opts());
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    sealed
}

fn unit(run: &str, stage: &str, name: &str, ms: i64) -> String {
    format!("INSERT INTO CI_UNITS VALUES ('{run}','{stage}','{stage}','{name}',1,2,{ms},'ran');")
}

fn task(run: &str, job: &str, task: &str, ms: i64) -> String {
    format!("INSERT INTO NX_TASKS (RUN_ID, SHA, TS, JOB, TASK, HASH, OUTCOME, MS, PASSED) VALUES ('{run}','a',1,'{job}','{task}','h{ms}','ran',{ms},1);")
}

#[test]
fn the_weights_file_is_regenerated_from_unit_spans_with_nx_tasks_for_units_that_have_none_and_android_units_included() {
    // Given a seed weights file, spans of two flow units (one also seen by Nx with a different time), an Nx-only flow unit and an Android test seen only by Nx
    let dir = tempfile::tempdir().unwrap();
    let seeded = dir.path().join("weights.json");
    std::fs::write(&seeded, json!({"schema": 1, "about": "Seeded.", "stages": {"flow-tests": {"default": 40, "units": {"crate-old": 10, "crate-a": 5}}, "android-e2e-tests": {"default": 40, "units": {"voice-edit": 150}}}}).to_string()).unwrap();
    let statements = [
        unit("1", "flow-tests", "crate-a", 120_000),
        unit("2", "flow-tests", "crate-a", 180_000),
        unit("3", "flow-tests", "crate-a", 600_000),
        task("1", "flow-tests (1/2)", "crate-a:cargo-flow", 9_000),
        task("1", "flow-tests (2/2)", "crate-b:cargo-flow", 61_500),
        task("1", "android-e2e-tests (1/2)", "e2e-voice-edit:e2e-android", 170_000),
        task("1", "android-e2e-tests (2/2)", "e2e-local-run:e2e-android", 36_200),
    ]
    .join("\n");
    let sealed = sealed(dir.path(), &statements);

    // When the weights are regenerated and written
    let r = run_wf(&["ci", "plan", "shard-weights", "--write", "--sealed-file", &sealed, "--out", &seeded.display().to_string()], &opts());

    // Then a unit with spans takes their median (180 s), not the Nx task's 9 s; an Nx-only unit takes its tasks; the Android tests are weighted too; the rest stays
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let file: Value = serde_json::from_str(&std::fs::read_to_string(&seeded).unwrap()).unwrap();
    let flow = &file["stages"]["flow-tests"]["units"];
    assert_eq!((flow["crate-a"].clone(), flow["crate-b"].clone(), flow["crate-old"].clone()), (json!(180), json!(62), json!(10)), "{file}");
    assert_eq!((file["stages"]["android-e2e-tests"]["units"]["voice-edit"].clone(), file["stages"]["android-e2e-tests"]["units"]["local-run"].clone()), (json!(170), json!(37)), "{file}");
    let keys: Vec<&str> = flow.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, ["crate-a", "crate-b", "crate-old"], "longest first");
    assert_eq!(last_json(&r)["measuredBy"], json!({"ci_units": 3, "nx_tasks": 4}));
    assert!(file["about"].as_str().unwrap().starts_with("Seeded. Measured units regenerated from CI_UNITS"), "{file}");

    // And without --write the file is left as it was
    let before = std::fs::read_to_string(&seeded).unwrap();
    std::fs::write(&seeded, before.replace("180", "181")).unwrap();
    let dry = run_wf(&["ci", "plan", "shard-weights", "--sealed-file", &sealed, "--out", &seeded.display().to_string()], &opts());
    assert_eq!((dry.exit_code, last_json(&dry)["written"].clone()), (0, json!(null)));
    assert!(std::fs::read_to_string(&seeded).unwrap().contains("181"));
}

#[test]
fn a_database_that_has_no_unit_spans_yet_still_regenerates_from_nx_tasks() {
    // Given a sealed database with only Nx task rows
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("weights.json");
    let sealed = sealed(dir.path(), &task("1", "flow-tests (1/2)", "crate-b:cargo-flow", 61_500));

    // When the weights are regenerated
    let r = run_wf(&["ci", "plan", "shard-weights", "--write", "--sealed-file", &sealed, "--out", &out.display().to_string()], &opts());

    // Then the unit is weighted from its task
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let file: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(file["stages"]["flow-tests"]["units"]["crate-b"], json!(62), "{file}");
}

#[test]
fn a_report_sets_each_shards_predicted_seconds_from_the_committed_weights_against_the_seconds_its_units_took() {
    // Given a run of two flow shards whose two units took 20 s more and 30 s less than the committed weights say
    let committed: Value = serde_json::from_str(&std::fs::read_to_string(wf_testkit::repo_root().join("clis/pipeline/data/shard-weights.json")).unwrap()).unwrap();
    let weight = |u: &str| committed["stages"]["flow-tests"]["units"][u].as_i64().unwrap();
    let (heavy, light) = ("crate-pipeline", "crate-deploy");
    assert!(weight(heavy) > weight(light), "the test names the heavier unit first");
    let dir = tempfile::tempdir().unwrap();
    let statements = [
        unit("9", "flow-tests", heavy, (weight(heavy) + 20) * 1000),
        unit("9", "flow-tests", light, (weight(light) - 30) * 1000),
        "INSERT INTO CI_JOBS (RUN_ID, SHA, TS, KIND, JOB, SECS, QUEUED) VALUES ('9','a',1,'flow-tests','flow-tests (1/2)',1,0), ('9','a',1,'flow-tests','flow-tests (2/2)',1,0);".to_string(),
    ]
    .join("\n");
    let sealed = sealed(dir.path(), &statements);

    // When the split of that run is reported
    let r = run_wf(&["ci", "plan", "shard-weights", "--report", "--run", "9", "--sealed-file", &sealed], &opts());

    // Then the heavy unit is alone on shard 1 and the light one on shard 2, each with its predicted and its actual seconds, and the table says how far off each was
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let out = last_json(&r);
    assert_eq!(out["stages"], json!([{"stage": "flow-tests", "shards": 2, "predicted_s": [weight(heavy), weight(light)], "actual_s": [weight(heavy) + 20, weight(light) - 30]}]), "{out}");
    let table = out["table"].as_str().unwrap();
    assert!(table.contains("| flow-tests | 1/2 |") && table.contains("+20 s") && table.contains("-30 s"), "{table}");

    // And the newest run is the default
    let newest = run_wf(&["ci", "plan", "shard-weights", "--report", "--sealed-file", &sealed], &opts());
    assert_eq!(last_json(&newest)["run"], json!("9"));
}

#[test]
fn a_report_of_a_run_the_database_does_not_hold_says_what_to_do_and_exits_1() {
    // Given a sealed database with no unit spans
    let dir = tempfile::tempdir().unwrap();
    let sealed = sealed(dir.path(), &task("1", "flow-tests (1/2)", "crate-b:cargo-flow", 1_000));

    // When a report is asked for
    let r = run_wf(&["ci", "plan", "shard-weights", "--report", "--run", "5", "--sealed-file", &sealed], &opts());

    // Then it refuses with a next step
    let out = last_json(&r);
    assert_eq!((r.exit_code, out["ok"].clone()), (1, json!(false)), "{}", r.stdout);
    assert!(out["error"].as_str().unwrap().contains("no rows of run 5") && out["next"].is_string(), "{out}");
}
