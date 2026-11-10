//! Runs the `wf-mimic` binary of the `clis` workspace from a test of this workspace, which never depends on a `clis` crate (decision D2b):
//! the binary comes from `WF_MIMIC_BIN`, or else it is built from `clis` into a build directory of its own under the target directory.
use super::{mask, Fixture, Mask};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Builds `wf-mimic` behind a lock file (the cargo running the tests holds the lock of its own target directory, and two test binaries
/// starting together must not build at once).
fn build() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../clis/Cargo.toml");
    let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| std::env::current_exe().unwrap().ancestors().nth(3).unwrap().to_path_buf());
    let build_dir = target.join("wf-mimic-build");
    std::fs::create_dir_all(&build_dir).unwrap();
    let lock = std::fs::File::create(build_dir.join(".build.lock")).unwrap();
    lock.lock().unwrap();
    let status = Command::new(env!("CARGO")).args(["build", "--locked", "--quiet", "-p", "wf-mimic", "--bin", "wf-mimic", "--manifest-path"]).arg(&manifest).arg("--target-dir").arg(&build_dir).status().unwrap();
    assert!(status.success(), "building wf-mimic failed");
    build_dir.join("debug").join("wf-mimic")
}

/// Runs `wf-mimic mimic <args>` and returns its exit code and the JSON object it printed.
pub fn run(args: &[&str]) -> (i32, Value) {
    let bin = std::env::var_os("WF_MIMIC_BIN").map(PathBuf::from).unwrap_or_else(build).display().to_string();
    let out = Command::new(&bin).arg("mimic").args(args).stdin(Stdio::null()).output().unwrap_or_else(|e| panic!("{bin} could not start: {e}"));
    let text = String::from_utf8_lossy(&out.stdout);
    (out.status.code().unwrap_or(-1), serde_json::from_str(&text).unwrap_or_else(|e| panic!("{bin} mimic {args:?} printed no JSON ({e}): {text}")))
}

/// Builds an example of this crate and returns the executable cargo reports (`cargo test` builds no example of a dependency itself).
pub fn example(name: &str) -> PathBuf {
    let out = Command::new(env!("CARGO")).args(["build", "--example", name, "--message-format=json"]).current_dir(env!("CARGO_MANIFEST_DIR")).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).find_map(|m| m["executable"].as_str().map(PathBuf::from)).expect("cargo names the example's executable")
}

/// Each of `payloads` as `wf mimic record` writes it under `masks` (one interaction per payload, the payload the first step's), so a test can
/// hold `testing::mask` to mimic's own result.
pub fn masked_by_wf_mimic(masks: &[Mask], payloads: &[Value]) -> Vec<Value> {
    let dir = std::env::temp_dir().join(format!("agent-api-mask-{}-{}", std::process::id(), payloads.len()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let exe = example("mimic_echo").display().to_string();
    let cases: Vec<Value> = payloads.iter().enumerate().map(|(i, p)| json!({"id": format!("p-{i}"), "input": {"payload": p}})).collect();
    let rules: Vec<Value> = masks.iter().map(Mask::mimic_rule).collect();
    let files = [
        ("pin.json", json!({"version": "1"}).to_string()),
        ("rules.json", json!({"schema": "mimic-rules/1", "rules": rules}).to_string()),
        ("corpus.json", json!({"schema": "mimic-corpus/1", "seed": 0, "generators": [{"kind": "corpus", "name": "masks", "op": "echo", "cases": cases}]}).to_string()),
        ("mimic.toml", format!("rules = \"rules.json\"\ncorpus = \"corpus.json\"\n[reference]\ncommand = [{exe:?}]\npin = \"pin.json\"\nversion_pointer = \"/version\"\n")),
    ];
    for (name, text) in files {
        std::fs::write(dir.join(name), text).unwrap();
    }
    let (code, record) = run(&["record", &dir.display().to_string()]);
    assert_eq!((code, record["ok"].clone()), (0, json!(true)), "{record}");
    let text = std::fs::read_to_string(dir.join("interactions.jsonl")).unwrap();
    let by_id: Vec<Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let _ = std::fs::remove_dir_all(&dir);
    (0..payloads.len()).map(|i| by_id.iter().find(|it| it["id"] == format!("p-{i}")).unwrap_or_else(|| panic!("no interaction p-{i}"))["steps"][0]["payload"].clone()).collect()
}

/// A response as another build would answer it: each volatile field holds a value of its own.
fn another_build(response: &Value, masks: &[Mask]) -> Value {
    let mut other = response.clone();
    for (i, m) in masks.iter().enumerate() {
        if let Some(field) = other.pointer_mut(&m.pointer) {
            *field = json!(1_000 + i);
        }
    }
    other
}

/// Holds the in-process masks to `wf mimic record` for every response of `fixtures` (and the same response as another build would answer it).
pub fn assert_masks_match_wf_mimic(fixtures: &[Fixture]) {
    // Given every exchange of the group, its recorded response and the same response as another build would answer it
    let mut cases: Vec<(String, Vec<Mask>, Value)> = Vec::new();
    for fixture in fixtures {
        for exchange in &fixture.exchanges {
            let masks = exchange.masks();
            cases.push((format!("{}: {}", fixture.method, exchange.name), masks.clone(), another_build(&exchange.response, &masks)));
            cases.push((format!("{}: {} (recorded)", fixture.method, exchange.name), masks, exchange.response.clone()));
        }
    }
    assert!(cases.iter().any(|(_, masks, _)| !masks.is_empty()), "the fixtures mask nothing");
    // When each is masked by wf mimic (one set per distinct rule list) and by the loader
    let mut groups: Vec<(Vec<Mask>, Vec<usize>)> = Vec::new();
    for (i, (_, masks, _)) in cases.iter().enumerate() {
        match groups.iter_mut().find(|(m, _)| m == masks) {
            Some((_, at)) => at.push(i),
            None => groups.push((masks.clone(), vec![i])),
        }
    }
    for (masks, at) in groups {
        let payloads: Vec<Value> = at.iter().map(|&i| cases[i].2.clone()).collect();
        let by_mimic = masked_by_wf_mimic(&masks, &payloads);
        // Then they are the same value, and a field that was masked no longer holds what the build answered
        for (&i, theirs) in at.iter().zip(&by_mimic) {
            assert_eq!(&mask(&masks, cases[i].2.clone()), theirs, "{}", cases[i].0);
            for m in &masks {
                assert!(theirs.pointer(&m.pointer).is_none_or(|v| v == &json!(m.placeholder)), "{}: {} not masked", cases[i].0, m.pointer);
            }
        }
    }
}
