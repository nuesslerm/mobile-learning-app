//! Every answer the real, pinned T3 server gave on the VM (the sets `cassettes/t3-v*-recorded-*`, kind `recorded`, one slice of the corpus each) replayed
//! against `t3-serve serve` over the whole chain, each interaction against a fresh chain as it was recorded against a fresh server. The inputs and the expected answers are the cassette's; nothing here names a method. A difference that is not a deliberate divergence
//! (the allowlist) is an adapter bug: it is listed in `known-gaps.json` with its row and severity until the adapter is fixed, so a new
//! difference fails and a fixed one has to leave the list. Each set is a test of its own, and an Nx task of its own (`[package.metadata.integration.groups]`).
use crate::rig::*;
use crate::support::mimic_t3;
use crate::support::*;
use mimic_core::differ::Severity;
use mimic_core::gaps;

const CASSETTES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/cassettes");
/// The crawl set (every valid, boundary and invalid case of every endpoint): its reads share one chain and its state-changing cases another, in
/// cassette order, as the VM recording shared one server between them. The recorded slices run each interaction on a chain of its own.
const CRAWL: &str = "t3-v0.1.9-crawl";
const PIN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tooling/t3-conformance/t3-pin.json");
/// The sets that have a test, in the order `recorded_sets` lists them.
const SETS: [&str; 4] = [CRAWL, "t3-v0.1.9-recorded-1", "t3-v0.1.9-recorded-2", "t3-v0.1.9-recorded-3"];

fn read(path: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"))).unwrap()
}

/// `sha256:` of a file, the way `wf mimic record` stamps the pin.
fn pin_hash() -> String {
    let out = std::process::Command::new("sha256sum").arg(PIN).output().expect("sha256sum runs");
    format!("sha256:{}", String::from_utf8_lossy(&out.stdout).split_whitespace().next().unwrap())
}

/// The recorded sets, by directory: every `t3-v*-recorded-*` beside the imported one.
fn recorded_sets() -> Vec<String> {
    let mut sets: Vec<String> = std::fs::read_dir(CASSETTES).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("t3-v") && n.contains("-recorded-") || n == CRAWL).collect();
    sets.sort();
    assert!(!sets.is_empty(), "no recorded set under {CASSETTES}");
    sets
}

/// One set's replay. Its trace and diff table go to `wf-mimic-t3-recorded/<set>/`: a task writes its own set only, and `wf parity coverage --traces`
/// reads every `trace.json` under the output dir.
fn replay(name: &str) {
    let dir = format!("{CASSETTES}/{name}");
    // Given the set the pinned server recorded on the VM, stamped with the hash of the pin file as it is now
    let (set, allow) = (mimic_t3::load_set(&dir), mimic_t3::load_allowlist(&dir));
    let header = read(&format!("{dir}/cassette.json"));
    assert_eq!((header["provenance"]["kind"].clone(), header["provenance"]["target"]["pin_hash"].clone()), (json!("recorded"), json!(pin_hash())), "{name}: {header}");
    let gaps_path = format!("{dir}/known-gaps.json");
    // When every interaction is replayed against its own t3-serve wired to the real rca-service and rca-exec
    let scopes = mimic_t3::granted_scopes(&set);
    let mut results = Vec::new();
    // an interaction the two recording runs disagreed on is noise, not a reference answer: it is not compared
    let stable = |i: &&mimic_core::cassette::Interaction| !i.tags.iter().any(|t| t == "nondeterministic");
    let has = |i: &mimic_core::cassette::Interaction, tag: &str| i.tags.iter().any(|t| t == tag);
    let (shared, own): (Vec<_>, Vec<_>) = set.cassette.interactions.iter().filter(stable).partition(|i| name == CRAWL && !has(i, "state:fresh"));
    let (writes, reads): (Vec<_>, Vec<_>) = shared.into_iter().partition(|i| has(i, "state:write"));
    let ids = |v: Vec<&mimic_core::cassette::Interaction>| v.into_iter().map(|i| i.id.clone()).collect::<Vec<_>>();
    let groups: Vec<Vec<String>> = [ids(reads), ids(writes)].into_iter().filter(|g| !g.is_empty()).chain(own.into_iter().map(|i| vec![i.id.clone()])).collect();
    for group in groups {
        let chain = std::rc::Rc::new(Chain::start());
        let mut candidate = chain_driver(&chain, scopes.clone(), mimic_t3::prelude_of(&set), true);
        let verdict = mimic_core::usecase::replay(&set, &mut candidate, &allow, &mimic_core::presets::t3_diff_options(), Some(&group)).expect("the replay runs");
        results.extend(verdict.results);
    }
    // Then the interactions with a difference are exactly the listed gaps, each with the differences it was listed for
    let rows: Vec<Value> = results.iter().map(|r| json!({"id": r.id, "row": r.row, "label": r.label, "severity": r.severity, "error": r.error,
        "diffs": r.diffs.iter().filter(|d| d.severity != Severity::None).map(|d| json!({"path": d.path, "kind": d.kind, "severity": d.severity, "expected": d.expected, "actual": d.actual})).collect::<Vec<_>>()})).collect();
    let found: Vec<gaps::Found> = rows.iter().map(|r| {
        let mut lines: Vec<String> = r["diffs"].as_array().unwrap().iter().map(|d| format!("{} {} {}", d["path"].as_str().unwrap(), d["kind"].as_str().unwrap(), d["severity"].as_str().unwrap())).collect();
        lines.extend(r["error"].as_str().map(|e| format!("error {e}")));
        gaps::Found { id: r["id"].as_str().unwrap().to_string(), lines }
    }).collect();
    // WF_MIMIC_WRITE_GAPS=<set>[,<set>...] lists what this replay found as that set's known gaps (to review before it is committed)
    if std::env::var("WF_MIMIC_WRITE_GAPS").is_ok_and(|v| v.split(',').any(|s| s == name)) {
        std::fs::write(&gaps_path, serde_json::to_string_pretty(&gaps::file(&found)).unwrap() + "\n").unwrap();
    }
    let mut listed = gaps::listed(&read(&gaps_path)).expect("known-gaps.json parses");
    // the machine's cpu count and load differ by runner: its two getHostResources rows are not compared
    let mut found = found;
    found.retain(|f| !f.id.contains("getHostResources"));
    listed.retain(|id, _| !id.contains("getHostResources"));
    let (new, stale) = gaps::compare(&found, &listed);
    let failure = (!new.is_empty() || !stale.is_empty()).then(|| format!("{name}: differences not in known-gaps.json:\n{}\nlisted gaps that no longer differ as listed:\n{stale:?}", new.join("\n")));
    // And it says what it found per row, for the report
    if let Some(out) = std::env::var_os("WF_TEST_OUTPUT_DIR").filter(|d| !d.is_empty()) {
        let dir = std::path::Path::new(&out).join("wf-mimic-t3-recorded").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        // the method trace `wf parity coverage --traces` reads (the shape `wf mimic replay --suite` writes): one test per compared interaction
        let tests: Vec<Value> = rows.iter().map(|r| json!({"test": format!("{name}::{}", r["id"].as_str().unwrap()), "cassette": "recorded", "verdict": if matches!(r["label"].as_str(), Some("clean" | "allowlisted")) { "pass" } else { "fail" }, "ms": 1,
            "calls": [{"id": r["id"], "rows": [r["row"]], "ok": matches!(r["label"].as_str(), Some("clean" | "allowlisted")), "diff": r["label"], "agent": [], "executor": []}]})).collect();
        let trace = json!({"schema": "method-trace/1", "app": "t3", "suite": "wf-mimic-t3-recorded", "sha": null, "complete": true, "tests": tests});
        std::fs::write(dir.join("trace.json"), serde_json::to_string_pretty(&trace).unwrap()).unwrap();
        let table: Vec<Value> = rows.into_iter().map(|mut r| (r["set"] = json!(name), r).1).collect();
        std::fs::write(dir.join("recorded-diff.json"), serde_json::to_string_pretty(&json!({"schema": "mimic-diff-table/1", "results": table})).unwrap()).unwrap();
    }
    assert!(failure.is_none(), "{}", failure.unwrap_or_default());
}

macro_rules! set_test {
    ($module:ident, $index:expr) => {
        mod $module {
            #[test]
            fn the_answers_the_real_server_recorded_replay_through_t3_serve_and_every_difference_is_a_listed_adapter_bug() {
                super::replay(super::SETS[$index]);
            }
        }
    };
}
set_test!(crawl, 0);
set_test!(recorded_1, 1);
set_test!(recorded_2, 2);
set_test!(recorded_3, 3);

#[test]
fn every_recorded_set_on_disk_has_a_test_above_and_so_a_task_of_its_own() {
    exec_testkit::case!(SUITE, |_case| {
        // Given the sets under cassettes/, When they are compared with the tests, Then a set with no test of its own would be replayed by nothing
        assert_eq!(recorded_sets(), SETS, "a recorded set needs its own set_test! line and its own [package.metadata.integration.groups] entry");
    });
}
