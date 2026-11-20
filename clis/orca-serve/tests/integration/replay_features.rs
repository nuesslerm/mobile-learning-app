//! Every answer the real headless Orca host (`orcad`, built at the pin on the VM) gave to the phone's transport (the sets named in
//! `[package.metadata.integration.replay] sets`, kind `recorded`) replayed against `orca-serve`, one mapping crate's rows at a time. Each mapping
//! crate names the rows it answers in its own `[package.metadata.integration.replay] rows`, and this file has a test of its own for each, so a change
//! to one crate reruns its rows alone (`wf test plan`: the Nx task `replay-<crate>`). Inputs and expected answers are the cassette's. A difference that
//! is not deliberate (the allowlist) is an adapter bug: it is listed in `known-gaps.json` with its row until fixed, so a new difference fails and a
//! fixed one must leave.
use crate::mimic_x6::{mimic, out_dir, read_json, root, scratch};
use crate::rig::*;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;

fn candidate_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// `sha256:` of the pin file, the way `wf mimic record` stamps it.
fn pin_hash() -> String {
    let out = Command::new("sha256sum").arg(root().join("tooling/orca-conformance/orca-pin.json")).output().unwrap();
    format!("sha256:{}", String::from_utf8_lossy(&out.stdout).split_whitespace().next().unwrap())
}

const CRAWL: &str = "orca-v1.4.259-crawl";
/// Ids on one command line: the `--only` list of a whole crawl would not fit an argument.
const IDS_PER_CALL: usize = 1500;

fn replay_table(manifest: &Path) -> Option<toml::Table> {
    let text = std::fs::read_to_string(manifest).ok()?;
    let doc: toml::Table = text.parse().ok()?;
    doc.get("package")?.get("metadata")?.get("integration")?.get("replay")?.as_table().cloned()
}

fn strings(table: &toml::Table, key: &str) -> Vec<String> {
    table.get(key).and_then(toml::Value::as_array).into_iter().flatten().filter_map(toml::Value::as_str).map(String::from).collect()
}

/// The sets the candidate replays, from its own manifest.
fn sets() -> Vec<String> {
    strings(&replay_table(&candidate_dir().join("Cargo.toml")).expect("orca-serve declares [package.metadata.integration.replay]"), "sets")
}

/// The features: the id of each mapping crate that declares rows (its package name with `_`) and its row prefixes.
fn features() -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = std::fs::read_dir(candidate_dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let manifest = e.path().join("Cargo.toml");
            let rows = strings(&replay_table(&manifest)?, "rows");
            let doc: toml::Table = std::fs::read_to_string(&manifest).ok()?.parse().ok()?;
            let name = doc["package"]["name"].as_str()?.replace('-', "_");
            (!rows.is_empty()).then_some((name, rows))
        })
        .collect();
    out.sort();
    out
}

fn claims(rows: &[String], row: &str) -> bool {
    rows.iter().any(|p| row.starts_with(p.as_str()))
}

fn has(i: &serde_json::Value, tag: &str) -> bool {
    i["tags"].as_array().is_some_and(|t| t.iter().any(|x| x == tag))
}

fn interactions(set: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(set.join("interactions.jsonl")).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

fn replay(id: &str) {
    let started = std::time::Instant::now();
    let rows = features().into_iter().find(|(name, _)| name == id).unwrap_or_else(|| panic!("no crate declares replay rows as {id}")).1;
    let base = candidate_dir().join("cassettes");
    let scratch_dir = scratch("recorded");
    let out_root = out_dir(&scratch_dir);
    let out = out_root.join(id);
    let (mut table, mut failures) = (Vec::new(), Vec::new());
    let mut trace_tests: Vec<serde_json::Value> = Vec::new();
    let mut compared_total = 0;
    for name in sets() {
        // Given the set orcad recorded on the VM, stamped with the hash of the pin file as it is now, and the list of known gaps
        let set = base.join(&name);
        let header = read_json(&set.join("cassette.json"));
        assert_eq!((header["provenance"]["kind"].clone(), header["provenance"]["target"]["pin_hash"].clone()), (json!("recorded"), json!(pin_hash())), "{header}");
        // (an interaction the recording runs disagreed on is noise, not a reference answer: it is not compared), of this crate's rows
        let interactions: Vec<serde_json::Value> = interactions(&set).into_iter().filter(|i| !has(i, "nondeterministic")).filter(|i| claims(&rows, i["row"].as_str().unwrap_or_default())).collect();
        if interactions.is_empty() {
            continue;
        }
        compared_total += interactions.len();
        let ids = |keep: &dyn Fn(&serde_json::Value) -> bool| interactions.iter().filter(|i| keep(i)).map(|i| i["id"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        // the crawl's reads share one host and its state-changing cases another, in cassette order, as the VM recording shared one server
        let groups = if name == CRAWL { vec![ids(&|i| !has(i, "state:write")), ids(&|i| has(i, "state:write"))] } else { vec![ids(&|_| true)] };
        let mut results = Vec::new();
        let (mut new, mut stale): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
        // When wf-mimic replays them against orca-serve, the phone's transport driven by the link orca-serve pair printed
        for group in groups.iter().filter(|g| !g.is_empty()) {
            let chain = Chain::start_recorded();
            for ids in group.chunks(IDS_PER_CALL) {
                let only = ids.join(",");
                let mut args = vec!["replay", set.to_str().unwrap(), "--candidate", "orca-serve", "--only", &only, "--gaps", "check"];
                // the one-call recorded set leaves its own method trace; the crawl's is written below from every interaction
                if name != CRAWL {
                    args.extend(["--suite", "wf-mimic-orca-recorded"]);
                }
                let run = mimic(&args, &chain.pair_url(), &out);
                let found = run.json["results"].as_array().unwrap_or_else(|| panic!("no results: {}\n{}", run.json, chain.evidence())).clone();
                results.extend(found);
                new.extend(run.json["gaps"]["new"].as_array().into_iter().flatten().filter_map(|n| n.as_str().map(String::from)));
                stale.extend(run.json["gaps"]["stale"].as_array().into_iter().flatten().filter_map(|n| n.as_str().map(String::from)));
            }
        }
        // Then the interactions with a difference are exactly the listed gaps, each with the differences it was listed for (the verb compared them)
        if !new.is_empty() || !stale.is_empty() {
            failures.push(format!("{name}: differences not in known-gaps.json:\n{}\nlisted gaps that no longer differ as listed:\n{stale:?}", new.join("\n")));
        }
        if name == CRAWL {
            // the method trace `wf mimic coverage --traces` reads (the shape `wf mimic replay --suite` writes): one test per compared interaction, clean unless listed
            let label = |id: &str| results.iter().find(|r| r["id"] == id).map_or("clean", |r| r["label"].as_str().unwrap_or("breaking")).to_string();
            let tests: Vec<serde_json::Value> = interactions
                .iter()
                .map(|i| {
                    let (id, diff) = (i["id"].as_str().unwrap(), label(i["id"].as_str().unwrap()));
                    let ok = matches!(diff.as_str(), "clean" | "allowlisted");
                    json!({"test": format!("{name}::{id}"), "cassette": "recorded", "verdict": if ok { "pass" } else { "fail" }, "ms": 1, "calls": [{"id": id, "rows": [i["row"]], "ok": ok, "diff": diff, "agent": [], "executor": []}]})
                })
                .collect();
            trace_tests.extend(tests);
        }
        table.push(json!({"set": name, "compared": interactions.len(), "differing": results.len(), "results": results}));
    }
    std::fs::remove_dir_all(&scratch_dir).ok();
    // A crate that declares rows no recorded set holds would replay nothing and pass
    assert!(compared_total > 0, "{id}: no interaction of the sets {:?} has a row starting with {rows:?}", sets());
    if std::env::var_os("WF_TEST_OUTPUT_DIR").is_some_and(|d| !d.is_empty()) {
        let dir = out.join("orca-serve-integration");
        std::fs::create_dir_all(&dir).unwrap();
        if !trace_tests.is_empty() {
            let failure = (!failures.is_empty()).then(|| failures.join("\n"));
            exec_testkit::write_replay_record(&out.join("wf-mimic-orca-crawl"), "wf-mimic-orca-crawl", "orca", &trace_tests, failure.as_deref(), started).unwrap();
        }
        std::fs::write(dir.join("recorded-diff.json"), serde_json::to_string_pretty(&json!({"schema": "mimic-diff-table/1", "feature": id, "sets": table})).unwrap()).unwrap();
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

macro_rules! feature_test {
    ($($module:ident),* $(,)?) => {
        const TESTED: &[&str] = &[$(stringify!($module)),*];
        $(
            mod $module {
                #[test]
                fn the_answers_the_real_host_recorded_for_these_rows_replay_through_orca_serve_and_every_difference_is_a_listed_adapter_bug() {
                    super::replay(stringify!($module));
                }
            }
        )*
    };
}
feature_test!(orca_core, orca_map_accounts, orca_map_agent, orca_map_browser, orca_map_git, orca_map_hosted, orca_map_mobile, orca_map_schedules, orca_map_settings, orca_map_state, orca_map_terminal, orca_map_workspace);

#[test]
fn every_crate_that_declares_replay_rows_has_a_test_above_and_so_a_task_of_its_own() {
    // Given the crates under orca-serve with a `[package.metadata.integration.replay] rows`, When they are compared with the tests, Then a crate with no test would be replayed by nothing
    let declared: Vec<String> = features().into_iter().map(|(id, _)| id).collect();
    assert_eq!(declared, TESTED, "a crate that declares replay rows needs its own feature_test! entry");
}

#[test]
fn every_row_of_every_replayed_set_is_claimed_by_a_crate() {
    // Given the rows each crate declares and the rows the sets hold, Then none is left out: a row nobody claims is replayed by no task
    let features = features();
    let mut unclaimed = std::collections::BTreeSet::new();
    for name in sets() {
        for i in interactions(&candidate_dir().join("cassettes").join(&name)) {
            let row = i["row"].as_str().unwrap_or_default();
            if !features.iter().any(|(_, rows)| claims(rows, row)) {
                unclaimed.insert(format!("{name}: {row:?}"));
            }
        }
    }
    assert!(unclaimed.is_empty(), "rows no crate's [package.metadata.integration.replay] rows claims: {unclaimed:?}");
}
