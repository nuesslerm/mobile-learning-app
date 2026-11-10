//! The Evidence port over the real recorded sets (`clis/t3-serve/cassettes/t3-recorded`, `clis/orca-serve/cassettes/orca-recorded`): a row decided
//! `oracle: mimic` is served only by a clean replay of a set whose kind is `recorded`. The replays are those the chain tests of the two adapters
//! proved (an interaction differs only where `known-gaps.json` lists it); the sets, their rows and their gaps are read from the repository.
use super::support::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The recorded sets of an app: every directory of its crate's cassettes named `<app>-recorded*`.
fn recorded_sets(app: &str, cassettes: &str) -> Vec<PathBuf> {
    let mut sets: Vec<PathBuf> = std::fs::read_dir(clis().join(cassettes)).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&format!("{app}-recorded")))).collect();
    sets.sort();
    assert!(!sets.is_empty(), "no recorded set of {app}");
    sets
}

fn clis() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The interaction ids a `known-gaps.json` lists, from the flat list of schema 1 (`gaps[].id`) or the groups of schema 2 (`groups[].ids`).
fn listed_gaps(file: &Value) -> BTreeSet<String> {
    let ids = |list: &str, key: &str| -> Vec<String> { file[list].as_array().into_iter().flatten().flat_map(|g| match &g[key] { Value::Array(a) => a.iter().map(|x| x.as_str().expect("a gap id is a string").to_string()).collect(), v => vec![v.as_str().expect("a gap has an id").to_string()] }).collect() };
    match file["schema"].as_str() {
        Some("mimic-known-gaps/1") => ids("gaps", "id").into_iter().collect(),
        Some("mimic-known-gaps/2") => ids("groups", "ids").into_iter().collect(),
        other => panic!("known-gaps.json of schema {other:?} is not read"),
    }
}

struct World {
    dir: tempfile::TempDir,
    app: &'static str,
    /// Per interaction: its id, its row and whether the replay found it clean.
    replays: Vec<(String, String, bool)>,
}

impl World {
    fn of(app: &'static str, sets: &[PathBuf]) -> World {
        let mut replays = Vec::new();
        for set in sets {
            let gaps = listed_gaps(&serde_json::from_str::<Value>(&read(&set.join("known-gaps.json"))).unwrap());
            let header: Value = serde_json::from_str(&read(&set.join("cassette.json"))).unwrap();
            assert_eq!(header["provenance"]["kind"], "recorded", "{}", set.display());
            replays.extend(read(&set.join("interactions.jsonl")).lines().map(|l| serde_json::from_str::<Value>(l).unwrap()).filter_map(|i| Some((i["id"].as_str()?.to_string(), i["row"].as_str()?.to_string(), i["tags"].as_array().is_none_or(|t| !t.iter().any(|x| x == "nondeterministic"))))).map(|(id, row, stable)| (id.clone(), row, stable && !gaps.contains(&id))));
        }
        let dir = tempfile::tempdir().unwrap();
        let coverage = dir.path().join("docs/plans/rca-parity/coverage");
        std::fs::create_dir_all(&coverage).unwrap();
        let repo = clis().join("../docs/plans/rca-parity/coverage");
        for name in ["methods", "decisions", "evidence"] {
            std::fs::copy(repo.join(format!("{app}-{name}.json")), coverage.join(format!("{app}-{name}.json"))).unwrap();
        }
        for pin in ["tooling/t3-conformance/t3-pin.json", "tooling/orca-conformance/orca-pin.json"] {
            std::fs::create_dir_all(dir.path().join(pin).parent().unwrap()).unwrap();
            std::fs::copy(clis().join("..").join(pin), dir.path().join(pin)).unwrap();
        }
        let w = World { dir, app, replays };
        // Every row a recorded interaction names is decided to be served by the oracle only (a row with another decision is left as it is)
        let path = w.dir.path().join(format!("docs/plans/rca-parity/coverage/{app}-decisions.json"));
        let mut decisions: Value = serde_json::from_str(&read(&path)).unwrap();
        for (_, row, _) in &w.replays {
            let entry = decisions["decisions"].as_object_mut().unwrap().entry(row.clone()).or_insert_with(|| json!({}));
            if entry.get("state").is_none() {
                entry["oracle"] = json!("mimic");
            }
        }
        std::fs::write(&path, serde_json::to_string_pretty(&decisions).unwrap()).unwrap();
        w
    }

    /// The trace `wf mimic replay --suite` writes: one test per interaction, labelled by its replay's diff and by the set's kind.
    fn trace(&self, kind: &str, drifted: &[&str]) -> String {
        let tests: Vec<Value> = self.replays.iter().map(|(id, row, clean)| {
            let diff = if *clean && !drifted.contains(&row.as_str()) { "clean" } else { "drift" };
            json!({"test": format!("recorded::{id}"), "cassette": kind, "verdict": "pass", "ms": 1, "calls": [{"id": id, "rows": [row], "ok": true, "diff": diff, "agent": [], "executor": []}]})
        }).collect();
        serde_json::to_string_pretty(&json!({"schema": "method-trace/1", "app": self.app, "suite": "wf-mimic-recorded", "sha": null, "complete": true, "tests": tests})).unwrap()
    }

    /// The rows the evidence says an oracle replay served, after the trace is read.
    fn served(&self, trace: &str) -> BTreeSet<String> {
        let traces = self.dir.path().join("traces");
        std::fs::create_dir_all(traces.join("wf-mimic-recorded")).unwrap();
        std::fs::write(traces.join("wf-mimic-recorded/trace.json"), trace).unwrap();
        let run = coverage(self.dir.path(), &["--app", self.app, "--traces", traces.to_str().unwrap()]);
        assert_eq!(run.code, 0, "{}", run.stdout);
        let evidence: Value = serde_json::from_str(&read(&self.dir.path().join(format!("docs/plans/rca-parity/coverage/{}-evidence.json", self.app)))).unwrap();
        evidence["rows"].as_object().unwrap().iter().filter(|(_, e)| e["oracle"] == "mimic").map(|(row, _)| row.clone()).collect()
    }
}

#[test]
fn a_row_is_served_by_a_clean_replay_of_a_recorded_set_and_by_nothing_planted_in_its_place() {
    for (app, cassettes) in [("t3", "t3-serve/cassettes"), ("orca", "orca-serve/cassettes")] {
        // Given the sets the real server recorded (all of kind recorded), their replays' clean rows
        let w = World::of(app, &recorded_sets(app, cassettes));
        let mut by_row: BTreeMap<&str, bool> = BTreeMap::new();
        for (_, row, clean) in &w.replays {
            *by_row.entry(row).or_insert(true) &= clean;
        }
        let clean: BTreeSet<&str> = by_row.iter().filter(|(_, c)| **c).map(|(r, _)| *r).collect();
        // When the clean replay is read, Then rows are served, and only clean ones
        let served = w.served(&w.trace("recorded", &[]));
        assert!(!served.is_empty(), "{app}: no row is served by its recorded cassette");
        assert!(served.iter().all(|r| clean.contains(r.as_str())), "{app}: a row served although a replay of it differed: {served:?}");
        // And the same replay of a set planted as authored (a double, not the real server) serves nothing
        assert_eq!(w.served(&w.trace("authored", &[])), BTreeSet::new(), "{app}: an authored cassette served a row");
        // And a planted difference on one served row takes that row, and only that row, out of the served ones
        let planted = served.iter().next().unwrap().as_str();
        let after = w.served(&w.trace("recorded", &[planted]));
        assert_eq!(after.iter().cloned().collect::<Vec<_>>(), served.iter().filter(|r| r.as_str() != planted).cloned().collect::<Vec<_>>(), "{app}: {planted}");
    }
}
