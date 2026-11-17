//! Every answer the real, pinned T3 server gave on the VM (the sets named in `[package.metadata.integration.replay] sets`, kind `recorded`) replayed
//! through `wf-mimic mimic replay` against `t3-serve serve` over the whole chain, each interaction against a fresh chain as it was recorded against a
//! fresh server, one feature's rows at a time. `t3-core` names the rows of each feature (`[package.metadata.integration.replay.<feature>]`) and this
//! file has a test of its own for each, so a change confined to one feature's files reruns its rows alone (`wf test plan`: the Nx task
//! `replay-t3-core-<feature>`). The inputs and the expected answers are the cassette's; nothing here names a method. A difference that is not a
//! deliberate divergence (the allowlist) is an adapter bug: it is listed in `known-gaps.json` with its row and severity until the adapter is fixed,
//! so a new difference fails and a fixed one has to leave the list.
use crate::rig::*;
use crate::support::mimic_t3;
use crate::support::*;

pub const CASSETTES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/cassettes");
/// The crawl set (every valid, boundary and invalid case of every endpoint): its reads share one chain and its state-changing cases another, in
/// cassette order, as the VM recording shared one server between them. The recorded slices run each interaction on a chain of its own.
const CRAWL: &str = "t3-v0.1.9-crawl";
const PIN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tooling/t3-conformance/t3-pin.json");

fn replay_table(manifest: &str) -> toml::Table {
    let doc: toml::Table = std::fs::read_to_string(manifest).unwrap_or_else(|e| panic!("{manifest}: {e}")).parse().unwrap();
    doc["package"]["metadata"]["integration"]["replay"].as_table().cloned().unwrap_or_else(|| panic!("{manifest} has no [package.metadata.integration.replay]"))
}

fn strings(v: &toml::Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(toml::Value::as_str).map(String::from).collect()
}

/// The sets the candidate replays, from its own manifest.
fn sets() -> Vec<String> {
    strings(&replay_table(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))["sets"])
}

/// The features of t3-core: `t3_core_<feature>` and its row prefixes.
fn features() -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = replay_table(concat!(env!("CARGO_MANIFEST_DIR"), "/core/Cargo.toml")).into_iter().filter_map(|(name, f)| Some((format!("t3_core_{name}"), strings(f.get("rows")?)))).collect();
    out.sort();
    out
}

fn claims(rows: &[String], row: &str) -> bool {
    rows.iter().any(|p| row.starts_with(p.as_str()))
}

fn read(path: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"))).unwrap()
}

/// `sha256:` of a file, the way `wf mimic record` stamps the pin.
fn pin_hash() -> String {
    let out = std::process::Command::new("sha256sum").arg(PIN).output().expect("sha256sum runs");
    format!("sha256:{}", String::from_utf8_lossy(&out.stdout).split_whitespace().next().unwrap())
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
/// methods that answered each interaction are in the trace `wf mimic coverage --traces` reads.
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

/// One feature's replay: its rows of every set. Its trace and diff table go to `wf-mimic-t3-recorded/<feature>/<set>/`: a task writes its own feature only, and
/// `wf mimic coverage --traces` reads every `trace.json` under the output dir.
fn replay(id: &str) {
    let rows = features().into_iter().find(|(f, _)| f == id).unwrap_or_else(|| panic!("no feature {id}")).1;
    let mut compared = 0;
    let mut failures = Vec::new();
    for name in sets() {
        compared += replay_set(id, &rows, &name, &mut failures);
    }
    assert!(compared > 0, "{id}: no interaction of the sets has a row starting with {rows:?}");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn replay_set(id: &str, rows: &[String], name: &str, failures: &mut Vec<String>) -> usize {
    let dir = format!("{CASSETTES}/{name}");
    // Given the set the pinned server recorded on the VM, stamped with the hash of the pin file as it is now
    let set = mimic_t3::load_set(&dir);
    let header = read(&format!("{dir}/cassette.json"));
    assert_eq!((header["provenance"]["kind"].clone(), header["provenance"]["target"]["pin_hash"].clone()), (json!("recorded"), json!(pin_hash())), "{name}: {header}");
    let scopes = mimic_t3::granted_scopes(&set);
    // an interaction the two recording runs disagreed on is noise, not a reference answer: it is not compared; nor is one of another feature's rows
    let stable = |i: &&mimic_core::cassette::Interaction| !i.tags.iter().any(|t| t == "nondeterministic") && claims(rows, i.row.as_deref().unwrap_or_default());
    let has = |i: &mimic_core::cassette::Interaction, tag: &str| i.tags.iter().any(|t| t == tag);
    let (shared, own): (Vec<_>, Vec<_>) = set.cassette.interactions.iter().filter(stable).partition(|i| name == CRAWL && !has(i, "state:fresh"));
    let chosen = shared.len() + own.len();
    if chosen == 0 {
        return 0;
    }
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
    // And the interactions with a difference are exactly the listed gaps, each with the differences it was listed for (the verb compared them);
    // the machine's cpu count and load differ by runner: its two getHostResources rows are not compared
    new.retain(|n| !n.contains("getHostResources"));
    stale.retain(|n| !n.contains("getHostResources"));
    let failure = (!new.is_empty() || !stale.is_empty()).then(|| format!("{name}: differences not in known-gaps.json:\n{}\nlisted gaps that no longer differ as listed:\n{stale:?}", new.join("\n")));
    // And it says what it found per row, for the report
    if let Some(out) = std::env::var_os("WF_TEST_OUTPUT_DIR").filter(|d| !d.is_empty()) {
        let dir = std::path::Path::new(&out).join(SUITE_NAME).join(id).join(name);
        std::fs::create_dir_all(&dir).unwrap();
        // the method trace `wf mimic coverage --traces` reads (the shape `wf mimic replay --suite` writes, each call with the agent methods that answered it)
        let trace = json!({"schema": "method-trace/1", "app": "t3", "suite": SUITE_NAME, "sha": null, "complete": true, "tests": tests});
        std::fs::write(dir.join("trace.json"), serde_json::to_string_pretty(&trace).unwrap()).unwrap();
        let table: Vec<Value> = results.into_iter().map(|mut r| (r["set"] = json!(name), r).1).collect();
        std::fs::write(dir.join("recorded-diff.json"), serde_json::to_string_pretty(&json!({"schema": "mimic-diff-table/1", "compared": tests.len(), "results": table})).unwrap()).unwrap();
    }
    failures.extend(failure);
    chosen
}

macro_rules! feature_test {
    ($($module:ident),* $(,)?) => {
        const TESTED: &[&str] = &[$(stringify!($module)),*];
        $(
            mod $module {
                #[test]
                fn the_answers_the_real_server_recorded_for_these_rows_replay_through_t3_serve_and_every_difference_is_a_listed_adapter_bug() {
                    super::replay(stringify!($module));
                }
            }
        )*
    };
}
feature_test!(t3_core_attachments, t3_core_core, t3_core_preview, t3_core_pull_requests, t3_core_schedules, t3_core_terminal, t3_core_vcs, t3_core_workspace);

#[test]
fn every_feature_t3_core_declares_has_a_test_above_and_so_a_task_of_its_own() {
    exec_testkit::case!(SUITE, |_case| {
        // Given the features in t3-core's manifest, When they are compared with the tests, Then a feature with no test would be replayed by nothing
        let declared: Vec<String> = features().into_iter().map(|(id, _)| id).collect();
        assert_eq!(declared, TESTED, "a feature needs its own feature_test! entry");
    });
}

#[test]
fn every_row_of_every_replayed_set_is_claimed_by_exactly_one_feature() {
    exec_testkit::case!(SUITE, |_case| {
        // Given the rows each feature declares and the rows of the sets, Then none is left out and none is replayed twice
        let features = features();
        let mut wrong = std::collections::BTreeSet::new();
        for name in sets() {
            for i in mimic_t3::load_set(&format!("{CASSETTES}/{name}")).cassette.interactions {
                let row = i.row.clone().unwrap_or_default();
                let n = features.iter().filter(|(_, rows)| claims(rows, &row)).count();
                if n != 1 {
                    wrong.insert(format!("{name}: {row:?} claimed by {n}"));
                }
            }
        }
        assert!(wrong.is_empty(), "{wrong:?}");
    });
}
