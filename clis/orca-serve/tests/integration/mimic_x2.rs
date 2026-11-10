//! The recorded Orca mobile bundle (`wf mimic` cassette of the pinned Orca contract) replayed against the host's window serving, with the run
//! record the integration stage publishes (`$WF_TEST_OUTPUT_DIR/wf-mimic-orca-mobile-bundle`, `report.json` and the method trace beside it):
//! `wf parity coverage --app orca --traces` reads that trace, so the evidence of a mimic-backed row is made in CI and not only on a desk.
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const SUITE: &str = "wf-mimic-orca-mobile-bundle";

pub(crate) fn clis() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

pub(crate) fn executable(args: &[&str]) -> PathBuf {
    let out = Command::new(env!("CARGO")).args(args).arg("--message-format=json").current_dir(clis()).output().unwrap();
    assert!(out.status.success(), "cargo {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).find_map(|m| m["executable"].as_str().map(PathBuf::from)).expect("cargo names the executable")
}

#[test]
fn given_the_recorded_mobile_bundle_when_the_host_serves_its_windows_then_the_run_record_names_each_row_with_a_clean_diff_on_a_recorded_cassette() {
    // Given the recorded set, the interactions that name a parity row and the host's driver as the candidate
    let set = Path::new(env!("CARGO_MANIFEST_DIR")).join("map-mobile/fixtures/orca-mobile-bundle");
    let rows: Vec<String> = std::fs::read_to_string(set.join("interactions.jsonl")).unwrap().lines().map(|l| serde_json::from_str::<Value>(l).unwrap()).filter(|i| i.get("row").is_some()).map(|i| i["id"].as_str().unwrap().to_string()).collect();
    assert!(!rows.is_empty());
    let scratch = std::env::temp_dir().join(format!("orca-serve-mimic-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    let out_dir = std::env::var_os("WF_TEST_OUTPUT_DIR").filter(|d| !d.is_empty()).map(PathBuf::from).unwrap_or_else(|| scratch.join("out"));
    let host = executable(&["build", "-p", "orca-map-mobile", "--example", "mimic_orca_bundle"]);
    let target = scratch.join("candidates.toml");
    std::fs::write(&target, format!("[candidate.host]\ncommand = [{:?}]\n", host.display().to_string())).unwrap();
    let bin = std::env::var_os("WF_MIMIC_BIN").map(PathBuf::from).unwrap_or_else(|| executable(&["build", "--locked", "-p", "wf-mimic", "--bin", "wf-mimic"]));

    // When wf-mimic replays them as a suite of this run
    let out = Command::new(&bin)
        .args(["mimic", "replay"])
        .arg(&set)
        .arg("--target")
        .arg(&target)
        .args(["--candidate", "host", "--only", &rows.join(","), "--suite", SUITE])
        .env("WF_TEST_OUTPUT_DIR", &out_dir)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let replay: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("wf-mimic printed no JSON ({e}): {}", String::from_utf8_lossy(&out.stdout)));

    // Then every window is clean, and the record beside the others carries the trace with the row, the clean diff and the cassette's kind
    assert_eq!((out.status.code(), replay["severity"].clone()), (Some(0), json!("none")), "{replay}");
    let trace: Value = serde_json::from_str(&std::fs::read_to_string(out_dir.join(SUITE).join("trace.json")).unwrap()).unwrap();
    assert_eq!(trace["tests"][0]["cassette"], "recorded");
    let calls: Vec<Value> = trace["tests"].as_array().unwrap().iter().flat_map(|t| t["calls"].as_array().unwrap().clone()).collect();
    assert_eq!(calls.len(), rows.len());
    assert!(calls.iter().all(|c| c["diff"] == "clean" && c["rows"].as_array().is_some_and(|r| r.len() == 1)), "{trace}");
    assert!(out_dir.join(SUITE).join("report.json").is_file());
    std::fs::remove_dir_all(&scratch).ok();
}
