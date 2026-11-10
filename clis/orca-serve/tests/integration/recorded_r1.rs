//! Every answer the real headless Orca host (`orcad`, built at the pin on the VM) gave to the phone's transport (`cassettes/orca-recorded`,
//! kind `recorded`) replayed against `orca-serve`. Inputs and expected answers are the cassette's. A difference that is not deliberate (the
//! allowlist) is an adapter bug: it is listed in `known-gaps.json` with its row until fixed, so a new difference fails and a fixed one must leave.
use crate::mimic_x6::{mimic, out_dir, read_json, root, scratch};
use crate::rig::*;
use serde_json::json;
use mimic_core::gaps;
use std::path::Path;
use std::process::Command;

pub fn set_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("cassettes/orca-recorded")
}

/// `sha256:` of the pin file, the way `wf mimic record` stamps it.
fn pin_hash() -> String {
    let out = Command::new("sha256sum").arg(root().join("tooling/orca-conformance/orca-pin.json")).output().unwrap();
    format!("sha256:{}", String::from_utf8_lossy(&out.stdout).split_whitespace().next().unwrap())
}

const CRAWL: &str = "orca-crawl";
/// Ids on one command line: the `--only` list of a whole crawl would not fit an argument.
const IDS_PER_CALL: usize = 1500;

fn sets() -> Vec<&'static str> {
    vec!["orca-recorded", CRAWL]
}

#[test]
fn the_answers_the_real_host_recorded_replay_through_orca_serve_and_every_difference_is_a_listed_adapter_bug() {
    let base = set_dir().parent().unwrap().to_path_buf();
    let (mut table, mut failures) = (Vec::new(), Vec::new());
    let mut trace_tests: Vec<serde_json::Value> = Vec::new();
    for name in sets() {
        // Given the set orcad recorded on the VM, stamped with the hash of the pin file as it is now, and the list of known gaps
        let set = base.join(name);
        let header = read_json(&set.join("cassette.json"));
        assert_eq!((header["provenance"]["kind"].clone(), header["provenance"]["target"]["pin_hash"].clone()), (json!("recorded"), json!(pin_hash())), "{header}");
        let gaps_path = set.join("known-gaps.json");
        // (an interaction the recording runs disagreed on is noise, not a reference answer: it is not compared)
        let interactions: Vec<serde_json::Value> = std::fs::read_to_string(set.join("interactions.jsonl")).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).filter(|i: &serde_json::Value| !has(i, "nondeterministic")).collect();
        let ids = |keep: &dyn Fn(&serde_json::Value) -> bool| interactions.iter().filter(|i| keep(i)).map(|i| i["id"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        // the crawl's reads share one host and its state-changing cases another, in cassette order, as the VM recording shared one server
        let groups = if name == CRAWL { vec![ids(&|i| !has(i, "state:write")), ids(&|i| has(i, "state:write"))] } else { vec![ids(&|_| true)] };
        let mut results = Vec::new();
        // When wf-mimic replays them against orca-serve, the phone's transport driven by the link orca-serve pair printed
        for group in groups.iter().filter(|g| !g.is_empty()) {
            let chain = Chain::start(None);
            let scratch = scratch("recorded");
            let out = out_dir(&scratch);
            for ids in group.chunks(IDS_PER_CALL) {
                let only = ids.join(",");
                let mut args = vec!["replay", set.to_str().unwrap(), "--candidate", "orca-serve", "--only", &only];
                // the one-call recorded set leaves its own method trace; the crawl's is written below from every interaction
                if name != CRAWL { args.extend(["--suite", "wf-mimic-orca-recorded"]); }
                let run = mimic(&args, &chain.pair_url(), &out);
                let found = run.json["results"].as_array().unwrap_or_else(|| panic!("no results: {}\n{}", run.json, chain.evidence())).clone();
                results.extend(found);
            }
            std::fs::remove_dir_all(&scratch).ok();
        }
        // Then the interactions with a difference (the report lists only those) are exactly the listed gaps, each with the differences it was listed for
        let found: Vec<gaps::Found> = results.iter().map(|r| {
            let mut lines: Vec<String> = r["diffs"].as_array().into_iter().flatten().filter(|d| d["severity"] != "none").map(|d| format!("{} {} {}", d["path"].as_str().unwrap(), d["kind"].as_str().unwrap(), d["severity"].as_str().unwrap())).collect();
            lines.extend(r["error"].as_str().map(|e| format!("error {e}")));
            gaps::Found { id: r["id"].as_str().unwrap().to_string(), lines }
        }).collect();
        // WF_MIMIC_WRITE_GAPS=<set> lists what this replay found as that set's known gaps (to review before it is committed)
        if std::env::var("WF_MIMIC_WRITE_GAPS").is_ok_and(|v| v == name) {
            std::fs::write(&gaps_path, serde_json::to_string_pretty(&gaps::file(&found)).unwrap() + "\n").unwrap();
        }
        let (new, stale) = gaps::compare(&found, &gaps::listed(&read_json(&gaps_path)).expect("known-gaps.json parses"));
        if !new.is_empty() || !stale.is_empty() {
            failures.push(format!("{name}: differences not in known-gaps.json:\n{}\nlisted gaps that no longer differ as listed:\n{stale:?}", new.join("\n")));
        }
        if name == CRAWL {
            // the method trace `wf parity coverage --traces` reads (the shape `wf mimic replay --suite` writes): one test per compared interaction, clean unless listed
            let label = |id: &str| results.iter().find(|r| r["id"] == id).map_or("clean", |r| r["label"].as_str().unwrap_or("breaking")).to_string();
            let tests: Vec<serde_json::Value> = interactions.iter().map(|i| {
                let (id, diff) = (i["id"].as_str().unwrap(), label(i["id"].as_str().unwrap()));
                let ok = matches!(diff.as_str(), "clean" | "allowlisted");
                json!({"test": format!("{name}::{id}"), "cassette": "recorded", "verdict": if ok { "pass" } else { "fail" }, "ms": 1, "calls": [{"id": id, "rows": [i["row"]], "ok": ok, "diff": diff, "agent": [], "executor": []}]})
            }).collect();
            trace_tests.extend(tests);
        }
        table.push(json!({"set": name, "compared": interactions.len(), "differing": results.len(), "results": results}));
    }
    if let Some(dir) = std::env::var_os("WF_TEST_OUTPUT_DIR").filter(|d| !d.is_empty()) {
        let dir = Path::new(&dir).join("orca-serve-integration");
        std::fs::create_dir_all(&dir).unwrap();
        let trace = json!({"schema": "method-trace/1", "app": "orca", "suite": "wf-mimic-orca-crawl", "sha": null, "complete": true, "tests": trace_tests});
        std::fs::create_dir_all(dir.parent().unwrap().join("wf-mimic-orca-crawl")).unwrap();
        std::fs::write(dir.parent().unwrap().join("wf-mimic-orca-crawl/trace.json"), serde_json::to_string_pretty(&trace).unwrap()).unwrap();
        std::fs::write(dir.join("recorded-diff.json"), serde_json::to_string_pretty(&json!({"schema": "mimic-diff-table/1", "sets": table})).unwrap()).unwrap();
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn has(i: &serde_json::Value, tag: &str) -> bool {
    i["tags"].as_array().is_some_and(|t| t.iter().any(|x| x == tag))
}
