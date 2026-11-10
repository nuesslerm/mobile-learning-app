//! The voice-edit DSL cassette replayed by `wf-mimic` against the Rust port, with the run record the integration stage publishes
//! (`$WF_TEST_OUTPUT_DIR/wf-mimic-voice-edit-dsl`: `report.json` and the method trace): the flow test (tests/flow/voice.rs) proves the replay
//! and the planted defects, this one leaves the evidence of every recorded case, property cases and their shrink ladders included, in CI.
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const SET: &str = "tests/cassettes/voice-edit-dsl";
const SUITE: &str = "wf-mimic-voice-edit-dsl";

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn cargo_json(args: &[&str], dir: &Path) -> Vec<Value> {
    let out = Command::new(env!("CARGO")).args(args).arg("--message-format=json").current_dir(dir).output().unwrap();
    assert!(out.status.success(), "cargo {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).collect()
}

/// `wf-mimic` from WF_MIMIC_BIN, or built from the clis workspace into a build directory of its own, behind a lock.
fn mimic_bin() -> PathBuf {
    if let Some(bin) = std::env::var_os("WF_MIMIC_BIN") {
        return PathBuf::from(bin);
    }
    let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| std::env::current_exe().unwrap().ancestors().nth(3).unwrap().to_path_buf());
    let build_dir = target.join("wf-mimic-build");
    std::fs::create_dir_all(&build_dir).unwrap();
    let lock = std::fs::File::create(build_dir.join(".build.lock")).unwrap();
    lock.lock().unwrap();
    let manifest = root().join("../../../clis/Cargo.toml");
    let status = Command::new(env!("CARGO")).args(["build", "--locked", "--quiet", "-p", "wf-mimic", "--bin", "wf-mimic", "--manifest-path"]).arg(manifest).arg("--target-dir").arg(&build_dir).status().unwrap();
    assert!(status.success(), "building wf-mimic failed");
    build_dir.join("debug").join("wf-mimic")
}

#[test]
fn given_the_dsl_cassette_when_the_port_replays_it_as_a_suite_then_every_case_is_clean_and_the_run_record_holds_the_trace() {
    // Given the cassette, the port behind its driver and a directory for the run record
    let scratch = std::env::temp_dir().join(format!("rca-service-mimic-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    let out_dir = std::env::var_os("WF_TEST_OUTPUT_DIR").filter(|d| !d.is_empty()).map(PathBuf::from).unwrap_or_else(|| scratch.join("out"));
    let built = cargo_json(&["build", "-p", "rca-service-core", "--example", "mimic_voice_dsl"], root());
    let exe = built.iter().find_map(|m| m["executable"].as_str().map(PathBuf::from)).expect("cargo names the example's executable");
    let target = scratch.join("candidates.toml");
    std::fs::write(&target, format!("[candidate.port]\ncommand = [{:?}]\n", exe.display().to_string())).unwrap();
    let recorded = std::fs::read_to_string(root().join(SET).join("interactions.jsonl")).unwrap().lines().count();

    // When wf-mimic replays every recorded input
    let out = Command::new(mimic_bin())
        .args(["mimic", "replay"])
        .arg(root().join(SET))
        .arg("--target")
        .arg(&target)
        .args(["--candidate", "port", "--suite", SUITE])
        .env("WF_TEST_OUTPUT_DIR", &out_dir)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let replay: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("wf-mimic printed no JSON ({e}): {}", String::from_utf8_lossy(&out.stdout)));

    // Then each case is clean and the trace names them all against a recorded cassette
    assert_eq!((out.status.code(), replay["severity"].clone()), (Some(0), json!("none")), "{replay}");
    assert_eq!(replay["summary"]["clean"].as_u64(), Some(recorded as u64), "{replay}");
    let trace: Value = serde_json::from_str(&std::fs::read_to_string(out_dir.join(SUITE).join("trace.json")).unwrap()).unwrap();
    let tests = trace["tests"].as_array().unwrap();
    assert!(tests.iter().all(|t| t["cassette"] == "recorded" && t["verdict"] == "pass"), "{trace}");
    assert_eq!(tests.iter().map(|t| t["calls"].as_array().map_or(0, Vec::len)).sum::<usize>(), recorded, "{trace}");
    assert!(out_dir.join(SUITE).join("report.json").is_file());
    std::fs::remove_dir_all(&scratch).ok();
}
