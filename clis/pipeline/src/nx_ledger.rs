//! What Nx did with each task of a run, read from the `run.json` it leaves beside its cache (`~/.nx/<workspace id>/cache/run.json`,
//! overwritten by every `nx` invocation): the task, its input hash, whether it ran or replayed, and its milliseconds. One
//! `wf: nx ledger {...}` line carries them into the job log, where `wf ci audit` judges which misses should have been hits.
use serde_json::{json, Value};
use std::path::Path;
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq)]
pub struct TaskRun {
    pub task: String,
    pub hash: String,
    /// `ran` when Nx executed the task, `hit` when it replayed it from a cache.
    pub outcome: &'static str,
    pub ms: i64,
    pub passed: bool,
}

fn millis(stamp: &Value) -> Option<i64> {
    let secs = crate::ci_comment::epoch_of(stamp.as_str()?.split('.').next()?)?;
    let fraction = stamp.as_str()?.split('.').nth(1).and_then(|f| f.trim_end_matches('Z').get(..3)).and_then(|f| f.parse::<i64>().ok()).unwrap_or(0);
    Some(secs * 1000 + fraction)
}

pub fn parse(raw: &str) -> Vec<TaskRun> {
    let Ok(doc) = serde_json::from_str::<Value>(raw) else { return Vec::new() };
    doc["tasks"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|t| {
            let (task, hash) = (t["taskId"].as_str()?, t["hash"].as_str()?);
            let ms = millis(&t["endTime"]).zip(millis(&t["startTime"])).map_or(0, |(end, start)| (end - start).max(0));
            let outcome = if t["cacheStatus"].as_str().is_some_and(|s| s == "cache-miss") { "ran" } else { "hit" };
            Some(TaskRun { task: task.to_string(), hash: hash.to_string(), outcome, ms, passed: t["status"].as_i64() == Some(0) })
        })
        .collect()
}

/// The tasks every `wf: nx ledger` line of a job log names, in order.
pub fn from_log(log: &str) -> Vec<TaskRun> {
    log.lines()
        .filter_map(|line| line.split_once("wf: nx ledger ").map(|(_, json)| json))
        .filter_map(|json| serde_json::from_str::<Value>(json.trim()).ok())
        .flat_map(|v| v["tasks"].as_array().cloned().unwrap_or_default())
        .filter_map(|t| {
            Some(TaskRun {
                task: t["task"].as_str()?.to_string(),
                hash: t["hash"].as_str()?.to_string(),
                outcome: if t["outcome"] == "ran" { "ran" } else { "hit" },
                ms: t["ms"].as_i64()?,
                passed: t["passed"].as_bool()?,
            })
        })
        .collect()
}

/// The tasks of the newest `run.json` under `home` written at or after `since`.
pub fn read(home: &Path, since: SystemTime) -> Vec<TaskRun> {
    let Ok(workspaces) = std::fs::read_dir(home.join(".nx")) else { return Vec::new() };
    // File times come from a coarser kernel clock than the `since` the caller read, so a run.json written right after `since` can carry an earlier time.
    let since = since.checked_sub(std::time::Duration::from_millis(100)).unwrap_or(since);
    let newest = workspaces
        .filter_map(Result::ok)
        .map(|w| w.path().join("cache/run.json"))
        .filter_map(|p| Some((std::fs::metadata(&p).ok()?.modified().ok()?, p)))
        .filter(|(at, _)| *at >= since)
        .max_by_key(|(at, _)| *at);
    newest.and_then(|(_, path)| std::fs::read_to_string(path).ok()).map(|raw| parse(&raw)).unwrap_or_default()
}

/// How many of the tasks ran and how many a cache replayed, and the share replayed (a run of no tasks replayed all it had).
pub fn replay_ratio(tasks: &[TaskRun]) -> (usize, usize, f64) {
    let hit = tasks.iter().filter(|t| t.outcome == "hit").count();
    let ratio = if tasks.is_empty() { 1.0 } else { hit as f64 / tasks.len() as f64 };
    (tasks.len() - hit, hit, ratio)
}

/// What every `run_many` of this process has run and replayed per stage: the gauges are the stage's, not the last kind's.
static STAGE_TOTALS: std::sync::Mutex<std::collections::BTreeMap<String, (usize, usize)>> = std::sync::Mutex::new(std::collections::BTreeMap::new());

/// The stage's event and gauges: `ci.nx.tasks{stage,outcome}` and `ci.nx.replay_ratio{stage}`.
fn report(stage: &str, target: &str, tasks: &[TaskRun]) {
    use wf_bucket::ci_metrics::{event, Gauges, ALL};
    let (ran, hit, _) = replay_ratio(tasks);
    let (stage_ran, stage_hit) = {
        let mut totals = STAGE_TOTALS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let total = totals.entry(stage.to_string()).or_default();
        total.0 += ran;
        total.1 += hit;
        *total
    };
    let ratio = if stage_ran + stage_hit == 0 { 1.0 } else { stage_hit as f64 / (stage_ran + stage_hit) as f64 };
    event("info", "nx.stage.summary", &[("stage", stage.to_string()), ("target", target.to_string()), ("ran", ran.to_string()), ("replayed", hit.to_string()), ("replay_ratio", format!("{ratio:.2}"))]);
    Gauges::new().set("ci.nx.tasks", stage, "ran", stage_ran as f64).set("ci.nx.tasks", stage, "hit", stage_hit as f64).set("ci.nx.replay_ratio", stage, ALL, ratio).send();
}

/// Prints the ledger line of the Nx run that started at `since` (nothing when it left no `run.json`).
pub fn emit(since: SystemTime, stage: &str, target: &str) {
    let Some(home) = std::env::var_os("HOME") else { return };
    let tasks = read(Path::new(&home), since);
    if tasks.is_empty() {
        return;
    }
    let list: Vec<Value> = tasks.iter().map(|t| json!({ "task": t.task, "hash": t.hash, "outcome": t.outcome, "ms": t.ms, "passed": t.passed })).collect();
    eprintln!("wf: nx ledger {}", json!({ "tasks": list }));
    record(&tasks);
    report(stage, target, &tasks);
    why(since, "stage");
}

/// The misses of the Nx run that started at `since`, each explained against the earlier runs of its task, and this run's inputs recorded
/// for the next one (`wf: nx why {...}` on stderr: `wf cache nx --help` has the schema). A gate run says nothing unless something is wrong.
pub fn why(since: SystemTime, kind: &str) {
    let Some(home) = std::env::var_os("HOME").filter(|_| wf_bucket::nx_why::enabled()) else { return };
    let tasks = wf_bucket::nx_why::run_tasks(Path::new(&home), since);
    if tasks.is_empty() {
        return;
    }
    let report = wf_bucket::nx_why::report(&crate::e2e_tests::repo(), &tasks, true);
    if kind == "stage" || report["ok"] != true {
        eprintln!("wf: nx why {report}");
    }
}

/// Writes the tasks to the ledger from the job itself, keyed like the rows `wf ci audit` merges later, so the ledger holds every run's tasks
/// whether or not GitHub keeps the job's log (it has not, since 31 Oct 00:19 UTC).
fn record(tasks: &[TaskRun]) {
    let var = |key: &str| std::env::var(key).ok().filter(|v| !v.is_empty());
    let (Some(run), Some(sha)) = (var("GITHUB_RUN_ID"), var("GITHUB_SHA")) else { return };
    let job = var("GITHUB_JOB").unwrap_or_default();
    let ts = wf_kit::time::now_ms() as i64;
    let rows: Vec<crate::ci_ledger::Observed> = tasks.iter().map(|t| crate::ci_ledger::Observed { run: run.clone(), sha: sha.clone(), ts, job: job.clone(), task: t.task.clone(), hash: t.hash.clone(), outcome: t.outcome.to_string(), ms: t.ms, passed: t.passed }).collect();
    if let Err(reason) = crate::ci_ledger::open().and_then(|ledger| ledger.record_tasks(&rows)) {
        wf_kit::wf_err!("wf: nx ledger not recorded: {reason}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUN: &str = r#"{"run":{},"tasks":[
      {"taskId":"@lib/a:typecheck","hash":"111","startTime":"2026-10-30T19:34:12.295Z","endTime":"2026-10-30T19:34:14.488Z","cacheStatus":"cache-miss","status":0},
      {"taskId":"crate-b:cargo-flow","hash":"222","startTime":"2026-10-30T19:34:12.000Z","endTime":"2026-10-30T19:34:12.100Z","cacheStatus":"remote-cache","status":0}]}"#;

    #[test]
    fn given_a_run_summary_when_parsed_then_a_miss_ran_and_a_cache_status_is_a_hit_with_its_milliseconds() {
        let tasks = parse(RUN);
        assert_eq!(tasks.iter().map(|t| (t.task.as_str(), t.hash.as_str(), t.outcome, t.ms)).collect::<Vec<_>>(), [("@lib/a:typecheck", "111", "ran", 2193), ("crate-b:cargo-flow", "222", "hit", 100)]);
    }

    #[test]
    fn given_ledger_lines_in_a_log_when_read_then_every_task_comes_back_with_its_hash_and_outcome() {
        let log = "2026-10-30T10:00:00.0000000Z wf: nx ledger {\"tasks\":[{\"task\":\"a:lint\",\"hash\":\"7\",\"outcome\":\"ran\",\"ms\":900,\"passed\":true}]}\nnoise\nwf: nx ledger {\"tasks\":[{\"task\":\"b:lint\",\"hash\":\"8\",\"outcome\":\"hit\",\"ms\":5,\"passed\":true}]}\n";
        let tasks = from_log(log);
        assert_eq!(tasks.iter().map(|t| (t.task.as_str(), t.hash.as_str(), t.outcome)).collect::<Vec<_>>(), [("a:lint", "7", "ran"), ("b:lint", "8", "hit")]);
    }

    #[test]
    fn given_no_summary_or_an_old_one_when_read_then_there_are_no_tasks() {
        let home = tempfile::tempdir().unwrap();
        assert!(read(home.path(), SystemTime::UNIX_EPOCH).is_empty());
        let dir = home.path().join(".nx/abc/cache");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("run.json"), RUN).unwrap();
        assert_eq!(read(home.path(), SystemTime::UNIX_EPOCH).len(), 2);
        assert!(read(home.path(), SystemTime::now() + std::time::Duration::from_secs(60)).is_empty());
    }
}
