//! Every answer the real, pinned T3 server gave on the VM (the sets `cassettes/t3-v*-recorded-*`, kind `recorded`, one slice of the corpus each) replayed
//! through `wf-mimic mimic replay` against `t3-serve serve` over the whole chain, each interaction against a fresh chain as it was recorded against a fresh server. The inputs and the expected answers are the cassette's; nothing here names a method. A difference that is not a deliberate divergence
//! (the allowlist) is an adapter bug: it is listed in `known-gaps.json` with its row and severity until the adapter is fixed, so a new
//! difference fails and a fixed one has to leave the list. Each set is a test of its own, and an Nx task of its own (`[package.metadata.integration.groups]`).
use crate::rig::*;
use crate::support::mimic_t3;
use crate::support::*;

pub const CASSETTES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/cassettes");
/// The crawl set (every valid, boundary and invalid case of every endpoint): its reads share one chain and its state-changing cases another, in
/// cassette order, as the VM recording shared one server between them. The recorded slices run each interaction on a chain of its own.
const CRAWL: &str = "t3-v0.1.9-crawl";
const PIN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tooling/t3-conformance/t3-pin.json");
/// The sets that have a test, in the order `recorded_sets` lists them.
const SETS: [&str; 5] = [CRAWL, "t3-v0.1.9-recorded-1", "t3-v0.1.9-recorded-2", "t3-v0.1.9-recorded-3", "t3-v0.1.9-scenarios"];

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
    let mut sets: Vec<String> = std::fs::read_dir(CASSETTES).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("t3-v") && (n.contains("-recorded-") || n.ends_with("-scenarios")) || n == CRAWL).collect();
    sets.sort();
    assert!(!sets.is_empty(), "no recorded set under {CASSETTES}");
    sets
}

/// The `wf-mimic` binary: `WF_MIMIC_BIN`, else the one this workspace builds.
fn wf_mimic() -> &'static std::path::Path {
    static BIN: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    BIN.get_or_init(|| {
        if let Some(bin) = std::env::var_os("WF_MIMIC_BIN") {
            return bin.into();
        }
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let out = std::process::Command::new(env!("CARGO")).args(["build", "--locked", "-p", "wf-mimic", "--bin", "wf-mimic", "--message-format=json"]).current_dir(workspace).output().unwrap();
        assert!(out.status.success(), "cargo build wf-mimic: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).find_map(|m| m["executable"].as_str().map(std::path::PathBuf::from)).expect("cargo names the executable")
    })
}

/// Ids on one command line: a crawl group's whole `--only` list fits one argument, which a chain with a session prelude needs (a second process
/// would pair and create the prelude's project and thread a second time on the same chain).
const IDS_PER_CALL: usize = 2500;
const SUITE_NAME: &str = "wf-mimic-t3-recorded";

/// One call of `wf-mimic mimic replay` of `ids` against `chain`: its address, its repository as the server sees it and the pairing command with the
/// scopes the recorded session was granted. The call's trace (each call's window) is attributed from what the chain's tap saw, so the agent
/// methods that answered each interaction are in the trace `wf parity coverage --traces` reads.
pub fn mimic(dir: &str, ids: &[String], chain: &Chain, scopes: &[String], out: &std::path::Path) -> (Value, Value) {
    let workspace = std::fs::canonicalize(chain.repo.path()).unwrap().display().to_string();
    let only = ids.join(",");
    let output = std::process::Command::new(wf_mimic())
        .args(["mimic", "replay", dir, "--candidate", "t3-serve", "--only", &only, "--gaps", "check", "--suite", SUITE_NAME, "--addr", &chain.addr.to_string(), "--workspace", &workspace, "--mint", &chain.pair_command(scopes), "--cli-dir", &chain.cli.dir().display().to_string()])
        .env("WF_TEST_OUTPUT_DIR", out)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let run: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| panic!("wf-mimic printed no JSON ({e}): {}\n{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr), chain.evidence()));
    let trace_file = out.join(SUITE_NAME).join("trace.json");
    assert!(trace_file.is_file(), "wf-mimic wrote no trace: {run}\n{}", chain.evidence());
    let mut trace = read(&trace_file.display().to_string());
    let stamps = chain.tap.stamped();
    exec_testkit::attribute(&mut trace, &stamps);
    // Then each hit is in the window of the call that made it: the hits of the session prelude (before the first window) and of a push between two windows are nobody's
    let windows: Vec<(i64, i64)> = trace["tests"].as_array().into_iter().flatten().flat_map(|t| t["calls"].as_array().into_iter().flatten()).filter_map(|c| Some((c["startMs"].as_i64()?, c["endMs"].as_i64()?))).collect();
    let attributed: usize = trace["tests"].as_array().into_iter().flatten().flat_map(|t| t["calls"].as_array().into_iter().flatten()).map(|c| c["agent"].as_array().map_or(0, Vec::len) + c["executor"].as_array().map_or(0, Vec::len)).sum();
    let inside = stamps.iter().filter(|(at, _)| windows.iter().any(|(s, e)| (s..=e).contains(&at))).count();
    assert_eq!(attributed, inside, "a call holds exactly the hits stamped inside its window");
    assert!(!windows.is_empty() && windows.iter().all(|(s, e)| s <= e), "every call of the trace carries its window: {trace}");
    (run, trace)
}

/// One set's replay. Its trace and diff table go to `wf-mimic-t3-recorded/<set>/`: a task writes its own set only, and `wf parity coverage --traces`
/// reads every `trace.json` under the output dir.
fn replay(name: &str) {
    let dir = format!("{CASSETTES}/{name}");
    // Given the set the pinned server recorded on the VM, stamped with the hash of the pin file as it is now
    let set = mimic_t3::load_set(&dir);
    let header = read(&format!("{dir}/cassette.json"));
    assert_eq!((header["provenance"]["kind"].clone(), header["provenance"]["target"]["pin_hash"].clone()), (json!("recorded"), json!(pin_hash())), "{name}: {header}");
    let scopes = mimic_t3::granted_scopes(&set);
    // an interaction the two recording runs disagreed on is noise, not a reference answer: it is not compared
    let stable = |i: &&mimic_core::cassette::Interaction| !i.tags.iter().any(|t| t == "nondeterministic");
    let has = |i: &mimic_core::cassette::Interaction, tag: &str| i.tags.iter().any(|t| t == tag);
    let (shared, own): (Vec<_>, Vec<_>) = set.cassette.interactions.iter().filter(stable).partition(|i| name == CRAWL && !has(i, "state:fresh"));
    let (writes, reads): (Vec<_>, Vec<_>) = shared.into_iter().partition(|i| has(i, "state:write"));
    let ids = |v: Vec<&mimic_core::cassette::Interaction>| v.into_iter().map(|i| i.id.clone()).collect::<Vec<_>>();
    let groups: Vec<Vec<String>> = [ids(reads), ids(writes)].into_iter().filter(|g| !g.is_empty()).chain(own.into_iter().map(|i| vec![i.id.clone()])).collect();
    // When wf-mimic replays every group against a chain of its own: t3-serve wired to the real rca-service and rca-exec
    let (mut results, mut tests): (Vec<Value>, Vec<Value>) = (Vec::new(), Vec::new());
    let (mut new, mut stale): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    let scratch = std::env::temp_dir().join(format!("t3-serve-recorded-{name}-{}", std::process::id()));
    for group in &groups {
        let chain = Chain::start();
        for ids in group.chunks(IDS_PER_CALL) {
            let _ = std::fs::remove_dir_all(&scratch);
            let (run, trace) = mimic(&dir, ids, &chain, &scopes, &scratch);
            let listed = |key: &str| run["gaps"][key].as_array().into_iter().flatten().filter_map(|n| n.as_str().map(String::from)).collect::<Vec<_>>();
            assert!(run["results"].is_array() && run["gaps"]["mode"] == "check", "no results or no gaps comparison: {run}\n{}", chain.evidence());
            results.extend(run["results"].as_array().cloned().unwrap_or_default());
            new.extend(listed("new"));
            stale.extend(listed("stale"));
            tests.extend(trace["tests"].as_array().cloned().unwrap_or_default());
        }
    }
    std::fs::remove_dir_all(&scratch).ok();
    // Then the answers were made by the agent API: a call's window holds the agent methods that answered it
    let held = tests.iter().flat_map(|t| t["calls"].as_array().into_iter().flatten()).filter(|c| c["agent"].as_array().is_some_and(|a| !a.is_empty())).count();
    assert!(held > 0, "{name}: no call's window holds an agent hit, so the attribution is empty");
    // And the interactions with a difference are exactly the listed gaps, each with the differences it was listed for (the verb compared them);
    // the machine's cpu count and load differ by runner: its two getHostResources rows are not compared
    new.retain(|n| !n.contains("getHostResources"));
    stale.retain(|n| !n.contains("getHostResources"));
    let failure = (!new.is_empty() || !stale.is_empty()).then(|| format!("{name}: differences not in known-gaps.json:\n{}\nlisted gaps that no longer differ as listed:\n{stale:?}", new.join("\n")));
    // And it says what it found per row, for the report
    if let Some(out) = std::env::var_os("WF_TEST_OUTPUT_DIR").filter(|d| !d.is_empty()) {
        let dir = std::path::Path::new(&out).join(SUITE_NAME).join(name);
        std::fs::create_dir_all(&dir).unwrap();
        // the method trace `wf parity coverage --traces` reads (the shape `wf mimic replay --suite` writes, each call with the agent methods that answered it)
        let trace = json!({"schema": "method-trace/1", "app": "t3", "suite": SUITE_NAME, "sha": null, "complete": true, "tests": tests});
        std::fs::write(dir.join("trace.json"), serde_json::to_string_pretty(&trace).unwrap()).unwrap();
        let table: Vec<Value> = results.into_iter().map(|mut r| (r["set"] = json!(name), r).1).collect();
        std::fs::write(dir.join("recorded-diff.json"), serde_json::to_string_pretty(&json!({"schema": "mimic-diff-table/1", "compared": tests.len(), "results": table})).unwrap()).unwrap();
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
set_test!(scenarios, 4);

#[test]
fn every_recorded_set_on_disk_has_a_test_above_and_so_a_task_of_its_own() {
    exec_testkit::case!(SUITE, |_case| {
        // Given the sets under cassettes/, When they are compared with the tests, Then a set with no test of its own would be replayed by nothing
        assert_eq!(recorded_sets(), SETS, "a recorded set needs its own set_test! line and its own [package.metadata.integration.groups] entry");
    });
}
