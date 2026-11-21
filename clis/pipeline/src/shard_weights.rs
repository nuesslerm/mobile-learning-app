//! `wf ci plan shard-weights [--write] [--report [--run <id>] [--send]] [--sealed-file <path>] [--out <path>]`: the cost of each flow, typecheck, integration, cli-e2e,
//! browser and Android unit as the CI database measured it, in the format of `data/shard-weights.json`, so the shard split follows what the stages took and not a
//! guess. The measure is the shard runner's own span of the unit (`CI_UNITS`); a unit it has none for falls back to its Nx tasks (`NX_TASKS`: the milliseconds of
//! every task that ran). A unit's cost in one run is that sum; its weight is the median over the runs, in seconds. `--report` sets the split of one run against
//! what the shards measured: the seconds each shard was predicted to carry and the seconds its units took.
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use wf_bucket::ci_db::{self, UnitSpan, Working};

const STAGES: [&str; 6] = ["flow-tests", "typecheck-lint-unit-tests", "integration-tests", "cli-e2e-tests", "browser-e2e-tests", "android-e2e-tests"];
const FLOOR_S: u64 = 5;

/// A stage whose units the shards deal out by weight.
pub(crate) fn weighted(stage: &str) -> bool {
    STAGES.contains(&stage)
}

/// The stage a job belongs to: `flow-tests (3/6)` is `flow-tests`; a `core-tests` job runs two stages, told apart by the task's target.
fn stage_of(job: &str, task: &str) -> Option<&'static str> {
    let base = job.split(" (").next().unwrap_or(job);
    if base == "core-tests" {
        let target = crate::nx_ledger::target_of(task);
        return ["test:flow", "test:contract", "cargo-flow", "cargo-contract"].contains(&target).then_some("flow-tests");
    }
    STAGES.into_iter().find(|s| *s == base)
}

/// The unit a task counts for, as `wf ci plan tests` names them: a crate's replay group is `crate-<crate>/<group task>`, everything else of a project is the project.
pub(crate) fn unit_of(stage: &str, task: &str) -> Option<String> {
    let (project, target) = task.split_once(':')?;
    if stage == "integration-tests" && project.starts_with("crate-") && target.starts_with("replay-") {
        return Some(format!("{project}/{target}"));
    }
    if stage == "cli-e2e-tests" && project.starts_with("crate-") && target.starts_with("cargo-e2e-") && target != "cargo-e2e-smoke" {
        return Some(format!("{project}/{}", &target["cargo-e2e-".len()..]));
    }
    if stage.ends_with("-e2e-tests") && stage != "cli-e2e-tests" {
        // a browser or Android test is the Nx project `e2e-<name>` and the plan's unit is `<name>`
        return target.starts_with("e2e-").then(|| project.strip_prefix("e2e-").map(str::to_string)).flatten();
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

/// stage -> unit -> weight in seconds, from rows of `{RUN_ID, STAGE, UNIT, MS}` of the shard runner's own spans of units that ran.
pub fn costs_of_units(rows: &[Value]) -> BTreeMap<String, BTreeMap<String, u64>> {
    let mut sums: BTreeMap<(String, String), BTreeMap<String, u64>> = BTreeMap::new();
    for row in rows {
        let (Some(stage), Some(unit), Some(run), Some(ms)) = (row["STAGE"].as_str(), row["UNIT"].as_str(), row["RUN_ID"].as_str(), row["MS"].as_u64()) else { continue };
        if ms > 0 && weighted(stage) {
            *sums.entry((stage.to_string(), unit.to_string())).or_default().entry(run.to_string()).or_default() += ms;
        }
    }
    let mut out: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    for ((stage, unit), runs) in sums {
        let mut each: Vec<u64> = runs.into_values().collect();
        each.sort_unstable();
        out.entry(stage).or_default().insert(unit, each[each.len() / 2].div_ceil(1000).max(FLOOR_S));
    }
    out
}

/// The unit spans over the tasks: a unit the shard runner measured takes its span's weight, every other unit keeps the weight its Nx tasks give.
pub fn preferring_units(spans: BTreeMap<String, BTreeMap<String, u64>>, tasks: BTreeMap<String, BTreeMap<String, u64>>) -> BTreeMap<String, BTreeMap<String, u64>> {
    let mut out = tasks;
    for (stage, units) in spans {
        out.entry(stage).or_default().extend(units);
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
    let note = format!("Measured units regenerated from CI_UNITS, else NX_TASKS (median of the ran spans over {runs} run(s)) by `wf ci plan shard-weights --write`.");
    let about = file["about"].as_str().unwrap_or_default();
    let base = about.split(" Measured units regenerated").next().unwrap_or(about);
    file["about"] = json!(format!("{base} {note}").trim().to_string());
    file
}

fn fail(error: impl Into<String>, next: &str) -> Option<i32> {
    println!("{}", json!({ "ok": false, "error": error.into(), "next": next }));
    Some(1)
}

/// One stage of a run: the seconds each shard was predicted to carry (the committed weights of its units) and the seconds its units took.
pub struct StageReport {
    pub stage: String,
    pub predicted_s: Vec<u64>,
    pub actual_s: Vec<u64>,
}

/// The split of a run set against what it measured. A span does not record its shard, so both columns are for the re-dealt shards, not a measured per-shard time; only the per-unit error is measured. The deal is the one the plan makes (`assign_onto`, longest first, the typecheck deal starting from the flow
/// load of the machine it mirrors), recomputed over the units the run's spans name, and `counts` is how many shards each stage had.
pub fn report(spans: &[UnitSpan], counts: &BTreeMap<String, usize>) -> Vec<StageReport> {
    use crate::test_plan::{assign_onto, Weights};
    let mut by_stage: BTreeMap<&str, BTreeMap<&str, u64>> = BTreeMap::new();
    for u in spans.iter().filter(|u| weighted(&u.stage)) {
        *by_stage.entry(u.stage.as_str()).or_default().entry(u.unit.as_str()).or_default() += u.ms.unwrap_or(0).max(0) as u64;
    }
    let deal = |stage: &str, base: &[u64]| -> Option<(Vec<String>, Vec<usize>, Weights)> {
        let keys: Vec<String> = by_stage.get(stage)?.keys().map(|k| k.to_string()).collect();
        let weights = Weights::of_stage(stage);
        let count = counts.get(stage).copied().unwrap_or(1).max(1);
        let at = assign_onto(&keys, count, &weights, base);
        Some((keys, at, weights))
    };
    let flow_load = |count: usize| -> Vec<u64> {
        let mut load = vec![0u64; count];
        if let Some((keys, at, w)) = deal("flow-tests", &[]) {
            keys.iter().zip(at).for_each(|(k, s)| load[s.min(count - 1)] += w.cost(k));
        }
        load
    };
    let mut out = Vec::new();
    for stage in STAGES {
        let count = counts.get(stage).copied().unwrap_or(1).max(1);
        let base: Vec<u64> = if stage == "typecheck-lint-unit-tests" { let f = flow_load(count); (0..count).map(|s| f[count - 1 - s]).collect() } else { Vec::new() };
        let Some((keys, at, weights)) = deal(stage, &base) else { continue };
        let (mut predicted, mut actual) = (vec![0u64; count], vec![0u64; count]);
        for (key, shard) in keys.iter().zip(at) {
            predicted[shard] += weights.cost(key);
            actual[shard] += by_stage[stage][key.as_str()].div_ceil(1000);
        }
        out.push(StageReport { stage: stage.to_string(), predicted_s: predicted, actual_s: actual });
    }
    out
}

/// The report as the table the run's comment and `--report` print.
pub fn table(stages: &[StageReport]) -> String {
    let mut out = String::from("| Stage | Shard (re-dealt) | Predicted | Actual | Off by |\n|---|---|---|---|---|\n");
    for r in stages {
        for (i, (p, a)) in r.predicted_s.iter().zip(&r.actual_s).enumerate() {
            out.push_str(&format!("| {} | {}/{} | {p} s | {a} s | {:+} s |\n", r.stage, i + 1, r.predicted_s.len(), *a as i64 - *p as i64));
        }
    }
    out
}

/// How many shards each stage had, from the run's job names (`flow-tests (3/6)` is six shards of `flow-tests`; a `core-tests (1/3)` machine runs a flow and a
/// typecheck shard, so it counts for both).
pub fn shard_counts<'a>(jobs: impl IntoIterator<Item = &'a str>) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for name in jobs {
        let (stage, shard) = (crate::ci_timings::stage_name(name), crate::ci_timings::shard_of(name));
        let Some(total) = shard.split('/').nth(1).and_then(|t| t.parse::<usize>().ok()) else { continue };
        let stages: Vec<&str> = if stage == "core-tests" { vec!["flow-tests", "typecheck-lint-unit-tests"] } else { vec![stage.as_str()] };
        for s in stages {
            let slot = counts.entry(s.to_string()).or_insert(0);
            *slot = (*slot).max(total);
        }
    }
    counts
}

fn flags_help() -> &'static str {
    "wf ci plan shard-weights [--write] [--report [--run <id>] [--send]] [--sealed-file <path>] [--out <path>]"
}

pub fn run(args: &[String]) -> Option<i32> {
    let (mut write, mut report_mode, mut send, mut sealed, mut out, mut run_id) = (false, false, false, None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--write" => write = true,
            "--report" => report_mode = true,
            "--send" => send = true,
            "--run" => run_id = it.next().cloned(),
            "--sealed-file" => sealed = it.next().cloned(),
            "--out" => out = it.next().cloned(),
            other => return fail(format!("unknown argument {other}"), flags_help()),
        }
    }
    let work = match &sealed {
        None => Working::fetch(),
        Some(path) => ci_db::key().and_then(|k| Working::from_sealed(k, std::fs::read(path).ok())),
    };
    let work = match work {
        Ok(w) => w,
        Err(e) => return fail(e, "check CI_DB_KEY and that the state ref is reachable"),
    };
    let rows = |sql: &str| work.connect().and_then(|c| ci_db::ensure_ledger(&c).map(|()| c)).and_then(|c| ci_db::query(&c, sql));
    if report_mode {
        return report_run(&work, &rows, run_id, send);
    }
    let (tasks, units) = match (rows("SELECT RUN_ID, JOB, TASK, MS FROM NX_TASKS WHERE OUTCOME = 'ran'"), rows("SELECT RUN_ID, STAGE, UNIT, MS FROM CI_UNITS WHERE OUTCOME = 'ran'")) {
        (Ok(t), Ok(u)) => (t, u),
        (Err(e), _) | (_, Err(e)) => return fail(e, "check CI_DB_KEY and that the state ref is reachable"),
    };
    let runs = tasks.iter().chain(&units).filter_map(|r| r["RUN_ID"].as_str()).collect::<std::collections::BTreeSet<_>>().len();
    let measured = preferring_units(costs_of_units(&units), costs(&tasks));
    let path = out.map(std::path::PathBuf::from).unwrap_or_else(|| wf_kit::root::repo_root().join("clis/pipeline/data/shard-weights.json"));
    let existing: Value = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| json!({"schema": 1, "stages": {}}));
    let next = merged(&existing, &measured, runs);
    if write {
        if let Err(e) = std::fs::write(&path, format!("{}\n", serde_json::to_string_pretty(&next).unwrap_or_default())) {
            return fail(format!("{}: {e}", path.display()), "--out <path>");
        }
    }
    println!("{}", json!({"ok": true, "runs": runs, "units": measured.values().map(BTreeMap::len).sum::<usize>(), "measuredBy": {"ci_units": units.len(), "nx_tasks": tasks.len()}, "written": write.then(|| path.display().to_string()), "weights": next["stages"]}));
    Some(0)
}

fn report_run(work: &Working, rows: &dyn Fn(&str) -> Result<Vec<Value>, String>, run_id: Option<String>, send: bool) -> Option<i32> {
    let run = match run_id.or_else(|| std::env::var("GITHUB_RUN_ID").ok().filter(|v| !v.is_empty())) {
        Some(r) => r,
        None => match rows("SELECT RUN_ID FROM CI_UNITS ORDER BY STARTED_MS DESC LIMIT 1") {
            Ok(r) => match r.first().and_then(|r| r["RUN_ID"].as_str()) {
                Some(id) => id.to_string(),
                None => return fail("CI_UNITS holds no run yet", "run the shard stages in CI first, then `wf ci db merge`"),
            },
            Err(e) => return fail(e, "check CI_DB_KEY"),
        },
    };
    let spans = match ci_db::units_of(&work.path, Some(&run)) {
        Ok(s) if !s.is_empty() => s,
        Ok(_) => return fail(format!("CI_UNITS holds no rows of run {run}"), "pass --run <id> of a run the report job has merged"),
        Err(e) => return fail(e, "check CI_DB_KEY"),
    };
    let literal = run.replace('\'', "''");
    let names = rows(&format!("SELECT JOB FROM CI_JOBS WHERE RUN_ID = '{literal}'")).unwrap_or_default();
    let counts = shard_counts(names.iter().filter_map(|r| r["JOB"].as_str()));
    let stages = report(&spans, &counts);
    if send {
        let mut gauges = wf_bucket::ci_metrics::Gauges::new();
        for r in &stages {
            for (i, (p, a)) in r.predicted_s.iter().zip(&r.actual_s).enumerate() {
                let shard = format!("{}/{}", i + 1, r.predicted_s.len());
                gauges.set("ci.shard.predicted_s", &r.stage, &shard, *p as f64).set("ci.shard.actual_s", &r.stage, &shard, *a as f64);
            }
        }
        gauges.send();
    }
    let json_stages: Vec<Value> = stages.iter().map(|r| json!({"stage": r.stage, "shards": r.predicted_s.len(), "predicted_s": r.predicted_s, "actual_s": r.actual_s})).collect();
    println!("{}", json!({"ok": true, "run": run, "stages": json_stages, "shard_counts": if counts.is_empty() { "fallback: CI_JOBS holds no sharded job of this run, one shard per stage assumed" } else { "CI_JOBS" }, "table": table(&stages)}));
    Some(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(run: &str, job: &str, task: &str, ms: u64) -> Value {
        json!({"RUN_ID": run, "JOB": job, "TASK": task, "MS": ms})
    }

    #[test]
    fn given_a_core_tests_job_when_a_package_flow_task_is_read_then_it_counts_for_flow_tests_whatever_the_colons_in_its_target() {
        // Given the flow targets of a package (`test:flow`) and a crate (`cargo-flow`), and a unit target of a package
        // When the stage of each task of a core-tests job is read
        // Then both flow targets are flow-tests and the unit target is not
        assert_eq!(stage_of("core-tests (2/3)", "mla-tamagui:test:flow"), Some("flow-tests"));
        assert_eq!(stage_of("core-tests (2/3)", "@feature/settings:test:flow"), Some("flow-tests"));
        assert_eq!(stage_of("core-tests (2/3)", "crate-wf:cargo-flow"), Some("flow-tests"));
        assert_eq!(stage_of("core-tests (2/3)", "@feature/settings:test:unit"), None);
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
    fn given_typecheck_and_cli_e2e_tasks_when_costs_are_made_then_each_stage_holds_its_units_and_a_crates_smoke_and_e2e_tasks_add_up() {
        // Given a crate's unit and clippy tasks, its e2e and smoke tasks, and an e2e group task
        let rows = vec![
            row("1", "typecheck-lint-unit-tests", "crate-wf:cargo-unit", 52_000),
            row("1", "typecheck-lint-unit-tests", "crate-wf:cargo-clippy", 30_000),
            row("1", "cli-e2e-tests", "crate-wf:cargo-e2e", 43_000),
            row("1", "cli-e2e-tests", "crate-wf:cargo-e2e-smoke", 41_000),
            row("1", "cli-e2e-tests", "crate-wf-harness:cargo-e2e-record", 20_000),
        ];
        // When the costs are made
        let c = costs(&rows);
        // Then typecheck sums the crate's tasks, cli-e2e sums its crate and names a group as a unit of its own
        assert_eq!(c["typecheck-lint-unit-tests"]["crate-wf"], 82);
        assert_eq!(c["cli-e2e-tests"]["crate-wf"], 84);
        assert_eq!(c["cli-e2e-tests"]["crate-wf-harness/record"], 20);
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
        assert!(file["about"].as_str().unwrap().starts_with("Seeded. Measured units regenerated from CI_UNITS, else NX_TASKS"));
        assert_eq!(merged(&file, &measured, 3)["about"], file["about"]);
    }
}
