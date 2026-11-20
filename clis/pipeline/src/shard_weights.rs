//! `wf ci plan shard-weights [--write] [--sealed-file <path>] [--out <path>]`: the cost of each flow and integration unit as the CI database measured it
//! (`NX_TASKS`: the milliseconds of every task that ran), in the format of `data/shard-weights.json`, so the shard split follows what the stages took and
//! not a guess. A unit's cost in one run is the sum of its tasks that ran; its weight is the median over the runs, in seconds.
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use wf_bucket::ci_db::{self, Working};

const STAGES: [&str; 2] = ["flow-tests", "integration-tests"];
const FLOOR_S: u64 = 5;

/// The stage a job belongs to: `flow-tests (3/6)` is `flow-tests`; a `core-tests` job runs two stages, told apart by the task's target.
fn stage_of(job: &str, task: &str) -> Option<&'static str> {
    let base = job.split(" (").next().unwrap_or(job);
    if base == "core-tests" {
        let target = task.split(':').nth(1).unwrap_or_default();
        return ["test:flow", "test:contract", "cargo-flow", "cargo-contract"].contains(&target).then_some("flow-tests");
    }
    STAGES.into_iter().find(|s| *s == base)
}

/// The unit a task counts for, as `wf ci plan tests` names them: a crate's replay group is `crate-<crate>/<group task>`, everything else of a project is the project.
fn unit_of(stage: &str, task: &str) -> Option<String> {
    let (project, target) = task.split_once(':')?;
    if stage == "integration-tests" && project.starts_with("crate-") && target.starts_with("replay-") {
        return Some(format!("{project}/{target}"));
    }
    Some(project.to_string())
}

/// stage -> unit -> weight in seconds, from rows of `{RUN_ID, JOB, TASK, MS}` of tasks that ran.
pub fn costs(rows: &[Value]) -> BTreeMap<String, BTreeMap<String, u64>> {
    let mut sums: BTreeMap<(String, String), BTreeMap<String, u64>> = BTreeMap::new();
    for row in rows {
        let (Some(job), Some(task), Some(run), Some(ms)) = (row["JOB"].as_str(), row["TASK"].as_str(), row["RUN_ID"].as_str(), row["MS"].as_u64()) else { continue };
        let Some(stage) = stage_of(job, task) else { continue };
        let Some(unit) = unit_of(stage, task) else { continue };
        *sums.entry((stage.to_string(), unit)).or_default().entry(run.to_string()).or_default() += ms;
    }
    let mut out: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    for ((stage, unit), runs) in sums {
        let mut each: Vec<u64> = runs.into_values().collect();
        each.sort_unstable();
        let median = each[each.len() / 2];
        out.entry(stage).or_default().insert(unit, (median.div_ceil(1000)).max(FLOOR_S));
    }
    out
}

/// The weights file with the measured units replaced and the rest kept, units longest first.
pub fn merged(existing: &Value, measured: &BTreeMap<String, BTreeMap<String, u64>>, runs: usize) -> Value {
    let mut file = existing.clone();
    for (stage, units) in measured {
        let slot = &mut file["stages"][stage.as_str()];
        let mut all: BTreeMap<String, u64> = slot["units"].as_object().map(|m| m.iter().filter_map(|(k, v)| v.as_u64().map(|n| (k.clone(), n))).collect()).unwrap_or_default();
        all.extend(units.iter().map(|(k, v)| (k.clone(), *v)));
        let mut ordered: Vec<(String, u64)> = all.into_iter().collect();
        ordered.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        slot["units"] = Value::Object(ordered.into_iter().map(|(k, v)| (k, json!(v))).collect::<Map<_, _>>());
    }
    let note = format!("Measured units regenerated from NX_TASKS (median of the ran tasks over {runs} run(s)) by `wf ci plan shard-weights --write`.");
    let about = file["about"].as_str().unwrap_or_default();
    let base = about.split(" Measured units regenerated").next().unwrap_or(about);
    file["about"] = json!(format!("{base} {note}").trim().to_string());
    file
}

fn fail(error: impl Into<String>, next: &str) -> Option<i32> {
    println!("{}", json!({ "ok": false, "error": error.into(), "next": next }));
    Some(1)
}

pub fn run(args: &[String]) -> Option<i32> {
    let (mut write, mut sealed, mut out) = (false, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--write" => write = true,
            "--sealed-file" => sealed = it.next().cloned(),
            "--out" => out = it.next().cloned(),
            other => return fail(format!("unknown argument {other}"), "wf ci plan shard-weights [--write] [--sealed-file <path>] [--out <path>]"),
        }
    }
    let work = match &sealed {
        None => Working::fetch(),
        Some(path) => ci_db::key().and_then(|k| Working::from_sealed(k, std::fs::read(path).ok())),
    };
    let rows = match work.and_then(|w| ci_db::query(&w.connect()?, "SELECT RUN_ID, JOB, TASK, MS FROM NX_TASKS WHERE OUTCOME = 'ran'")) {
        Ok(rows) => rows,
        Err(e) => return fail(e, "check CI_DB_KEY and that the state ref is reachable"),
    };
    let runs = rows.iter().filter_map(|r| r["RUN_ID"].as_str()).collect::<std::collections::BTreeSet<_>>().len();
    let measured = costs(&rows);
    let path = out.map(std::path::PathBuf::from).unwrap_or_else(|| wf_kit::root::repo_root().join("clis/pipeline/data/shard-weights.json"));
    let existing: Value = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| json!({"schema": 1, "stages": {}}));
    let next = merged(&existing, &measured, runs);
    if write {
        if let Err(e) = std::fs::write(&path, format!("{}\n", serde_json::to_string_pretty(&next).unwrap_or_default())) {
            return fail(format!("{}: {e}", path.display()), "--out <path>");
        }
    }
    println!("{}", json!({"ok": true, "runs": runs, "units": measured.values().map(BTreeMap::len).sum::<usize>(), "written": write.then(|| path.display().to_string()), "weights": next["stages"]}));
    Some(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(run: &str, job: &str, task: &str, ms: u64) -> Value {
        json!({"RUN_ID": run, "JOB": job, "TASK": task, "MS": ms})
    }

    #[test]
    fn given_a_core_tests_job_when_costs_are_made_then_only_its_flow_tasks_count_for_flow_tests() {
        // Given a core-tests shard that ran a flow task and a typecheck task
        let rows = vec![row("1", "core-tests (1/3)", "crate-pipeline:cargo-flow", 40_000), row("1", "core-tests (1/3)", "crate-pipeline:typecheck", 90_000)];
        // When the costs are made
        let costs = costs(&rows);
        // Then the flow stage holds the flow task's seconds and nothing of the typecheck
        assert_eq!(costs["flow-tests"]["crate-pipeline"], 40);
    }

    #[test]
    fn given_tasks_of_several_runs_when_costs_are_made_then_a_unit_is_the_median_of_its_runs_and_a_replay_group_is_its_own_unit() {
        // Given a crate's flow task and two integration tasks of one crate over three runs
        let rows = vec![
            row("1", "flow-tests (1/6)", "crate-pipeline:cargo-flow", 100_000),
            row("2", "flow-tests (2/6)", "crate-pipeline:cargo-flow", 200_000),
            row("3", "flow-tests (1/6)", "crate-pipeline:cargo-flow", 900_000),
            row("1", "integration-tests", "crate-t3-serve:replay-t3-core-core", 598_000),
            row("1", "integration-tests", "crate-t3-serve:cargo-integration", 40_500),
            row("1", "integration-tests", "crate-t3-serve:cargo-integration", 10_000),
            row("1", "deploy-build", "app:build", 1_000_000),
        ];

        // When the costs are made
        let c = costs(&rows);

        // Then the flow unit is the median run (200 s), the replay group is a unit of its own, the crate's other tasks add up, and a deploy task is no unit
        assert_eq!(c["flow-tests"]["crate-pipeline"], 200);
        assert_eq!(c["integration-tests"]["crate-t3-serve/replay-t3-core-core"], 598);
        assert_eq!(c["integration-tests"]["crate-t3-serve"], 51);
        assert!(!c.contains_key("deploy-build"));
    }

    #[test]
    fn given_a_weights_file_when_measured_units_are_merged_then_they_replace_the_old_ones_and_the_others_stay_longest_first() {
        let existing = json!({"schema": 1, "about": "Seeded.", "stages": {"flow-tests": {"default": 40, "units": {"crate-a": 300, "crate-b": 10}}}});
        let mut measured = BTreeMap::new();
        measured.insert("flow-tests".to_string(), BTreeMap::from([("crate-b".to_string(), 500u64), ("crate-c".to_string(), 20u64)]));

        let file = merged(&existing, &measured, 3);

        let units: Vec<(&String, &Value)> = file["stages"]["flow-tests"]["units"].as_object().unwrap().iter().collect();
        assert_eq!(units.iter().map(|(k, v)| (k.as_str(), v.as_u64().unwrap())).collect::<Vec<_>>(), vec![("crate-b", 500), ("crate-a", 300), ("crate-c", 20)]);
        assert_eq!(file["stages"]["flow-tests"]["default"], 40);
        assert!(file["about"].as_str().unwrap().starts_with("Seeded. Measured units regenerated from NX_TASKS"));
        assert_eq!(merged(&file, &measured, 3)["about"], file["about"]);
    }
}
